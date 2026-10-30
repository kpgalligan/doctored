//! The Filter menu on a CMYK document (#1): every destructive filter
//! a CMYK document admits, run through the RGB selected-filter path
//! (`selected_adjustment` / `selected_filter`, unchanged) in two RGBA16
//! passes over the layer's stored domain `L = 1 − ink`:
//!
//! * pass A is `[L_c, L_m, L_y, α]`, pass B is `[L_k, L_k, L_k, α]`, each a
//!   one-layer `PrecisionDocument` of the layer's own size tagged with the
//!   built-in sRGB (the numbers are `L`, not a colour);
//! * both run `selected_edit_with_context` on that layer (index 0) with the
//!   canvas coverage remapped into the layer's frame (`coverage_at`), so the
//!   frames (Radial Blur's and Offset's selection box), the banding, the work
//!   caps, the seeds and the no-op rule are the RGB path's verbatim;
//! * the result is C/M/Y = 65535 − A's R/G/B, K = 65535 − B's G, α = A's
//!   (`cmyk_passes::join_sample`, the join every two-pass CMYK edit shares).
//!
//! Photoshop take1 and take2 (`scripts/photoshop-probe/cmyk_adjust_filter_take.py`)
//! measured every deterministic filter it ran against its RGB twins: each is
//! PLATE-L — every plate of the CMYK result equals 255 − the `L` twin's
//! channel (max 0 codes) — except Radial Blur (draft), Smart Sharpen and Lens
//! Blur, which match neither twin (NONE, shipped on `L`). No filter was
//! PLATE-INK, so no filter runs on ink.
//!
//! Pass B keeps the request's seed except Add Noise's, which is the seed XOR
//! `0x9E37_79B9_7F4A_7C15` so the K plate's noise is not C's. Clouds and
//! Difference Clouds take `foreground_ink` / `background_ink` (straight ink
//! in [0, 1], default K 100 % over paper) in place of the RGB colour keys:
//! each pass gets the `#rrggbb` of its `L` (`round(255·(1 − ink))`), so the
//! one noise field spans every plate.
//!
//! A transparency-locked layer keeps its alpha: `freezing_alpha` skips a CMYK
//! raster and the pass documents carry no locks, so this module applies
//! `precision_lock::restoring_alpha`'s rule itself (α = the source's) for
//! every filter, Offset included. A pixel whose final alpha is 0 keeps its
//! source ink. Liquify never reaches this module: `selected_edit` routes it
//! to phase 4b's `cmyk_warp` first. Both passes unchanged, or no stored code changed, is
//! `Ok(None)`. The edit runs under the CMYK document's `Pixels` lock.
//!
//! [`FILTER_REFUSALS`] names what a CMYK document refuses, with its
//! sentence: Lens Correction, the Develop Filter with Presence and Add
//! Grain (which are its controls), and Doctored's RGB-only Depth Blur and
//! its internal per-channel Levels. take1 found no filter that differs
//! between 8 and 16 bits.
//!
//! The plan is A's RGB `scratch_bytes` + layer px × 34 (A input 8, A output
//! held 8, B input 8, the CMYK output 10) + layer px × 2 (the remapped
//! coverage) + canvas × `DISPLAY_BYTES_PER_PIXEL` (the host's display
//! preparation) + bookkeeping; `output_bytes` is layer px × 10.

use serde_json::{json, Map, Value};

use crate::cmyk_adjust_layer::DISPLAY_BYTES_PER_PIXEL;
use crate::cmyk_image::CmykImage;
use crate::cmyk_passes::{join_sample, BLANK_PASS};
use crate::doc_lock::{EditKind, LOCK_TRANSPARENCY};
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::metadata::{Metadata, Resolution};
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{admit, pixel_count, reserved_vec, PrecisionImage, BOOKKEEPING_BYTES};
use crate::selected_adjustment::{coverage_at, validate_coverage, SelectedEdit};

