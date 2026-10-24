// C interface between Firefox's HTML parser core (C++) and the DOM (Rust).
//
// The tree builder reports what it builds through these callbacks. Nodes are identified by 32-bit
// handles chosen by the DOM; 0 means "no node". Strings are UTF-16.
#pragma once
#include <uchar.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif

typedef struct MrtAttr {
  int32_t ns;  // 0 none, 1 xmlns, 2 xml, 3 xhtml, 4 xlink, 5 mathml, 6 svg
  const char16_t* name;
  uint32_t name_len;
  const char16_t* value;
  uint32_t value_len;
} MrtAttr;

typedef struct MrtSink {
  void* ctx;
  // Create an element (not yet in the tree). `form` is the form owner or 0.
  uint32_t (*create_element)(void* ctx, int32_t ns, const char16_t* name, uint32_t name_len,
                             const MrtAttr* attrs, uint32_t n_attrs, uint32_t form);
  // Create the <html> element and make it the root of the document.
  uint32_t (*create_root)(void* ctx, const char16_t* name, uint32_t name_len, const MrtAttr* attrs,
                          uint32_t n_attrs);
  void (*append_child)(void* ctx, uint32_t child, uint32_t parent);
  void (*append_children_to_new_parent)(void* ctx, uint32_t old_parent, uint32_t new_parent);
  void (*detach_from_parent)(void* ctx, uint32_t node);
  // Insert `child` before `table` if `table` has a parent, else append it to `stack_parent`.
  void (*insert_foster_parented)(void* ctx, uint32_t child, uint32_t table, uint32_t stack_parent);
  void (*append_text)(void* ctx, uint32_t parent, const char16_t* text, uint32_t len);
  void (*insert_foster_text)(void* ctx, const char16_t* text, uint32_t len, uint32_t table,
                             uint32_t stack_parent);
  // parent 0 means the document.
  void (*append_comment)(void* ctx, uint32_t parent, const char16_t* text, uint32_t len);
  void (*add_attributes)(void* ctx, uint32_t node, const MrtAttr* attrs, uint32_t n_attrs);
  void (*append_doctype)(void* ctx, const char16_t* name, uint32_t name_len, const char16_t* public_id,
                         uint32_t public_len, const char16_t* system_id, uint32_t system_len);
  // 0 no-quirks, 1 almost-standards, 2 quirks
  void (*set_document_mode)(void* ctx, int32_t mode);
  int32_t (*has_children)(void* ctx, uint32_t node);
  // The node that holds a <template> element's contents.
  uint32_t (*template_contents)(void* ctx, uint32_t template_node);
  // A declarative shadow root (<template shadowrootmode>) for `host`, with MRT_SR_* flags; it also
  // becomes the template's contents. 0 when the host cannot have one (or has one already); the
  // template is then inserted as a plain template.
  uint32_t (*attach_shadow_root)(void* ctx, uint32_t host, uint32_t template_node, uint32_t flags);
  // An HTML <script> element's place in the document: the line and the one-based column its text
  // starts at. Called only when parsing a document with scripting enabled.
  void (*set_script_position)(void* ctx, uint32_t script, int32_t line, int32_t column);
} MrtSink;

// Shadow root options, as mrt-dom's SR_* constants.
#define MRT_SR_CLOSED 1u
#define MRT_SR_CLONABLE 2u
#define MRT_SR_SERIALIZABLE 4u
#define MRT_SR_DELEGATES_FOCUS 8u
#define MRT_SR_MANUAL_SLOTS 16u

typedef struct MrtParser MrtParser;

// Static tables (atoms, element and attribute names). Call once before parsing.
void mrt_parser_init(void);
// Parse a complete document given as UTF-16. `declarative_shadow_roots`: whether
// <template shadowrootmode> attaches a shadow root (a document loaded in a tab: yes; innerHTML: no).
// `scripting_enabled`: the tree builder's scripting flag; on, <noscript> is parsed as raw text (a
// document whose scripts run, and every fragment); off, as elements (DOMParser, a page without script).
MrtParser* mrt_parse_document(const MrtSink* sink, const char16_t* text, uint32_t len, int declarative_shadow_roots,
                              int scripting_enabled);
// A fragment in the context of an element (its local name and namespace, 3 for HTML), as innerHTML
// parses: the fragment's nodes are the children of the document's root element.
MrtParser* mrt_parse_fragment(const MrtSink* sink, const char16_t* text, uint32_t len, int scripting_enabled,
                              const char16_t* context_name, uint32_t context_len, int32_t context_ns, int quirks);
// A document whose scripts run at the parser's place (scripting on, declarative shadow roots attached):
// nothing is parsed until mrt_parser_resume.
MrtParser* mrt_parser_begin_document(const MrtSink* sink, const char16_t* text, uint32_t len);
// Parse on, reporting to the sink with context `ctx`, until a <script> (HTML or SVG) is popped, whose
// handle is returned (its script runs before the next call, Gecko's opRunScriptThatMayDocumentWriteOrBlock),
// or the document is parsed, when it returns 0.
uint32_t mrt_parser_resume(MrtParser* p, void* ctx);
void mrt_parser_free(MrtParser* p);

#ifdef __cplusplus
}
#endif
