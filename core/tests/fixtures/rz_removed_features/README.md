# rz_removed_features

Native `.rz` files written by commit `3e05aea6`, the last build with smart
objects, Stack Mode, live filters and non-RAW retained sources. Each `<name>.rz`
was saved by that build's own writer (byte files through `rz_doc_save_native`,
typed files through `PrecisionDocument::encode_native`), reopened, and the
reopened layers' stored pixels were written beside it as `<name>.pixels`: a u32
layer count, then per layer bottom first a u32 width, u32 height, i32 x, i32 y
and the samples (u8 RGBA for a byte file, u16 LE RGBA for a typed one).
`removed_feature_files.rs` reads them.

| File | What it holds | Leading words |
|---|---|---|
| `smart_object_{byte,typed}` | one layer converted to a smart object | 45 65 / 46 66 |
| `float_contents_{byte,typed}` | a layered 32-bit file placed as float contents | 45 73 72 65 / 46 68 67 66 |
| `stack_mode_{byte,typed}` | a three-member smart object at Median | 45 69 65 / 46 70 66 |
| `live_filter_placed_{byte,typed}` | a placed layer with a Gaussian + High Pass stack | 45 23 / 46 24 |
| `live_filter_masked_{byte,typed}` | a placed layer with one masked Gaussian | 45 23 / 46 24 |
| `live_filter_raw_{byte,typed}` | a RAW layer with a Gaussian live filter | 45 23 / 46 24 |
| `linked_placed_{byte,typed}` | a linked placed layer | 45 16 / 46 17 |
| `placed_{byte,typed}` | an embedded placed layer | 45 16 / 46 17 |
| `raw_{byte,typed}` | a RAW layer and nothing removed | 45 18 / 46 19 |
