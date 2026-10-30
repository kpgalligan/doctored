//! Photoshop's Pattern Fill layer record `PtFl`, Solid Color Fill layer
//! record `SoCo` and Gradient Fill layer record `GdFl` (module doc's
//! Gradient), read and written: a layer record whose adjustment block is
//! `PtFl`, its tile in the file's `Patt`/`Pat2` table (`psd_patterns`) named
//! by `Nm  ` and `Idnt`, carried as a Doctored pattern fill layer; or `SoCo`,
//! carried as a Doctored solid color fill layer (module doc's Solid Color).
//! Both share one record walk, one form ([`Form`]), one admission and one
//! written record shape.
//!
//! # The block
//!
//! `u32 16` then a descriptor (`psd_descriptor`) of class [`CLASS`] holding
//! the pattern fill keys `psd_pattern_fill::fill_items` writes and
//! `read_fill` reads: `Ptrn` {`Nm  `, `Idnt`}, `Angl`, `Scl `, the link key
//! [`LINK`] (absent = linked) and `phase`; Photoshop writes only the keys
//! that differ from their defaults. The Pattern Fill take1 read `Algn` as
//! the link and class `null` on every record. Its `fxrp` is the layer's
//! reference point, the place a linked tile's origin follows (origin =
//! phase + round(fxrp); unlinked, phase).
//!
//! # Photoshop's form
//!
//! Photoshop 26.10 writes the record WITH pixels (the Pattern Fill smoke,
//! controller ruling 1): its rectangle is the mask's, its colour channels
//! are its own rendering of the fill, a `-2` mask channel follows, and flags
//! bit 4 ("pixel data irrelevant to the appearance of the document") is set
//! beside bit 3. A reader that ignores `PtFl` still sees the fill. Its
//! 16-bit record (the Pattern Fill take1's P16) is pixel-less, its tile
//! 16-bit in `Pat2` (`psd_patterns`).
//!
//! # Reading
//!
//! [`decode`] reads the keys with `read_fill` (no reference, so the `anchor`
//! it would add is dropped) and keeps them as the description stores them:
//! Strict `style_pattern_fill::parse_fill`, re-written by
//! `style_json_write::put_pattern_fill`, so the description holds exactly
//! the keys the host's strict decoder requires. A pattern the file does not
//! carry declines with [`MISSING`]. [`render`] is the layer's canvas raster
//! through `artwork_render::render` with one `pattern` component, whose
//! `anchor` is −reference when linked — the host's own drawing. The record's
//! own pixels are never decoded: bit 4 says they do not decide the
//! appearance, so the fill is re-rendered from its description and the `-2`
//! mask is kept as the fill layer's mask. Every form is read — pixel-less,
//! Photoshop's (bit 4) and pixels without bit 4 — and the report names it
//! ([`Form`]). Each is admitted as a canvas raster, never as its rectangle
//! (`GroupRecords::canvas_raster`).
//!
//! # Writing
//!
//! [`exported`] recognizes a version-1 RGB fill description of kind
//! `pattern` (the host's `FillLayerPayload`); [`encode`] is its block. The
//! record is written in Photoshop's form: the rendered fill's pixels over
//! [`record_rect`] ([`planes`]), its `-2` mask, flags `0x18`, its preserved
//! style, `fxrp` = the layer's reference always, and its tile in the
//! document's pattern table (`psd_patterns::tail_blocks_with`). The
//! pixel-less form is read but never written.
//!
//! # CMYK
//!
//! A CMYK file's `PtFl` takes the RGB form both ways: the description is the
//! same version-1 one (a pattern fill carries no ink; its texels separate at
//! render), [`render_cmyk`] draws it in the document's ink, the record's
//! planes are alpha then C, M, Y, K ([`planes`]), and its tile goes in the
//! table as an RGB pattern, as a CMYK Pattern Overlay's does.
//!
//! # Solid Color
//!
//! `SoCo`'s body is `u32 16` then a descriptor of class `null` holding one
//! key, `Clr `: an RGB file's `RGBC` {`Rd  ` `Grn ` `Bl  ` doub 0…255, or
//! `redFloat`… 0…1} or a CMYK file's `CMYC` {`Cyn ` `Mgnt` `Ylw ` `Blck`
//! doub percent}, read by `psd_layer_effects_fx`'s ONE `rgbc_of`/`cmyc_of`
//! and written by its `colour_item`. Photoshop stores the colour in the
//! DOCUMENT's space; Doctored stores a fill's colour as authored sRGB (ink
//! on CMYK). On import an `RGBC` converts out of the file's profile
//! (`authored_color::wide_out`: linear sRGB components, extended range) and
//! `color` is that colour's sRGB numbers; where a component leaves sRGB (by
//! more than [`GAMUT_EPSILON`]) the description also keeps `linear`, the
//! wide-colour carriage built for style colours, so a Display P3
//! green is not clipped. A `CMYC` is the ink as it is. On export the
//! description's colour goes back into the document's numbers
//! (`authored_color::wide_into` of `linear`, or of `color`'s linear light):
//! the byte grid in an 8-bit file, fractional numbers in a 16-bit one. A
//! carried `SoCo` takes the same forms, mask, admission and written record
//! as a `PtFl` (its pixels re-rendered from the description; `fxrp` is the
//! ordinary reference rule, not always written). An RGB `SoCo` renders in
//! the file's profile, so a profile that cannot be a high-depth destination
//! declines it ([`solid_refusal`]) and the record keeps today's handling.
//!
//! # Gradient
//!
//! `GdFl` is carried as a Doctored gradient fill layer through the same
//! walk, forms, mask and admission; every body question — its decode and
//! named losses, its description, its render component, its export parse and
//! encoded body, its report sentences — is `psd_gradient_fill`'s, and this
//! module only dispatches. An RGB gradient renders in the file's profile like
//! an RGB solid ([`FillKind::renders_in_profile`], so [`solid_refusal`]
//! declines it too) and its wide stops charge the encoder
//! ([`FillKind::is_wide`]). It is written in Photoshop's form: with pixels at
//! 8 bits, PIXEL-LESS at 16 ([`Exported::pixel_less`]), as the Solid Color
//! and Gradient Fill take1 found Photoshop writing it; `fxrp` takes the
//! ordinary rule.
//!
//! An aligned gradient (`Algn`, absent = true) spans its layer's
//! Align with Layer box, [`align_frame`]: the bounds of the mask's coverage
//! by `fill_align_box::align_box` (`MaskRecord::align_box`), the rule the
//! host's own render follows; each import site computes it from the record's
//! mask at its own sample width and hands it to [`render`] / [`render_cmyk`].
//! That box is a different rule from [`record_rect`], the written record's
//! pixel rectangle, which stays the stored mask rectangle.
//!
//! # 32 bits
//!
//! Photoshop 26.10.0's 32-bit `SoCo` (`fill_style32.psd`) is PIXEL-LESS: rect
//! 0, its channels empty Raw, flags `0x18`, and its `Clr ` numbers the
//! document's LINEAR light × 255 (`f32(c / 255) · 255` for an sRGB
//! document; `FxColours::Float`, `style_float::FloatStyle::descriptor_numbers`).
//! The 32-bit writer takes the 16-bit record walk with that form
//! ([`Exported::pixel_less`] at 32, [`Exported::written_at_32`]); the open
//! reads the numbers through the profile's linear twin (`FxColours::Rgb`) as
//! the same authored sRGB colour and draws it with [`render_float`].
//! Photoshop has shown no 32-bit `GdFl` or `PtFl` (nor a 32-bit pattern
//! table), so both refuse by name at 32 bits, in both directions.

