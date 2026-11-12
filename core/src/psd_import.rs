//! PSD import preflight and explicit layered/stored-composite choices, for
//! all three layered containers: the report's `"format"` is the container's
//! own name (`PSD`, `PSB` or `TIFF`). A little-endian 32-bit layered TIFF
//! is reported from its big-endian form
//! (`tiff_layers_order::big_endian_form`) under its own fingerprint. A
//! layered TIFF has no stored composite
//! here — `Container::composite` refuses it by name, so the report gives
//! `stored_composite_available: false` and mode 1 returns that sentence.
//! Reports separate parser-observed losses from general importer limitations;
//! the bounded folder-record walk adds verified group properties and losses.
//!
//! The byte importer and flat open still refuse 16-bit and name the typed path,
//! and refuse 32-bit naming the float path. A 32-bit RGB file reports
//! `import_precision` `"rgba32"`: its profile must pass
//! `psd_import_float::profile_refusal`, its stored composite is
//! `psd_import_float::composite32`, a carried Gradient or Pattern fill refuses
//! the layered choice (`psd_import_float::layered_refusal`) and its float
//! losses and observation come from `psd_import_float::float_report_lines`.
//!
//! A CMYK file (colour mode 4, #1 phase 3) runs through the SAME record walk
//! with its own admission (`Container::cmyk_layered_admission`), profile
//! policy (`cmyk_io::psd_policy`), stored composite (`cmyk_admission` then
//! `cmyk_composite`), general limitations and report lines
//! (`psd_import_cmyk`); the RGB-only pieces — the layer-style sentences, the
//! 16-bit typed losses and the sRGB and white-matte observations — are
//! skipped for it. Every mode's report names the comps it narrows
//! (`psd_comps::report_losses`).
//!
//! A Grayscale file (colour mode 1) at 8, 16 or 32 bits (#1 phases 3a and
//! 3c) reports `import_precision` `"gray"`: the same walk through `parse_with`
//! with a decode-only Gray policy (`psd_import_gray::gray_policy` under Dot
//! Gain 20 %, since the report takes no working Gray; an unusable profile is
//! the common refusal), its own general limitations and report lines
//! (`psd_import_gray`, given the policy's twin at 32 bits for the float
//! rules), and the RGB layer-style and typed-loss lines skipped. A carried
//! adjustment the Gray table refuses, and every fill, gets the omission loss.
//! A 32-bit gray file's stored composite is `psd_import_float::composite32`.
//!
//! A Lab file (colour mode 9) at 8 or 16 bits (#1 Lab phase 3a) reports
//! `import_precision` `"lab"`: the same walk through `parse_with` with no
//! profile (a Lab file's embedded profile, when present, is unused and
//! named), its own admission (`psd_import_lab::lab_admission`, in place of
//! `support_refusal`, which refuses Lab), stored composite
//! (`psd_import_lab::lab_composite`), `layered_plan_bytes`, general
//! limitations and report lines (`psd_import_lab`), and the RGB layer-style
//! and typed-loss lines skipped. A carried adjustment the Lab table refuses,
//! and every fill, gets the omission loss; `"profile"` is null.
//!
//! A Duotone file (colour mode 8) at 8 or 16 bits (#1 Duotone phase 1b)
//! reports as the Gray file it stores (`Container::duotone_gray`,
//! `import_precision` `"gray"`) with `"color_mode"` `"Duotone"`, a first
//! observation naming the inks and `"duotone"` the normalized options; a
//! specification `duotone_ado` cannot read refuses both choices.

use std::hash::{Hash, Hasher};
use std::panic::{catch_unwind, AssertUnwindSafe};

use serde_json::{json, Value};

use crate::authored_color::AuthoredColors;
use crate::doc::DrDocument;
use crate::doc_group::MAX_GROUP_DEPTH;
use crate::job_control::JobContext;
use crate::psd_adjustment::AdjustmentBody;
use crate::psd_container::Container;
use crate::psd_group_records::GroupRecords;
use crate::psd_layer_effects::{channel_list, knockout_clause, StyleBody};

