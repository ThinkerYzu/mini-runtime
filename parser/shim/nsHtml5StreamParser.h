// The tokenizer calls back into Firefox's stream parser to handle <meta charset>. Not needed: the input is UTF-16.
#pragma once
#include "nsHtml5String.h"
class nsHtml5StreamParser {
 public:
  bool internalEncodingDeclaration(nsHtml5String aEncoding) { return false; }
  bool TemplatePushedOrHeadPopped() { return false; }
  void RememberGt(int32_t aPos) {}
};
