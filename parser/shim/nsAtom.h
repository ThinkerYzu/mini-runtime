// A small atom type. Static atoms (the ~2700 names Firefox knows about, listed in nsGkAtomList.h) are
// entries of one array, so the parser can compare names by pointer, as it does in Firefox.
#pragma once
#include <cstdint>
#include <string_view>
#include <cstring>
#include <cstdlib>
#include "nscore.h"
#include "mrt_parser.h"
#include "mozilla/Assertions.h"
#include "nsDebug.h"
#include "nsString.h"

struct nsAtom {
  const char16_t* mString;
  uint32_t mLength;
  uint32_t mHash;
  bool mIsStatic;
  const char16_t* GetUTF16String() const { return mString; }
  uint32_t GetLength() const { return mLength; }
  bool IsStatic() const { return mIsStatic; }
  std::u16string_view View() const { return {mString, mLength}; }
  void AddRef() {}
  void Release() {}
  void ToString(nsAString& aString) const { aString.assign(mString, mLength); }
  bool Equals(const char16_t* aChars, uint32_t aLength) const {
    return mLength == aLength && std::memcmp(mString, aChars, aLength * sizeof(char16_t)) == 0;
  }
};
using nsStaticAtom = nsAtom;

template <class T>
struct already_AddRefed {
  T* mRawPtr;
  already_AddRefed(T* aPtr) : mRawPtr(aPtr) {}
  T* take() { return mRawPtr; }
};

// Look up (or create) the atom for a string. A string that matches a static atom gives that atom.
nsAtom* mrt_atomize(std::u16string_view s);

// Atoms are not reference counted in this runtime; these macros keep the generated code compiling.
#define NS_IF_ADDREF(p) ((void)0)
#define NS_IF_RELEASE(p) ((void)0)
#define NS_ADDREF(p) ((void)0)
#define NS_RELEASE(p) ((void)0)

// RefPtr without counting: it only holds the pointer.
template <class T>
class RefPtr {
  T* mPtr = nullptr;
 public:
  RefPtr() = default;
  RefPtr(T* p) : mPtr(p) {}
  RefPtr& operator=(T* p) { mPtr = p; return *this; }
  T* get() const { return mPtr; }
  operator T*() const { return mPtr; }
  T* operator->() const { return mPtr; }
  already_AddRefed<T> forget() { T* p = mPtr; mPtr = nullptr; return already_AddRefed<T>(p); }
};