const GENERAL_LIMITATIONS: &[&str] = &[
    "Layer styles arrive as Photoshop layer effects (lfx2, lfxs on a group, or lmfx for stacked instances: up to 10 each of Drop Shadow, Inner Shadow, Stroke, Color Overlay and Gradient Overlay), fill opacity (iOpa) and Blend If blending ranges, and are carried as one Doctored layer style whose parameters are mapped onto Doctored's own renderer; a Pattern Overlay (patternFill) and a Stroke filled with a pattern (FrFX PntT Ptrn) arrive with their pattern from the file's Patt table (8- or 16-bit RGB or Gray tiles up to 512 px); contours with their Anti-aliased (TrnS, MpgS), the glows' Range and the bevel's Contour sub-panel are carried too, and so is the shadows' and glows' Noise (Nose), the glows' gradient fill and Jitter (Grad, ShdN) and the bevel's Texture (useTexture, with its pattern from the same table); legacy effects (lrFX) and the glows' technique are not imported, and an effect Doctored cannot render is discarded with its reason. Text/shape descriptions, smart objects and other live Photoshop constructs are not imported as editable state. Photoshop Invert, Threshold, Posterize, Levels, Curves, Color Balance, Hue/Saturation, Photo Filter, Exposure, Vibrance, Black & White, Channel Mixer, Selective Color and Brightness/Contrast adjustment layers are carried as Doctored adjustment layers with their parameters mapped onto Doctored's own arithmetic, so their appearance can differ from Photoshop's; a Pattern Fill layer (PtFl) is carried as a Doctored pattern fill layer with its pattern from the same Patt/Pat2 table, a Solid Color Fill layer (SoCo) as a Doctored solid color fill layer and a Gradient Fill layer (GdFl) as a Doctored gradient fill layer; every other adjustment or fill layer carries no pixels and is omitted from Import Layers, reported per record; the stored composite keeps their appearance. Knockout (knko: shallow or deep) and Blend Interior Effects as Group (infx) are carried on that same layer style at both depths; clbl is not interpreted.",
    "Layer/group masks retain ordinary 20-byte -2 Raw/RLE/ZIP masks with default0/255 and enabled state. Mask rectangles are read as canvas coordinates; the PSD position-relative flag is recorded but not applied, matching Photoshop. Imported masks are independent (unlinked); PSD linkage, extended masks and density/feather parameters are not retained. A vector mask (vmsk) with no raster mask beside it, whose path neither starts from all pixels nor subtracts or intersects its subpaths, is retained as the layer's vector mask — a shape layer's as its fill layer under the path (at 32 bits a Solid Color one only; a Gradient or Pattern shape layer keeps its pixels), unless its fill does not decode or it has an enabled stroke — while the file's paths and vector masks fit the document's 65,536-anchor cap, and every other vector mask is not. The file's paths (the work path and each saved path) are retained on both import choices. Saved alpha channels are not retained; spot channels are, at 8 and 16 bits.",
    "Group folder opacity, visibility, open/closed state, clipping and recognized blend keys are retained; both depths evaluate clipping. Blending ranges restricting gray, red, green or blue are imported as Blend If, one range per restricted channel; ranges on alpha, advanced masks and other advanced Photoshop blending options are not.",
    "Layer locks (lspf) are carried: transparency, pixels, position and Lock All. A folder dissolved beyond the 10-level nesting limit adds its locks to each layer it held. Photoshop's artboard-nesting lock, a Background layer's implicit transparency and position locks and the legacy header protection flag are not. Guides, ruler origin and capture metadata are not imported. Embedded RGB ICC and print resolution are retained. The file's layer comps (image resource 1065 and each layer's cmls settings) arrive with their names, comments and per-layer visibility; a comp's recorded position and appearance are not imported.",
    "Full admitted raster rectangles and signed offsets are retained, including hidden RGB and off-canvas pixels. Reversed or one-side-zero raster rectangles, incomplete color channels, unsupported compression and exceeded decode limits refuse layers — Raw, RLE and ZIP/ZIP-with-prediction channels are read at both depths, including from the `Lr16` tail block Photoshop's 16-bit files keep their records in, from a large-document PSB (up to 300000 pixels per side) and from the Photoshop layer block of a big-endian 8- or 16-bit RGB TIFF or of a 32-bit float RGB TIFF in either byte order (a little-endian one's descriptor blocks, such as layer effects, fills and adjustments, refuse layers by name); a PSB layer record holding a 64-bit-length block refuses layers, and a layered TIFF has no stored composite here because its flat image opens as an ordinary TIFF; a rectangle that is zero on both sides carries no pixels and is an adjustment/fill or empty record, handled per record; groups deeper than 10 levels are dissolved.",
    "These are general importer limits, not a claim that this file contains every listed feature. The bounded record walk additionally recognizes a finite set of style, adjustment/fill, text, smart/placed, vector and blending metadata markers. Marker presence does not establish active settings or matching appearance; descriptor payloads other than the carried adjustment kinds, unknown keys and global additional-info blocks are not interpreted.",
];

pub(crate) fn parse<'a>(
    bytes: &[u8],
    container: &Container<'a>,
) -> Result<(psd::Psd, GroupRecords<'a>), String> {
    parse_with(bytes, container, None)
}

