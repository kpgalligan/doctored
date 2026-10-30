//! Selections on a CMYK document (#547): the source canvases and the CMYK arms
//! of `dr_precision_doc_select`, which `precision_selection` dispatches to
//! first on a CMYK document.
//!
//! * `magic_wand`, `similar`, `alpha` and `color` read INK: the composite's
//!   straight CMYKA16 (`source` `composite`, the visible stack through
//!   `cmyk_stack`), the records below `below` (`source` `backdrop`,
//!   [`PrecisionDocument::cmyk_backdrop`]) or one record's own ink at full
//!   opacity with no mask (`source` `layer`, the stack's `only` view; a
//!   transparent pixel is bare paper, `[0; 5]`). The wand grows through
//!   `selection_region::grow_ink` and Select ▸ Grow / Similar through
//!   `selection_similar::similar` over the same `selection_region::ink_as_rgb`
//!   (the ink's naive RGB at the document's depth, `ReferenceRange`,
//!   the one `flood`: PN7), with the u16 selection as the references. `alpha`
//!   is the ink's alpha; `color` with `channel` `cyan`, `magenta`, `yellow` or
//!   `black` is that plate's STORED domain `1 − ink`
//!   (`cmyk_composite::plate_coverage`), what Load Selection from a plate and
//!   a Ctrl-click on its thumbnail load — exact against Photoshop 26.10.0 on
//!   every pixel (`scripts/photoshop-probe/cmyk_channels_take.py`, take1). A
//!   `red`, `green` or `blue` channel refuses as `The "red" channel selection
//!   is unavailable in a CMYK document`.
//! * `luminance` (Load Selection from the composite, a Ctrl-click on the CMYK
//!   thumbnail, and the luminosity masks) reads the composite's sRGB: its
//!   straight ink through `cmyk_color::CmykToRgb` into the built-in sRGB at
//!   relative colorimetric with black point compensation (what `convert_mode
//!   rgb` makes of it), then `blend::luma_code` (Rec. 709). Photoshop's take1
//!   composite load matched none of the three declared candidates within
//!   max 2 / mean 0.5 codes (the view's luma LV mean 11.97, max 23; PN7's
//!   naive RGB LN mean 13.22, max 72; this sRGB LP mean 11.78, max 42 — each
//!   brighter than Photoshop's), so LP ships as the smallest mean, fitted on
//!   one take and disclosed (`designs/cmyk-mode-shipped.md`).
//! * `color_range`, `quick` and `refine_edge` read the VIEW: the CMYK →
//!   Display P3 leg (`CmykViewLink`) over the composite's straight ink with
//!   its alpha kept, exactly as `cmyk_view_window` renders it, in RGBA16
//!   codes. Their Lab profile is the document's `profile()`, the built-in
//!   Display P3 the view is in, and they run the shipped kernels unchanged
//!   (`selection_tools::color_range_samples`, `quick_samples`,
//!   `precision_selection::refined_coverage`).
//! * A `layer` source is admitted for the ink kinds and `luminance` and
//!   refused for the view kinds (`A layer source for the "<kind>" selection
//!   …`); a `backdrop` source is admitted for every kind (`below` a postorder
//!   record index, at most the record count; 0 is bare paper). `luminance`
//!   with a `layer` source is that record's own ink at full opacity with no
//!   mask (the stack's `only` view, as the ink kinds read it) through the
//!   same LP leg: the CMYK composite of one layer as Apply Image and
//!   Calculations read it against one plane or channel (#547, the
//!   remaining-gaps batch, its only caller).
//!
//! The canvases stream `cmyk_stack` rows, so beside the canvas the scratch is
//! one f32 ink row (`w·20`), the stack's prepared masks and, for the view, the
//! view link's node table. The plan is `8·BOOKKEEPING + n·40 + held + canvas +
//! stack + w·20`: `n·40` and `held` are the RGB plan's terms
//! (`SelectionRequest::checked_buffers`), the canvas is `n·10` for ink,
//! `n·8 + CmykPolicy::view_bytes()` for the view and `n·8` plus the sRGB
//! leg's memo for `luminance`; a backdrop charges the whole document's stack.

use serde_json::{json, Value};

