// Our replacement for Firefox's tree-builder glue (definitions). Every hook that Firefox turns into
// a queued DOM operation is a direct call into the MrtSink here.
#include <vector>
#include "mozilla/CheckedInt.h"
#include "mozilla/MathAlgorithms.h"

#ifndef MAX_POWER_OF_TWO_IN_INT32
#define MAX_POWER_OF_TWO_IN_INT32 0x40000000
#endif
#define MRT_H(x) (static_cast<uint32_t>(reinterpret_cast<uintptr_t>(x)))
#define MRT_P(x) (reinterpret_cast<nsIContentHandle*>(static_cast<uintptr_t>(x)))

nsHtml5TreeBuilder::nsHtml5TreeBuilder(const MrtSink* aSink)
    : mode(0),
      originalMode(0),
      framesetOk(false),
      tokenizer(nullptr),
      scriptingEnabled(false),
      needToDropLF(false),
      fragment(false),
      contextName(nullptr),
      contextNamespace(kNameSpaceID_None),
      contextNode(nullptr),
      templateModePtr(0),
      stackNodesIdx(0),
      numStackNodes(0),
      currentPtr(0),
      listPtr(0),
      formPointer(nullptr),
      headPointer(nullptr),
      charBufferLen(0),
      quirks(false),
      forceNoQuirks(false),
      allowDeclarativeShadowRoots(false),
      keepBuffer(false),
      mSink(*aSink),
      mViewSource(nullptr),
      mBroken(NS_OK),
      mDocumentMode(0) {}

nsHtml5TreeBuilder::~nsHtml5TreeBuilder() {}

// Turn the parser's attribute list into the plain structs the sink takes. The strings stay owned by
// the attribute list, so they are valid for the duration of the callback. The element's namespace picks
// which spelling of the names applies (SVG and MathML adjust some attribute names).
static void MrtConvertAttributes(nsHtml5HtmlAttributes* aAttributes, int32_t aElementNs, std::vector<MrtAttr>& aOut) {
  aOut.clear();
  if (!aAttributes) return;
  aOut.reserve(aAttributes->getLength());
  for (nsHtml5AttributeEntry& entry : *aAttributes) {
    int32_t ns = kNameSpaceID_None;
    nsAtom* local;
    if (aElementNs == kNameSpaceID_SVG) {
      nsHtml5NameTriple t = entry.NameSVG();
      ns = t.mNamespace; local = t.mLocal;
    } else if (aElementNs == kNameSpaceID_MathML) {
      nsHtml5NameTriple t = entry.NameMathML();
      ns = t.mNamespace; local = t.mLocal;
    } else {
      local = entry.NameHTML();
    }
    nsHtml5String value = entry.Value();
    aOut.push_back(MrtAttr{ns, local->GetUTF16String(), local->GetLength(), value.Data(), value.Length()});
  }
}

nsIContentHandle* nsHtml5TreeBuilder::createElement(int32_t aNamespace, nsAtom* aName,
                                                    nsHtml5HtmlAttributes* aAttributes,
                                                    nsIContentHandle* aIntendedParent,
                                                    nsHtml5ContentCreatorFunction aCreator) {
  return createElement(aNamespace, aName, aAttributes, nullptr, aIntendedParent, aCreator);
}

nsIContentHandle* nsHtml5TreeBuilder::createElement(int32_t aNamespace, nsAtom* aName,
                                                    nsHtml5HtmlAttributes* aAttributes,
                                                    nsIContentHandle* aFormElement,
                                                    nsIContentHandle* aIntendedParent,
                                                    nsHtml5ContentCreatorFunction aCreator) {
  std::vector<MrtAttr> attrs;
  MrtConvertAttributes(aAttributes, aNamespace, attrs);
  uint32_t element = mSink.create_element(mSink.ctx, aNamespace, aName->GetUTF16String(), aName->GetLength(),
                                          attrs.data(), (uint32_t)attrs.size(), MRT_H(aFormElement));
  // An HTML <script>'s place in the document, as Gecko's opSetScriptLineAndColumnNumberAndFreeze: the
  // tokenizer is at the start tag's '>', so the script's text starts one column on
  if (mWantsLineAndColumn && aNamespace == kNameSpaceID_XHTML && aName == nsGkAtoms::script) {
    mSink.set_script_position(mSink.ctx, element, tokenizer->getLineNumber(), tokenizer->getColumnNumber() + 1);
  }
  return MRT_P(element);
}

