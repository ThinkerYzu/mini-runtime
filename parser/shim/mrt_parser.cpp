// Drives Firefox's tokenizer and tree builder over a UTF-16 document and reports the tree to an MrtSink.
#include <memory>
#include <mutex>
#include "mrt_parser.h"
#include "nsHtml5AtomTable.h"
#include "nsHtml5AttributeName.h"
#include "nsHtml5ElementName.h"
#include "nsHtml5HtmlAttributes.h"
#include "nsHtml5NamedCharacters.h"
#include "nsHtml5Portability.h"
#include "nsHtml5StackNode.h"
#include "nsHtml5Tokenizer.h"
#include "nsHtml5TreeBuilder.h"
#include "nsHtml5UTF16Buffer.h"

namespace {
// The input buffer type has protected constructors; this exposes one.
class MrtBuffer : public nsHtml5UTF16Buffer {
 public:
  MrtBuffer(char16_t* aBuffer, int32_t aEnd) : nsHtml5UTF16Buffer(aBuffer, aEnd) {}
  ~MrtBuffer() {}
};
}  // namespace

struct MrtParser {
  nsHtml5AtomTable atoms;
  std::unique_ptr<nsHtml5TreeBuilder> builder;
  std::unique_ptr<nsHtml5Tokenizer> tokenizer;
  // the document's text, which the tokenizer edits in place, and where it is in it
  std::unique_ptr<char16_t[]> text;
  std::unique_ptr<MrtBuffer> buffer;
  bool lastWasCR = false;
  // after the end of the text: eof() called (it may pop a last script), then end()
  bool atEof = false;
  bool done = false;
};

// Called from any thread, any number of times; the first call does the work and every other call
// waits for it to finish (a plain `static bool done` let a second thread parse with half-built
// tables).
extern "C" void mrt_parser_init(void) {
  static std::once_flag once;
  std::call_once(once, [] {
  mrt_atomize(u"");  // builds the static atom table
  // The same list and order as nsHtml5Module::InitializeStatics() in Firefox.
  nsHtml5AttributeName::initializeStatics();
  nsHtml5ElementName::initializeStatics();
  nsHtml5HtmlAttributes::initializeStatics();
  nsHtml5NamedCharacters::initializeStatics();
  nsHtml5Portability::initializeStatics();
  nsHtml5StackNode::initializeStatics();
  nsHtml5Tokenizer::initializeStatics();
  nsHtml5TreeBuilder::initializeStatics();
  nsHtml5UTF16Buffer::initializeStatics();
  });
}

// A document, or (context_name non-null) a fragment parsed in the context of an element of that local
// name and namespace, as nsHtml5StringParser::ParseFragment does: the tree builder's fragment mode
// (setFragmentContext) picks the insertion mode and the tokenizer state from the context (RAWTEXT
// for <style>, RCDATA for <textarea>, table modes for table elements, ...). The fragment's nodes
// are the children of the root element the tree builder makes (Gecko gives it the DocumentFragment
// as context node instead).
static MrtParser* Begin(const MrtSink* sink, const char16_t* text, uint32_t len, int declarative_shadow_roots,
                        int scripting_enabled, const char16_t* context_name, uint32_t context_len,
                        int32_t context_ns, int quirks) {
  auto* p = new MrtParser();
  p->builder.reset(new nsHtml5TreeBuilder(sink));
  p->tokenizer.reset(new nsHtml5Tokenizer(p->builder.get(), false));
  p->tokenizer->setInterner(&p->atoms);
  if (context_name) {
    p->builder->setFragmentContext(mrt_atomize(std::u16string_view(context_name, context_len)), context_ns, nullptr,
                                   quirks != 0);
  }
  // a document whose scripts run, and every fragment (nsHtml5StringParser::ParseFragment), parse
  // <noscript> as text; DOMParser and a page without script as elements
  p->builder->setScriptingEnabled(scripting_enabled != 0);
  p->builder->setIsSrcdocDocument(false);
  p->builder->setAllowDeclarativeShadowRoots(declarative_shadow_roots != 0);
  // a document whose scripts run reports its <script> elements' places (the tokenizer reads this in
  // start())
  p->builder->SetWantsLineAndColumn(!context_name && scripting_enabled != 0);
  p->tokenizer->start();

  // The tokenizer edits the buffer in place, so give it a copy.
  p->text.reset(new char16_t[len + 1]);
  for (uint32_t i = 0; i < len; ++i) p->text[i] = text[i];
  p->text[len] = 0;
  p->buffer.reset(new MrtBuffer(p->text.get(), (int32_t)len));
  return p;
}

// Tokenize until a <script> is popped (only when the builder suspends at scripts) or the document
// ends; returns the script, or 0 once the whole document is parsed.
static uint32_t Resume(MrtParser* p) {
  if (p->done) return 0;
  // As nsHtml5StringParser::Tokenize: the tokenizer returns after every carriage return (it reports
  // it as a line feed), and the caller drops a line feed that follows it and calls again. Called
  // once, the parse stopped at the first CR (session 35: Amazon, served with CRLF). It also returns
  // after a popped script (a suspension), with the buffer at the next character.
  MrtBuffer& buffer = *p->buffer;
  while (!p->atEof && buffer.hasMore()) {
    buffer.adjust(p->lastWasCR);
    p->lastWasCR = false;
    if (buffer.hasMore()) {
      p->tokenizer->EnsureBufferSpace(buffer.getLength());
      p->lastWasCR = p->tokenizer->tokenizeBuffer(&buffer);
      if (uint32_t script = p->builder->TakePoppedScript()) return script;
    }
  }
  if (!p->atEof) {
    p->atEof = true;
    // (the end of the file pops what is open, an unclosed <script> too, which runs as in Gecko)
    p->tokenizer->eof();
    if (uint32_t script = p->builder->TakePoppedScript()) return script;
  }
  p->tokenizer->end();
  p->done = true;
  p->text.reset();
  return 0;
}

// The one-shot parse: every caller but a document whose scripts run at the parser's place.
static MrtParser* Parse(const MrtSink* sink, const char16_t* text, uint32_t len, int declarative_shadow_roots,
                        int scripting_enabled, const char16_t* context_name, uint32_t context_len,
                        int32_t context_ns, int quirks) {
  MrtParser* p = Begin(sink, text, len, declarative_shadow_roots, scripting_enabled, context_name, context_len,
                       context_ns, quirks);
  Resume(p);
  return p;
}

extern "C" MrtParser* mrt_parse_document(const MrtSink* sink, const char16_t* text, uint32_t len,
                                         int declarative_shadow_roots, int scripting_enabled) {
  return Parse(sink, text, len, declarative_shadow_roots, scripting_enabled, nullptr, 0, 0, 0);
}

extern "C" MrtParser* mrt_parse_fragment(const MrtSink* sink, const char16_t* text, uint32_t len,
                                         int scripting_enabled, const char16_t* context_name,
                                         uint32_t context_len, int32_t context_ns, int quirks) {
  return Parse(sink, text, len, 0, scripting_enabled, context_name, context_len, context_ns, quirks);
}

extern "C" MrtParser* mrt_parser_begin_document(const MrtSink* sink, const char16_t* text, uint32_t len) {
  MrtParser* p = Begin(sink, text, len, 1, 1, nullptr, 0, 0, 0);
  p->builder->SetSuspendAtScripts(true);
  return p;
}

extern "C" uint32_t mrt_parser_resume(MrtParser* p, void* ctx) {
  p->builder->SetSinkContext(ctx);
  return Resume(p);
}

extern "C" void mrt_parser_free(MrtParser* p) { delete p; }