use crate::cmyk_color::{ink_key, CmykToRgb, ConversionOptions};
use crate::cmyk_composite::{codes, plate_coverage, unit, InkPixel};
use crate::cmyk_convert_tree::carry;
use crate::cmyk_document::{refused_sentence, CmykPolicy};
use crate::cmyk_image::{CmykImage, CmykSample, CMYK_BYTES_PER_PIXEL};
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, pixel_count, reserved_vec, BOOKKEEPING_BYTES};
use crate::precision_selection::{SelectionRequest, SourceSelection};
use crate::scoped_statistics::AnalysisSource;
use crate::soft_proof::memo_peak_bytes_for;

/// What a kind's canvas reads.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Canvas {
    Ink,
    View,
    Luma,
}

impl SelectionRequest {
    /// The kind's wire name, for the refusals.
    fn kind_name(&self) -> &'static str {
        match self.operation {
            SourceSelection::Alpha {} => "alpha",
            SourceSelection::Luminance { .. } => "luminance",
            SourceSelection::Color { .. } => "color",
            SourceSelection::MagicWand { .. } => "magic_wand",
            SourceSelection::ColorRange { .. } => "color_range",
            SourceSelection::Similar { .. } => "similar",
            SourceSelection::Quick { .. } => "quick",
            SourceSelection::RefineEdge { .. } => "refine_edge",
        }
    }

    /// The canvas the kind reads on a CMYK document, or its refusal (module
    /// doc).
    fn cmyk_canvas(&self, doc: &PrecisionDocument) -> Result<Canvas, String> {
        let canvas = match self.operation {
            SourceSelection::MagicWand { .. }
            | SourceSelection::Similar { .. }
            | SourceSelection::Alpha {} => Canvas::Ink,
            SourceSelection::Color { channel } => {
                if channel.plate().is_none() {
                    return Err(refused_sentence(&format!(
                        "The \"{}\" channel selection",
                        channel.name()
                    )));
                }
                Canvas::Ink
            }
            SourceSelection::Luminance { gray: Some(_) } => {
                return Err("a CMYK document's luminosity takes no Gray working space".into());
            }
            SourceSelection::Luminance { gray: None } => Canvas::Luma,
            SourceSelection::ColorRange { .. }
            | SourceSelection::Quick { .. }
            | SourceSelection::RefineEdge { .. } => Canvas::View,
        };
        match self.source {
            AnalysisSource::Backdrop => {
                let below = self
                    .below
                    .ok_or("backdrop analysis requires below and no layer")?;
                if below > doc.layer_count() {
                    return Err(format!("record {below} is out of range"));
                }
                Ok(canvas)
            }
            AnalysisSource::Layer if canvas == Canvas::View => Err(refused_sentence(&format!(
                "A layer source for the \"{}\" selection",
                self.kind_name()
            ))),
            AnalysisSource::Layer => {
                let index = self.layer.ok_or("a layer selection source names a layer")?;
                doc.layer(index)
                    .and_then(|layer| layer.cmyk_source())
                    .ok_or("the selection source layer is not a CMYK raster layer")?;
                Ok(canvas)
            }
            AnalysisSource::Composite => Ok(canvas),
        }
    }

    /// The CMYK plan (module doc): the RGB plan's keys.
    pub(crate) fn cmyk_plan(&self, doc: &PrecisionDocument) -> Result<Value, String> {
        doc.require_cmyk_policy("a CMYK selection")?;
        let canvas = self.cmyk_canvas(doc)?;
        let (w, h) = doc.dimensions();
        let n = pixel_count(w, h)? as u64;
        let (input, held) = self.checked_buffers(doc)?;
        let source = match canvas {
            Canvas::Ink => n * CMYK_BYTES_PER_PIXEL,
            Canvas::View => n * 8 + CmykPolicy::view_bytes(),
            Canvas::Luma => n * 8 + memo_peak_bytes_for::<[f32; 3]>(n),
        };
        // A backdrop's stack is over fewer records than the document's, so
        // the document's own is its bound.
        let stack = if matches!(
            self.source,
            AnalysisSource::Composite | AnalysisSource::Backdrop
        ) {
            doc.cmyk_stack_scratch_bytes()?
        } else {
            0
        };
        let scratch = 8 * BOOKKEEPING_BYTES + n * 40 + held + source + stack + u64::from(w) * 20;
        Ok(
            json!({"retained_bytes":doc.retained_bytes(),"scratch_bytes":scratch,
            "input_samples":input,"output_samples":n,"width":w,"height":h}),
        )
    }

    /// The CMYK selection (module doc), admitted by [`Self::cmyk_plan`].
    pub(crate) fn cmyk_compute(
        &self,
        doc: &PrecisionDocument,
        input: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Vec<u16>, String> {
        let plan = self.cmyk_plan(doc)?;
        let scratch = plan["scratch_bytes"]
            .as_u64()
            .ok_or("invalid CMYK selection plan")?;
        admit(doc.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let job = job.with_reservation_floor(scratch);
        if input.len() as u64 != plan["input_samples"].as_u64().unwrap_or(u64::MAX) {
            return Err("typed selection buffer length differs from the core plan".into());
        }
        let canvas = self.cmyk_canvas(doc)?;
        let dimensions = doc.dimensions();
        let depth_bits = doc.require_cmyk_policy("a CMYK selection")?.depth.bits();
        // The document the canvas composites: the document, or the records
        // below `below` (`None`: no record, bare paper).
        let backdrop;
        let source = match (self.source, self.below) {
            (AnalysisSource::Backdrop, Some(0)) => None,
            (AnalysisSource::Backdrop, Some(below)) => {
                backdrop = doc.cmyk_backdrop(below)?;
                Some(&backdrop)
            }
            _ => Some(doc),
        };
        let layer = match self.source {
            AnalysisSource::Layer => self.layer,
            _ => None,
        };
        let read = job.range(0., 0.4);
        let work = job.range(0.4, 1.);
        let out = match canvas {
            Canvas::Ink => {
                let ink = match source {
                    Some(source) => source.cmyk_ink_canvas(layer, read)?,
                    None => filled(dimensions, [0; 5])?,
                };
                self.ink_kind(&ink, dimensions, input, depth_bits, work)?
            }
            Canvas::Luma => {
                let luma = match source {
                    Some(source) => source.cmyk_luma_canvas(layer, read)?,
                    None => {
                        let mut leg = srgb_leg(doc)?;
                        filled(dimensions, luma_of(&mut leg, [0.0; 5]))?
                    }
                };
                work.report(1., 5)?;
                luma
            }
            Canvas::View => {
                let view = match source {
                    Some(source) => source.cmyk_view_canvas(read)?,
                    None => {
                        let link = doc.require_cmyk_policy("the CMYK view")?.view()?;
                        filled(dimensions, PrecisionDocument::view_pixel(&link, [0.0; 5]))?
                    }
                };
                self.view_kind(doc, &view, dimensions, input, work)?
            }
        };
        job.report(1., 5)?;
        Ok(out)
    }

    /// An ink kind's coverage over the canvas-sized straight ink.
    fn ink_kind(
        &self,
        ink: &[CmykSample],
        dimensions: (u32, u32),
        input: &[u16],
        depth_bits: u32,
        work: JobContext<'_>,
    ) -> Result<Vec<u16>, String> {
        use crate::selection_region::ink_as_rgb;
        let per_pixel = |code: &dyn Fn(CmykSample) -> u16| -> Result<Vec<u16>, String> {
            let mut out = reserved_vec(ink.len())?;
            for (i, sample) in ink.iter().enumerate() {
                work.every(i)?;
                out.push(code(*sample));
            }
            Ok(out)
        };
        match &self.operation {
            SourceSelection::MagicWand {
                x,
                y,
                tolerance,
                contiguous,
                sample_size,
            } => crate::selection_region::grow_ink(
                dimensions,
                |i| ink[i],
                (*x, *y),
                *tolerance,
                *contiguous,
                *sample_size,
                depth_bits,
                work,
            ),
            SourceSelection::Similar {
                tolerance,
                contiguous,
            } => crate::selection_similar::similar(
                dimensions,
                |i| ink_as_rgb(ink[i], depth_bits),
                input,
                *tolerance,
                *contiguous,
                work,
            ),
            SourceSelection::Alpha {} => per_pixel(&|p| p[4]),
            SourceSelection::Color { channel } => {
                let plate = channel.plate().ok_or_else(|| {
                    refused_sentence(&format!("The \"{}\" channel selection", channel.name()))
                })?;
                per_pixel(&|p| plate_coverage(p, plate))
            }
            _ => Err(refused_sentence(&format!(
                "The \"{}\" selection",
                self.kind_name()
            ))),
        }
    }

    /// A view kind's coverage over the canvas-sized Display P3 view.
    fn view_kind(
        &self,
        doc: &PrecisionDocument,
        view: &[[u16; 4]],
        dimensions: (u32, u32),
        input: &[u16],
        work: JobContext<'_>,
    ) -> Result<Vec<u16>, String> {
        let pixel = |i: usize| view[i];
        match &self.operation {
            SourceSelection::ColorRange { params } => crate::selection_tools::color_range_samples(
                doc.profile(),
                dimensions,
                pixel,
                &Value::Object(params.clone()).to_string(),
                work,
            ),
            SourceSelection::Quick { params } => crate::selection_tools::quick_samples(
                doc.profile(),
                dimensions,
                pixel,
                &Value::Object(params.clone()).to_string(),
                work,
            ),
            SourceSelection::RefineEdge {
                edge_radius,
                smart_radius,
                area,
            } => crate::precision_selection::refined_coverage(
                view,
                input,
                dimensions,
                (*edge_radius, *smart_radius, *area),
                work,
            ),
            _ => Err(refused_sentence(&format!(
                "The \"{}\" selection",
                self.kind_name()
            ))),
        }
    }
}

/// A canvas-sized vector of one value (a backdrop with no record).
fn filled<T: Copy>(dimensions: (u32, u32), value: T) -> Result<Vec<T>, String> {
    let n = pixel_count(dimensions.0, dimensions.1)?;
    let mut out = reserved_vec(n)?;
    out.resize(n, value);
    Ok(out)
}

/// The composite luminosity's leg (module doc): the document's CMYK profile
/// into the built-in sRGB, relative colorimetric with BPC.
fn srgb_leg(doc: &PrecisionDocument) -> Result<CmykToRgb<'_>, String> {
    let policy = doc.require_cmyk_policy("the CMYK luminosity")?;
    CmykToRgb::new(
        &policy.profile,
        &IccProfile::srgb(),
        ConversionOptions::default(),
    )
}