nsIContentHandle* nsHtml5TreeBuilder::createHtmlElementSetAsRoot(nsHtml5HtmlAttributes* aAttributes) {
  std::vector<MrtAttr> attrs;
  MrtConvertAttributes(aAttributes, kNameSpaceID_XHTML, attrs);
  return MRT_P(mSink.create_root(mSink.ctx, nsGkAtoms::html->GetUTF16String(), nsGkAtoms::html->GetLength(),
                                 attrs.data(), (uint32_t)attrs.size()));
}

nsIContentHandle* nsHtml5TreeBuilder::createAndInsertFosterParentedElement(
    int32_t aNamespace, nsAtom* aName, nsHtml5HtmlAttributes* aAttributes, nsIContentHandle* aFormElement,
    nsIContentHandle* aTable, nsIContentHandle* aStackParent, nsHtml5ContentCreatorFunction aCreator) {
  nsIContentHandle* child = createElement(aNamespace, aName, aAttributes, aFormElement, aStackParent, aCreator);
  mSink.insert_foster_parented(mSink.ctx, MRT_H(child), MRT_H(aTable), MRT_H(aStackParent));
  return child;
}

void nsHtml5TreeBuilder::optionElementPopped(nsIContentHandle* aOption) {}

void nsHtml5TreeBuilder::detachFromParent(nsIContentHandle* aElement) {
  mSink.detach_from_parent(mSink.ctx, MRT_H(aElement));
}

bool nsHtml5TreeBuilder::hasChildren(nsIContentHandle* aElement) {
  return mSink.has_children(mSink.ctx, MRT_H(aElement)) != 0;
}

void nsHtml5TreeBuilder::appendElement(nsIContentHandle* aChild, nsIContentHandle* aParent) {
  mSink.append_child(mSink.ctx, MRT_H(aChild), MRT_H(aParent));
}

void nsHtml5TreeBuilder::appendChildrenToNewParent(nsIContentHandle* aOldParent, nsIContentHandle* aNewParent) {
  mSink.append_children_to_new_parent(mSink.ctx, MRT_H(aOldParent), MRT_H(aNewParent));
}

void nsHtml5TreeBuilder::insertFosterParentedCharacters(char16_t* aBuffer, int32_t aStart, int32_t aLength,
                                                        nsIContentHandle* aTable,
                                                        nsIContentHandle* aStackParent) {
  mSink.insert_foster_text(mSink.ctx, aBuffer + aStart, (uint32_t)aLength, MRT_H(aTable), MRT_H(aStackParent));
}

void nsHtml5TreeBuilder::insertFosterParentedChild(nsIContentHandle* aChild, nsIContentHandle* aTable,
                                                   nsIContentHandle* aStackParent) {
  mSink.insert_foster_parented(mSink.ctx, MRT_H(aChild), MRT_H(aTable), MRT_H(aStackParent));
}

void nsHtml5TreeBuilder::appendCharacters(nsIContentHandle* aParent, char16_t* aBuffer, int32_t aStart,
                                          int32_t aLength) {
  mSink.append_text(mSink.ctx, MRT_H(aParent), aBuffer + aStart, (uint32_t)aLength);
}

void nsHtml5TreeBuilder::appendComment(nsIContentHandle* aParent, char16_t* aBuffer, int32_t aStart,
                                       int32_t aLength) {
  mSink.append_comment(mSink.ctx, MRT_H(aParent), aBuffer + aStart, (uint32_t)aLength);
}

void nsHtml5TreeBuilder::appendCommentToDocument(char16_t* aBuffer, int32_t aStart, int32_t aLength) {
  mSink.append_comment(mSink.ctx, 0, aBuffer + aStart, (uint32_t)aLength);
}

void nsHtml5TreeBuilder::addAttributesToElement(nsIContentHandle* aElement, nsHtml5HtmlAttributes* aAttributes) {
  std::vector<MrtAttr> attrs;
  MrtConvertAttributes(aAttributes, kNameSpaceID_XHTML, attrs);
  mSink.add_attributes(mSink.ctx, MRT_H(aElement), attrs.data(), (uint32_t)attrs.size());
}

void nsHtml5TreeBuilder::markMalformedIfScript(nsIContentHandle* aElement) {}
void nsHtml5TreeBuilder::start(bool fragment) {}
void nsHtml5TreeBuilder::end() {}
void nsHtml5TreeBuilder::elementPushed(int32_t aNamespace, nsAtom* aName, nsIContentHandle* aElement) {}
// A script's end tag in a document parsed at its scripts' pace: the tokenizer stops after it, so that
// the script runs where the parser is (Gecko's opRunScriptThatMayDocumentWriteOrBlock and
// requestSuspension; async and defer scripts do not stop Gecko's parser, but nothing else runs before
// the next resume here, so stopping for them changes nothing)
void nsHtml5TreeBuilder::elementPopped(int32_t aNamespace, nsAtom* aName, nsIContentHandle* aElement) {
  if (mSuspendAtScripts && aNamespace != kNameSpaceID_MathML && aName == nsGkAtoms::script) {
    mPoppedScript = MRT_H(aElement);
    requestSuspension();
  }
}

