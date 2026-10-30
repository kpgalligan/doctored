//! Adjustment layers: the ONE meta shape the core interprets. A layer whose
//! `meta` parses as an [`Adjustment`] is composited by `doc` as a
//! non-destructive color adjustment of the accumulated backdrop instead of as
//! pixels (the layer's own pixels are ignored). Anything that does not parse
//! composites as an ordinary raster layer — the same graceful degradation as
//! text meta. Applying an adjustment layer and running the matching
//! destructive filter produce the same image: for the eight original ops the
//! per-pixel math mirrors `ops` / `ops_filters` exactly, and for everything
//! since there is only ONE spelling — `dr_image_adjust_op` runs the code
//! below (see "One implementation" further down).
//!
//! # Meta schema (the contract the host codes against)
//!
//! ```json
//! {"type": "adjust", "op": "<op>", "params": { ... }}
//! ```
//!
//! `type` must be exactly the string `"adjust"` and `op` one of the
//! operation names below; `params` is an object and may be omitted entirely
//! when every parameter has a default. Unknown keys — top-level or inside
//! `params` — are ignored (room for host-side versioning). A missing optional
//! parameter takes its default; a parameter that is present but has the wrong
//! JSON type, or a value outside a REQUIRED range, makes the whole meta
//! malformed (`from_meta` returns `None` and the layer composites as a plain
//! raster layer). All numbers are JSON numbers (integers accepted wherever a
//! float is expected, and vice versa where noted).
//!
//! | op            | params (key: type, range, default)                       |
//! |---------------|----------------------------------------------------------|
//! | exposure / white_balance | `evaluation`: string, `"legacy_srgb"`       |
//! |               | (default for persisted descriptions) or `"document_v2"`. |
//! |               | The latter uses the document ICC matrix/TRC context;    |
//! |               | invalid values refuse. See the colour note below.       |
//! | `"bcs"`       | `brightness`, `contrast`, `saturation`: floats, each     |
//! |               | CLAMPED to [-1, 1] (out-of-range values are not an       |
//! |               | error), default 0 = identity. Applied in that order,     |
//! |               | mirroring `dr_image_adjust`. Its `saturation` is a SCALE |
//! |               | about the luma — deliberately a different curve from   |
//! |               | `vibrance`'s and `hue_saturation`'s Saturation sliders,  |
//! |               | and kept as it is because these semantics already ship.  |
//! |               | `legacy`: bool, default TRUE when absent (every          |
//! |               | description from before this key is legacy and keeps its |
//! |               | bytes). `false` selects the MODERN form: brightness is   |
//! |               | `x^g`, `g = 2^(-1.5·b)`, then contrast the S-curve       |
//! |               | `x < 0.5 ? 0.5·(2x)^k : 1 − 0.5·(2(1−x))^k`, `k =        |
//! |               | 2^(1.5·c)`, then the same luma saturation scale; each    |
//! |               | power stage skipped at 0. 0 and 1 stay fixed, the curve  |
//! |               | is monotonic with slope k at 0.5, and a value outside    |
//! |               | [0, 1] passes both power stages unchanged. The why is on |
//! |               | `adjust_mix::brightness_contrast_modern`.                |
//! | `"levels"`    | `black`: float, default 0; `white`: float, default 1;    |
//! |               | requires 0 <= black < white <= 1. `gamma`: float in      |
//! |               | [0.1, 10], default 1 (gamma > 1 brightens).              |
//! |               | `output_black`, `output_white`: floats in [0, 1],        |
//! |               | defaults 0 and 1, either order (black > white inverts).  |
//! |               | `r`, `g`, `b`: each an OPTIONAL object of the same five  |
//! |               | keys, ranges and defaults; absent = identity. The        |
//! |               | channel's stage applies FIRST, then the master's. See    |
//! |               | `adjust_levels`.                                         |
//! | `"hue_rotate"`| `degrees`: float, any finite angle, default 0. Mirrors   |
//! |               | `dr_image_hue_rotate` (SVG feColorMatrix hueRotate).     |
//! | `"threshold"` | `level`: float in [0, 1], default 0.5. Mirrors           |
//! |               | `dr_image_threshold` (Rec. 709 luma >= level).           |
//! | `"posterize"` | `levels`: integer in [2, 64], REQUIRED (a float with a   |
//! |               | zero fraction, e.g. `5.0`, is accepted). Photoshop's     |
//! |               | bins: at 8 bits `k = floor(c*n/256)` of the code `c`,    |
//! |               | output code `floor(255*k/(n-1))`; at 16 bits the code    |
//! |               | first takes Photoshop's 15-bit grid, output `k/(n-1)`.   |
//! |               | Mirrors `dr_image_posterize` (`adjust_math::posterize`). |
//! | `"curves"`    | `rgb`, `r`, `g`, `b`: each OPTIONAL, an array of 2..=16  |
//! |               | `[in, out]` pairs, both numbers in [0, 255] (values are  |
//! |               | defensively clamped into that range). A missing channel  |
//! |               | is the identity. OR the map form: `rgb_map`, `r_map`,    |
//! |               | `g_map`, `b_map`, each OPTIONAL, exactly 256 integers    |
//! |               | 0…255 (zero-fraction floats accepted; never clamped),    |
//! |               | entry i the output for input code i. Any map key makes   |
//! |               | the whole op map form; a point key beside one is         |
//! |               | malformed. See `adjust_curves`.                          |
//! | `"invert"`    | none                                                     |
//! | `"grayscale"` | none                                                     |
//! | `"sepia"`     | none                                                     |
//! | `"exposure"`  | `exposure`: float in [-20, 20] stops, default 0;         |
//! |               | `offset`: float in [-0.5, 0.5], default 0; `gamma`:      |
//! |               | float in [0.01, 9.99], default 1. Linearize (legacy:     |
//! |               | sRGB; `document_v2`: each channel at the profile's       |
//! |               | effective pure gamma, 2.2 for sRGB-shaped curves, #578), |
//! |               | scale by 2^exposure, add offset, raise to the RECIPROCAL |
//! |               | power 1/gamma — Photoshop's Gamma Correction, so gamma > |
//! |               | 1 brightens, the same direction as `levels`' midtone     |
//! |               | gamma (measured) — then re-encode. See                   |
//! |               | `adjust_tone` and `adjust_calibrated`.                   |
//! | `"vibrance"`  | `vibrance`: float in [-1, 1], default 0;                 |
//! |               | `saturation`: float in [-1, 1], default 0 (Photoshop's   |
//! |               | sliders / 100). Vibrance acts on the encoded values: a   |
//! |               | pull about an anchor between min and max (at the max     |
//! |               | for full chroma above 0) by a gain that above 0 falls    |
//! |               | with the pixel's HSV saturation and is damped near red   |
//! |               | (below 0 it is flat);                                    |
//! |               | then Saturation scales the distance from the ROMM Y-row  |
//! |               | anchor by 1 + s on the document profile's linear values  |
//! |               | (#578, `adjust_vibrance`). See `adjust_color`.           |
//! | `"hue_saturation"` | `hue`: float in [-180, 180] degrees, default 0;     |
//! |               | `saturation`, `lightness`: floats in [-1, 1], default 0. |
//! |               | `bands`: OPTIONAL object with any of `reds`, `yellows`,  |
//! |               | `greens`, `cyans`, `blues`, `magentas`, each an object   |
//! |               | `{hue: [-180,180] deg 0, saturation: [-1,1] 0,           |
//! |               | lightness: [-1,1] 0, center: [0,360) deg (60*index by    |
//! |               | default), inner: [0,180] deg 15, falloff: [0,180] deg    |
//! |               | 30}`. `colorize`: bool, default false; `colorize_hue`:   |
//! |               | [0, 360), default 0; `colorize_saturation`: [0, 1],      |
//! |               | default 0.25; `colorize_lightness`: [-1, 1], default 0.  |
//! |               | Photoshop's form (take4's R0): lightness, a hue          |
//! |               | rotation on a 1/24576-turn grid, netted band lightness,  |
//! |               | then a saturation gain about (max+min)/2 on a 1/256      |
//! |               | grid; a grey takes only lightness. In                    |
//! |               | `adjust_hue_saturation`.                                 |
//! | `"color_balance"` | `shadows`, `midtones`, `highlights`: each an         |
//! |               | OPTIONAL object `{cyan_red, magenta_green,              |
//! |               | yellow_blue}`, floats in [-1, 1], default 0, acting in   |
//! |               | whole Photoshop units (`round(100·v)`, as PSD `blnc`).   |
//! |               | `preserve_luminosity`: bool, default true — Photoshop's  |
//! |               | renormalisation of each tone's vector, not a luma hold.  |
//! |               | Per-channel Levels per `adjust_color_balance`.           |
//! | `"black_and_white"` | `reds` 0.4, `yellows` 0.6, `greens` 0.4,           |
//! |               | `cyans` 0.6, `blues` 0.2, `magentas` 0.8: floats in      |
//! |               | [-2, 3]. `tint`: bool, default false; `tint_color`:      |
//! |               | "#rrggbb" in the DOCUMENT's numbers, default "#998a66"   |
//! |               | (the sRGB spelling of hue 42 deg / sat 20%; the sheet    |
//! |               | writes the document's own spelling) — all three numbers  |
//! |               | are used: the tint's luminosity is set to the grey       |
//! |               | (SetLum with ClipColor). Greys are never                 |
//! |               | tinted by the weights.                                   |
//! |               | `"grayscale"` is this op's DEGENERATE case — the weights |
//! |               | (0.2126, 0.9278, 0.7152, 0.7874, 0.0722, 0.2848), the    |
//! |               | luma of each wheel colour — and stays a separate op only  |
//! |               | because its numbers are frozen and its arithmetic is in   |
//! |               | 0..255. See `adjust_mix`.                                |
//! | `"photo_filter"` | `color`: "#rrggbb" in the DOCUMENT's numbers,         |
//! |               | default "#ec8a00" (the sRGB spelling of Warming 85; the  |
//! |               | sheet writes the document's own spelling); `density`:    |
//! |               | float in [0, 1], default 0.25; `preserve_luminosity`:    |
//! |               | bool, default true. A density-weighted multiply in the   |
//! |               | profile's D50 PCS, encoded, then (preserve) Photoshop's  |
//! |               | Luminosity blend at 8/16 bits; the native basis with Y   |
//! |               | held at 32 (`adjust_photo_filter`, `adjust_float`).      |
//! | `"channel_mixer"` | `monochrome`: bool, default false. `red`, `green`,   |
//! |               | `blue`, `gray`: each an OPTIONAL object `{r, g, b,       |
//! |               | constant}`, floats in [-2, 2]; defaults are the identity |
//! |               | rows ({1,0,0,0}, {0,1,0,0}, {0,0,1,0}) and gray          |
//! |               | {0.4, 0.4, 0.2, 0}. Values are FRACTIONS — Photoshop's   |
//! |               | percentages over 100 — truncated to 1/1024 as Photoshop  |
//! |               | does (`adjust_mix`). Monochrome sends the gray row to    |
//! |               | all three outputs. Encoded values, no linearization.     |
//! | `"selective_color"` | `method`: "relative" (default) or "absolute".      |
//! |               | `reds`, `yellows`, `greens`, `cyans`, `blues`,           |
//! |               | `magentas`, `whites`, `neutrals`, `blacks`: each an      |
//! |               | OPTIONAL object `{c, m, y, k}`, floats in [-1, 1],       |
//! |               | default 0. CMYK round trip and band weights in           |
//! |               | `adjust_mix`'s module doc.                               |
//! | `"shadows_highlights"` | `shadows`, `highlights`: each an OPTIONAL      |
//! |               | object `{amount: [0, 1], tone: [0.01, 1]}`; defaults     |
//! |               | shadows {0.35, 0.5}, highlights {0, 0.5}. `radius`:      |
//! |               | float in [0, 1000] px, default 30 (ONE estimate plane    |
//! |               | serves both bands). `color`: float in [-1, 1], default   |
//! |               | 0.2; `midtone_contrast`: float in [-1, 1], default 0.    |
//! |               | The only SPATIAL op: its weight comes from an            |
//! |               | ALPHA-WEIGHTED large-radius blur of the luma             |
//! |               | ([`Adjustment::guide`]), so transparent pixels           |
//! |               | contribute nothing and a cut-out gets no halo. Read the  |
//! |               | spatial exception below. See `adjust_tone`.              |
//! | `"white_balance"` | `temperature`: float in [1667, 25000] kelvin,       |
//! |               | default 6504; `tint`: float in [-150, 150], default 0.   |
//! |               | The two describe the ILLUMINANT the pixels were shot     |
//! |               | under; the op Bradford-adapts THAT white to D65, so a    |
//! |               | HIGHER kelvin WARMS the image and a POSITIVE tint pushes |
//! |               | it toward MAGENTA (the tint steps `tint/3000` along the  |
//! |               | ISOTHERM — the CIE 1960 UCS locus NORMAL, not `+v` — so  |
//! |               | it moves green-magenta without moving the white's CCT).  |
//! |               | Legacy evaluation uses sRGB numeric processing; v2      |
//! |               | passes the document profile through the shared kernel.  |
//! |               | 6504/0 is the EXACT identity,                            |
//! |               | snapped, not a near-identity out of the CCT fit. See     |
//! |               | `adjust_white_balance`.                                  |
//! | `"gradient_map"` | `gradient`: the same object SHAPE a layer style       |
//! |               | carries (`style`'s schema table: `stops`, 2..=32 of      |
//! |               | {position 0..1, color, opacity}, plus `style`, `angle`,  |
//! |               | `scale`, `reverse`, `align_with_layer`; the SAME         |
//! |               | parser) — but its stop COLOURS are the DOCUMENT's        |
//! |               | numbers, not the authored sRGB a style's are: the one    |
//! |               | difference between two identical objects. A map uses     |
//! |               | only `stops` and `reverse`; the rest is accepted and     |
//! |               | ignored. `method`: absent (legacy, linear; midpoints     |
//! |               | and `smoothness` ignored) or `"classic"` (the            |
//! |               | Classic ramp at `gradient.smoothness` 0..1, default 1,   |
//! |               | bracket-bounded, each stop's `midpoint` /                |
//! |               | `opacity_midpoint` 0.05..0.95 on its own segment; out    |
//! |               | of range refuses) or `"perceptual"` (the same            |
//! |               | law over the stops' Oklab values; CMYK refuses it);      |
//! |               | any other value refuses. See                             |
//! |               | `adjust_map`. The pixel's Rec. 709 luma is t (1 - t when |
//! |               | `reverse`); a stop's OPACITY at t blends between the     |
//! |               | original colour (0) and the mapped one (1). `dither`:    |
//! |               | bool, default true — a deterministic +/-0.5/255 offset   |
//! |               | to t keyed on canvas position                            |
//! |               | (`blend::dissolve_threshold`), which is what stops an    |
//! |               | 8-bit map banding over a smooth sky. See `adjust_map`.   |
//! | `"color_lookup"` | `kind`: "1d" or "3d", REQUIRED. `size`: integer,     |
//! |               | REQUIRED — 2..=1024 for 1d, 2..=33 for 3d (this build's  |
//! |               | storage cap). `table`: REQUIRED base64 of size (or       |
//! |               | size^3) * 3 little-endian f32 in red-fastest order.      |
//! |               | `source_size`: OPTIONAL integer 2..=65536, default =     |
//! |               | `size` — the .cube's OWN size (a device link's own CLUT  |
//! |               | grid, which `size` equals up to 33), so a host can say   |
//! |               | "resampled from 64"; display only, never read by the     |
//! |               | math. `domain_min`, `domain_max`: arrays of 3 floats,    |
//! |               | defaults [0,0,0] and [1,1,1], each max strictly above    |
//! |               | its min. `strength`: float in [0, 1], default 1 (a lerp  |
//! |               | from the original). `title`: optional string, at most    |
//! |               | 128 chars, display only. Built by `dr_lut_parse_cube`;   |
//! |               | see `adjust_lut`. `abstract_profile`: optional string,   |
//! |               | the standard base64 of an ABSTRACT ICC look's own bytes  |
//! |               | (at most 4 MiB), written only by `dr_lut_parse_profile`  |
//! |               | / `dr_lut_bake_profile` for an abstract; never read by   |
//! |               | the math, it is what every profile write re-bakes the    |
//! |               | table through (`icc_lookup::rebake_meta`, DRDC 71).      |
//! | `"replace_color"` | `colors`: REQUIRED array of 1..=8 "#rrggbb" in the   |
//! |               | DOCUMENT's numbers. `fuzziness`: float in [0, 200],      |
//! |               | default 40. `hue`: float in [-180, 180], default 0;      |
//! |               | `saturation`, `lightness`: floats in [-1, 1], default 0  |
//! |               | — `hue_saturation`'s master sliders, run through the     |
//! |               | SAME `HueSaturation`. A pixel's weight is 1 within       |
//! |               | fuzziness/2 (Euclidean, 0…255 code units) of the nearest |
//! |               | colour, falls linearly to 0 at fuzziness, and lerps the  |
//! |               | pixel toward its Hue/Saturation edit. See                |
//! |               | `adjust_replace_color`.                                  |
//!
//! # Application
//!
//! [`Adjustment::apply_at`] maps one straight (non-premultiplied) RGB triple
//! in [0, 1] to another; alpha never enters and is NEVER modified by any
//! adjustment. The compositor's accumulator already holds straight color, so
//! no unpremultiply round-trip is needed there. A 32-bit float document does
//! not call it: each op there takes its own closed form on the stored linear
//! values, or refuses by name (`adjust_float`).
//!
//! Two arguments ride along with the triple, both there for ops that cannot
//! be written as a function of one pixel alone. `guide` is this pixel's
//! value in the plane [`Adjustment::guide`] built once for the whole image
//! or composite — the alpha-weighted blurred luma Shadows/Highlights weights
//! its lift by; every other op returns no plane and pays nothing for it.
//! `xy` is the pixel's position — the CANVAS position on the layer path and
//! the IMAGE position on the destructive one, the same convention
//! `blend::dissolve_threshold` already uses — which `gradient_map`'s dither
//! is keyed on.
//!
//! # One implementation, and its three exceptions
//!
//! There is exactly ONE destructive twin, `dr_image_adjust_op`, and it runs
//! `Adjustment::from_op` plus `adjust_math::apply_to_image` — the same code
//! the compositor runs — so a filter and an adjustment layer with the same
//! params can never drift. At full strength (opacity 1, no mask) the two
//! are BYTE-identical, not merely close: `doc::composite_adjustment_into`
//! takes the blended value VERBATIM there rather than lerping to it, since
//! `cb + (e - cb) * 1.0` is not the floating-point identity and a value on
//! an exact half-code (30.5/255) quantizes one step apart. Pinned by
//! `adjustment_tests::full_strength_adjustment_layer_is_byte_identical_over_every_level`
//! over all 65 536 level combinations, not just the parity table's 24.
//!
//! **The first exception**: every op is a pure function of the pixels
//! handed to it; for `shadows_highlights` — the only SPATIAL op — those
//! pixels differ between the filter (the layer's own image) and the layer
//! (the backdrop below it), so the two agree on the same input and are
//! deliberately different pictures on different inputs.
//!
//! **The second exception** is historical and sits outside this twin: the
//! eight ops that predate it keep their older single-purpose exports
//! (`dr_image_grayscale`, `dr_image_sepia`, `dr_image_threshold`, ...),
//! which the Image > Adjustments menu and `apply_filter` still call. Those
//! do their arithmetic in 0..255 rather than 0..1, so on a half-code they
//! can round one step away from the layer. Their numbers are frozen — every
//! existing oracle pins them — which is why the parity table allows +/-1
//! for those rows alone; run the SAME eight op names through
//! `dr_image_adjust_op` and they are byte-identical like the rest.
//!
//! **The third exception** is `gradient_map`'s DITHER, and only while it is
//! on (`dither`, default true). The jitter is keyed on the pixel's position
//! — that is what makes it a stable ordered dither rather than noise — and
//! the two paths hand `apply_at` positions in different frames, exactly as
//! the Application section above says: the layer sees the CANVAS position,
//! the filter sees the position within the image it was handed. On a layer
//! whose offset is (0, 0) those are the same frame and the two are
//! byte-identical as usual; on a layer at any other offset the same jitter
//! pattern lands on different pixels, so roughly half the pixels differ by
//! one code (more where two stops sit close together and one dither step
//! spans a whole ramp). Set `dither: false` and the two agree exactly at any
//! offset. Both halves are pinned by
//! `adjust_map_tests::gradient_map_dither_is_keyed_on_the_position_it_is_given`.
//! This is not fixable by giving the destructive export a canvas origin
//! without putting an origin argument on all twenty-one ops for the sake of
//! one, so it is documented instead — here, in the README, in
//! `adjust_math::apply_to_image` and in the `add_adjustment_layer` catalog
//! text, which is what an agent codes against.
//!
//! # Colour, and the space an adjustment's parameters live in
//!
//! Parameters containing colours are the DOCUMENT's numbers. Exposure and
//! White Balance also accept `params.evaluation`: absent or `"legacy_srgb"`
//! preserves the original sRGB arithmetic regardless of profile;
//! `"document_v2"` uses the actual document ICC matrix/TRC profile through
//! `adjust_calibrated`. Any other value makes the description malformed.
//! New UI/MCP RGB edits explicitly author `document_v2`; old descriptions and
//! old Actions retain legacy evaluation. Scalars keep legacy grayscale
//! semantics. A LUT-only RGB profile refuses calibrated processing, including
//! projection of a saved calibrated layer; native saving remains available.
//! The new evaluation is persisted in DRDC version 9 (unchanged byte layout).