use serde_json::{Map, Value};

use crate::authored_color::{srgb_linear, srgb_numbers, wide_out, LINEAR_LIMIT};
use crate::icc::IccProfile;
use crate::psd_descriptor::Descriptor;
use crate::psd_layer_effects_fx::{cmyc_of, colour_item, rgbc_of, FxColours};
use crate::psd_patterns::PatternTable;
use crate::style::PatternFill;

/// The record's adjustment key.
pub(crate) const KEY: [u8; 4] = *b"PtFl";

/// The record's kind in the import report.
pub(crate) const KIND: &str = "Pattern Fill";

/// A `PtFl` whose pattern is not in the file's table.
pub(crate) const MISSING: &str = "pattern fill names a pattern the file does not carry";

/// The Solid Color Fill record's adjustment key (module doc's Solid Color).
pub(crate) const SOLID_KEY: [u8; 4] = *b"SoCo";

/// A `SoCo` record's kind in the import report.
pub(crate) const SOLID_KIND: &str = "Solid Color Fill";

/// An RGB file's `SoCo` whose `Clr ` is not an `RGBC`.
pub(crate) const NOT_RGB: &str = "colour is not an RGB colour";

/// A CMYK file's `SoCo` whose `Clr ` is not a `CMYC`.
pub(crate) const NOT_INK: &str = "colour is not a CMYK colour";

