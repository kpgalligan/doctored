# Pattern tiles: native samples (ADR)

- **Status:** Accepted, built 2026-11-15. Open: a CMYK pattern record in a PSD
  ([#48](https://github.com/kpgalligan/doctored/issues/48)), below.
- **Code:** `core/src/pattern_samples.rs` (KERNEL: the kinds, the codec, the RGB and ink texels, the
  turn permutation), `pattern_define.rs` (Define Pattern's samples), `pattern_tile.rs` (the field, the
  levels, the extended read), `style_pattern_fill.rs` (the JSON), `pattern_samples_native.rs` (`.doctored`
  word 115, the style-body cap, the CMYK ink map), `psd_patterns.rs` (16-bit `Pat2` arrays); export
  `dr_clipboard_payload_pattern_native` (`ffi_style.rs`). Host: `PatternOverlay.swift` (`PatternNative`),
  `PatternLibrary.swift` (schema 2), `DefinePattern.swift`. Tests `core/tests/pattern_samples.rs`,
  `core/tests/psd_pattern_samples.rs`; drive `scripts/verify_define_pattern.py`.

## Context

A pattern tile was straight RGBA8, at most 512 px a side, in every depth and colour mode, so Edit ▸ Define
Pattern… lost what the source held beyond 8-bit RGB: a CMYK separation (a K-only black came back as the
profile's four-colour black, 1226/65535 ink per plate), 16-bit codes (rounded to 8 bits) and float light
(8-bit linear, clipped above white, 0.00188 linear).

## Decisions in force

1. **The form.** A tile keeps its RGBA8 raster and may also hold NATIVE samples, `PatternTile::samples`:
   `rgb16` (R, G, B, A as `u16`, the document's codes), `rgb32` (R, G, B, A as `f32`, linear, colour finite
   within ±65504, alpha 0…1) or `cmyk16` (C, M, Y, K, A as `u16`, 65535 = full ink). The raster stays on every
   tile: it is the swatch and what every other document reads. No tile without samples changes.
2. **Who reads what.** A document reads samples only of a kind it stores, verbatim; nothing converts one
   kind into another:

   | Reading document | `rgb16` | `rgb32` | `cmyk16` | none |
   |---|---|---|---|---|
   | RGB, Indexed, 8/16 bits | code / 65535 | value clamped to 0…1 | raster | raster |
   | RGB 32-bit float | code / 65535 | value, above white kept | raster | raster |
   | Gray (8/16/32) | as RGB while every texel is neutral, else greyed from the raster | same | raster | as before |
   | CMYK | raster → ink, as before | same | ink, verbatim | as before |
   | Lab, Bitmap's Custom Pattern, a bevel texture's height | raster | raster | raster | raster |

   The clamp-free read is `PatternSampler::linear`, set by the three float readers (the style composite,
   Fill…, a pattern fill layer), and only on a level built from `rgb32` samples (`LevelOf::extended`).
   `cmyk16` ink is not snapped to an 8-bit document's grid: that document's own paint stores codes off it
   too, and snapping moved a defined pattern off its source by up to 100/65535.
3. **Define Pattern** (`dr_clipboard_payload_pattern_native` over the Copy Merged payload): a CMYK composite
   always keeps its ink; a 16-bit RGB or Gray composite keeps `rgb16` unless every code is a multiple of 257;
   a float composite keeps `rgb32` unless every component reads back from its raster code exactly; Lab keeps
   none. The float samples are held to the stored range (NaN as 0, ±65504, alpha 0…1), a guard: no payload
   can hold anything else. So a pattern defined from 8-bit-representable pixels is the tile it always was.
4. **The JSON.** A custom pattern object may hold `"native": {"kind", "data"}`, `data` the standard base64
   of the samples' little-endian components; written only when present, refused in both strictness modes
   when malformed (kind, length, range, a stray key).
5. **`.doctored`: conditional word 115** at both depths, no table: some stored pattern holds samples (a
   layer's or a comp's style — overlay, stroke `pattern_fill`, bevel texture — or a Pattern Fill
   description), checked both ways before any pixel decodes. A typed `LSTY` body whose style holds samples
   may reach 16 MiB (the byte format's own style cap) in a file that claims 115; every other body keeps
   2 MiB. A typed request carrying a tile may pass 2 MiB only when it holds a `"native"` key, up to 16 MiB
   beside its envelope: one 512² `rgb32` tile is 7.0 MB of base64. The comps payload stays 8 MiB, so one
   such appearance fits a comp and a second refuses by the comps sentence.
6. **Convert to Profile**, a converting layers paste and the comps' twin map a `cmyk16` tile's ink through
   the conversion's leg, as every style ink; Assign keeps it. A CMYK document of another profile reads a
   library tile's ink verbatim, as it reads a style's ink slot. CMYK → RGB and RGB → Gray leave or drop
   samples as the table says (Gray's greying drops them).
7. **Turns.** A quarter turn or flip turns the samples with the raster: the one pixel turn
   (`ops::rotate*`, `ops::flip_*`) is run over a raster of texel indices, which then permutes the samples.
8. **The library and the exchange file.** A library pattern without samples is written as before,
   `schema_version` 1; one with them is `schema_version` 2 with a `native` key; a file is at most 7 MiB.
   The `.doctoredpatterns` file is `doctored_patterns` 2 when some entry is schema 2, else 1; a build reads
   1 and 2 and refuses a higher number by name. An older build lists a schema-2 library file as a broken
   `unknown_version` row and refuses a `doctored_patterns` 2 file, so nothing it could open changes. An export
   past the 96 MiB file bound refuses by name. MCP: `define_pattern` and `list_patterns` report `samples`
   (`rgba8` or the kind); every pattern argument takes `native`.
9. **PSD.** A 16-bit `Patt`/`Pat2` array (0…32768) whose sample is not the stored image of an 8-bit code
   keeps its own codes as `rgb16`, round(min(v, 32768) · 65535 / 32768); Photoshop's own P16, made from 8-bit
   codes, reads with none. Under `Pat2` a tile with `rgb16` samples writes 16-bit raw arrays, each code
   stored round(c · 32768 / 65535): the format keeps a code within one, and a first import's codes are fixed
   points. `rgb32` and `cmyk16` tiles write their raster (no recorded PSD form). Whether Photoshop opens
   Doctored's own 16-bit record is a hand check for the owner.

## Open

- **CMYK `Patt` records (mode 4)** ([#48](https://github.com/kpgalligan/doctored/issues/48)): read and
  written. No record on disk shows their channel order or ink polarity, so the reader declines them by name
  and a CMYK export writes the raster's RGB record. It needs a Photoshop take: a CMYK document with a
  Pattern Fill layer of a K-only tile, saved as PSD.
