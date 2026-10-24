// Our replacement for Firefox's tokenizer glue. Parse errors and view-source are not part of this runtime.
#include "mozilla/CheckedInt.h"
#include "mozilla/MathAlgorithms.h"

#define MAX_POWER_OF_TWO_IN_INT32 0x40000000

bool nsHtml5Tokenizer::EnsureBufferSpace(int32_t aLength) {
  MOZ_RELEASE_ASSERT(aLength >= 0, "Negative length.");
  if (aLength > MAX_POWER_OF_TWO_IN_INT32) return false;
  mozilla::CheckedInt<int32_t> worstCase(strBufLen);
  worstCase += aLength;
  worstCase += charRefBufLen;
  worstCase += 2;
  if (!worstCase.isValid()) return false;
  if (worstCase.value() > MAX_POWER_OF_TWO_IN_INT32) return false;
  if (!tokenHandler->EnsureBufferSpace(worstCase.value())) return false;
  if (!strBuf) {
    if (worstCase.value() < MAX_POWER_OF_TWO_IN_INT32) worstCase += 1;
    strBuf = jArray<char16_t, int32_t>::newFallibleJArray(mozilla::RoundUpPow2(worstCase.value()));
    if (!strBuf) return false;
  } else if (worstCase.value() > strBuf.length) {
    jArray<char16_t, int32_t> newBuf = jArray<char16_t, int32_t>::newFallibleJArray(mozilla::RoundUpPow2(worstCase.value()));
    if (!newBuf) return false;
    memcpy(newBuf, strBuf, sizeof(char16_t) * size_t(strBufLen));
    strBuf = newBuf;
  }
  return true;
}

bool nsHtml5Tokenizer::TemplatePushedOrHeadPopped() { return false; }
void nsHtml5Tokenizer::RememberGt(int32_t aPos) {}
void nsHtml5Tokenizer::StartPlainText() { stateSave = nsHtml5Tokenizer::PLAINTEXT; }
void nsHtml5Tokenizer::EnableViewSource(nsHtml5Highlighter* aHighlighter) { mViewSource = mozilla::WrapUnique(aHighlighter); }
bool nsHtml5Tokenizer::ShouldFlushViewSource() { return false; }
void nsHtml5Tokenizer::StartViewSource(const nsAutoString& aTitle) {}
void nsHtml5Tokenizer::StartViewSourceBodyContents() {}
bool nsHtml5Tokenizer::EndViewSource() { return true; }
void nsHtml5Tokenizer::RewindViewSource() {}

nsHtml5String nsHtml5Tokenizer::TryAtomizeForSingleDigit() {
  if (strBufLen == 1 && strBuf[0] >= '0' && strBuf[0] <= '9') {
    static nsStaticAtom* const digitAtoms[10] = {nsGkAtoms::_0, nsGkAtoms::_1, nsGkAtoms::_2, nsGkAtoms::_3,
                                                 nsGkAtoms::_4, nsGkAtoms::_5, nsGkAtoms::_6, nsGkAtoms::_7,
                                                 nsGkAtoms::_8, nsGkAtoms::_9};
    nsHtml5String result = nsHtml5String::FromStaticAtom(digitAtoms[strBuf[0] - '0']);
    clearStrBufAfterUse();
    return result;
  }
  return nullptr;
}

// Parse errors: ignored.
void nsHtml5Tokenizer::errGarbageAfterLtSlash() {}
void nsHtml5Tokenizer::errLtSlashGt() {}
void nsHtml5Tokenizer::errWarnLtSlashInRcdata() {}
void nsHtml5Tokenizer::errCharRefLacksSemicolon() {}
void nsHtml5Tokenizer::errNoDigitsInNCR() {}
void nsHtml5Tokenizer::errGtInSystemId() {}
void nsHtml5Tokenizer::errGtInPublicId() {}
void nsHtml5Tokenizer::errNamelessDoctype() {}
void nsHtml5Tokenizer::errConsecutiveHyphens() {}
void nsHtml5Tokenizer::errPrematureEndOfComment() {}
void nsHtml5Tokenizer::errBogusComment() {}
void nsHtml5Tokenizer::errUnquotedAttributeValOrNull(char16_t c) {}
void nsHtml5Tokenizer::errSlashNotFollowedByGt() {}
void nsHtml5Tokenizer::errNoSpaceBetweenAttributes() {}
void nsHtml5Tokenizer::errLtOrEqualsOrGraveInUnquotedAttributeOrNull(char16_t c) {}
void nsHtml5Tokenizer::errAttributeValueMissing() {}
void nsHtml5Tokenizer::errBadCharBeforeAttributeNameOrNull(char16_t c) {}
void nsHtml5Tokenizer::errEqualsSignBeforeAttributeName() {}
void nsHtml5Tokenizer::errBadCharAfterLt(char16_t c) {}
void nsHtml5Tokenizer::errLtGt() {}
void nsHtml5Tokenizer::errProcessingInstruction() {}
void nsHtml5Tokenizer::errUnescapedAmpersandInterpretedAsCharacterReference() {}
void nsHtml5Tokenizer::errNotSemicolonTerminated() {}
void nsHtml5Tokenizer::errNoNamedCharacterMatch() {}
void nsHtml5Tokenizer::errQuoteBeforeAttributeName(char16_t c) {}
void nsHtml5Tokenizer::errQuoteOrLtInAttributeNameOrNull(char16_t c) {}
void nsHtml5Tokenizer::errExpectedPublicId() {}
void nsHtml5Tokenizer::errBogusDoctype() {}
void nsHtml5Tokenizer::maybeErrAttributesOnEndTag(nsHtml5HtmlAttributes* attrs) {}
void nsHtml5Tokenizer::maybeErrSlashInEndTag(bool selfClosing) {}
void nsHtml5Tokenizer::errNcrSurrogate() {}
void nsHtml5Tokenizer::errNcrInC1Range() {}
void nsHtml5Tokenizer::errEofInPublicId() {}
void nsHtml5Tokenizer::errEofInComment() {}
void nsHtml5Tokenizer::errEofInDoctype() {}
void nsHtml5Tokenizer::errEofInAttributeValue() {}
void nsHtml5Tokenizer::errEofInAttributeName() {}
void nsHtml5Tokenizer::errEofWithoutGt() {}
void nsHtml5Tokenizer::errEofInTagName() {}
void nsHtml5Tokenizer::errEofInEndTag() {}
void nsHtml5Tokenizer::errEofAfterLt() {}
void nsHtml5Tokenizer::errNcrOutOfRange() {}
void nsHtml5Tokenizer::errNcrUnassigned() {}
void nsHtml5Tokenizer::errDuplicateAttribute() {}
void nsHtml5Tokenizer::errEofInSystemId() {}
void nsHtml5Tokenizer::errExpectedSystemId() {}
void nsHtml5Tokenizer::errMissingSpaceBeforeDoctypeName() {}
void nsHtml5Tokenizer::errNestedComment() {}
void nsHtml5Tokenizer::errNcrZero() {}
void nsHtml5Tokenizer::errNoSpaceBetweenDoctypeSystemKeywordAndQuote() {}
void nsHtml5Tokenizer::errNoSpaceBetweenPublicAndSystemIds() {}
void nsHtml5Tokenizer::errNoSpaceBetweenDoctypePublicKeywordAndQuote() {}
