// The C++ side of mrt-text. See mrt_text.h. This is the only code written here in C++.
#include "mrt_text.h"

#include "hb.h"
#include "hb-ot.h"

extern "C" {

// Gecko: gfx/thebes/gfxHarfBuzzShaper.cpp gfxHarfBuzzShaper::CreateHBFont
hb_font_t* mrt_create_hb_font(hb_face_t* aFace, double aSize) {
  hb_font_t* result = hb_font_create(aFace);
  hb_font_set_ppem(result, aSize, aSize);
  uint32_t scale = uint32_t(65536 * aSize);  // 16.16 fixed-point
  hb_font_set_scale(result, scale, scale);
  return result;
}

// Gecko: gfx/thebes/gfxHarfBuzzShaper.cpp gfxHarfBuzzShaper::ShapeText
// NOT PORTED: feature merging (MergeFontFeatures), CJK kerning rules, vertical text, small-caps; script
// is guessed from the text (Firefox gets it from its own itemizer).
int32_t mrt_shape_utf16(hb_font_t* aFont, const uint16_t* aText, uint32_t aLength, int aRightToLeft,
                        const char* aLanguage, mrt_glyph* aOut, uint32_t aCapacity) {
  hb_buffer_t* buffer = hb_buffer_create();
  hb_buffer_set_cluster_level(buffer, HB_BUFFER_CLUSTER_LEVEL_MONOTONE_CHARACTERS);
  hb_buffer_set_direction(buffer, aRightToLeft ? HB_DIRECTION_RTL : HB_DIRECTION_LTR);
  if (aLanguage) {
    hb_buffer_set_language(buffer, hb_language_from_string(aLanguage, -1));
  }
  hb_buffer_add_utf16(buffer, aText, aLength, 0, aLength);
  hb_buffer_guess_segment_properties(buffer);  // fills in the script; keeps direction and language
  hb_shape(aFont, buffer, nullptr, 0);
  if (aRightToLeft) {
    hb_buffer_reverse(buffer);
  }
  unsigned int n = 0;
  const hb_glyph_info_t* info = hb_buffer_get_glyph_infos(buffer, &n);
  const hb_glyph_position_t* pos = hb_buffer_get_glyph_positions(buffer, nullptr);
  if (!pos && n) {
    hb_buffer_destroy(buffer);
    return -1;
  }
  for (unsigned int i = 0; i < n && i < aCapacity; ++i) {
    aOut[i] = mrt_glyph{info[i].codepoint, info[i].cluster, pos[i].x_advance,
                        pos[i].y_advance,  pos[i].x_offset, pos[i].y_offset};
  }
  hb_buffer_destroy(buffer);
  return int32_t(n);
}

const char* mrt_hb_version(void) { return hb_version_string(); }

}  // extern "C"
