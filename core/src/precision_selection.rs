//! Captured source-derived native selections. Named sources share the scoped
//! analysis compositor; raw source placement never passes through a byte plate.
//!
//! Native source selection reuses the scoped compositor, generic byte/native Lab range/Quick
//! algorithms and `selection_region`'s one anchored wand walk. `precision_scalar_selection.rs`
//! tests independent arithmetic, exact outside/other channels, native source thresholds, placement,
//! transforms, no-ops/refusals and cancellation after actual Gaussian-row progress. At 32-bit float
//! the source selection's `alpha` / `color` kinds place each f32 sample through
//! `unit_to_u16` (clamp to 0…1, Photoshop 2025's measured mapping, whatever the tone map) inside
//! `canvas_samples`, and `luminance` places the document profile's Y (its rXYZ/gXYZ/bXYZ Y row,
//! `MatrixTrc::linear_to_pcs`) over the UNclamped samples through the same `unit_to_u16`, which
//! Photoshop 2025 matched exactly on sRGB and ROMM documents. At 16 bits `luminance` with
//! `gray` (Color Settings' Gray working space) is `gray_working`'s grey of the profile's PCS Y
//! (`cmyk_color::document_xyz`), measured against Photoshop 2025; a float or CMYK document refuses
//! `gray`. `magic_wand` / `color_range` / `similar` / `quick` place each f32 sample as its R1
//! display code (`display_encoding::display_code`: the profile's forward curve over the sample
//! clamped to 0…1, so a value above white compares as white; never the tone map) and run the
//! same kernels on those codes (R2). The `refine_edge` kind is Select and Mask's edge stage:
//! `edge_refine` over the captured composite as the guide, with the coverage to refine (and the
//! Refine Edge categories) as its input. At float it reads the same R1 codes as the comparison
//! kinds (`display_encoding` R2, rule E), so a value above white refines as flat white; its
//! output is native u16 coverage at every depth. A CMYK document dispatches first to
//! `cmyk_select` (#1): the wand, Similar, alpha and the ink plates on ink, the colour
//! kinds on the view; an ink plate refuses on an RGB document. On a Lab
//! document `color_range` reads the stored L\*a\*b\* codes by `selection_tools`'
//! Lab arm (#1 phase 4b-ii, `lab_select`); its wand, Similar and Grow
//! compare `lab_select::wand_key` (#5) wherever
//! `lab_document::selection_refusal` admits them. Since phase 6b-ii `quick` reads the codes'
//! own L\*a\*b\* (`selection_tools::quick_samples`), and `refine_edge` guides
//! on the codes per channel, the RGB kernel with no Lab arm.
use crate::cmyk_color::document_xyz;
use crate::display_encoding::display_code;
use crate::gray_working::GrayWorkingSpace;
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{
    admit, pixel_count, reserved_vec, unit_to_u16, PrecisionImage, PrecisionRun, BOOKKEEPING_BYTES,
};
use crate::scoped_statistics::{AnalysisRequest, AnalysisSource};
use serde::Deserialize;
use serde_json::{json, Map, Value};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionRequest {
    pub(crate) op: String,
    #[serde(default = "composite")]
    pub(crate) source: AnalysisSource,
    pub(crate) layer: Option<usize>,
    pub(crate) below: Option<usize>,
    pub(crate) operation: SourceSelection,
}
fn composite() -> AnalysisSource {
    AnalysisSource::Composite
}
fn yes() -> bool {
    true
}
fn one() -> u32 {
    1
}
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum SourceSelection {
    Alpha {},
    /// The composite's luminosity; at 8 and 16 bits with `gray` (Color
    /// Settings' Gray working space) it is that space's
    /// grey of the document profile's PCS Y (`gray_working`), else Rec. 709.
    Luminance {
        #[serde(default)]
        gray: Option<GrayWorkingSpace>,
    },
    Color {
        channel: ColorChannel,
    },
    MagicWand {
        x: u32,
        y: u32,
        tolerance: u16,
        #[serde(default = "yes")]
        contiguous: bool,
        /// The Sample Size: 1 = Point Sample, else the box mean of
        /// the compared codes (`selection_region` module doc).
        #[serde(default = "one")]
        sample_size: u32,
    },
    ColorRange {
        params: Map<String, Value>,
    },
    Similar {
        tolerance: u16,
        contiguous: bool,
    },
    Quick {
        params: Map<String, Value>,
    },
    /// Select and Mask's edge stage (`edge_refine`): the input is the
    /// coverage to refine (n samples), then, when `area`, the Refine Edge
    /// categories as u16 values 0 / 128 / 255 (2n in all).
    RefineEdge {
        edge_radius: f32,
        #[serde(default)]
        smart_radius: bool,
        #[serde(default)]
        area: bool,
    },
}
/// A `color` selection's channel: an RGB document's red, green or blue, or
/// a CMYK document's ink plate (`cmyk_select`), each refused on the other.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorChannel {
    Red,
    Green,
    Blue,
    Cyan,
    Magenta,
    Yellow,
    Black,
}
impl ColorChannel {
    /// The wire name.
    pub(crate) fn name(self) -> &'static str {
        match self {
            ColorChannel::Red => "red",
            ColorChannel::Green => "green",
            ColorChannel::Blue => "blue",
            ColorChannel::Cyan => "cyan",
            ColorChannel::Magenta => "magenta",
            ColorChannel::Yellow => "yellow",
            ColorChannel::Black => "black",
        }
    }
    /// R 0, G 1, B 2; `None` for an ink plate.
    pub(crate) fn rgb_index(self) -> Option<usize> {
        match self {
            ColorChannel::Red => Some(0),
            ColorChannel::Green => Some(1),
            ColorChannel::Blue => Some(2),
            _ => None,
        }
    }
    /// C 0, M 1, Y 2, K 3; `None` for an RGB channel.
    pub(crate) fn plate(self) -> Option<usize> {
        match self {
            ColorChannel::Cyan => Some(0),
            ColorChannel::Magenta => Some(1),
            ColorChannel::Yellow => Some(2),
            ColorChannel::Black => Some(3),
            _ => None,
        }
    }
    /// An ink plate's refusal on an RGB document.
    pub(crate) fn rgb_refusal(self) -> String {
        format!("The {} channel requires a CMYK document", self.name())
    }
}
impl SelectionRequest {
    pub fn parse(text: &str) -> Result<Self, String> {
        let request: Self = serde_json::from_str(text)
            .map_err(|e| format!("invalid native source selection: {e}"))?;
        if request.op != "selection" {
            return Err("expected selection request".into());
        }
        request.analysis()?;
        Ok(request)
    }
    fn analysis(&self) -> Result<AnalysisRequest, String> {
        AnalysisRequest::parse(&json!({"operation":"histogram","source":self.source_name(),"layer":self.layer,"below":self.below}).to_string())
    }
    fn source_name(&self) -> &'static str {
        match self.source {
            AnalysisSource::Composite => "composite",
            AnalysisSource::Layer => "layer",
            AnalysisSource::Backdrop => "backdrop",
        }
    }
    pub fn plan(&self, doc: &PrecisionDocument) -> Result<Value, String> {
        if doc.is_cmyk() {
            return self.cmyk_plan(doc);
        }
        if let SourceSelection::Color { channel } = &self.operation {
            if channel.rgb_index().is_none() {
                return Err(channel.rgb_refusal());
            }
        }
        if let SourceSelection::Luminance { gray: Some(_) } = &self.operation {
            if doc.is_float() {
                return Err(
                    "a 32-bit float document's luminance takes no Gray working space".into(),
                );
            }
            if doc.profile().model().is_none() && doc.profile().lut().is_none() {
                return Err("selection source requires a matrix/TRC RGB profile".into());
            }
        }
        let (w, h) = doc.dimensions();
        let n = pixel_count(w, h)? as u64;
        let (input, held) = self.checked_buffers(doc)?;
        let scratch =
            8 * BOOKKEEPING_BYTES + n * 40 + held + doc.scoped_analysis_plan(&self.analysis()?)?;
        Ok(
            json!({"retained_bytes":doc.retained_bytes(),"scratch_bytes":scratch,"input_samples":input,"output_samples":n,"width":w,"height":h}),
        )
    }
    /// The checks every plan runs before its arithmetic — the wand's seed and
    /// each kind's parameters — and the kind's planned input samples and the
    /// bytes it holds beside the shared 40 B a pixel `(input, held)`. Shared by
    /// the RGB plan and the CMYK one (`cmyk_select`).
    pub(crate) fn checked_buffers(&self, doc: &PrecisionDocument) -> Result<(u64, u64), String> {
        let (w, h) = doc.dimensions();
        let n = pixel_count(w, h)? as u64;
        if let SourceSelection::MagicWand {
            x, y, sample_size, ..
        } = &self.operation
        {
            if *x >= w || *y >= h {
                return Err("wand seed is outside the canvas".into());
            }
            if !crate::selection_region::WAND_SAMPLE_SIZES.contains(sample_size) {
                return Err("wand sample_size must be 1, 3, 5, 11, 31, 51 or 101".into());
            }
        }
        match &self.operation {
            SourceSelection::ColorRange { params } => {
                crate::selection_tools::color_range_parameters::<u16>(
                    doc.profile(),
                    (w, h),
                    &Value::Object(params.clone()).to_string(),
                )?;
            }
            SourceSelection::Quick { params } => {
                if doc.profile().model().is_none() && !doc.is_lab() {
                    return Err("selection source requires a matrix/TRC RGB profile".into());
                }
                crate::selection_tools::quick_parameters(
                    (w, h),
                    &Value::Object(params.clone()).to_string(),
                )?;
            }
            SourceSelection::RefineEdge { edge_radius, .. } => {
                if !edge_radius.is_finite()
                    || !(0.0..=crate::edge_refine::MAX_EDGE_RADIUS).contains(edge_radius)
                {
                    return Err(format!(
                        "edge_radius must be between 0 and {}",
                        crate::edge_refine::MAX_EDGE_RADIUS
                    ));
                }
            }
            _ => {}
        }
        // The immutable compositor result, Lab/queue/output and final output
        // staging remain live together. Named raw-leaf sources share storage.
        // Quick Selection's brush keeps this 40 B a pixel: its smoothing
        // ring and hole-fill bitset live in budget the growth frees
        // (`selection_quick`, MEMORY). Record (`edge_refine::BYTES_PER_PIXEL`'s
        // harness and document; `quick_select` with one seed, brush_size 201,
        // smooth 1): E 788,791,872 / 788,906,536 / 788,791,872 against a plan
        // of at least 1,161,437,184 (1.472×; 9 MiB + 48 B a pixel, leaving out
        // the analysis term's render and statistics scratch).
        // At float the same plan holds; measured with
        // `examples/float_selection_admission.rs` (Release, one case per
        // process, `MallocLargeCache=0`, three runs and a twin reading 0, a
        // 6000×4000 float document, the composite source, E the interval
        // peak over the call): wand (seed (0, 0), tolerance 65535) E max
        // 769,999,400, range (sampled, tolerance 50) and quick (one seed,
        // brush 201, smooth 1) 769,983,016, against a plan of 2,340,582,912;
        // the edge stage (`refine`: edge radius 8, smart radius, a centred
        // rectangle of coverage, rule E) E 769,966,632 / 769,966,632 /
        // 769,983,040 against a plan of 3,924,582,912;
        // `selected_color_planes_24mp.rs` pins ⌈1.25 × max E⌉ of each.
        // Similar also holds its captured u16 coverage input. The edge stage
        // holds its input (coverage, and the categories when `area`) and
        // `edge_refine`'s own bound, which covers the f32 coverage, the byte
        // categories and the result beside the kernel's planes.
        Ok(match self.operation {
            SourceSelection::Similar { .. } => (n, 2 * n),
            SourceSelection::RefineEdge { area, .. } => {
                let input = if area { 2 * n } else { n };
                (input, input * 2 + crate::edge_refine::BYTES_PER_PIXEL * n)
            }
            _ => (0, 0),
        })
    }
    pub(crate) fn compute(
        &self,
        doc: &PrecisionDocument,
        input: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Vec<u16>, String> {
        if doc.is_cmyk() {
            return self.cmyk_compute(doc, input, limit, other, job);
        }
        let lab = self.lab_grid(doc)?;
        let plan = self.plan(doc)?;
        let scratch = plan["scratch_bytes"].as_u64().unwrap();
        admit(doc.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let job = job.with_reservation_floor(scratch);
        let request = self.analysis()?;
        let (image, offset) = doc.analysis_source(
            &request,
            limit,
            other + scratch - doc.scoped_analysis_plan(&request)?,
            job.range(0., 0.4),
        )?;
        let dimensions = doc.dimensions();
        let (w, h) = dimensions;
        // f32 samples clamp to 0...1 (Photoshop 2025 measured, LIN).
        // A float `luminance` weights the UNclamped samples by the
        // document profile's own Y row and clamps only the sum (Photoshop
        // 2025 measured on sRGB and ROMM), so its Y code is placed in R, G
        // and B here and the `Luminance` arm reads it back unweighted.
        let float_luma =
            doc.is_float() && matches!(self.operation, SourceSelection::Luminance { gray: None });
        // The Gray working space's grey of the profile's relative PCS Y over
        // the 16-bit codes (`plan` refused float and a profile with no Y).
        let gray = match self.operation {
            SourceSelection::Luminance { gray } => gray,
            _ => None,
        };
        let model =
            if float_luma {
                Some(doc.profile().model().ok_or_else(|| {
                    "selection source requires a matrix/TRC RGB profile".to_string()
                })?)
            } else {
                None
            };
        // R2: the tolerance and distance kinds and the edge stage
        // compare R1 display codes, never the clamped linear samples.
        let display = doc.float.as_deref().filter(|_| {
            matches!(
                self.operation,
                SourceSelection::MagicWand { .. }
                    | SourceSelection::ColorRange { .. }
                    | SourceSelection::Similar { .. }
                    | SourceSelection::Quick { .. }
                    | SourceSelection::RefineEdge { .. }
            )
        });
        let codes = |p: [f32; 4]| match (model, display) {
            (Some(model), _) => {
                let y = model.linear_to_pcs([p[0], p[1], p[2]].map(f64::from))[1];
                let code = unit_to_u16(y as f32);
                [code, code, code, unit_to_u16(p[3])]
            }
            (None, Some(policy)) => display_code(policy.encoder(), p),
            (None, None) => p.map(unit_to_u16),
        };
        let canvas = canvas_samples(&image, offset, dimensions, codes)?;
        drop(image);
        let pixel = |i: usize| canvas[i];
        let work = job.range(0.4, 1.);
        let out = match &self.operation {
            SourceSelection::ColorRange { params } => crate::selection_tools::color_range_samples(
                doc.profile(),
                dimensions,
                pixel,
                &Value::Object(params.clone()).to_string(),
                work,
            )?,
            SourceSelection::Quick { params } => crate::selection_tools::quick_samples(
                doc.profile(),
                dimensions,
                pixel,
                &Value::Object(params.clone()).to_string(),
                work,
            )?,
            SourceSelection::MagicWand {
                x,
                y,
                tolerance,
                contiguous,
                sample_size,
            } => match lab {
                Some(grid) => crate::lab_select::grow(
                    dimensions,
                    pixel,
                    (*x, *y),
                    *tolerance,
                    *contiguous,
                    *sample_size,
                    grid,
                    work,
                )?,
                None => crate::selection_region::grow(
                    dimensions,
                    pixel,
                    (*x, *y),
                    *tolerance,
                    *contiguous,
                    *sample_size,
                    work,
                )?,
            },
            SourceSelection::Similar {
                tolerance,
                contiguous,
            } => match lab {
                Some(grid) => crate::lab_select::similar(
                    dimensions,
                    pixel,
                    input,
                    *tolerance,
                    *contiguous,
                    grid,
                    work,
                )?,
                None => crate::selection_similar::similar(
                    dimensions,
                    pixel,
                    input,
                    *tolerance,
                    *contiguous,
                    work,
                )?,
            },
            SourceSelection::RefineEdge {
                edge_radius,
                smart_radius,
                area,
            } => refined_coverage(
                &canvas,
                input,
                dimensions,
                (*edge_radius, *smart_radius, *area),
                work,
            )?,
            operation => {
                let n = w as usize * h as usize;
                let mut out = reserved_vec(n)?;
                for i in 0..n {
                    work.every(i)?;
                    if i & 65535 == 0 {
                        work.report(i as f64 / n as f64, 6)?;
                    }
                    let p = pixel(i);
                    let code = match operation {
                        SourceSelection::Alpha {} => p[3],
                        SourceSelection::Color { channel } => {
                            p[channel.rgb_index().ok_or_else(|| channel.rgb_refusal())?]
                        }
                        _ if float_luma => p[0],
                        _ => match gray {
                            Some(space) => {
                                let rgb = [p[0], p[1], p[2]].map(|v| f64::from(v) / 65535.0);
                                let y = document_xyz(doc.profile(), rgb)[1];
                                unit_to_u16(space.gray(y) as f32)
                            }
                            None => crate::blend::luma_code(p),
                        },
                    };
                    out.push(code);
                }
                out
            }
        };
        job.report(1., 5)?;
        Ok(out)
    }
    /// The Lab grid a Lab document's wand or Similar/Grow compares
    /// `lab_select`'s key on (#5), `None` for every other kind and
    /// document; the refusal of a (key, depth) `lab_document`'s table refuses.
    fn lab_grid(
        &self,
        doc: &PrecisionDocument,
    ) -> Result<Option<crate::lab_blend::LabGrid>, String> {
        let (kind, contiguous) = match self.operation {
            SourceSelection::MagicWand { contiguous, .. } => ("magic_wand", contiguous),
            SourceSelection::Similar { contiguous, .. } => ("similar", contiguous),
            _ => return Ok(None),
        };
        let Some(policy) = doc.lab_policy() else {
            return Ok(None);
        };
        let key = crate::lab_document::selection_key(kind, contiguous);
        if let Some(refusal) =
            key.and_then(|key| crate::lab_document::selection_refusal(key, policy.depth()))
        {
            return Err(refusal);
        }
        Ok(doc.lab_grid())
    }
}

/// Select and Mask's edge stage over a captured canvas of u16 codes (the RGB
/// arm's samples, or a CMYK document's view codes, `cmyk_select`): `input` is
/// the coverage to refine, then the categories when `area`; `edge_refine`
/// reads the canvas premultiplied by its alpha. The output is u16 coverage.
pub(crate) fn refined_coverage(
    canvas: &[[u16; 4]],
    input: &[u16],
    dimensions: (u32, u32),
    (edge_radius, smart_radius, area): (f32, bool, bool),
    work: JobContext<'_>,
) -> Result<Vec<u16>, String> {
    let n = dimensions.0 as usize * dimensions.1 as usize;
    if input.len() != if area { 2 * n } else { n } {
        return Err("typed selection buffer length differs from the core plan".into());
    }
    let mut coverage: Vec<f32> = reserved_vec(n)?;
    coverage.extend(input[..n].iter().map(|v| f32::from(*v) / 65535.0));
    // A category outside a byte is not one of the three: 1 makes
    // the kernel name it.
    let categories: Option<Vec<u8>> = area.then(|| {
        input[n..]
            .iter()
            .map(|v| u8::try_from(*v).unwrap_or(1))
            .collect()
    });
    let refined = crate::edge_refine::refine(
        |i| {
            let p = canvas[i];
            let alpha = f32::from(p[3]) / 65535.0;
            [p[0], p[1], p[2]].map(|v| f32::from(v) / 65535.0 * alpha)
        },
        &coverage,
        categories.as_deref(),
        dimensions,
        crate::edge_refine::EdgeRefine {
            radius: edge_radius,
            smart: smart_radius,
        },
        work,
    )?;
    let mut out = reserved_vec(n)?;
    for (i, value) in refined.iter().enumerate() {
        work.every(i)?;
        out.push((f64::from(*value).clamp(0., 1.) * 65535.).round() as u16);
    }
    Ok(out)
}

/// The canvas-sized row-major u16 samples of `image` placed at `offset`:
/// `Window::u16`'s column/row rule (`precision_clipboard.rs`) with the window
/// the whole canvas, no coverage and no checkpoint — zeros where the image
/// does not reach, else its `row_segments` runs. An f32 run is placed
/// through `codes`, pixel by pixel.
fn canvas_samples(
    image: &PrecisionImage,
    offset: (i32, i32),
    (width, height): (u32, u32),
    codes: impl Fn([f32; 4]) -> [u16; 4],
) -> Result<Vec<[u16; 4]>, String> {
    let (w, h) = image.dimensions();
    let a = -i64::from(offset.0);
    let lx0 = a.clamp(0, i64::from(w));
    let lx1 = (a + i64::from(width)).clamp(0, i64::from(w));
    let row = width as usize;
    let mut out = reserved_vec(pixel_count(width, height)?)?;
    for y in 0..height {
        let start = out.len();
        let ly = i64::from(y) - i64::from(offset.1);
        if ly >= 0 && ly < i64::from(h) && lx0 < lx1 {
            out.resize(start + (lx0 - a) as usize, [0; 4]);
            for (_, run) in image.row_segments(ly as u32, lx0 as u32, lx1 as u32) {
                match run {
                    PrecisionRun::Rgba16(run) => out.extend_from_slice(run),
                    PrecisionRun::Rgba32(run) => out.extend(run.iter().map(|p| codes(*p))),
                }
            }
        }
        out.resize(start + row, [0; 4]);
    }
    Ok(out)
}