/// [`parse`], with a gray file's policy (#1 phase 3a): `Some` decodes its
/// layer styles' `Grsc` colours through that Gray profile
/// (`FxColours::Gray`, `GroupRecords::read`); `None` keeps every other read.
pub(crate) fn parse_with<'a>(
    bytes: &[u8],
    container: &Container<'a>,
    gray: Option<&crate::gray_document::GrayPolicy>,
) -> Result<(psd::Psd, GroupRecords<'a>), String> {
    // The ONE colour conversion a carried layer style takes on the way in: a
    // style colour is authored sRGB, so the file's DOCUMENT numbers convert
    // back (`authored_color`). `None` for an sRGB file and for a LUT profile
    // that cannot convert both ways, whose numbers stay as stored — and for a
    // CMYK file, whose profile `Container::profile` does not read and whose
    // import carries no effect colour (#1 phase 3), and for a gray file,
    // whose Gray profile `Container::profile` does not read either (#1
    // phase 3a; its colours cross through `gray`), and for a Lab file, whose
    // document is the Lab twin and whose colours are `LbCl` (#1 Lab 3a).
    let profile = if matches!(container.color, 1 | 4 | 9) {
        None
    } else {
        container.profile()?
    };
    // A 32-bit file's linear twin of a built-in crosses its colours as that
    // built-in's OWN twin: the open relabels the document as
    // the built-in without converting a sample
    // (`psd_import_float::document_profile`), so its colour numbers take the
    // same relabel. Photoshop's recomputed twin sits up to 12/65536 off the
    // built-in's, and converting through it moved `fill_style32.psd`'s
    // `SoCo` 40.0000014 to 39.9997 on a re-save.
    let profile = match profile {
        Some(file) if container.depth == 32 => {
            let document = crate::psd_import_float::document_profile(std::sync::Arc::clone(&file));
            if std::sync::Arc::ptr_eq(&file, &document) {
                Some(file)
            } else {
                crate::icc::IccProfile::parse(&crate::icc_builtin::linear_twin(&document))
                    .map(std::sync::Arc::new)
                    .or(Some(file))
            }
        }
        other => other,
    };
    let colors = profile.as_deref().and_then(AuthoredColors::out_of_document);
    // The reverse, for a colour this crate DECODES as sRGB and must hand to
    // the document: today only the `phfl` Lab photo-filter colour.
    let to_document = profile.as_deref().and_then(AuthoredColors::into_document);
    // A CMYK file's (#1): an sRGB colour Photoshop wrote where the record
    // means ink (a `phfl` in RGB or Lab) goes through the file's own CMYK
    // profile, relative colorimetric with BPC (`cmyk_io::psd_policy`).
    let policy = if container.color == 4 {
        let depth = if container.depth == 16 {
            crate::cmyk_document::CmykDepth::Sixteen
        } else {
            crate::cmyk_document::CmykDepth::Eight
        };
        crate::cmyk_io::psd_policy(container, depth)
            .ok()
            .map(|(p, _)| p)
    } else {
        None
    };
    let leg = policy.as_ref().and_then(|policy| {
        crate::cmyk_color::RgbToCmyk::new(
            crate::icc::IccProfile::srgb(),
            &policy.profile,
            crate::cmyk_color::ConversionOptions::default(),
        )
        .ok()
    });
    let to_ink = |rgb: [u8; 3]| -> Option<[f32; 4]> {
        let ink = leg.as_ref()?.evaluate(rgb.map(|v| f64::from(v) / 255.0));
        Some(ink.map(|v| v as f32))
    };
    // The file's pattern table, which a Pattern Overlay's `Ptrn` resolves
    // through (`psd_patterns`): every `Patt`/`Pat2`/`Pat3` tail block's
    // records, in that order.
    let mut patterns = crate::psd_patterns::PatternTable::default();
    for key in [*b"Patt", *b"Pat2", *b"Pat3"] {
        if let Some(body) = container.tail_block(key) {
            patterns
                .entries
                .extend(crate::psd_patterns::read_table(body).entries);
        }
    }
    // Why a carried RGB `SoCo` cannot render in this file's profile,
    // decided once so the report and the byte walk agree; `None` on CMYK.
    let solid_refusal = crate::psd_fill_layer::solid_refusal(profile.as_deref());
    let records = GroupRecords::read(
        container.layer_info()?,
        container.sample_bytes(),
        crate::psd_paths::MaskRoom::of(container),
        container.kind.large(),
        container.color,
        colors.as_ref(),
        to_document.as_ref(),
        Some(&to_ink),
        &patterns,
        solid_refusal.as_deref(),
        gray,
    )?;
    records.admit(container, bytes.len(), profile.as_deref())?;
    let metadata = records.metadata_psd();
    let psd = catch_unwind(AssertUnwindSafe(|| psd::Psd::from_bytes(&metadata)))
        .map_err(|_| "PSD layer parser refused malformed or unsupported data".to_string())?
        .map_err(|e| format!("PSD layer parser refused this file: {e}"))?;
    records.verify(&psd)?;
    Ok((psd, records))
}

/// One record's carried or declined layer style, as the shared report words
/// it. The marker sentences are unchanged and still carry whatever this style
/// did not (`FeatureMarkers::carry_style`).
pub(crate) fn report_style(
    body: &StyleBody,
    target: &str,
    losses: &mut Vec<String>,
    observations: &mut Vec<String>,
) {
    match body {
        StyleBody::Absent => {}
        StyleBody::Carried(carried) => {
            let mut what = carried.effects.join(", ");
            if carried.fill {
                if !what.is_empty() {
                    what.push_str(", ");
                }
                what.push_str(&format!(
                    "fill opacity {:.0} %",
                    carried.style.fill_opacity * 100.0
                ));
            }
            // The two blending-options keys, named in the same clause list
            // and before Blend If, which stays the sentence's last clause.
            if let Some(clause) = knockout_clause(
                if carried.knockout {
                    carried.style.knockout
                } else {
                    crate::style::Knockout::None
                },
                carried.interior,
            ) {
                if !what.is_empty() {
                    what.push_str(", ");
                }
                what.push_str(&clause);
            }
            if let Some(blend_if) = carried.style.blend_if.filter(|_| carried.ranges) {
                if !what.is_empty() {
                    what.push_str(" and ");
                }
                let channels: Vec<_> = blend_if.ranges().map(|r| r.channel).collect();
                what.push_str(&format!("Blend If ranges on {}", channel_list(&channels)));
            }
            observations.push(format!("{target} has Photoshop layer effects ({what}); layered import carries them as a Doctored layer style. Effect parameters are mapped onto Doctored's own renderer and settings it does not model (the glows' technique) are ignored, so its appearance can differ from Photoshop's."));
        }
        StyleBody::Declined { reason } => losses.push(format!(
            "{target} has Photoshop layer effects whose {reason}; layered import discards them without applying them."
        )),
    }
}

pub(crate) fn fingerprint(bytes: &[u8]) -> String {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hash);
    format!("{:016x}", hash.finish())
}