use serde_json::{Map, Value};

use crate::adjust_calibrated::Calibrated;
use crate::adjust_color::Vibrance;
use crate::adjust_color_balance::ColorBalance;
use crate::adjust_curves::CurveLuts;
use crate::adjust_hue_saturation::HueSaturation;
use crate::adjust_levels::Levels;
use crate::adjust_lut::{self, CubeLut, MEMO_MIN_META_LEN};
use crate::adjust_map::GradientMap;
use crate::adjust_math::{luma, posterize};
use crate::adjust_mix::{
    brightness_contrast_modern, brightness_contrast_saturation, BlackAndWhite, ChannelMixer,
    PhotoFilter, SelectiveColor,
};
use crate::adjust_parse::boolean;
use crate::adjust_photo_filter::BoundPhotoFilter;
use crate::adjust_replace_color::ReplaceColor;
use crate::adjust_tone::{tone_plane_with_job, Exposure, GuidePlane, ShadowsHighlights};
use crate::adjust_vibrance::BoundVibrance;
use crate::adjust_white_balance::WhiteBalance;
use crate::job_control::JobContext;
use crate::ops_filters::hue_rotate_matrix;

/// One parsed color adjustment. Derived values (the contrast slope, the hue
/// matrix, curve LUTs, the white-balance matrix) are computed at parse time
/// so [`Adjustment::apply_at`] is cheap per pixel.
///
/// `Clone` is what lets `adjust_lut`'s memo hand back a parsed adjustment
/// without re-parsing: a `color_lookup`'s table is behind an `Arc`, so the
/// clone is a refcount bump rather than 431 KB of floats.
#[derive(Clone)]
pub(crate) enum Adjustment {
    /// Version-2 colour evaluation, bound to the document only at render time.
    Calibrated(Calibrated),
    /// Brightness/contrast/saturation, mirroring `ops::adjust`: the stored
    /// values are the pre-clamped brightness and the derived slope/scale.
    Bcs {
        brightness: f32,
        contrast_slope: f32,
        saturation_scale: f32,
    },
    /// The modern (`legacy: false`) Brightness/Contrast: the brightness
    /// power `gamma`, the contrast S-curve's power and the shared saturation
    /// scale (`adjust_mix::brightness_contrast_modern`).
    BcsModern {
        gamma: f32,
        contrast_power: f32,
        saturation_scale: f32,
    },
    /// Levels: a master stage, per-channel stages and output ranges
    /// (`adjust_levels`); boxed for its size.
    Levels(Box<Levels>),
    /// Hue rotation, mirroring `ops_filters::hue_rotate`.
    HueRotate { matrix: [[f32; 3]; 3] },
    /// Luma threshold, mirroring `ops_filters::threshold`.
    Threshold { level: f32 },
    /// Posterize to `levels` values (`adjust_math::posterize`) at the depth
    /// whose code range is `top`: 255 as parsed, 65535 once
    /// `PrecisionAdjustment::parse` prepares it for a 16-bit document.
    Posterize { levels: u32, top: u32 },
    /// Per-channel curves through cached LUTs.
    Curves(Box<CurveLuts>),
    /// Channel inversion, mirroring `ops::invert_pixel`.
    Invert,
    /// Rec. 709 grayscale, mirroring `ops::grayscale`.
    Grayscale,
    /// Sepia matrix, mirroring `ops::sepia`.
    Sepia,
    /// Light-linear gain, pedestal and gamma (`adjust_tone`).
    Exposure(Exposure),
    /// The Vibrance and Saturation sliders on the document
    /// profile's linear values (`adjust_vibrance`, kernel `adjust_color`);
    /// parsed bound to builtin sRGB, rebound by `in_profile` /
    /// `PrecisionAdjustment::parse`.
    Vibrance(BoundVibrance),
    /// Master and six-band Hue/Saturation, or Colorize
    /// (`adjust_hue_saturation`); boxed because
    /// its six band structs are the widest payload of the set.
    HueSaturation(Box<HueSaturation>),
    /// Per-tone-band channel shifts (`adjust_color`).
    ColorBalance(Box<ColorBalance>),
    /// Six hue weights to a grey, optionally tinted (`adjust_mix`).
    BlackAndWhite(Box<BlackAndWhite>),
    /// A density-weighted colour multiply in the document profile's D50
    /// PCS (`adjust_photo_filter`); parsed bound to builtin sRGB, rebound by
    /// `in_profile` / `PrecisionAdjustment::parse`.
    PhotoFilter(BoundPhotoFilter),
    /// One 3x4 matrix over the encoded values (`adjust_mix`).
    ChannelMixer(ChannelMixer),
    /// Nine ranges of CMYK ink nudges (`adjust_mix`); boxed for its size.
    SelectiveColor(Box<SelectiveColor>),
    /// The one SPATIAL op (`adjust_tone`); boxed because its payload is the
    /// largest of the set and every other variant would grow to match.
    ShadowsHighlights(Box<ShadowsHighlights>),
    /// Illuminant-to-D65 Bradford adaptation (`adjust_white_balance`).
    WhiteBalance(WhiteBalance),
    /// Luma through a multi-stop gradient (`adjust_map`); boxed because the
    /// stop list is a `Vec` behind a fill struct.
    GradientMap(Box<GradientMap>),
    /// A parsed .cube lookup table (`adjust_lut`); boxed for the same
    /// reason as the curve LUTs.
    ColorLookup(Box<CubeLut>),
    /// Hue/Saturation weighted by distance to sampled colours
    /// (`adjust_replace_color`); boxed for its embedded `HueSaturation`.
    ReplaceColor(Box<ReplaceColor>),
}