/// One straight ink sample's luminosity code (module doc).
fn luma_of(leg: &mut CmykToRgb<'_>, ink: InkPixel) -> u16 {
    let sample = codes(ink);
    let ink4 = [sample[0], sample[1], sample[2], sample[3]];
    let rgb = leg.rgb(ink_key(ink4), ink4.map(|v| f64::from(unit(v))));
    let c = |v: f32| (v.clamp(0.0, 1.0) * 65535.0).round() as u16;
    crate::blend::luma_code([c(rgb[0]), c(rgb[1]), c(rgb[2]), sample[4]])
}

impl PrecisionDocument {
    /// The canvas-sized straight CMYKA16 ink, row-major: the visible
    /// composite (`layer` None) or record `layer`'s own ink at full opacity
    /// with no mask (`cmyk_stack`'s `only`), streamed a row at a time.
    pub(crate) fn cmyk_ink_canvas(
        &self,
        layer: Option<usize>,
        job: JobContext<'_>,
    ) -> Result<Vec<CmykSample>, String> {
        let (w, h) = self.dimensions();
        let mut stack = self.cmyk_stack(layer, job)?;
        let mut out = reserved_vec(pixel_count(w, h)?)?;
        let mut acc = vec![[0f32; 5]; w as usize];
        for y in 0..h {
            job.check()?;
            stack.row(i64::from(y), 0, &mut acc);
            out.extend(acc.iter().map(|ink| codes(*ink)));
            job.report(f64::from(y + 1) / f64::from(h), 5)?;
        }
        Ok(out)
    }

