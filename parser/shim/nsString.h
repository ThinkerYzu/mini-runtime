// Firefox's nsString family reduced to what the parser core touches: a UTF-16 std::u16string
// with the few Firefox-style accessors the generated code calls.
#pragma once
#include <string>
class nsAString : public std::u16string {
 public:
  using std::u16string::u16string;
  nsAString() = default;
  nsAString(const std::u16string& s) : std::u16string(s) {}
  uint32_t Length() const { return (uint32_t)size(); }
  const char16_t* BeginReading() const { return data(); }
};
using nsString = nsAString;
using nsAutoString = nsAString;
inline nsAString nsDependentSubstring(const char16_t* aBegin, const char16_t* aEnd) { return nsAString(aBegin, aEnd - aBegin); }