/// An RGB `SoCo` in a file whose profile cannot be rendered into
/// ([`solid_refusal`]).
pub(crate) const NOT_RENDERABLE: &str = "colour cannot be rendered in the file's profile";

/// How far outside 0…1 a linear sRGB component may sit and still be in
/// sRGB's gamut: an in-gamut colour read out of a matrix profile lands up to
/// a few 1e-6 outside through the f32 matrix's round-off, and that must not
/// mint a wide colour (a deliberate out-of-gamut colour is 1e-3 or more off).
const GAMUT_EPSILON: f32 = 1e-5;

/// The block's Link with Layer key (module doc).
const LINK: [u8; 4] = *b"Algn";

/// The block descriptor's class (module doc).
const CLASS: &[u8] = b"null";

/// The form a carried `PtFl` record arrived in (module doc's Photoshop's
/// form); the import report names it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Form {
    /// No rectangle and no pixels.
    PixelLess,
    /// Pixels with flags bit 4 set: Photoshop's own form.
    Irrelevant,
    /// Pixels without bit 4.
    Unflagged,
}

impl Form {
    /// The report's clause after "(PtFl)": which form, and that its pixels
    /// are replaced by the re-rendered fill.
    pub(crate) fn clause(self) -> &'static str {
        match self {
            Form::PixelLess => " with no pixels",
            Form::Irrelevant => " in Photoshop's form, its own rendering marked irrelevant to the document's appearance (flags bit 4), which layered import discards and re-renders across the canvas from the fill's description",
            Form::Unflagged => " with pixels it does not mark irrelevant (no flags bit 4), which layered import discards and re-renders across the canvas from the fill's description",
        }
    }
}

/// What a carried fill record holds.
pub(crate) enum FillKind {
    /// A `PtFl`: the `pattern_fill` keys as the description stores them (no
    /// `anchor`); `linked` = its `link_with_layer`.
    Pattern {
        fill: Map<String, Value>,
        linked: bool,
    },
    /// An RGB file's `SoCo`: `color` authored sRGB in 0…1, `linear` its
    /// linear sRGB components when they leave sRGB (module doc's Solid
    /// Color).
    Solid {
        color: [f64; 3],
        linear: Option<[f32; 3]>,
    },
    /// A CMYK file's `SoCo`: straight ink in 0…1.
    SolidInk { ink: [f64; 4] },
    /// A `GdFl`, RGB or CMYK (`psd_gradient_fill`).
    Gradient(crate::psd_gradient_fill::GradientCarry),
}

impl FillKind {
    /// Whether the fill renders in the file's RGB profile (module doc's
    /// Solid Color and Gradient): an RGB solid, or a gradient without inks.
    pub(crate) fn renders_in_profile(&self) -> bool {
        match self {
            FillKind::Solid { .. } => true,
            FillKind::Gradient(carry) => carry.inks.is_none(),
            FillKind::Pattern { .. } | FillKind::SolidInk { .. } => false,
        }
    }

    /// Whether its render builds the high-depth encoder: a solid or a
    /// gradient holding a wide `linear` colour.
    pub(crate) fn is_wide(&self) -> bool {
        match self {
            FillKind::Solid { linear, .. } => linear.is_some(),
            FillKind::Gradient(carry) => carry.linear.is_some(),
            FillKind::Pattern { .. } | FillKind::SolidInk { .. } => false,
        }
    }
}

/// A decoded `PtFl`, `SoCo` or `GdFl`; `form` = the record's form, set by the
/// record walk.
pub(crate) struct CarriedFill {
    pub kind: FillKind,
    pub form: Form,
}

impl CarriedFill {
    /// The record's adjustment key.
    pub(crate) fn key(&self) -> [u8; 4] {
        match self.kind {
            FillKind::Pattern { .. } => KEY,
            FillKind::Solid { .. } | FillKind::SolidInk { .. } => SOLID_KEY,
            FillKind::Gradient(_) => crate::psd_gradient_fill::KEY,
        }
    }

    /// The record's kind in the import report.
    pub(crate) fn kind_name(&self) -> &'static str {
        kind_of(self.key())
    }
}

/// Whether `key` is a fill layer key this module carries.
pub(crate) fn is_key(key: [u8; 4]) -> bool {
    key == KEY || key == SOLID_KEY || key == crate::psd_gradient_fill::KEY
}