    /// The canvas-sized view, RGBA16 Display P3 codes with the composite's
    /// alpha, row-major: `cmyk_view_window`'s rows (`view_pixel` over the
    /// stack's straight ink) into one vector.
    pub(crate) fn cmyk_view_canvas(&self, job: JobContext<'_>) -> Result<Vec<[u16; 4]>, String> {
        let (w, h) = self.dimensions();
        let view = self.require_cmyk_policy("the CMYK view")?.view()?;
        let mut stack = self.cmyk_stack(None, job)?;
        let mut out = reserved_vec(pixel_count(w, h)?)?;
        let mut acc = vec![[0f32; 5]; w as usize];
        for y in 0..h {
            job.check()?;
            stack.row(i64::from(y), 0, &mut acc);
            out.extend(acc.iter().map(|ink| Self::view_pixel(&view, *ink)));
            job.report(f64::from(y + 1) / f64::from(h), 5)?;
        }
        Ok(out)
    }

    /// The luminosity (module doc), row-major u16 codes: of the visible
    /// composite (`layer` None) or of record `layer`'s own ink at full
    /// opacity with no mask (`cmyk_stack`'s `only`).
    pub(crate) fn cmyk_luma_canvas(
        &self,
        layer: Option<usize>,
        job: JobContext<'_>,
    ) -> Result<Vec<u16>, String> {
        let (w, h) = self.dimensions();
        let mut leg = srgb_leg(self)?;
        let mut stack = self.cmyk_stack(layer, job)?;
        let mut out = reserved_vec(pixel_count(w, h)?)?;
        let mut acc = vec![[0f32; 5]; w as usize];
        for y in 0..h {
            job.check()?;
            stack.row(i64::from(y), 0, &mut acc);
            for ink in &acc {
                out.push(luma_of(&mut leg, *ink));
            }
            job.report(f64::from(y + 1) / f64::from(h), 5)?;
        }
        Ok(out)
    }