void nsHtml5TreeBuilder::appendDoctypeToDocument(nsAtom* aName, nsHtml5String aPublicId, nsHtml5String aSystemId) {
  mSink.append_doctype(mSink.ctx, aName->GetUTF16String(), aName->GetLength(), aPublicId.Data(), aPublicId.Length(),
                       aSystemId.Data(), aSystemId.Length());
}

void nsHtml5TreeBuilder::documentMode(nsHtml5DocumentMode m) {
  mDocumentMode = (uint32_t)m;
  mSink.set_document_mode(mSink.ctx, (int32_t)m);
}

// As Firefox's (nsHtml5TreeBuilder::getShadowRootFromHost with a builder), with the attach done by
// the sink: `mode` must be open or closed, slot assignment is manual only when asked for (the pref
// dom.shadowdom.shadowRootSlotAssignment.enabled is on by default). The sink also makes the shadow
// root the template's contents (setDocumentFragmentForTemplate), so the template's children go into
// it. Scoped registries and reference targets are off by default in Firefox 157 and not passed.
nsIContentHandle* nsHtml5TreeBuilder::getShadowRootFromHost(
    nsIContentHandle* aHost, nsIContentHandle* aTemplateNode, nsHtml5String aShadowRootMode,
    bool aShadowRootIsClonable, bool aShadowRootIsSerializable, bool aShadowRootDelegatesFocus,
    bool aShadowRootCustomElementRegistry, nsHtml5String aShadowRootSlotAssignment,
    nsHtml5String aShadowRootReferenceTarget) {
  uint32_t flags;
  if (aShadowRootMode.LowerCaseEqualsASCII("open")) {
    flags = 0;
  } else if (aShadowRootMode.LowerCaseEqualsASCII("closed")) {
    flags = MRT_SR_CLOSED;
  } else {
    return nullptr;
  }
  if (aShadowRootIsClonable) flags |= MRT_SR_CLONABLE;
  if (aShadowRootIsSerializable) flags |= MRT_SR_SERIALIZABLE;
  if (aShadowRootDelegatesFocus) flags |= MRT_SR_DELEGATES_FOCUS;
  if (aShadowRootSlotAssignment && aShadowRootSlotAssignment.LowerCaseEqualsASCII("manual")) {
    flags |= MRT_SR_MANUAL_SLOTS;
  }
  uint32_t root = mSink.attach_shadow_root(mSink.ctx, MRT_H(aHost), MRT_H(aTemplateNode), flags);
  return root ? MRT_P(root) : nullptr;
}

nsIContentHandle* nsHtml5TreeBuilder::getDocumentFragmentForTemplate(nsIContentHandle* aTemplate) {
  return MRT_P(mSink.template_contents(mSink.ctx, MRT_H(aTemplate)));
}

void nsHtml5TreeBuilder::accumulateCharacters(const char16_t* aBuf, int32_t aStart, int32_t aLength) {
  MOZ_RELEASE_ASSERT(charBufferLen + aLength <= charBuffer.length, "About to memcpy past the end of the buffer!");
  memcpy(charBuffer + charBufferLen, aBuf + aStart, sizeof(char16_t) * aLength);
  charBufferLen += aLength;
}

bool nsHtml5TreeBuilder::EnsureBufferSpace(int32_t aLength) {
  mozilla::CheckedInt<int32_t> worstCase(charBufferLen);
  worstCase += aLength;
  if (!worstCase.isValid()) return false;
  if (worstCase.value() > MAX_POWER_OF_TWO_IN_INT32) return false;
  if (!charBuffer) {
    if (worstCase.value() < MAX_POWER_OF_TWO_IN_INT32) worstCase += 1;
    charBuffer = jArray<char16_t, int32_t>::newFallibleJArray(mozilla::RoundUpPow2(worstCase.value()));
    if (!charBuffer) return false;
  } else if (worstCase.value() > charBuffer.length) {
    jArray<char16_t, int32_t> newBuf = jArray<char16_t, int32_t>::newFallibleJArray(mozilla::RoundUpPow2(worstCase.value()));
    if (!newBuf) return false;
    memcpy(newBuf, charBuffer, sizeof(char16_t) * size_t(charBufferLen));
    charBuffer = newBuf;
  }
  return true;
}