/// Each filter a CMYK document refuses, with its whole sentence (module doc).
pub(crate) const FILTER_REFUSALS: &[(&str, &str)] = &[
    (
        "lens_correction",
        "Lens Correction is unavailable in a CMYK document",
    ),
    (
        "develop_filter",
        "Develop Filter is unavailable in a CMYK document",
    ),
    ("presence", "Presence is unavailable in a CMYK document"),
    ("add_grain", "Add Grain is unavailable in a CMYK document"),
    (
        "depth_blur",
        "Depth Blur is unavailable in a CMYK document: Doctored offers it on RGB documents only",
    ),
    (
        "levels_channels",
        "Per-channel Levels is unavailable in a CMYK document: Doctored offers it on RGB \
         documents only",
    ),
];

/// Add Noise's pass-B seed salt (module doc): the 64-bit golden ratio.
const NOISE_SEED_SALT: u64 = 0x9E37_79B9_7F4A_7C15;

/// The two passes (module doc).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pass {
    Cmy,
    K,
}

/// Four numbers in [0, 1], or `default` when absent.
fn ink_param(
    params: &Map<String, Value>,
    key: &str,
    default: [f64; 4],
) -> Result<[f64; 4], String> {
    let Some(value) = params.get(key) else {
        return Ok(default);
    };
    let list = value
        .as_array()
        .filter(|l| l.len() == 4)
        .ok_or_else(|| format!("{key} must be four ink fractions"))?;
    let mut out = [0.0; 4];
    for (slot, v) in out.iter_mut().zip(list) {
        *slot = v
            .as_f64()
            .filter(|n| (0.0..=1.0).contains(n))
            .ok_or_else(|| format!("{key} must be four ink fractions"))?;
    }
    Ok(out)
}

/// `edit` as pass `pass` runs it: layer 0, the clouds inks as that pass's
/// colours, Add Noise's pass-B seed.
fn pass_edit(edit: &SelectedEdit, pass: Pass) -> Result<SelectedEdit, String> {
    let mut params = edit.params.clone();
    if matches!(edit.op.as_str(), "clouds" | "difference_clouds") {
        for key in ["foreground_color", "background_color"] {
            if params.contains_key(key) {
                return Err(crate::cmyk_document::refused_sentence(&format!(
                    "The RGB \"{key}\" key (give Clouds' ink)"
                )));
            }
        }
        let fg = ink_param(&params, "foreground_ink", [0.0, 0.0, 0.0, 1.0])?;
        let bg = ink_param(&params, "background_ink", [0.0; 4])?;
        params.remove("foreground_ink");
        params.remove("background_ink");
        let hex = |ink: [f64; 4]| {
            let l = |v: f64| (255.0 * (1.0 - v)).round() as u8;
            let rgb = match pass {
                Pass::Cmy => [l(ink[0]), l(ink[1]), l(ink[2])],
                Pass::K => [l(ink[3]); 3],
            };
            Value::String(format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]))
        };
        params.insert("foreground_color".into(), hex(fg));
        params.insert("background_color".into(), hex(bg));
    }
    if edit.op == "add_noise" && pass == Pass::K {
        let seed = match params.get("seed") {
            None => 1,
            Some(v) => v
                .as_u64()
                .or_else(|| v.as_i64().map(|v| v as u64))
                .ok_or("seed must be a 64-bit integer")?,
        };
        params.insert("seed".into(), json!(seed ^ NOISE_SEED_SALT));
    }
    Ok(SelectedEdit {
        layer: 0,
        op: edit.op.clone(),
        params,
        description: None,
        analysis_scope: edit.analysis_scope,
    })
}