    /// The records below `below` (a postorder record index, 1 ≤ below ≤ the
    /// record count): every record whose FIRST postorder index is less than
    /// `below`, each enclosing group kept with its opacity, mask, mode and
    /// isolation around the children it keeps; no channel. The source of a
    /// `backdrop` selection and of the CMYK tone analysis's backdrop
    /// (`AnalysisSource`'s contract: "visible contributions with bottom-first
    /// record index less than below; enclosing group opacity/mask/isolation
    /// retained").
    pub(crate) fn cmyk_backdrop(&self, below: usize) -> Result<PrecisionDocument, String> {
        fn kept(
            layers: &[PrecisionDocumentLayer],
            next: &mut usize,
            below: usize,
        ) -> Result<Vec<PrecisionDocumentLayer>, String> {
            let mut out = Vec::new();
            for layer in layers {
                let first = *next;
                match layer.children() {
                    Some(children) => {
                        let children = kept(children, next, below)?;
                        *next += 1;
                        if first < below && !children.is_empty() {
                            out.push(carry(
                                layer,
                                PrecisionDocumentLayer::group(
                                    &layer.name,
                                    children,
                                    layer.opacity,
                                    layer.visible,
                                    layer.is_pass_through(),
                                )?,
                            ));
                        }
                    }
                    None => {
                        *next += 1;
                        if first < below {
                            out.push(layer.clone());
                        }
                    }
                }
            }
            Ok(out)
        }
        if below == 0 || below > self.layer_count() {
            return Err(format!("record {below} is out of range"));
        }
        let (w, h) = self.dimensions();
        let layers = kept(self.root_layers(), &mut 0, below)?;
        self.rebuilt(w, h, layers, Vec::new())
    }

    /// The visible composite's straight ink as one canvas-sized CMYK raster
    /// (ink and alpha kept): Select and Mask's New Layer foreground
    /// (`cmyk_cutout`).
    pub(crate) fn cmyk_composite_image(&self, job: JobContext<'_>) -> Result<CmykImage, String> {
        let (w, h) = self.dimensions();
        let mut stack = self.cmyk_stack(None, job)?;
        let mut acc = vec![[0f32; 5]; w as usize];
        CmykImage::from_rows_with_job(
            w,
            h,
            |y, row| {
                stack.row(i64::from(y), 0, &mut acc);
                for (dst, ink) in row.iter_mut().zip(acc.iter()) {
                    *dst = codes(*ink);
                }
                job.check()
            },
            job,
        )
    }
}