pub(crate) fn report(original: &[u8], path: &str) -> Result<Value, String> {
    // A little-endian 32-bit layered TIFF is read as its big-endian
    // form (`tiff_layers_order::big_endian_form`); the fingerprint stays the
    // file's own. A block that walk does not read is this report's error, so
    // the file opens flat.
    let converted = crate::tiff_layers_order::big_endian_form(original)
        .map_err(|e| format!("PSD {path}: {e}"))?;
    let bytes = converted.as_deref().unwrap_or(original);
    let read = Container::read(bytes).map_err(|e| format!("PSD {path}: {e}"))?;
    // A Bitmap file (colour mode 0, #1 Bitmap phase 1c) has its own report
    // (`bitmap_psd`): one 1-bit composite, no layers.
    if read.color == 0 {
        return crate::bitmap_psd::import_report(&read, bytes, path);
    }
    // An Indexed Color file (colour mode 2, #1) has its own report
    // (`indexed_psd`): one index plane and its table, no layered choice.
    if read.color == 2 {
        return crate::indexed_psd::import_report(&read, bytes, path);
    }
    // A Duotone file (#1 Duotone phase 1b) reports as the Gray file it
    // stores (`Container::duotone_gray`), with its inks beside it.
    let duotone = read
        .duotone_gray()
        .is_some()
        .then(|| crate::duotone_files::container_inks(&read));
    let container = read.duotone_gray().unwrap_or(read);
    // A CMYK file (#1 phase 3) takes its own admission and profile policy
    // in place of `support_refusal` and `profile`, which are RGB/gray only.
    let cmyk = container.color == 4;
    // A Grayscale file at 8, 16 or 32 bits (#1 phases 3a and 3c) opens as
    // a Gray document: its policy decodes its records, and its profile is not
    // `Container::profile`'s.
    let gray = container.color == 1 && matches!(container.depth, 8 | 16 | 32);
    // A Lab file at 8 or 16 bits (#1 Lab phase 3a) opens as a Lab document:
    // its own admission, no profile, its own composite and lines.
    let lab = container.color == 9 && matches!(container.depth, 8 | 16);
    // An RGB file at 32 bits opens as a float document, and an RGB or gray
    // file at 16 as a typed one; any other file at those depths is refused by
    // `support_refusal` and must not report a precision it cannot open as.
    let float = container.color == 3 && container.depth == 32;
    let duotone_refusal = duotone
        .as_ref()
        .and_then(|inks| inks.as_ref().err().cloned());
    let unsupported = if let Some(reason) = duotone_refusal {
        Some(reason)
    } else if cmyk {
        container.cmyk_layered_admission().err()
    } else if lab {
        crate::psd_import_lab::lab_admission(&container).err()
    } else {
        container.support_refusal()
    };
    let profile = if cmyk || gray || lab {
        Ok(None)
    } else {
        container.profile()
    };
    let gray_policy = gray.then(|| {
        crate::psd_import_gray::gray_policy(
            &container,
            crate::gray_working::GrayWorkingSpace::DotGain20,
        )
    });
    let policy = cmyk.then(|| {
        let depth = if container.depth == 16 {
            crate::cmyk_document::CmykDepth::Sixteen
        } else {
            crate::cmyk_document::CmykDepth::Eight
        };
        crate::cmyk_io::psd_policy(&container, depth)
    });
    // The float rules' profile of a 32-bit gray file: its policy's twin.
    let gray_twin = gray_policy
        .as_ref()
        .and_then(|p| p.as_ref().ok())
        .filter(|_| container.depth == 32)
        .map(|(p, _)| p.twin().clone());
    let gray_float = gray_twin.as_ref();
    let resolution = container.resolution();
    let color_refusal = match (&policy, &gray_policy) {
        (Some(policy), _) => policy.as_ref().err().cloned(),
        (None, Some(policy)) => policy.as_ref().err().cloned(),
        (None, None) => profile.as_ref().err().cloned().or_else(|| {
            // A float document's profile rule, on both choices.
            float
                .then(|| {
                    profile
                        .as_ref()
                        .ok()
                        .and_then(|p| crate::psd_import_float::profile_refusal(p.as_deref()))
                })
                .flatten()
        }),
    };
    let resolution_refusal = resolution.as_ref().err().cloned();
    let common_refusal = unsupported
        .or(color_refusal.clone())
        .or(resolution_refusal.clone());
    let parsed = if common_refusal.is_none() {
        Some(parse_with(
            bytes,
            &container,
            gray_policy
                .as_ref()
                .and_then(|p| p.as_ref().ok())
                .map(|(p, _)| p),
        ))
    } else {
        None
    };
    // Only availability is reported here, so the composite is decoded at the
    // file's own depth and discarded; a 16-bit file's preview is `composite16`,
    // a CMYK file's `cmyk_composite` behind its own admission (a TIFF has
    // none here).
    let composite = if cmyk {
        Some(
            container
                .cmyk_admission()
                .and_then(|()| match &color_refusal {
                    Some(refusal) => Err(refusal.clone()),
                    None => Ok(()),
                })
                .and_then(|()| match &resolution_refusal {
                    Some(refusal) => Err(refusal.clone()),
                    None => Ok(()),
                })
                .and_then(|()| container.cmyk_composite().map(|_| ())),
        )
    } else if lab {
        common_refusal
            .is_none()
            .then(|| crate::psd_import_lab::lab_composite(&container).map(|_| ()))
    } else if common_refusal.is_none() {
        Some(if container.depth == 16 {
            container.composite16().map(|_| ())
        } else if container.depth == 32 {
            // A 32-bit RGB or gray file (#1 phase 3c).
            crate::psd_import_float::composite32(&container).map(|_| ())
        } else {
            container.composite().map(|_| ())
        })
    } else {
        None
    };
    let mut observations = Vec::<String>::new();
    let mut losses = Vec::<String>::new();
    let mut layered_plan_bytes = None;
    let mut layer_count = None;
    let mut group_count = None;
    let mut float_refusal = None;
    if let Some(Ok((psd, records))) = parsed.as_ref() {
        layer_count = Some(psd.layers().len());
        group_count = Some(psd.groups().len());
        for (index, layer) in psd.layers().iter().enumerate() {
            let raw = &records.layers[index];
            records.layer_features[index].report(
                &format!("Layer {index} (“{}”)", layer.name()),
                &mut losses,
                &mut observations,
            );
            if let Some(carried) = records.carried(index) {
                // A gray file's adjustment the Gray table refuses is omitted
                // by the gray open (#1 phase 3a).
                if let Some(reason) = gray
                    .then(|| crate::psd_import_gray::adjustment_refusal(layer, carried, gray_float))
                    .flatten()
                {
                    losses.push(format!("Layer {index} (“{}”) is a Photoshop {} adjustment layer ({}) that a Grayscale document cannot hold ({reason}); layered import omits it and its mask, so the layers beneath it appear without that adjustment. The stored composite keeps the saved appearance.", layer.name(), carried.kind, String::from_utf8_lossy(&carried.key)));
                    continue;
                }
                // …and a Lab file's the Lab table refuses (#1 Lab phase 3a).
                if let Some(reason) = lab
                    .then(|| crate::psd_import_lab::adjustment_refusal(carried))
                    .flatten()
                {
                    losses.push(format!("Layer {index} (“{}”) is a Photoshop {} adjustment layer ({}) that a Lab Color document cannot hold ({reason}); layered import omits it and its mask, so the layers beneath it appear without that adjustment. The stored composite keeps the saved appearance.", layer.name(), carried.kind, String::from_utf8_lossy(&carried.key)));
                    continue;
                }
                // Carried at the origin as a canvas-sized adjustment layer,
                // so the outside-canvas observation cannot apply to it.
                records.masks[index].report(
                    &format!("Layer {index} (“{}”)", layer.name()),
                    "raster mask channel",
                    &mut losses,
                    &mut observations,
                );
                observations.push(format!(
                    "Layer {index} (“{}”) is a Photoshop {} adjustment layer ({}); layered import carries it as a Doctored {} adjustment layer with its mask, opacity, blend mode and clipping. Its parameters are mapped onto Doctored's own arithmetic, so its appearance can differ from Photoshop's.",
                    layer.name(),
                    carried.kind,
                    String::from_utf8_lossy(&carried.key),
                    carried.op()
                ));
                // Its effects, Fill and Blend If ride along as the
                // adjustment layer's style, reported as a raster's are.
                if !cmyk && !gray && !lab {
                    report_style(
                        &records.layer_styles[index],
                        &format!("Layer {index} (“{}”)", layer.name()),
                        &mut losses,
                        &mut observations,
                    );
                }
                continue;
            }
            if let Some(fill) = records.fill(index) {
                // A gray file's fill layers are omitted (#1 phase 3a).
                if gray {
                    losses.push(format!("Layer {index} (“{}”) is a Photoshop {} layer ({}); a Grayscale document's layered import omits fill layers and their masks. The stored composite keeps the saved appearance.", layer.name(), fill.kind_name(), String::from_utf8_lossy(&fill.key())));
                    continue;
                }
                // …and so does a Lab file's (#1 Lab phase 3a).
                if lab {
                    losses.push(format!("Layer {index} (“{}”) is a Photoshop {} layer ({}); a Lab Color document's layered import omits fill layers and their masks. The stored composite keeps the saved appearance.", layer.name(), fill.kind_name(), String::from_utf8_lossy(&fill.key())));
                    continue;
                }
                // Carried at the origin as a canvas-sized pattern, solid color
                // or gradient fill layer (`psd_fill_layer`), as
                // an adjustment is, in whichever form the record took.
                records.masks[index].report(
                    &format!("Layer {index} (“{}”)", layer.name()),
                    "raster mask channel",
                    &mut losses,
                    &mut observations,
                );
                if let crate::psd_fill_layer::FillKind::Gradient(carry) = &fill.kind {
                    crate::psd_gradient_fill::report(
                        index,
                        layer.name(),
                        carry,
                        fill.form.clause(),
                        &mut observations,
                        &mut losses,
                    );
                } else {
                    observations.push(match &fill.kind {
                        crate::psd_fill_layer::FillKind::Pattern { .. } => format!(
                            "Layer {index} (“{}”) is a Photoshop Pattern Fill layer (PtFl){}; layered import carries it as a Doctored pattern fill layer with its tile, Scale, Angle, Link with Layer, phase, mask, opacity, blend mode and clipping.",
                            layer.name(),
                            fill.form.clause()
                        ),
                        kind => format!(
                            "Layer {index} (“{}”) is a Photoshop {} layer ({}){}; layered import carries it as a Doctored solid color fill layer with its colour, mask, opacity, blend mode and clipping.{}",
                            layer.name(),
                            fill.kind_name(),
                            String::from_utf8_lossy(&fill.key()),
                            fill.form.clause(),
                            if matches!(
                                kind,
                                crate::psd_fill_layer::FillKind::Solid {
                                    linear: Some(_),
                                    ..
                                }
                            ) {
                                " Its colour lies outside sRGB and is kept as a wide colour."
                            } else {
                                ""
                            }
                        ),
                    });
                }
                // A CMYK file's styles are reported by `cmyk_report_lines`.
                if !cmyk && !gray && !lab {
                    report_style(
                        &records.layer_styles[index],
                        &format!("Layer {index} (“{}”)", layer.name()),
                        &mut losses,
                        &mut observations,
                    );
                }
                continue;
            }
            if records.omitted(index) {
                // The record is dropped whole, so its mask and its rectangle
                // have nothing left to disclose.
                if let AdjustmentBody::Declined { key, reason, .. } = &records.adjustments[index] {
                    if *key == crate::psd_fill_layer::KEY {
                        losses.push(format!("Layer {index} (“{}”) is a Photoshop Pattern Fill layer (PtFl) whose {reason}; layered import omits it and its mask. The stored composite keeps the saved appearance.", layer.name()));
                        continue;
                    }
                    if *key == crate::psd_fill_layer::SOLID_KEY {
                        losses.push(format!("Layer {index} (“{}”) is a Photoshop Solid Color Fill layer (SoCo) with no pixels whose {reason}; layered import omits it and its mask. The stored composite keeps the saved appearance.", layer.name()));
                        continue;
                    }
                    if *key == crate::psd_gradient_fill::KEY {
                        losses.push(format!("Layer {index} (“{}”) is a Photoshop Gradient Fill layer (GdFl) with no pixels whose {reason}; layered import omits it and its mask. The stored composite keeps the saved appearance.", layer.name()));
                        continue;
                    }
                }
                if let AdjustmentBody::Declined { key, kind, reason } = &records.adjustments[index]
                {
                    losses.push(format!("Layer {index} (“{}”) is a Photoshop {kind} adjustment layer ({}) with no pixels whose {reason}; layered import omits it and its mask, so the layers beneath it appear without that adjustment. The stored composite keeps the saved appearance.", layer.name(), String::from_utf8_lossy(key)));
                    continue;
                }
                let key = records.layer_features[index]
                    .adjustment_key()
                    .map(|key| String::from_utf8_lossy(&key).trim_end().to_string())
                    .unwrap_or_default();
                losses.push(format!("Layer {index} (“{}”) is a Photoshop adjustment or fill layer ({key}) with no pixels; layered import omits it and its mask, so the layers beneath it appear without that adjustment. The stored composite keeps the saved appearance.", layer.name()));
                continue;
            }
            if !cmyk && !gray && !lab {
                report_style(
                    &records.layer_styles[index],
                    &format!("Layer {index} (“{}”)", layer.name()),
                    &mut losses,
                    &mut observations,
                );
            }
            records.masks[index].report(
                &format!("Layer {index} (“{}”)", layer.name()),
                "raster mask channel",
                &mut losses,
                &mut observations,
            );
            if raw.is_empty() {
                // Created at the origin, so the outside-canvas observation
                // below cannot apply to it either.
                observations.push(format!("Layer {index} (“{}”) has no pixels; layered import creates it as an empty canvas-sized layer.", layer.name()));
                continue;
            }
            let [top, left, bottom, right] = raw.bounds;
            if left < 0
                || top < 0
                || i64::from(right) > i64::from(container.width)
                || i64::from(bottom) > i64::from(container.height)
            {
                observations.push(format!("Layer {index} (“{}”) extends outside the canvas; layered import retains its complete raster rectangle and signed offset.", layer.name()));
            }
        }
        if !psd.groups().is_empty() {
            observations.push(format!("{} layer groups detected. Folder opacity, visibility, open/closed state and recognized blend keys are retained within the supported nesting limit; unsupported group options can still change appearance.", psd.groups().len()));
            for (index, group) in records.groups.iter().enumerate() {
                let id = index as u32 + 1;
                let name = psd.groups()[&id].name();
                group.features.report(
                    &format!("Group {id} (“{name}”)"),
                    &mut losses,
                    &mut observations,
                );
                if !cmyk && !gray && !lab {
                    report_style(
                        &records.group_styles[index],
                        &format!("Group {id} (“{name}”)"),
                        &mut losses,
                        &mut observations,
                    );
                }
                group.mask.report(
                    &format!("Group {id} (“{name}”)"),
                    "raster mask record/channel",
                    &mut losses,
                    &mut observations,
                );
                if group.scene {
                    losses.push(format!("Group {id} (“{name}”) has a nonstandard scene subtype; layered import retains only the ordinary folder."));
                }
            }
        }
        let nothing = if gray {
            crate::psd_import_gray::imports_nothing(psd, records, gray_float)
        } else if lab {
            crate::psd_import_lab::imports_nothing(psd, records)
        } else {
            records.retains_nothing()
        };
        if nothing && !psd.layers().is_empty() {
            observations.push("Every layer record is an adjustment or fill layer with no pixels; Import Layers opens the stored composite as a single layer.".into());
        }
        if cmyk {
            crate::psd_import_cmyk::cmyk_report_lines(
                psd,
                records,
                &container,
                &mut losses,
                &mut observations,
            );
            layered_plan_bytes =
                crate::psd_import_cmyk::layered_plan(&container, records, bytes.len())
                    .ok()
                    .map(|(retained, scratch)| retained.saturating_add(scratch));
        } else if gray {
            crate::psd_import_gray::gray_report_lines(
                psd,
                records,
                &container,
                gray_float,
                &mut losses,
                &mut observations,
            );
        } else if lab {
            crate::psd_import_lab::lab_report_lines(
                psd,
                records,
                &container,
                &mut losses,
                &mut observations,
            );
            layered_plan_bytes =
                crate::psd_import_lab::layered_plan(&container, records, bytes.len())
                    .ok()
                    .map(|(retained, scratch)| retained.saturating_add(scratch));
        } else if container.depth == 16 {
            crate::psd_import16::typed_losses(
                psd,
                records,
                &container,
                &mut losses,
                &mut observations,
            );
        } else if float {
            let profile = profile
                .as_ref()
                .ok()
                .and_then(Clone::clone)
                .unwrap_or_else(crate::icc::IccProfile::srgb);
            crate::psd_import_float::float_report_lines(
                psd,
                records,
                &profile,
                (container.width, container.height),
                &mut losses,
                &mut observations,
            );
            float_refusal = crate::psd_import_float::layered_refusal(psd, records);
        }
        // Layer comps (resource 1065 and each record's `cmls`), which every
        // layered open imports (`psd_comps`).
        losses.extend(crate::psd_comps::report_losses(&container, records));
        records.divider_features.report(
            "PSD bounding-divider metadata",
            &mut losses,
            &mut observations,
        );
        // Every group is visited (in id order, so the report is stable): the
        // "deeper than 10" loss once, then one loss per dissolved folder that
        // holds locks, which `PsdTree::emit` adds to each layer it held.
        let mut ids: Vec<u32> = psd.groups().keys().copied().collect();
        ids.sort_unstable();
        let mut dissolved_any = false;
        for id in ids {
            let mut parent = psd.groups()[&id].parent_id();
            let mut ancestors = 0;
            while let Some(parent_id) = parent {
                ancestors += 1;
                if ancestors >= MAX_GROUP_DEPTH as usize {
                    break;
                }
                parent = psd.groups().get(&parent_id).and_then(|g| g.parent_id());
            }
            if ancestors < MAX_GROUP_DEPTH as usize {
                continue;
            }
            if !dissolved_any {
                dissolved_any = true;
                losses.push("Groups deeper than 10 levels are present; those extra folder rows are dissolved.".into());
            }
            if records.groups[id as usize - 1].features.locks() != 0 {
                let name = psd.groups()[&id].name();
                losses.push(format!("Group {id} (“{name}”) is locked and lies beyond the 10-level nesting limit; its folder row is dissolved and its locks are added to each layer it held."));
            }
        }
    }
    if cmyk {
        crate::psd_import_cmyk::cmyk_file_lines(
            &container,
            policy.as_ref().and_then(|p| p.as_ref().ok()),
            composite.as_ref().is_some_and(|result| result.is_ok()),
            &mut losses,
            &mut observations,
        );
    } else if gray {
        crate::psd_import_gray::gray_file_lines(
            &container,
            gray_policy.as_ref().and_then(|p| p.as_ref().ok()),
            composite.as_ref().is_some_and(|result| result.is_ok()),
            &mut losses,
            &mut observations,
        );
    } else if lab {
        crate::psd_import_lab::lab_file_lines(
            &container,
            composite.as_ref().is_some_and(|result| result.is_ok()),
            &mut losses,
            &mut observations,
        );
    } else {
        let base_channels = if container.color == 3 { 3 } else { 1 };
        if container.merged_alpha
            && container.channels > base_channels
            && composite.as_ref().is_some_and(|result| result.is_ok())
        {
            observations.push("Saved-composite transparency is decoded from a preview stored against white; faint transparent colors may have less detail than editable layers.".into());
        }
        let spots = container.report_spot_records();
        let extra = spots.alpha;
        if extra > 0 && matches!(container.color, 1 | 3) {
            losses.push(format!("{extra} additional saved channels detected in the header; neither import choice retains them as editable channels."));
        }
        losses.extend(spots.losses.iter().cloned());
        observations.extend(spots.observation());
        if profile.as_ref().is_ok_and(|p| p.is_none()) {
            observations.push("No embedded ICC profile was found; imported RGB values are explicitly assumed to be sRGB.".into());
        }
        // Image resource 1073: both import choices carry the samplers, so
        // their sentences are the file's, not one choice's.
        if let Ok(samplers) = container.color_samplers() {
            losses.extend(samplers.losses);
            observations.extend(samplers.observations);
        }
    }
    // Resources 1025 and 2000–2997: every importer carries the paths on both
    // choices (`psd_paths`), so their sentences are the file's too.
    losses.extend(crate::psd_paths::file_paths(&container).1);
    let mut layer_error = common_refusal
        .clone()
        .or_else(|| parsed.as_ref().and_then(|p| p.as_ref().err().cloned()))
        .or(float_refusal);
    let composite_error = if cmyk {
        composite.as_ref().and_then(|p| p.as_ref().err().cloned())
    } else {
        common_refusal.or_else(|| composite.as_ref().and_then(|p| p.as_ref().err().cloned()))
    };
    // Nothing at all would be retained — an empty layer section, or a file
    // whose every record is an omitted adjustment/fill layer — so the layered
    // choice IS the stored composite and takes its availability.
    let retains_nothing = match parsed.as_ref() {
        Some(Ok((psd, records))) if gray => {
            crate::psd_import_gray::imports_nothing(psd, records, gray_float)
        }
        Some(Ok((psd, records))) if lab => crate::psd_import_lab::imports_nothing(psd, records),
        Some(Ok((_, records))) => records.retains_nothing(),
        _ => false,
    };
    if layer_error.is_none() && retains_nothing {
        layer_error = composite_error.clone();
    }
    let profile_name = match (&policy, &gray_policy) {
        (Some(policy), _) => policy.as_ref().ok().map(|(p, _)| p.name().to_string()),
        // An untagged gray file names no profile: the working Gray is the
        // open's (`gray_file_lines`).
        (None, Some(policy)) => policy
            .as_ref()
            .ok()
            .filter(|(_, untagged)| untagged.is_none())
            .map(|(p, _)| p.name().to_string()),
        (None, None) => profile.ok().flatten().map(|p| p.name().to_string()),
    };
    let mut out = json!({
        "format": container.kind.name(), "path": path, "source_fingerprint": fingerprint(original), "width": container.width, "height": container.height,
        "depth": container.depth, "color_mode": container.color_name(),
        "import_precision": if cmyk { "cmyk" } else if gray { "gray" } else if lab { "lab" } else if container.color == 3 && container.depth == 16 { "rgba16" } else if float { "rgba32" } else { "rgba8" },
        "layer_count": layer_count, "group_count": group_count,
        "layered_available": layer_error.is_none(), "layered_refusal": layer_error,
        "layered_note": "Availability means bounded raster/mask extents, channel/row framing, decode memory and the existing metadata parser were admitted. Pixel decoding can still refuse; it never substitutes a composite implicitly.",
        "stored_composite_available": composite_error.is_none(), "stored_composite_refusal": composite_error,
        "stored_composite_note": if cmyk { crate::psd_import_cmyk::STORED_COMPOSITE_NOTE } else if gray { crate::psd_import_gray::STORED_COMPOSITE_NOTE } else if lab { crate::psd_import_lab::STORED_COMPOSITE_NOTE } else { "Uses the file's saved composite, including reconstruction from white when it has merged transparency. It is a single raster layer; editable layers, masks and descriptions are intentionally discarded. The saved composite may reflect an older save by the authoring application." },
        "verified_losses": losses, "observations": observations,
        "general_limitations": if cmyk { crate::psd_import_cmyk::CMYK_GENERAL_LIMITATIONS } else if gray { crate::psd_import_gray::GRAY_GENERAL_LIMITATIONS } else if lab { crate::psd_import_lab::LAB_GENERAL_LIMITATIONS } else { GENERAL_LIMITATIONS },
        "profile": profile_name,
        "resolution": resolution.ok().flatten().map(|r| json!({"x": r.x, "y": r.y})),
        "metadata_captured": false,
        "requires_explicit_choice": true
    });
    if cmyk || lab {
        // The layered open's plan (`psd_import_cmyk::layered_plan`,
        // `psd_import_lab::layered_plan`): what the host admits Import
        // Layers against.
        out["layered_plan_bytes"] = json!(layered_plan_bytes);
    }
    if let Some(Ok(inks)) = &duotone {
        out["color_mode"] = json!("Duotone");
        out["duotone"] = inks.to_json();
        if let Some(observations) = out["observations"].as_array_mut() {
            observations.insert(0, json!(format!(
                "This file opens as a Duotone document: its {} inks are kept, and its gray channel opens as a Grayscale file's does.",
                inks.type_name()
            )));
        }
    }
    Ok(out)
}

