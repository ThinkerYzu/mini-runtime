#include <mutex>
#include <shared_mutex>
#include <string>
#include <string_view>
#include <unordered_map>
#include "nsGkAtoms.h"

namespace mrt_detail {
// The strings, in the same order as the atoms.
static const char16_t* const kStrings[] = {
#define GK_ATOM(name, str) u"" str,
#include "nsGkAtomList.h"
#undef GK_ATOM
};
static const uint32_t kLengths[] = {
#define GK_ATOM(name, str) (uint32_t)(sizeof(u"" str) / sizeof(char16_t) - 1),
#include "nsGkAtomList.h"
#undef GK_ATOM
};
nsAtom gStaticAtoms[kAtomCount];
}  // namespace mrt_detail

namespace {
struct Table {
  std::unordered_map<std::u16string_view, nsAtom*> map;
  Table() {
    using namespace mrt_detail;
    for (uint32_t i = 0; i < kAtomCount; ++i) {
      gStaticAtoms[i] = nsAtom{kStrings[i], kLengths[i], 0, true};
      map.emplace(std::u16string_view(kStrings[i], kLengths[i]), &gStaticAtoms[i]);
    }
  }
};
Table& table() {
  static Table t;
  return t;
}
}  // namespace

// Parsers on several threads share the table: lookups take the shared lock, adding a name takes the
// exclusive one. (The static atoms never change once built.)
static std::shared_mutex gTableLock;

nsAtom* mrt_atomize(std::u16string_view s) {
  Table& t = table();  // built on first use; function-local statics are initialised thread-safely
  {
    std::shared_lock<std::shared_mutex> read(gTableLock);
    auto it = t.map.find(s);
    if (it != t.map.end()) return it->second;
  }
  std::unique_lock<std::shared_mutex> write(gTableLock);
  auto it = t.map.find(s);  // another thread may have added it meanwhile
  if (it != t.map.end()) return it->second;
  // A name Firefox has no static atom for: keep a copy for the life of the process.
  char16_t* copy = new char16_t[s.size() + 1];
  for (size_t i = 0; i < s.size(); ++i) copy[i] = s[i];
  copy[s.size()] = 0;
  nsAtom* a = new nsAtom{copy, (uint32_t)s.size(), 0, false};
  t.map.emplace(std::u16string_view(copy, s.size()), a);
  return a;
}