/// The report's kind for a carried fill key.
pub(crate) fn kind_of(key: [u8; 4]) -> &'static str {
    if key == SOLID_KEY {
        SOLID_KIND
    } else if key == crate::psd_gradient_fill::KEY {
        crate::psd_gradient_fill::KIND
    } else {
        KIND
    }
}

/// Why an RGB `SoCo` or `GdFl` cannot render in a file tagged `profile`: its render
/// encodes into the profile at high depth, which a profile whose tone curves
/// the parser repaired refuses (`MatrixTrc::validate_precision`). `None` for
/// no profile, sRGB, or a model-less (LUT) profile, which the render takes
/// without encoding and never refuses.
pub(crate) fn solid_refusal(profile: Option<&IccProfile>) -> Option<String> {
    let model = profile?.model()?;
    model
        .validate_precision()
        .err()
        .map(|e| format!("{NOT_RENDERABLE} ({e})"))
}

/// A `PtFl`, `SoCo` or `GdFl` body (`key`) as a carried fill (module doc's Reading
/// and Solid Color); `colours` is the file's colour crossing (an RGB file's
/// out-of-document conversion, or CMYK). `Err` is the decline reason, read
/// after "whose" in the import report.
pub(crate) fn decode(
    key: [u8; 4],
    body: &[u8],
    patterns: &PatternTable,
    colours: FxColours<'_>,
) -> Result<CarriedFill, String> {
    let d = crate::psd_descriptor::read(body)?;
    if key == SOLID_KEY || key == crate::psd_gradient_fill::KEY {
        return Ok(CarriedFill {
            kind: if key == SOLID_KEY {
                solid(&d, colours)?
            } else {
                FillKind::Gradient(crate::psd_gradient_fill::decode(&d, colours)?)
            },
            form: Form::PixelLess,
        });
    }
    let mut keys = crate::psd_pattern_fill::read_fill(&d, &LINK, patterns, None, (0, 0), MISSING)?;
    keys.remove("anchor");
    let parsed = crate::style_pattern_fill::parse_fill(
        &keys,
        "pattern_fill",
        crate::style::Strictness::Strict,
    )?;
    let mut fill = Map::new();
    crate::style_json_write::put_pattern_fill(
        &mut fill,
        &PatternFill {
            anchor: None,
            ..parsed
        },
    );
    let linked = fill
        .get("link_with_layer")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    Ok(CarriedFill {
        kind: FillKind::Pattern { fill, linked },
        form: Form::PixelLess,
    })
}

/// A `SoCo` descriptor's colour (module doc's Solid Color).
fn solid(d: &Descriptor, colours: FxColours<'_>) -> Result<FillKind, String> {
    let object = d.object(b"Clr ").ok().flatten();
    match colours {
        FxColours::Cmyk => {
            let ink = object
                .filter(|o| o.class == b"CMYC")
                .and_then(cmyc_of)
                .ok_or(NOT_INK)?;
            Ok(FillKind::SolidInk { ink })
        }
        FxColours::Rgb(colors) | FxColours::Rgb16(colors) => {
            let values = object
                .filter(|o| o.class == b"RGBC")
                .and_then(rgbc_of)
                .ok_or(NOT_RGB)?;
            let numbers = values.map(|v| (v / 255.0).clamp(0.0, 1.0) as f32);
            let linear = wide_out(colors, numbers);
            let color = srgb_numbers(linear).map(f64::from);
            let inside = -GAMUT_EPSILON..=1.0 + GAMUT_EPSILON;
            let wide = !linear.iter().all(|v| inside.contains(v));
            Ok(FillKind::Solid {
                color,
                linear: wide.then_some(linear),
            })
        }
        // A gray file's fills keep `FxColours::Rgb` (`GroupRecords::read`),
        // and its open omits every fill (#1 phase 3a).
        FxColours::Gray(_) => Err(NOT_RGB.to_string()),
        // …and a Lab file's are omitted too (#1 Lab phase 3a).
        FxColours::Lab => Err(NOT_RGB.to_string()),
        // Write-only; the 32-bit open reads through `Rgb` (module doc's 32 bits).
        FxColours::Float(_) => Err(NOT_RGB.to_string()),
    }
}