/// 0 imports the supported layer subset; 1 opens only the saved composite.
/// A layered decode error is never replaced with a composite implicitly.
/// `job` is checked before the parse, per imported record and before the
/// composite decode; a cancel answers exactly `operation cancelled`.
pub(crate) fn open(
    bytes: &[u8],
    path: &str,
    mode: i32,
    job: JobContext<'_>,
) -> Result<DrDocument, String> {
    crate::cancel_io::settle(job, open_checked(bytes, path, mode, job))
}

fn open_checked(
    bytes: &[u8],
    path: &str,
    mode: i32,
    job: JobContext<'_>,
) -> Result<DrDocument, String> {
    job.check()?;
    let container = Container::read(bytes).map_err(|e| format!("PSD {path}: {e}"))?;
    if let Some(reason) = container.support_refusal() {
        return Err(format!("PSD {path}: {reason}"));
    }
    if container.depth == 16 {
        // Never a silent depth conversion: the typed importer owns this file.
        return Err(format!(
            "PSD {path}: 16-bit PSD opens only as a 16-bit document"
        ));
    }
    if container.depth == 32 {
        // …and the float importer owns this one.
        return Err(format!(
            "PSD {path}: 32-bit PSD opens only as a 32-bit float document"
        ));
    }
    container
        .profile()
        .map_err(|e| format!("PSD {path}: {e}"))?;
    let composite = || {
        job.check()?;
        container
            .composite()
            .map_err(|e| format!("PSD {path}: {e}"))
    };
    let doc = match mode {
        0 => {
            let (psd, records) = parse(bytes, &container)
                .map_err(|e| format!("PSD {path}: {e}; inspect the saved-composite option"))?;
            job.check()?;
            if records.retains_nothing() {
                DrDocument::from_pixels(composite()?)
            } else {
                crate::psd::import_layers(&psd, &records, &container, path, job)?
            }
        }
        1 => DrDocument::from_pixels(composite()?),
        _ => return Err("unknown PSD import mode; use 0 (layers) or 1 (stored composite)".into()),
    };
    job.check()?;
    container.tag(doc).map_err(|e| format!("PSD {path}: {e}"))
}
