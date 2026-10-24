/* The C interface between Firefox's HarfBuzz and the Rust code in mrt-text. Everything else Rust needs
 * from HarfBuzz (blobs, faces, tables, cmap and advance lookups) it calls directly. */
#ifndef MRT_TEXT_H
#define MRT_TEXT_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct hb_font_t hb_font_t;
typedef struct hb_face_t hb_face_t;

/* One shaped glyph. Positions are 16.16 fixed-point CSS pixels, as in gfxHarfBuzzShaper. */
typedef struct {
  uint32_t glyph;
  uint32_t cluster; /* index of the first UTF-16 unit of the cluster */
  int32_t x_advance;
  int32_t y_advance;
  int32_t x_offset;
  int32_t y_offset;
} mrt_glyph;

/* gfxHarfBuzzShaper::CreateHBFont: ppem and 16.16 scale both set to the font size in CSS px. */
hb_font_t* mrt_create_hb_font(hb_face_t* face, double size);

/* The core of gfxHarfBuzzShaper::ShapeText for horizontal text: one UTF-16 run, default features
 * plus the given ones (aFeatures may be null), cluster level MONOTONE_CHARACTERS. Returns the number
 * of glyphs (visual order reversed back to logical order for RTL, like Firefox), or -1 on failure. If
 * the result is larger than aCapacity only aCapacity glyphs are written. aLanguage may be null. */
int32_t mrt_shape_utf16(hb_font_t* aFont, const uint16_t* aText, uint32_t aLength, int aRightToLeft,
                        const char* aLanguage, mrt_glyph* aOut, uint32_t aCapacity);

/* Version string of the vendored HarfBuzz. */
const char* mrt_hb_version(void);

#ifdef __cplusplus
}
#endif
#endif