/// The layer's description: a pattern's
/// `{"type":"fill","version":1,"fill":"pattern","pattern_fill":{…},"canvas":[w,h]}`,
/// an RGB solid's `{…,"fill":"solid","color":[r,g,b,1]}` (plus `linear`
/// when wide) and a CMYK solid's version 2 `{…,"ink":"cmyk","color":
/// [c,m,y,k,1]}` (the host's `FillLayerPayload`), and a gradient's
/// `psd_gradient_fill::describe` (an inked one under `policy`, the CMYK
/// document's; it errs without one).
pub(crate) fn description(
    fill: &CarriedFill,
    canvas: (u32, u32),
    policy: Option<&crate::cmyk_document::CmykPolicy>,
) -> Result<String, String> {
    let mut o = Map::new();
    o.insert("canvas".into(), Value::from(vec![canvas.0, canvas.1]));
    o.insert("type".into(), Value::from("fill"));
    match &fill.kind {
        FillKind::Pattern { fill, .. } => {
            o.insert("fill".into(), Value::from("pattern"));
            o.insert("pattern_fill".into(), Value::Object(fill.clone()));
            o.insert("version".into(), Value::from(1));
        }
        FillKind::Solid { color, linear } => {
            o.insert("fill".into(), Value::from("solid"));
            o.insert(
                "color".into(),
                Value::from(vec![color[0], color[1], color[2], 1.0]),
            );
            if let Some(linear) = linear {
                o.insert("linear".into(), Value::from(linear.map(f64::from).to_vec()));
            }
            o.insert("version".into(), Value::from(1));
        }
        FillKind::SolidInk { ink } => {
            o.insert("fill".into(), Value::from("solid"));
            o.insert(
                "color".into(),
                Value::from(vec![ink[0], ink[1], ink[2], ink[3], 1.0]),
            );
            o.insert("ink".into(), Value::from("cmyk"));
            o.insert("version".into(), Value::from(2));
        }
        FillKind::Gradient(carry) => crate::psd_gradient_fill::describe(carry, &mut o, policy)?,
    }
    Ok(Value::Object(o).to_string())
}

/// A pattern's render component `fill`: the carried keys, whose anchor is
/// −`reference` when linked (module doc's Reading).
fn placed(keys: &Map<String, Value>, linked: bool, reference: (i32, i32)) -> Value {
    let mut keys = keys.clone();
    if linked {
        keys.insert(
            "anchor".into(),
            Value::from(vec![
                i64::from(reference.0).saturating_neg(),
                i64::from(reference.1).saturating_neg(),
            ]),
        );
    }
    Value::Object(keys)
}

/// A carried gradient's Align with Layer box (module doc's Gradient):
/// `Some(mask.align_box(canvas, sample_bytes))` iff `fill` is a gradient
/// whose carry is aligned, else `None` (the canvas). `sample_bytes` is the
/// width the site's own mask attach decodes the plane at. `Err` is the
/// mask plane's decode error.
pub(crate) fn align_frame(
    fill: &CarriedFill,
    mask: crate::psd_raster_masks::MaskRecord<'_>,
    canvas: (u32, u32),
    sample_bytes: usize,
) -> Result<Option<[i32; 4]>, String> {
    match &fill.kind {
        FillKind::Gradient(carry) if carry.fill.align_with_layer => {
            Ok(Some(mask.align_box(canvas, sample_bytes)?))
        }
        _ => Ok(None),
    }
}

/// The u16 canvas raster in `profile` (module doc's Reading): one `pattern`
/// component whose anchor is −`reference` when linked, one `fill`
/// component of a solid's colour (and `linear`), or a gradient's
/// `psd_gradient_fill::component` over `frame` ([`align_frame`]; `None` the
/// canvas). A CMYK solid or gradient errs.
pub(crate) fn render(
    fill: &CarriedFill,
    reference: (i32, i32),
    frame: Option<[i32; 4]>,
    canvas: (u32, u32),
    profile: std::sync::Arc<IccProfile>,
    job: crate::job_control::JobContext<'_>,
) -> Result<crate::precision_image::PrecisionImage, String> {
    let component = match &fill.kind {
        FillKind::Pattern { fill, linked } => crate::artwork_render::Component::Pattern {
            fill: placed(fill, *linked, reference),
        },
        FillKind::Solid { color, linear } => crate::artwork_render::Component::Fill {
            color: [color[0] as f32, color[1] as f32, color[2] as f32, 1.0],
            linear: *linear,
        },
        FillKind::Gradient(carry) => crate::psd_gradient_fill::component(carry, frame)?,
        FillKind::SolidInk { .. } => {
            return Err("a CMYK solid color fill has no RGB rendering".into())
        }
    };
    crate::artwork_render::render(canvas.0, canvas.1, &[component], &[], &[], profile, job)
}