/// The classic sepia matrix (`ops::sepia`), rows red, green, blue. Its one
/// home: the 8/16-bit kernel clamps its rows at 1, a 32-bit float document
/// applies it to linear values with no clamp (`adjust_float`).
pub(crate) const SEPIA: [[f32; 3]; 3] = [
    [0.393, 0.769, 0.189],
    [0.349, 0.686, 0.168],
    [0.272, 0.534, 0.131],
];

/// Reads `params[key]` as a number, with `default` when the key is absent.
/// A present non-number value is malformed (`None`). JSON cannot encode NaN
/// or infinities, so the result is always finite.
pub(crate) fn num(params: &Map<String, Value>, key: &str, default: f64) -> Option<f64> {
    match params.get(key) {
        None => Some(default),
        Some(v) => v.as_f64(),
    }
}

impl Adjustment {
    /// Parses a layer's meta as an adjustment description per the module-level
    /// schema. `None` for anything else — non-JSON, a different `type`, an
    /// unknown `op`, or invalid params — in which case the layer composites
    /// as an ordinary raster layer.
    ///
    /// This runs on EVERY `composite_layer_into` call, which is why a meta
    /// bigger than [`MEMO_MIN_META_LEN`] goes through `adjust_lut`'s
    /// `MEMO_CAPACITY`-entry LRU memo instead of re-scanning a
    /// half-megabyte of JSON per render, export, thumbnail and brush dab.
    /// That capacity is a floor, not a budget — a memo smaller than the
    /// number of Color Lookup layers one composite walks evicts exactly the
    /// entry the next lookup wants, and its own doc says so. The memo is
    /// transparent — same input, same output — so nothing else changes.
    pub(crate) fn from_meta(meta: &str) -> Option<Adjustment> {
        if meta.len() > MEMO_MIN_META_LEN {
            return adjust_lut::memoized(meta, Adjustment::parse_meta);
        }
        Adjustment::parse_meta(meta)
    }

