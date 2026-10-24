#include "nsHtml5String.h"
#include <atomic>
#include <cstdlib>
#include <cstring>
#include <new>

struct nsHtml5String::Buffer {
  std::atomic<uint32_t> mRefCount;
  uint32_t mLength;
  // characters follow, then a terminating zero
  char16_t* Chars() { return reinterpret_cast<char16_t*>(this + 1); }
};

static nsHtml5String::Buffer* AllocBuffer(uint32_t aLength) {
  void* mem = std::aligned_alloc(8, (sizeof(nsHtml5String::Buffer) + (aLength + 1) * sizeof(char16_t) + 7) & ~size_t(7));
  if (!mem) std::abort();
  auto* b = new (mem) nsHtml5String::Buffer();
  b->mRefCount.store(1);
  b->mLength = aLength;
  return b;
}

uint32_t nsHtml5String::Length() const {
  switch (GetKind()) {
    case eBuffer: return AsBuffer()->mLength;
    case eAtom: return AsAtom()->GetLength();
    default: return 0;
  }
}

const char16_t* nsHtml5String::Data() const {
  static const char16_t kEmpty[1] = {0};
  switch (GetKind()) {
    case eBuffer: return AsBuffer()->Chars();
    case eAtom: return AsAtom()->GetUTF16String();
    default: return kEmpty;
  }
}

void nsHtml5String::ToString(nsAString& aString) { aString.assign(Data(), Length()); }

void nsHtml5String::CopyToBuffer(char16_t* aBuffer) const { std::memcpy(aBuffer, Data(), Length() * sizeof(char16_t)); }

bool nsHtml5String::LowerCaseEqualsASCII(const char* aLowerCaseLiteral) const {
  uint32_t n = Length();
  if (std::strlen(aLowerCaseLiteral) != n) return false;
  const char16_t* d = Data();
  for (uint32_t i = 0; i < n; ++i) {
    char16_t c = d[i];
    if (c >= 'A' && c <= 'Z') c += 0x20;
    if (c != (unsigned char)aLowerCaseLiteral[i]) return false;
  }
  return true;
}

bool nsHtml5String::EqualsASCII(const char* aLiteral) const {
  uint32_t n = Length();
  if (std::strlen(aLiteral) != n) return false;
  const char16_t* d = Data();
  for (uint32_t i = 0; i < n; ++i) if (d[i] != (unsigned char)aLiteral[i]) return false;
  return true;
}

bool nsHtml5String::LowerCaseStartsWithASCII(const char* aLowerCaseLiteral) const {
  uint32_t n = (uint32_t)std::strlen(aLowerCaseLiteral);
  if (Length() < n) return false;
  const char16_t* d = Data();
  for (uint32_t i = 0; i < n; ++i) {
    char16_t c = d[i];
    if (c >= 'A' && c <= 'Z') c += 0x20;
    if (c != (unsigned char)aLowerCaseLiteral[i]) return false;
  }
  return true;
}

bool nsHtml5String::Equals(nsHtml5String aOther) const {
  uint32_t n = Length();
  return n == aOther.Length() && std::memcmp(Data(), aOther.Data(), n * sizeof(char16_t)) == 0;
}

nsHtml5String nsHtml5String::Clone() {
  if (GetKind() == eBuffer) AsBuffer()->mRefCount.fetch_add(1);
  return nsHtml5String(mBits);
}

void nsHtml5String::Release() {
  if (GetKind() == eBuffer) {
    Buffer* b = AsBuffer();
    if (b->mRefCount.fetch_sub(1) == 1) std::free(b);
  }
  mBits = eNull;
}

nsHtml5String nsHtml5String::FromBuffer(char16_t* aBuffer, int32_t aLength, nsHtml5TreeBuilder*) {
  if (!aLength) return nsHtml5String(uintptr_t(eEmpty));
  Buffer* b = AllocBuffer((uint32_t)aLength);
  std::memcpy(b->Chars(), aBuffer, aLength * sizeof(char16_t));
  b->Chars()[aLength] = 0;
  return nsHtml5String(reinterpret_cast<uintptr_t>(b) | eBuffer);
}

nsHtml5String nsHtml5String::FromLiteral(const char* aLiteral) {
  size_t n = std::strlen(aLiteral);
  if (!n) return nsHtml5String(uintptr_t(eEmpty));
  Buffer* b = AllocBuffer((uint32_t)n);
  for (size_t i = 0; i < n; ++i) b->Chars()[i] = (unsigned char)aLiteral[i];
  b->Chars()[n] = 0;
  return nsHtml5String(reinterpret_cast<uintptr_t>(b) | eBuffer);
}

nsHtml5String nsHtml5String::FromString(const nsAString& aString) {
  if (aString.empty()) return nsHtml5String(uintptr_t(eEmpty));
  Buffer* b = AllocBuffer((uint32_t)aString.size());
  std::memcpy(b->Chars(), aString.data(), aString.size() * sizeof(char16_t));
  b->Chars()[aString.size()] = 0;
  return nsHtml5String(reinterpret_cast<uintptr_t>(b) | eBuffer);
}

nsHtml5String nsHtml5String::FromAtom(already_AddRefed<nsAtom> aAtom) {
  return nsHtml5String(reinterpret_cast<uintptr_t>(aAtom.take()) | eAtom);
}

nsHtml5String nsHtml5String::FromStaticAtom(nsStaticAtom* aAtom) {
  return nsHtml5String(reinterpret_cast<uintptr_t>(aAtom) | eAtom);
}
