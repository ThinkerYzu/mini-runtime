// The view-source highlighter is not part of this runtime. The tokenizer holds a pointer to it that is
// always null; the methods exist so the generated code compiles.
#pragma once
#include "mozilla/UniquePtr.h"
#include "nsHtml5String.h"
class nsAtom;
class nsHtml5UTF16Buffer;
class nsHtml5Highlighter {
 public:
  int32_t Transition(int32_t aState, bool aReconsume, int32_t aPos) { return aState; }
  void CompletedNamedCharacterReference() {}
  template <class... A> void SetBuffer(A...) {}
  template <class... A> void DropBuffer(A...) {}
  template <class... A> void MaybeLinkifyAttributeValue(A...) {}
  template <class... A> void AddErrorToCurrentNode(A...) {}
  template <class... A> void AddErrorToCurrentRun(A...) {}
  template <class... A> void AddErrorToCurrentSlash(A...) {}
};