    /// [`Adjustment::from_meta`] without the memo — the actual parse.
    fn parse_meta(meta: &str) -> Option<Adjustment> {
        let root: Value = serde_json::from_str(meta).ok()?;
        let obj = root.as_object()?;
        if obj.get("type")?.as_str()? != "adjust" {
            return None;
        }
        let op = obj.get("op")?.as_str()?;
        let empty = Map::new();
        let params = match obj.get("params") {
            None => &empty,
            Some(v) => v.as_object()?,
        };
        Adjustment::from_op(op, params)
    }

    /// The op name plus its params object, without the meta envelope — the
    /// entry point the ONE destructive export (`dr_image_adjust_op`) uses,
    /// so a filter and an adjustment layer run identical code.
    pub(crate) fn from_op(op: &str, params: &Map<String, Value>) -> Option<Adjustment> {
        if matches!(op, "exposure" | "white_balance") {
            match params.get("evaluation") {
                None => {}
                Some(Value::String(value)) if value == "legacy_srgb" => {}
                Some(Value::String(value)) if value == "document_v2" => {
                    return Calibrated::parse(op, params).map(Adjustment::Calibrated);
                }
                _ => return None,
            }
        }
        match op {
            "bcs" => {
                // Mirrors `ops::adjust`: each control clamps to [-1, 1]
                // rather than refusing, and the slope/scale floors at 0.
                let brightness = num(params, "brightness", 0.0)? as f32;
                let contrast = num(params, "contrast", 0.0)? as f32;
                let saturation = num(params, "saturation", 0.0)? as f32;
                // Absent = legacy: every description written before the
                // `legacy` key keeps its bytes. The modern exponents are
                // 2^(∓1.5·slider), so the full range spans a 2.83× power
                // either way (the why is on `brightness_contrast_modern`).
                if !boolean(params, "legacy", true)? {
                    return Some(Adjustment::BcsModern {
                        gamma: (-1.5 * brightness.clamp(-1.0, 1.0)).exp2(),
                        contrast_power: (1.5 * contrast.clamp(-1.0, 1.0)).exp2(),
                        saturation_scale: (1.0 + saturation.clamp(-1.0, 1.0)).max(0.0),
                    });
                }
                Some(Adjustment::Bcs {
                    brightness: brightness.clamp(-1.0, 1.0),
                    contrast_slope: (1.0 + contrast.clamp(-1.0, 1.0)).max(0.0),
                    saturation_scale: (1.0 + saturation.clamp(-1.0, 1.0)).max(0.0),
                })
            }
            "levels" => Levels::parse(params).map(|levels| Adjustment::Levels(Box::new(levels))),
            "hue_rotate" => {
                let degrees = num(params, "degrees", 0.0)? as f32;
                // JSON numbers are finite, but the f64 -> f32 narrowing can
                // overflow to infinity; refuse like `ops_filters::hue_rotate`.
                if !degrees.is_finite() {
                    return None;
                }
                Some(Adjustment::HueRotate {
                    matrix: hue_rotate_matrix(degrees),
                })
            }
            "threshold" => {
                let level = num(params, "level", 0.5)? as f32;
                if !(0.0..=1.0).contains(&level) {
                    return None;
                }
                Some(Adjustment::Threshold { level })
            }
            "posterize" => {
                // Required; integral floats (e.g. 5.0) are accepted, the
                // range is `ops_filters::posterize`'s 2..=64.
                let levels = params.get("levels")?.as_f64()?;
                if levels.fract() != 0.0 || !(2.0..=64.0).contains(&levels) {
                    return None;
                }
                Some(Adjustment::Posterize {
                    levels: levels as u32,
                    top: 255,
                })
            }
            "curves" => CurveLuts::parse(params).map(|luts| Adjustment::Curves(Box::new(luts))),
            "invert" => Some(Adjustment::Invert),
            "grayscale" => Some(Adjustment::Grayscale),
            "sepia" => Some(Adjustment::Sepia),
            "exposure" => Exposure::parse(params).map(Adjustment::Exposure),
            "vibrance" => {
                Vibrance::parse(params).map(|v| Adjustment::Vibrance(BoundVibrance::srgb(v)))
            }
            "hue_saturation" => {
                HueSaturation::parse(params).map(|hs| Adjustment::HueSaturation(Box::new(hs)))
            }
            "color_balance" => {
                ColorBalance::parse(params).map(|cb| Adjustment::ColorBalance(Box::new(cb)))
            }
            "black_and_white" => {
                BlackAndWhite::parse(params).map(|bw| Adjustment::BlackAndWhite(Box::new(bw)))
            }
            "photo_filter" => PhotoFilter::parse(params)
                .map(|pf| Adjustment::PhotoFilter(BoundPhotoFilter::srgb(pf))),
            "channel_mixer" => ChannelMixer::parse(params).map(Adjustment::ChannelMixer),
            "selective_color" => {
                SelectiveColor::parse(params).map(|sc| Adjustment::SelectiveColor(Box::new(sc)))
            }
            "shadows_highlights" => ShadowsHighlights::parse(params)
                .map(|sh| Adjustment::ShadowsHighlights(Box::new(sh))),
            "white_balance" => WhiteBalance::parse(params).map(Adjustment::WhiteBalance),
            "gradient_map" => {
                GradientMap::parse(params).map(|map| Adjustment::GradientMap(Box::new(map)))
            }
            "color_lookup" => {
                CubeLut::parse(params).map(|lut| Adjustment::ColorLookup(Box::new(lut)))
            }
            "replace_color" => {
                ReplaceColor::parse(params).map(|rc| Adjustment::ReplaceColor(Box::new(rc)))
            }
            _ => None,
        }
    }