/// A one-layer RGBA16 pass document of `pixels` (module doc).
fn pass_document(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> Result<PrecisionDocument, String> {
    let image = PrecisionImage::from_rgba16_unseeded(w, h, pixels, IccProfile::srgb(), None)?;
    PrecisionDocument::from_image(image, Metadata::default(), Resolution::default())
}

impl PrecisionDocument {
    /// The CMYK filter's refusal for `op`, if any.
    fn cmyk_filter_refusal(op: &str) -> Result<(), String> {
        match FILTER_REFUSALS.iter().find(|(name, _)| *name == op) {
            Some((_, sentence)) => Err((*sentence).to_string()),
            None => Ok(()),
        }
    }

    /// The plan of a CMYK filter (`selected_edit_plan`'s keys; module doc).
    pub(crate) fn cmyk_filter_plan(&self, edit: &SelectedEdit) -> Result<Value, String> {
        Self::cmyk_filter_refusal(&edit.op)?;
        let image = self
            .layer(edit.layer)
            .and_then(crate::precision_document::PrecisionDocumentLayer::cmyk_source)
            .ok_or("a CMYK filter requires a CMYK raster layer")?;
        let (w, h) = image.dimensions();
        let layer = pixel_count(w, h)? as u64;
        let (cw, ch) = self.dimensions();
        let canvas = pixel_count(cw, ch)? as u64;
        // The RGB path's own plan for pass A over a plane of the layer's
        // size (a filled stand-in: the plan reads only its dimensions).
        let stand_in = PrecisionImage::filled_rgba16(w, h, [0; 4], IccProfile::srgb())?;
        let pass =
            PrecisionDocument::from_image(stand_in, Metadata::default(), Resolution::default())?;
        let rgb = pass.selected_edit_plan(&pass_edit(edit, Pass::Cmy)?)?;
        let a_scratch = rgb["scratch_bytes"]
            .as_u64()
            .ok_or("invalid CMYK filter plan")?;
        let scratch = a_scratch
            + layer * 34
            + layer * 2
            + canvas * DISPLAY_BYTES_PER_PIXEL
            + 2 * BOOKKEEPING_BYTES;
        Ok(
            json!({"retained_bytes":self.retained_bytes(),"scratch_bytes":scratch,
            "output_bytes":layer * 10,"input_samples":canvas,"input_width":cw,
            "input_height":ch}),
        )
    }

    /// Runs `edit`'s filter on CMYK raster `edit.layer` (module doc) within
    /// the canvas `coverage` (empty is the whole layer). `Ok(None)` when no
    /// stored code changes.
    pub(crate) fn cmyk_filter(
        &self,
        edit: &SelectedEdit,
        coverage: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        validate_coverage(self, coverage)?;
        self.under_locks(edit.layer, EditKind::Pixels, |doc| {
            doc.cmyk_filter_unlocked(edit, coverage, limit, other, job)
        })
    }

    fn cmyk_filter_unlocked(
        &self,
        edit: &SelectedEdit,
        coverage: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let plan = self.cmyk_filter_plan(edit)?;
        let scratch = plan["scratch_bytes"]
            .as_u64()
            .ok_or("invalid CMYK filter plan")?;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let job = job.with_reservation_floor(scratch);
        let entry = self
            .layer(edit.layer)
            .ok_or("high-depth layer index is out of range")?;
        let image = entry
            .cmyk_source()
            .ok_or("a CMYK filter requires a CMYK raster layer")?;
        let offset = entry.offset();
        let (w, h) = image.dimensions();
        let n = pixel_count(w, h)?;
        let canvas = self.dimensions();
        let local: Vec<u16> = if coverage.is_empty() {
            Vec::new()
        } else {
            let mut out = reserved_vec(n)?;
            out.extend((0..n).map(|i| coverage_at(coverage, canvas, (w, h), offset, i)));
            out
        };
        let mut a_px = reserved_vec(n)?;
        let mut b_px = reserved_vec(n)?;
        for y in 0..h {
            job.check()?;
            for (_, run) in image.row_segments(y, 0, w) {
                for s in run {
                    a_px.push([65535 - s[0], 65535 - s[1], 65535 - s[2], s[4]]);
                    b_px.push([65535 - s[3], 65535 - s[3], 65535 - s[3], s[4]]);
                }
            }
        }
        let held = self.retained_bytes() + other;
        let a_doc = pass_document(w, h, a_px)?;
        let a_out = a_doc.selected_edit_with_context(
            &pass_edit(edit, Pass::Cmy)?,
            &local,
            limit,
            held + n as u64 * 8,
            job.range(0.0, 0.45),
        )?;
        let b_doc = pass_document(w, h, b_px)?;
        let b_out = b_doc.selected_edit_with_context(
            &pass_edit(edit, Pass::K)?,
            &local,
            limit,
            held + n as u64 * 16,
            job.range(0.45, 0.9),
        )?;
        if a_out.is_none() && b_out.is_none() {
            job.report(1.0, 5)?;
            return Ok(None);
        }
        let a = a_out
            .as_ref()
            .unwrap_or(&a_doc)
            .layer(0)
            .and_then(|l| l.source())
            .ok_or("the CMYK filter's pass lost its layer")?;
        let b = b_out
            .as_ref()
            .unwrap_or(&b_doc)
            .layer(0)
            .and_then(|l| l.source())
            .ok_or("the CMYK filter's pass lost its layer")?;
        if a.dimensions() != (w, h) || b.dimensions() != (w, h) {
            return Err("the CMYK filter's pass changed the layer's size".into());
        }
        let frozen = self.lock_owner(edit.layer, LOCK_TRANSPARENCY).is_some();
        let mut changed = false;
        let out = CmykImage::from_rows_with_job(
            w,
            h,
            |y, row| {
                let mut x = 0u32;
                for (_, run) in image.row_segments(y, 0, w) {
                    for s in run {
                        let pa = a.rgba16_at(x, y).unwrap_or(BLANK_PASS);
                        let pb = b.rgba16_at(x, y).unwrap_or(BLANK_PASS);
                        let next = join_sample(*s, pa, Some(pb), frozen);
                        changed |= next != *s;
                        row[x as usize] = next;
                        x += 1;
                    }
                }
                job.every(y as usize * w as usize)
            },
            job.range(0.9, 1.0),
        )?;
        let result = if changed {
            self.replacing_content(edit.layer, Content::CmykRaster { image: out, offset })?
        } else {
            None
        };
        job.report(1.0, 5)?;
        Ok(result)
    }

    /// The descriptor's filter plans on the top CMYK raster (module doc):
    /// Gaussian Blur at 8 (op `blur` sigma 8, what Filter ▸ Blur ▸ Gaussian
    /// Blur and `apply_filter blur` run), Surface Blur 10/15, Dust &
    /// Scratches 4/0, and a destructive Shadows/Highlights at its defaults.
    /// `null` where the edit refuses.
    pub(crate) fn cmyk_filter_descriptor_plans(&self) -> Map<String, Value> {
        let top = (0..self.layer_count())
            .rev()
            .find(|&i| self.layer(i).and_then(|l| l.cmyk_source()).is_some());
        let filter = |op: &str, params: Value| -> Value {
            let Some(layer) = top else {
                return Value::Null;
            };
            let Value::Object(params) = params else {
                return Value::Null;
            };
            let edit = SelectedEdit {
                layer,
                op: op.to_string(),
                params,
                description: None,
                analysis_scope: Default::default(),
            };
            self.cmyk_filter_plan(&edit)
                .ok()
                .and_then(|plan| plan["scratch_bytes"].as_u64())
                .map_or(Value::Null, Value::from)
        };
        let shadows = r#"{"type":"adjust","evaluation":"precision_v1","op":"shadows_highlights","params":{}}"#;
        let mut out = Map::new();
        out.insert(
            "cmyk_filter_gaussian_scratch_bytes".into(),
            filter("blur", json!({"sigma":8})),
        );
        out.insert(
            "cmyk_filter_surface_scratch_bytes".into(),
            filter("surface_blur", json!({"radius":10,"threshold":15})),
        );
        out.insert(
            "cmyk_filter_dust_scratch_bytes".into(),
            filter("dust_and_scratches", json!({"radius":4,"threshold":0})),
        );
        out.insert(
            "cmyk_shadows_apply_scratch_bytes".into(),
            top.and_then(|i| self.cmyk_adjustment_plan(Some(i), shadows, true).ok())
                .and_then(|plan| plan["scratch_bytes"].as_u64())
                .map_or(Value::Null, Value::from),
        );
        out
    }
}
