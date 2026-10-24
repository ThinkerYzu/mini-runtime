// Our replacement for Firefox's tree-builder glue (declarations). The generated tree builder calls
// these hooks; they report to an MrtSink instead of queuing Firefox DOM operations.
private:
MrtSink mSink;
nsHtml5Highlighter* mViewSource;
nsresult mBroken;
uint32_t mDocumentMode;
void* mBuilder = nullptr;  // Firefox's direct-DOM builder; not used here
bool mCurrentHtmlScriptCannotDocumentWriteOrBlock = false;
// line and column tracking, on for a document whose scripts run (see WantsLineAndColumn)
bool mWantsLineAndColumn = false;
// a document parsed at its scripts' pace: the tokenizer stops after each popped <script>
// (mrt_parser_begin_document), which is left here for mrt_parser_resume
bool mSuspendAtScripts = false;
uint32_t mPoppedScript = 0;

void documentMode(nsHtml5DocumentMode m);
nsIContentHandle* getDocumentFragmentForTemplate(nsIContentHandle* aTemplate);
void setDocumentFragmentForTemplate(nsIContentHandle* aTemplate, nsIContentHandle* aFragment) {}
nsIContentHandle* getShadowRootFromHost(nsIContentHandle* aHost, nsIContentHandle* aTemplateNode,
                                        nsHtml5String aShadowRootMode, bool aShadowRootIsClonable,
                                        bool aShadowRootIsSerializable, bool aShadowRootDelegatesFocus,
                                        bool aShadowRootCustomElementRegistry,
                                        nsHtml5String aShadowRootSlotAssignment,
                                        nsHtml5String aShadowRootReferenceTarget);
nsIContentHandle* getFormPointerForContext(nsIContentHandle* aContext) { return nullptr; }
void accumulateCharactersForced(const char16_t* aBuf, int32_t aStart, int32_t aLength) {
  accumulateCharacters(aBuf, aStart, aLength);
}
void MarkAsBrokenFromPortability(nsresult aRv) { MarkAsBroken(aRv); }

public:
explicit nsHtml5TreeBuilder(const MrtSink* aSink);
~nsHtml5TreeBuilder();
// Gecko's is on unless script execution is prevented (innerHTML, DOMParser); here, on for a document
// whose scripts run, which is what reads it: a <script>'s place for its errors and stacks
bool WantsLineAndColumn() { return mWantsLineAndColumn; }
void SetWantsLineAndColumn(bool aWants) { mWantsLineAndColumn = aWants; }
void SetSuspendAtScripts(bool aSuspend) { mSuspendAtScripts = aSuspend; }
// the sink's context for the next callbacks (a resumed parse: the DOM may have moved)
void SetSinkContext(void* aCtx) { mSink.ctx = aCtx; }
// the <script> popped since the last call, or 0
uint32_t TakePoppedScript() {
  uint32_t script = mPoppedScript;
  mPoppedScript = 0;
  return script;
}
bool HasBuilder() { return false; }
bool HasScriptThatMayDocumentWriteOrBlock() { return false; }
void EnableViewSource(nsHtml5Highlighter* aHighlighter) { mViewSource = aHighlighter; }
bool EnsureBufferSpace(int32_t aLength);
void NeedsCharsetSwitchTo(const void* aEncoding, int32_t aSource, int32_t aLineNumber) {}
void MaybeComplainAboutCharset(const char* aMsgId, bool aError, int32_t aLineNumber) {}
void MarkAsBroken(nsresult aRv) { mBroken = aRv; }
nsresult IsBroken() { return mBroken; }
void StreamEnded() {}
// Parse errors are not reported.
  void errDeepTree() {}
  void errStrayStartTag(nsAtom* aName) {}
  void errStrayEndTag(nsAtom* aName) {}
  void errUnclosedElements(int32_t aIndex, nsAtom* aName) {}
  void errUnclosedElementsImplied(int32_t aIndex, nsAtom* aName) {}
  void errUnclosedElementsCell(int32_t aIndex) {}
  void errStrayDoctype() {}
  void errAlmostStandardsDoctype() {}
  void errQuirkyDoctype() {}
  void errNonSpaceInTrailer() {}
  void errNonSpaceAfterFrameset() {}
  void errNonSpaceInFrameset() {}
  void errNonSpaceAfterBody() {}
  void errNonSpaceInColgroupInFragment() {}
  void errNonSpaceInNoscriptInHead() {}
  void errFooBetweenHeadAndBody(nsAtom* aName) {}
  void errStartTagWithoutDoctype() {}
  void errNoSelectInTableScope() {}
  void errStartSelectWhereEndSelectExpected() {}
  void errStartTagWithSelectOpen(nsAtom* aName) {}
  void errBadStartTagInNoscriptInHead(nsAtom* aName) {}
  void errImage() {}
  void errIsindex() {}
  void errFooSeenWhenFooOpen(nsAtom* aName) {}
  void errHeadingWhenHeadingOpen() {}
  void errFramesetStart() {}
  void errNoCellToClose() {}
  void errStartTagInTable(nsAtom* aName) {}
  void errFormWhenFormOpen() {}
  void errTableSeenWhileTableOpen() {}
  void errStartTagInTableBody(nsAtom* aName) {}
  void errEndTagSeenWithoutDoctype() {}
  void errEndTagAfterBody() {}
  void errEndTagSeenWithSelectOpen(nsAtom* aName) {}
  void errGarbageInColgroup() {}
  void errEndTagBr() {}
  void errNoElementToCloseButEndTagSeen(nsAtom* aName) {}
  void errHtmlStartTagInForeignContext(nsAtom* aName) {}
  void errNoTableRowToClose() {}
  void errNonSpaceInTable() {}
  void errUnclosedChildrenInRuby() {}
  void errStartTagSeenWithoutRuby(nsAtom* aName) {}
  void errSelfClosing() {}
  void errNoCheckUnclosedElementsOnStack() {}
  void errEndTagDidNotMatchCurrentOpenElement(nsAtom* aName, nsAtom* aOther) {}
  void errEndTagViolatesNestingRules(nsAtom* aName) {}
  void errEndWithUnclosedElements(nsAtom* aName) {}
  void errListUnclosedStartTags(int32_t aIgnored) {}