/// The f32 canvas raster of a 32-bit float document in `profile` (module
/// doc's 32 bits): an RGB solid as one `fill` component of its linear sRGB
/// colour (`linear`, or `color`'s linear light), which
/// `artwork_render::render_float` carries through the sRGB → document
/// matrix — the host's own float drawing. Every other kind errs: it has no
/// 32-bit record.
pub(crate) fn render_float(
    fill: &CarriedFill,
    canvas: (u32, u32),
    profile: std::sync::Arc<IccProfile>,
    job: crate::job_control::JobContext<'_>,
) -> Result<crate::precision_image::PrecisionImage, String> {
    let FillKind::Solid { color, linear } = &fill.kind else {
        return Err(format!(
            "a {} layer has no 32-bit rendering",
            fill.kind_name()
        ));
    };
    let l = linear.unwrap_or_else(|| srgb_linear(color.map(|v| v as f32)));
    let component = crate::artwork_render::Component::Fill {
        color: [l[0], l[1], l[2], 1.0],
        linear: Some(l),
    };
    crate::artwork_render::render_float(canvas.0, canvas.1, &[component], &[], &[], profile, job)
}

/// [`render`] on a CMYK document: the canvas raster in `policy`'s ink
/// through `cmyk_artwork_render`'s `pattern` component, its texels
/// separated by the Pattern Overlay's leg (`cmyk_style::TexelInk`, built
/// only for a pattern), its `fill` component of a solid's ink, or a
/// gradient's `psd_gradient_fill::cmyk_component` over `frame` — the host's
/// own CMYK drawing. An RGB solid or gradient errs.
pub(crate) fn render_cmyk(
    fill: &CarriedFill,
    reference: (i32, i32),
    frame: Option<[i32; 4]>,
    canvas: (u32, u32),
    policy: &crate::cmyk_document::CmykPolicy,
    job: crate::job_control::JobContext<'_>,
) -> Result<crate::cmyk_image::CmykImage, String> {
    let (component, mut texel) = match &fill.kind {
        FillKind::Pattern { fill, linked } => (
            crate::cmyk_artwork_render::CmykComponent::Pattern {
                fill: placed(fill, *linked, reference),
                parsed: None,
            },
            Some(crate::cmyk_style::TexelInk::new(policy)?),
        ),
        FillKind::SolidInk { ink } => (
            crate::cmyk_artwork_render::CmykComponent::Fill {
                ink: [
                    ink[0] as f32,
                    ink[1] as f32,
                    ink[2] as f32,
                    ink[3] as f32,
                    1.0,
                ],
            },
            None,
        ),
        FillKind::Gradient(carry) => (
            crate::psd_gradient_fill::cmyk_component(carry, frame)?,
            None,
        ),
        FillKind::Solid { .. } => {
            return Err("an RGB solid color fill has no CMYK rendering".into())
        }
    };
    let samples = crate::cmyk_artwork_render::assemble_cmyk(
        canvas.0,
        canvas.1,
        &[component],
        &[],
        &[],
        &mut |_| [0.0; 4],
        &mut |rgb| texel.as_mut().map_or([0.0; 4], |texel| texel.ink(rgb)),
        job,
    )?;
    let row = canvas.0 as usize;
    crate::cmyk_image::CmykImage::from_rows_with_job(
        canvas.0,
        canvas.1,
        |y, out| {
            let at = y as usize * row;
            out.copy_from_slice(&samples[at..at + row]);
            Ok(())
        },
        job,
    )
}

/// A fill layer the export writes as its fill record.
pub(crate) enum Exported {
    /// A pattern fill, its anchor dropped (`PtFl`).
    Pattern(PatternFill),
    /// An RGB solid: authored sRGB `color` and its optional wide `linear`
    /// (`SoCo`).
    Solid {
        color: [f64; 3],
        linear: Option<[f32; 3]>,
    },
    /// A CMYK solid's straight ink (`SoCo`).
    SolidInk([f64; 4]),
    /// A gradient, RGB or CMYK (`GdFl`, `psd_gradient_fill::exported`).
    Gradient(crate::psd_gradient_fill::GradientCarry),
}

impl Exported {
    /// The pattern a `PtFl` carries in the document's pattern table.
    pub(crate) fn pattern(&self) -> Option<&PatternFill> {
        match self {
            Exported::Pattern(fill) => Some(fill),
            Exported::Solid { .. } | Exported::SolidInk(_) | Exported::Gradient(_) => None,
        }
    }

