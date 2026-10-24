// A sink that builds a small tree and prints it, to check the parser runs.
#include <chrono>
#include <cstdio>
#include <string>
#include <vector>
#include "mrt_parser.h"

struct N { int kind; std::string name; std::string text; uint32_t parent = 0; std::vector<uint32_t> kids; std::vector<std::pair<std::string,std::string>> attrs; };
static std::vector<N> nodes(2);  // 0 unused, 1 document
static std::string u8(const char16_t* s, uint32_t n) { std::string o; for (uint32_t i = 0; i < n; ++i) o += s[i] < 128 ? (char)s[i] : '?'; return o; }
static uint32_t mk(int kind, std::string name, std::string text = "") { nodes.push_back(N{kind, name, text}); return nodes.size() - 1; }
static void detach(uint32_t c) { N& n = nodes[c]; if (!n.parent) return; auto& k = nodes[n.parent].kids; for (size_t i = 0; i < k.size(); ++i) if (k[i] == c) { k.erase(k.begin() + i); break; } n.parent = 0; }
static void app(uint32_t c, uint32_t p) { detach(c); nodes[c].parent = p; nodes[p].kids.push_back(c); }
static void attrs(uint32_t id, const MrtAttr* a, uint32_t n) { for (uint32_t i = 0; i < n; ++i) nodes[id].attrs.push_back({u8(a[i].name, a[i].name_len), u8(a[i].value, a[i].value_len)}); }
static uint32_t par(uint32_t p) { return p ? p : 1; }
static void dump(uint32_t id, int d) {
  N& n = nodes[id];
  std::string pad(d * 2, ' ');
  if (n.kind == 0) std::printf("%s#document\n", pad.c_str());
  else if (n.kind == 1) { std::printf("%s<%s", pad.c_str(), n.name.c_str()); for (auto& a : n.attrs) std::printf(" %s=\"%s\"", a.first.c_str(), a.second.c_str()); std::printf(">\n"); }
  else if (n.kind == 2) std::printf("%s\"%s\"\n", pad.c_str(), n.text.c_str());
  else if (n.kind == 3) std::printf("%s<!--%s-->\n", pad.c_str(), n.text.c_str());
  else std::printf("%s<!DOCTYPE %s>\n", pad.c_str(), n.name.c_str());
  for (uint32_t k : n.kids) dump(k, d + 1);
}
int main(int argc, char** argv) {
  nodes[1] = N{0, ""};
  MrtSink s{};
  s.create_element = [](void*, int32_t, const char16_t* nm, uint32_t nl, const MrtAttr* a, uint32_t na, uint32_t) { uint32_t i = mk(1, u8(nm, nl)); attrs(i, a, na); return i; };
  s.create_root = [](void*, const char16_t* nm, uint32_t nl, const MrtAttr* a, uint32_t na) { uint32_t i = mk(1, u8(nm, nl)); attrs(i, a, na); app(i, 1); return i; };
  s.append_child = [](void*, uint32_t c, uint32_t p) { app(c, par(p)); };
  s.append_children_to_new_parent = [](void*, uint32_t o, uint32_t n) { auto k = nodes[o].kids; for (uint32_t c : k) app(c, n); };
  s.detach_from_parent = [](void*, uint32_t c) { detach(c); };
  s.insert_foster_parented = [](void*, uint32_t c, uint32_t t, uint32_t sp) { if (nodes[t].parent) { uint32_t p = nodes[t].parent; detach(c); auto& k = nodes[p].kids; for (size_t i = 0; i < k.size(); ++i) if (k[i] == t) { k.insert(k.begin() + i, c); break; } nodes[c].parent = p; } else app(c, sp); };
  s.append_text = [](void*, uint32_t p, const char16_t* t, uint32_t n) { app(mk(2, "", u8(t, n)), par(p)); };
  s.insert_foster_text = [](void*, const char16_t* t, uint32_t n, uint32_t tb, uint32_t sp) { uint32_t c = mk(2, "", u8(t, n)); if (nodes[tb].parent) { uint32_t p = nodes[tb].parent; auto& k = nodes[p].kids; for (size_t i = 0; i < k.size(); ++i) if (k[i] == tb) { k.insert(k.begin() + i, c); break; } nodes[c].parent = p; } else app(c, sp); };
  s.append_comment = [](void*, uint32_t p, const char16_t* t, uint32_t n) { app(mk(3, "", u8(t, n)), par(p)); };
  s.add_attributes = [](void*, uint32_t id, const MrtAttr* a, uint32_t n) { attrs(id, a, n); };
  s.append_doctype = [](void*, const char16_t* nm, uint32_t nl, const char16_t*, uint32_t, const char16_t*, uint32_t) { app(mk(4, u8(nm, nl)), 1); };
  s.set_document_mode = [](void*, int32_t m) { std::printf("[mode %d]\n", m); };
  s.has_children = [](void*, uint32_t id) { return (int32_t)!nodes[id].kids.empty(); };
  s.template_contents = [](void*, uint32_t t) { return t; };
  if (argc > 1) {  // parse a file, report node count and time
    FILE* f = std::fopen(argv[1], "rb"); std::string bytes; char buf[65536]; size_t n;
    while ((n = std::fread(buf, 1, sizeof buf, f)) > 0) bytes.append(buf, n);
    std::fclose(f);
    std::u16string u16;  // UTF-8 to UTF-16
    for (size_t i = 0; i < bytes.size();) {
      unsigned char c = bytes[i]; uint32_t cp; int len;
      if (c < 0x80) { cp = c; len = 1; } else if (c < 0xE0) { cp = c & 0x1F; len = 2; } else if (c < 0xF0) { cp = c & 0x0F; len = 3; } else { cp = c & 0x07; len = 4; }
      for (int k = 1; k < len && i + k < bytes.size(); ++k) cp = (cp << 6) | (bytes[i + k] & 0x3F);
      i += len;
      if (cp >= 0x10000) { cp -= 0x10000; u16 += (char16_t)(0xD800 + (cp >> 10)); u16 += (char16_t)(0xDC00 + (cp & 0x3FF)); } else u16 += (char16_t)cp;
    }
    mrt_parser_init();
    auto t0 = std::chrono::steady_clock::now();
    MrtParser* p = mrt_parse_document(&s, u16.data(), (uint32_t)u16.size(), 0, 0);
    double ms = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - t0).count();
    size_t elements = 0, texts = 0; for (auto& nd : nodes) { elements += nd.kind == 1; texts += nd.kind == 2; }
    std::printf("%s: %zu bytes, %zu elements, %zu text nodes, %.0f ms\n", argv[1], bytes.size(), elements, texts, ms);
    mrt_parser_free(p);
    return 0;
  }
  const char* html = "<!DOCTYPE html><html><head><title>T</title></head><body class=a><p>Hello <b>wor<i>ld</b></i> &amp; &lt;done&gt;<table><tr><td>x</table><!-- c --><div id=x>y</div></body></html>";
  std::u16string u; for (const char* c = html; *c; ++c) u += (char16_t)*c;
  mrt_parser_init();
  MrtParser* p = mrt_parse_document(&s, u.data(), (uint32_t)u.size(), 0, 0);
  dump(1, 0);
  mrt_parser_free(p);
}
