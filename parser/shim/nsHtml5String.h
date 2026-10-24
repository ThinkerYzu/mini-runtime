// The parser's string type. Same shape as Firefox's: one word, either null, empty, a pointer to a
// reference-counted UTF-16 buffer, or a pointer to an atom. It is copied bitwise by the generated code,
// so reference counting is manual (Clone() and Release()), exactly as in Firefox.
#pragma once
#include <cstdint>
#include <string>
#include "nsAtom.h"
#include "nsString.h"

class nsHtml5TreeBuilder;

class nsHtml5String final {
 public:
  struct Buffer;  // length, count, characters

 private:
  static const uintptr_t kKindMask = uintptr_t(3);
  static const uintptr_t kPtrMask = ~kKindMask;
  enum Kind : uintptr_t { eNull = 0, eEmpty = 1, eBuffer = 2, eAtom = 3 };
  Kind GetKind() const { return (Kind)(mBits & kKindMask); }
  Buffer* AsBuffer() const { return reinterpret_cast<Buffer*>(mBits & kPtrMask); }

 public:
  bool IsAtom() const { return GetKind() == eAtom; }
  nsAtom* AsAtom() const { return reinterpret_cast<nsAtom*>(mBits & kPtrMask); }
  nsHtml5String() : mBits(eNull) {}
  nsHtml5String(decltype(nullptr)) : mBits(eNull) {}
  uint32_t Length() const;
  operator bool() const { return mBits; }
  nsAtom* MaybeAsAtom() { return IsAtom() ? AsAtom() : nullptr; }
  const char16_t* Data() const;
  void ToString(nsAString& aString);
  void MoveToString(nsAString& aString) { ToString(aString); Release(); }
  void CopyToBuffer(char16_t* aBuffer) const;
  bool LowerCaseEqualsASCII(const char* aLowerCaseLiteral) const;
  bool EqualsASCII(const char* aLiteral) const;
  bool LowerCaseStartsWithASCII(const char* aLowerCaseLiteral) const;
  bool Equals(nsHtml5String aOther) const;
  nsHtml5String Clone();
  void Release();
  static nsHtml5String FromBuffer(char16_t* aBuffer, int32_t aLength, nsHtml5TreeBuilder* aTreeBuilder);
  static nsHtml5String FromLiteral(const char* aLiteral);
  static nsHtml5String FromString(const nsAString& aString);
  static nsHtml5String FromAtom(already_AddRefed<nsAtom> aAtom);
  static nsHtml5String FromStaticAtom(nsStaticAtom* aAtom);
  static nsHtml5String EmptyString() { return nsHtml5String(uintptr_t(eEmpty)); }

 private:
  explicit nsHtml5String(uintptr_t aBits) : mBits(aBits) {}
  uintptr_t mBits;
};