    /// Whether its record's colours build the high-depth encoder: a solid or
    /// a gradient holding a wide `linear` colour.
    pub(crate) fn is_wide(&self) -> bool {
        match self {
            Exported::Solid { linear, .. } => linear.is_some(),
            Exported::Gradient(carry) => carry.linear.is_some(),
            Exported::Pattern(_) | Exported::SolidInk(_) => false,
        }
    }

    /// Whether its record is written PIXEL-LESS at `depth` bits per sample
    /// (module doc's Gradient and 32 bits): never at 8, a gradient at 16,
    /// every fill at 32.
    pub(crate) fn pixel_less(&self, depth: u32) -> bool {
        match depth {
            8 => false,
            16 => matches!(self, Exported::Gradient(_)),
            _ => true,
        }
    }

    /// Whether a 32-bit file carries its record (module doc's 32 bits): an
    /// RGB solid only, the one fill Photoshop has shown at 32 bits.
    pub(crate) fn written_at_32(&self) -> bool {
        matches!(self, Exported::Solid { .. })
    }

    /// The fill's kind in a refusal sentence.
    pub(crate) fn kind_word(&self) -> &'static str {
        match self {
            Exported::Pattern(_) => "pattern",
            Exported::Solid { .. } | Exported::SolidInk(_) => "solid color",
            Exported::Gradient(_) => "gradient",
        }
    }
}

/// The fill a layer description holds, as the export writes it: a version-1
/// fill of kind `pattern` (`artwork_render::pattern_fill`, Strict, anchor
/// dropped) at either ink; on an RGB document (`cmyk` false) a version-1
/// `solid` with no `ink`, a `color` of four finite numbers in 0…1 whose
/// alpha is 1 and an optional `linear` of three finite numbers within
/// ±`LINEAR_LIMIT`; on a CMYK one a version-2 `solid` with `ink` "cmyk" and
/// five numbers in 0…1 whose alpha is 1; and a `gradient` at either ink
/// (`psd_gradient_fill::exported`). `None` for any other description,
/// which exports as its pixels with the description loss.
pub(crate) fn exported(description: &str, cmyk: bool) -> Option<Exported> {
    let root: Value = serde_json::from_str(description).ok()?;
    let root = root.as_object()?;
    if root.get("type")?.as_str()? != "fill" {
        return None;
    }
    let version = root.get("version")?.as_u64()?;
    let numbers = |key: &str| -> Option<Vec<f64>> {
        root.get(key)?
            .as_array()?
            .iter()
            .map(|v| v.as_f64().filter(|v| v.is_finite()))
            .collect()
    };
    let unit = |v: &f64| (0.0..=1.0).contains(v);
    match root.get("fill")?.as_str()? {
        "pattern" if version == 1 && !root.contains_key("ink") => {
            let fill = crate::artwork_render::pattern_fill(root.get("pattern_fill")?).ok()?;
            Some(Exported::Pattern(PatternFill {
                anchor: None,
                ..fill
            }))
        }
        "solid" if !cmyk && version == 1 && !root.contains_key("ink") => {
            let color = numbers("color")?;
            if color.len() != 4 || !color.iter().all(unit) || color[3] != 1.0 {
                return None;
            }
            let linear = match root.get("linear") {
                None => None,
                Some(_) => {
                    let linear = numbers("linear")?;
                    let limit = f64::from(LINEAR_LIMIT);
                    if linear.len() != 3 || !linear.iter().all(|v| v.abs() <= limit) {
                        return None;
                    }
                    Some([linear[0] as f32, linear[1] as f32, linear[2] as f32])
                }
            };
            Some(Exported::Solid {
                color: [color[0], color[1], color[2]],
                linear,
            })
        }
        "solid" if cmyk && version == 2 && root.get("ink")?.as_str()? == "cmyk" => {
            let color = numbers("color")?;
            if color.len() != 5 || !color.iter().all(unit) || color[4] != 1.0 {
                return None;
            }
            Some(Exported::SolidInk([color[0], color[1], color[2], color[3]]))
        }
        "gradient" => crate::psd_gradient_fill::exported(root, cmyk).map(Exported::Gradient),
        _ => None,
    }
}