    /// The `op` string `from_op` parsed: one arm per variant, the inverse of
    /// `from_op`'s match; the `Calibrated` arm returns `op.op_name()`.
    pub(crate) fn op_name(&self) -> &'static str {
        match self {
            Adjustment::Calibrated(op) => op.op_name(),
            Adjustment::Bcs { .. } | Adjustment::BcsModern { .. } => "bcs",
            Adjustment::Levels(_) => "levels",
            Adjustment::HueRotate { .. } => "hue_rotate",
            Adjustment::Threshold { .. } => "threshold",
            Adjustment::Posterize { .. } => "posterize",
            Adjustment::Curves(_) => "curves",
            Adjustment::Invert => "invert",
            Adjustment::Grayscale => "grayscale",
            Adjustment::Sepia => "sepia",
            Adjustment::Exposure(_) => "exposure",
            Adjustment::Vibrance(_) => "vibrance",
            Adjustment::HueSaturation(_) => "hue_saturation",
            Adjustment::ColorBalance(_) => "color_balance",
            Adjustment::BlackAndWhite(_) => "black_and_white",
            Adjustment::PhotoFilter(_) => "photo_filter",
            Adjustment::ChannelMixer(_) => "channel_mixer",
            Adjustment::SelectiveColor(_) => "selective_color",
            Adjustment::ShadowsHighlights(_) => "shadows_highlights",
            Adjustment::WhiteBalance(_) => "white_balance",
            Adjustment::GradientMap(_) => "gradient_map",
            Adjustment::ColorLookup(_) => "color_lookup",
            Adjustment::ReplaceColor(_) => "replace_color",
        }
    }

    /// The GUIDE plane an adjustment reads a neighbourhood through, built
    /// ONCE per image or per composite: Shadows/Highlights' large-radius,
    /// ALPHA-WEIGHTED blur of the luma (`adjust_tone::tone_plane`, which
    /// routes through THE plane blur in `style_render`). `None` for every
    /// other op, which is why they cost nothing here.
    ///
    /// `rgba_at(i)` reads the STRAIGHT rgba of pixel `i` of a `w` x `h`
    /// row-major buffer; alpha is what keeps a transparent surround from
    /// dragging the estimate to black and haloing every cut-out.
    ///
    /// On the layer path the buffer is the compositor's ACCUMULATOR, which
    /// is a window (merge-down composites into a union extent), so a merged
    /// Shadows/Highlights layer can differ from the full-canvas one near the
    /// window's edge — the same class of edge effect every blur has.
    ///
    /// There is deliberately no plane cache: the plane is a function of the
    /// BACKDROP, which a live stroke on a layer underneath changes on every
    /// tick, so anything keyed more cheaply than the backdrop's contents
    /// would hand back a stale guide. What is bounded instead is the cost of
    /// building it — [`adjust_tone::GuidePlane`] holds it at the resolution
    /// a blur at that radius actually carries, and Shadows/Highlights whose
    /// two amounts are zero never reads it at all and so builds nothing.
    pub(crate) fn guide(
        &self,
        w: u32,
        h: u32,
        rgba_at: &dyn Fn(usize) -> [f32; 4],
    ) -> Option<GuidePlane> {
        match self {
            Adjustment::ShadowsHighlights(sh) => sh.guide_plane(w, h, rgba_at),
            _ => None,
        }
    }

    /// [`Adjustment::guide`] polling `job` (`adjust_tone::tone_plane_with_job`,
    /// the same byte build): the regional compositor's staged guide (Phase
    /// 13A-4a). `Ok(None)` exactly where `guide` is `None`.
    pub(crate) fn guide_with_job(
        &self,
        w: u32,
        h: u32,
        rgba_at: &dyn Fn(usize) -> [f32; 4],
        job: JobContext<'_>,
    ) -> Result<Option<GuidePlane>, String> {
        match self.guide_radius() {
            Some(radius) => tone_plane_with_job(w, h, radius, rgba_at, job).map(Some),
            None => Ok(None),
        }
    }

    /// The radius of the guide this op reads — `Some` exactly when
    /// [`Adjustment::is_pointwise`] is false (a Shadows/Highlights whose guide
    /// is read), for the regional compositor's halo and plane geometry.
    pub(crate) fn guide_radius(&self) -> Option<f32> {
        match self {
            Adjustment::ShadowsHighlights(op) if op.reads_guide() => Some(op.radius()),
            _ => None,
        }
    }

    /// Whether one output pixel depends on its own backdrop pixel ALONE — the
    /// regional compositor's admission question (`regional`). Every op is
    /// pointwise except Shadows/Highlights whose guide is read
    /// (`ShadowsHighlights::reads_guide`): that one weights by
    /// a large-radius blur of the backdrop, a spatial neighbourhood.
    pub(crate) fn is_pointwise(&self) -> bool {
        !matches!(self, Adjustment::ShadowsHighlights(op) if op.reads_guide())
    }

    /// Applies the adjustment to one straight (non-premultiplied) RGB triple
    /// in [0, 1], returning a triple clamped back into [0, 1]. Alpha never
    /// enters: no adjustment reads or modifies it. The math mirrors the
    /// matching destructive filter per component (worked in normalized [0, 1]
    /// space, so the two can differ by at most one 8-bit rounding step).
    ///
    /// `guide` is this pixel's value in the plane [`Adjustment::guide`]
    /// built for the whole image, and is read ONLY by an op whose `guide`
    /// returned `Some` — every caller passes 0.0 for the others, which
    /// ignore it. `xy` is the pixel's position, read only by
    /// `gradient_map`'s dither; it is the CANVAS position on the layer path
    /// and the IMAGE position on the destructive one, the same convention
    /// `blend::dissolve_threshold` already uses for Dissolve.
    pub(crate) fn apply_at(&self, rgb: [f32; 3], guide: f32, xy: (i64, i64)) -> [f32; 3] {
        let rgb = [
            rgb[0].clamp(0.0, 1.0),
            rgb[1].clamp(0.0, 1.0),
            rgb[2].clamp(0.0, 1.0),
        ];
        let out = match self {
            Adjustment::Calibrated(op) => op.apply(rgb),
            // `ops::adjust`: brightness, then contrast about 0.5, then
            // luma-anchored saturation.
            Adjustment::Bcs {
                brightness,
                contrast_slope,
                saturation_scale,
            } => {
                brightness_contrast_saturation(rgb, *brightness, *contrast_slope, *saturation_scale)
            }
            // `adjust_mix`: brightness power, contrast S-curve, then the same
            // luma-anchored saturation.
            Adjustment::BcsModern {
                gamma,
                contrast_power,
                saturation_scale,
            } => brightness_contrast_modern(rgb, *gamma, *contrast_power, *saturation_scale),
            // `adjust_levels`: each channel's stage, then the master's.
            Adjustment::Levels(levels) => levels.apply(rgb),
            // `ops_filters::hue_rotate`: one matrix multiply.
            Adjustment::HueRotate { matrix } => {
                matrix.map(|row| row[0] * rgb[0] + row[1] * rgb[1] + row[2] * rgb[2])
            }
            // `ops_filters::threshold`: Rec. 709 luma against the level.
            Adjustment::Threshold { level } => {
                let v = if luma(rgb) >= *level { 1.0 } else { 0.0 };
                [v, v, v]
            }
            // `adjust_math::posterize`: Photoshop's bins at this depth.
            Adjustment::Posterize { levels, top } => rgb.map(|v| posterize(v, *levels, *top)),
            // `adjust_curves`: the per-channel table, then the master one.
            Adjustment::Curves(luts) => luts.apply(rgb),
            // Floating-point twin of the byte kernel `ops::invert_pixel`.
            Adjustment::Invert => rgb.map(|v| 1.0 - v),
            // `ops::grayscale`: Rec. 709 luma on every channel.
            Adjustment::Grayscale => {
                let v = luma(rgb);
                [v, v, v]
            }
            // `ops::sepia`: the classic sepia matrix, clamped high side only
            // (the coefficients are all non-negative).
            Adjustment::Sepia => {
                SEPIA.map(|row| (row[0] * rgb[0] + row[1] * rgb[1] + row[2] * rgb[2]).min(1.0))
            }
            // `adjust_tone`: linearize, gain, offset, gamma, re-encode.
            Adjustment::Exposure(e) => e.apply(rgb),
            // `adjust_vibrance`: decode, Vibrance then Saturation, encode.
            Adjustment::Vibrance(v) => v.apply(rgb),
            // `adjust_hue_saturation`: Photoshop's R0 form.
            Adjustment::HueSaturation(hs) => hs.apply(rgb),
            // `adjust_color_balance`: one Levels curve per channel.
            Adjustment::ColorBalance(cb) => cb.apply(rgb),
            // `adjust_mix`: the hue decomposition weighted to a grey.
            Adjustment::BlackAndWhite(bw) => bw.apply(rgb),
            // `adjust_photo_filter`: the multiply in D50 PCS, encode, then
            // the Luminosity blend.
            Adjustment::PhotoFilter(pf) => pf.apply(rgb),
            // `adjust_mix`: one matrix multiply on the encoded values.
            Adjustment::ChannelMixer(cm) => cm.apply(rgb),
            // `adjust_mix`: the CMYK round trip with per-range nudges.
            Adjustment::SelectiveColor(sc) => sc.apply(rgb),
            // `adjust_tone`: the one op that reads the guide plane.
            Adjustment::ShadowsHighlights(sh) => sh.apply(rgb, guide),
            // `adjust_white_balance`: one linear-RGB matrix.
            Adjustment::WhiteBalance(wb) => wb.apply(rgb),
            // `adjust_map`: the ONE op that reads the pixel's position.
            Adjustment::GradientMap(map) => map.apply_at(rgb, xy),
            // `adjust_lut`: tetrahedral (or linear) table lookup.
            Adjustment::ColorLookup(lut) => lut.apply(rgb),
            // `adjust_replace_color`: the colour-distance weighted HSL edit.
            Adjustment::ReplaceColor(rc) => rc.apply(rgb),
        };
        out.map(|v| v.clamp(0.0, 1.0))
    }
}
