// Interns strings to atoms for the tokenizer (element and attribute names it has not seen as static atoms).
#pragma once
#include "nsAtom.h"
#include "nsHtml5String.h"

class nsHtml5AtomTable {
 public:
  nsHtml5AtomTable() {}
  ~nsHtml5AtomTable() {}
  nsAtom* GetAtom(const nsAString& aKey) { return mrt_atomize(std::u16string_view(aKey.data(), aKey.size())); }
  nsAtom* GetAtomOrNull(const nsAString& aKey) { return GetAtom(aKey); }
};