/// The fill record's key and body (module doc's Block and Solid Color),
/// even-length: a pattern's `PtFl`, a solid's `SoCo` whose `Clr ` is the
/// document's numbers (`colours`: an RGB document's into-document
/// conversion, 8- or 16-bit, or CMYK ink), or a gradient's `GdFl`
/// (`psd_gradient_fill::items`, its stops in the same numbers).
pub(crate) fn encode(fill: &Exported, colours: FxColours<'_>) -> ([u8; 4], Vec<u8>) {
    let (key, items) = match fill {
        Exported::Pattern(fill) => (KEY, crate::psd_pattern_fill::fill_items(fill, &LINK)),
        Exported::Solid { color, linear } => {
            let linear = linear.unwrap_or_else(|| srgb_linear(color.map(|v| v as f32)));
            (
                SOLID_KEY,
                vec![(
                    b"Clr ".to_vec(),
                    colour_item([0; 3], None, Some(linear), colours),
                )],
            )
        }
        Exported::SolidInk(ink) => (
            SOLID_KEY,
            vec![(
                b"Clr ".to_vec(),
                colour_item([0; 3], Some(ink.map(|v| v as f32)), None, FxColours::Cmyk),
            )],
        ),
        Exported::Gradient(carry) => (
            crate::psd_gradient_fill::KEY,
            crate::psd_gradient_fill::items(carry, colours),
        ),
    };
    let mut out = crate::psd_descriptor::write(&Descriptor {
        class: CLASS.to_vec(),
        items,
    });
    if !out.len().is_multiple_of(2) {
        out.push(0);
    }
    (key, out)
}

/// The rectangle (top, left, bottom, right) a written fill record's pixels
/// cover (module doc's Photoshop's form): the mask's rectangle within the
/// canvas when the mask is enabled, hides everything outside it (outside
/// coverage 0) and meets the canvas; the whole canvas otherwise, since the
/// fill then shows outside any smaller rectangle. `mask` is the mask's
/// rectangle, outside coverage and enabled state. Not the Align with Layer
/// box ([`align_frame`], the mask's coverage bounds): a different rule.
pub(crate) fn record_rect(canvas: (u32, u32), mask: Option<([i32; 4], u8, bool)>) -> [i32; 4] {
    let whole = [
        0,
        0,
        i32::try_from(canvas.1).unwrap_or(i32::MAX),
        i32::try_from(canvas.0).unwrap_or(i32::MAX),
    ];
    match mask {
        Some(([top, left, bottom, right], 0, true)) => {
            let rect = [
                top.max(0),
                left.max(0),
                bottom.min(whole[2]),
                right.min(whole[3]),
            ];
            if rect[0] < rect[2] && rect[1] < rect[3] {
                rect
            } else {
                whole
            }
        }
        _ => whole,
    }
}

/// The alpha plane then each colour plane in order (PSD's channel ids −1,
/// 0, 1, …: alpha, R, G, B for RGBA samples; alpha, C, M, Y, K for CMYKA) of
/// `rect` (top, left, bottom, right) cut from a `size` raster at `offset`
/// whose row-major `samples` hold the colours then alpha; a pixel the raster
/// does not cover is `clear`. One walk of the samples and no copy of the
/// raster.
pub(crate) fn planes<T: Copy, const N: usize>(
    samples: impl Iterator<Item = [T; N]>,
    size: (u32, u32),
    offset: (i32, i32),
    rect: [i32; 4],
    clear: T,
) -> ([u32; 2], [Vec<T>; N]) {
    let [top, left, bottom, right] = rect;
    let (w, h) = (
        (i64::from(right) - i64::from(left)).max(0) as usize,
        (i64::from(bottom) - i64::from(top)).max(0) as usize,
    );
    let mut out: [Vec<T>; N] = std::array::from_fn(|_| vec![clear; w * h]);
    let width = size.0.max(1) as usize;
    for (i, sample) in samples.enumerate().take(width * size.1 as usize) {
        let x = (i % width) as i64 + i64::from(offset.0) - i64::from(left);
        let y = (i / width) as i64 + i64::from(offset.1) - i64::from(top);
        if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
            continue;
        }
        let at = y as usize * w + x as usize;
        for (plane, value) in out.iter_mut().zip(sample.iter().cycle().skip(N - 1)) {
            plane[at] = *value;
        }
    }
    ([w as u32, h as u32], out)
}

/// The `fxrp` body of a written pattern fill record: the layer's reference,
/// always (module doc's Writing).
pub(crate) fn reference_block(reference: (i32, i32)) -> ([u8; 4], Vec<u8>) {
    let mut body = Vec::with_capacity(16);
    body.extend_from_slice(&f64::from(reference.0).to_be_bytes());
    body.extend_from_slice(&f64::from(reference.1).to_be_bytes());
    (*b"fxrp", body)
}
