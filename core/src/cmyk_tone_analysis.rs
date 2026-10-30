//! What Levels and Curves read from a CMYK document (#1 phase 5): the
//! per-plate histograms, Auto's endpoints under Auto Options, the black,
//! grey and white eyedroppers' sample and target, and Curves' ⌥ clipping
//! display, with their plans.
//!
//! # The histogram
//!
//! Levels runs on `L = 1 − ink` (`cmyk_adjust`), so the histograms are of
//! L: one 65536-bin plane per plate of `65535 − ink16`, each pixel weighted
//! by its alpha × the selection coverage (`selected_adjustment::coverage_at`
//! for a layer's own pixels; a canvas pixel's coverage otherwise), the
//! source being one raster's own ink (`"layer"`), the composite of the
//! records below an adjustment record (`"backdrop"`, `below` a postorder
//! record index, enclosing groups kept: `cmyk_select::cmyk_backdrop`) or the whole
//! composite (`"composite"`), both read through `cmyk_stack`. The display
//! histograms are those planes in 256 bins (`round(L16 / 257)`, the white
//! walk's own 8-bit binning, `ops_auto`'s `bin8`), in pixel units; the
//! CMYK (composite) histogram is the four plates summed.
//!
//! # Auto
//!
//! `per_channel` (Enhance Per Channel Contrast) clips the C, M
//! and Y plates each on its own L plane through
//! `ops_auto::contrast_endpoints_clipped` — Auto Contrast's one rule
//! (`black_code`, the white walk) with separate Shadows and Highlights
//! clip masses — and leaves K at (0, 1): Photoshop's per-channel CMYK Auto
//! holds the black plate (the phase-5 take; confirmed on every pixel by
//! `cmyk_k_take.py`'s take). `monochromatic` (Enhance Monochromatic
//! Contrast) takes one pair from the plane of all four plates summed and
//! gives it to all four, K included: that take's Auto Contrast moved K (by
//! up to 19 codes), so its build table kept this form (MA). The clip
//! fractions are Auto Options' percentages (0.1 % by default, 0…9.99). A
//! plate that is empty or flat keeps (0, 1); with nothing counted the
//! answer is `{"empty":true}`. The endpoints are L fractions — a Levels
//! stage's own `black`/`white`.
//!
//! # The eyedroppers
//!
//! The sample is the composite's ink at `(x, y)` over the `reach` square
//! (`cmyk_sample`'s rule). The target is where that sample should go: white
//! = paper, `[0, 0, 0, 0]`; black = sRGB black separated through the
//! document's profile (`cmyk_color::srgb_to_ink`, the request's Color
//! Settings intent and BPC — the profile's rich black); grey = the sample's
//! own PCS Lab (the profile's relative `A2B`) with `a* = b* = 0`, back
//! through the profile's relative `B2A` with no BPC — `ToDevice` at relative
//! without BPC is exactly that one `from_xyz`.
//!
//! # The clipping display
//!
//! The composite on paper (`cmyk_composite::on_paper`), opaque RGBA8:
//! `highlights` shows white where every listed plate is at zero ink (clipped
//! to paper) and black elsewhere; `shadows` shows black where any listed
//! plate is at 100 % and white elsewhere.

use serde_json::{json, Map, Value};

use crate::cmyk_color::{srgb_to_ink, ConversionOptions};
use crate::cmyk_composite::{code, on_paper, unit};
use crate::dr_image::DrImage;
use crate::icc_intent::RenderingIntent;
use crate::icc_lut::{lab_to_xyz, xyz_to_lab};
use crate::job_control::{DrJobControl, JobContext};
use crate::ops_auto::contrast_endpoints_clipped;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{admit, pixel_count, reserved_vec, BOOKKEEPING_BYTES};
use crate::selected_adjustment::{coverage_at, validate_coverage};

/// Bins of one L plane.
const BINS: usize = 65536;

/// One whole counted pixel's mass: an opaque pixel inside a full selection.
const UNIT: f64 = 65535.0;

/// The fixed working memory of an analysis: the four u64 planes and the
/// summed one (2.6 MB), their display bins and bookkeeping.
const ANALYSIS_FIXED_BYTES: u64 = 5 * BINS as u64 * 8 + BOOKKEEPING_BYTES;

/// Bytes a canvas pixel the clipping display charges: the RGBA8 result,
/// the host's copy and its drawing (`clip-display-16`); a miss raises this
/// term.
pub(crate) const CLIP_BYTES_PER_PIXEL: u64 = 16;

/// Where the analysis reads its pixels (module doc).
enum Source {
    Layer(usize),
    Backdrop(usize),
    Composite,
}

struct AutoRequest {
    monochromatic: bool,
    shadows: f64,
    highlights: f64,
}

struct Eyedropper {
    kind: Kind,
    x: i32,
    y: i32,
    reach: u32,
    options: ConversionOptions,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Black,
    Gray,
    White,
}

struct Request {
    source: Source,
    auto: Option<AutoRequest>,
    eyedropper: Option<Eyedropper>,
}

fn keys(object: &Map<String, Value>, allowed: &[&str], what: &str) -> Result<(), String> {
    match object.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(key) => Err(format!("{what} has an unknown key \"{key}\"")),
        None => Ok(()),
    }
}

fn index(object: &Map<String, Value>, key: &str) -> Result<usize, String> {
    object
        .get(key)
        .and_then(Value::as_u64)
        .map(|v| v as usize)
        .ok_or_else(|| format!("the CMYK tone analysis needs \"{key}\", a record index"))
}

fn percent(object: &Map<String, Value>, key: &str) -> Result<f64, String> {
    match object.get(key).filter(|v| !v.is_null()) {
        None => Ok(0.1),
        Some(value) => value
            .as_f64()
            .filter(|v| (0.0..10.0).contains(v))
            .ok_or_else(|| format!("Auto Options {key} is a percentage from 0 to 9.99")),
    }
}

impl Request {
    fn parse(text: &str) -> Result<Self, String> {
        let value: Value = if text.trim().is_empty() {
            Value::Object(Map::new())
        } else {
            serde_json::from_str(text)
                .map_err(|e| format!("the CMYK tone analysis request is not valid JSON: {e}"))?
        };
        let object = value
            .as_object()
            .ok_or("the CMYK tone analysis request must be a JSON object")?;
        keys(
            object,
            &["source", "layer", "below", "auto", "eyedropper"],
            "the CMYK tone analysis request",
        )?;
        let source = match object.get("source").and_then(Value::as_str) {
            None | Some("composite") => Source::Composite,
            Some("layer") => Source::Layer(index(object, "layer")?),
            Some("backdrop") => Source::Backdrop(index(object, "below")?),
            Some(other) => {
                return Err(format!(
                    "the CMYK tone analysis source \"{other}\" is not one of layer, backdrop or composite"
                ))
            }
        };
        let auto = match object.get("auto").filter(|v| !v.is_null()) {
            None => None,
            Some(value) => {
                let auto = value.as_object().ok_or("auto must be a JSON object")?;
                keys(auto, &["algorithm", "shadows", "highlights"], "auto")?;
                let monochromatic = match auto.get("algorithm").and_then(Value::as_str) {
                    None | Some("per_channel") => false,
                    Some("monochromatic") => true,
                    Some(other) => {
                        return Err(format!(
                        "the Auto algorithm \"{other}\" is not one of per_channel or monochromatic"
                    ))
                    }
                };
                Some(AutoRequest {
                    monochromatic,
                    shadows: percent(auto, "shadows")? / 100.0,
                    highlights: percent(auto, "highlights")? / 100.0,
                })
            }
        };
        let eyedropper = match object.get("eyedropper").filter(|v| !v.is_null()) {
            None => None,
            Some(value) => {
                let pick = value
                    .as_object()
                    .ok_or("eyedropper must be a JSON object")?;
                keys(
                    pick,
                    &[
                        "kind",
                        "x",
                        "y",
                        "reach",
                        "intent",
                        "black_point_compensation",
                    ],
                    "eyedropper",
                )?;
                let kind = match pick.get("kind").and_then(Value::as_str) {
                    Some("black") => Kind::Black,
                    Some("gray") => Kind::Gray,
                    Some("white") => Kind::White,
                    _ => return Err("eyedropper kind is black, gray or white".into()),
                };
                let coordinate = |key: &str| -> Result<i32, String> {
                    pick.get(key)
                        .and_then(Value::as_i64)
                        .and_then(|v| i32::try_from(v).ok())
                        .ok_or_else(|| format!("eyedropper {key} must be an integer"))
                };
                let mut options = ConversionOptions::default();
                if let Some(intent) = pick.get("intent").filter(|v| !v.is_null()) {
                    let text = intent.as_str().ok_or("intent must be a string")?;
                    options.intent = RenderingIntent::parse(text).ok_or_else(|| {
                        format!("intent \"{text}\" is not one of perceptual, relative, saturation or absolute")
                    })?;
                }
                match pick.get("black_point_compensation") {
                    None | Some(Value::Null) => {}
                    Some(Value::Bool(v)) => options.black_point_compensation = *v,
                    Some(_) => return Err("black_point_compensation must be a boolean".into()),
                }
                Some(Eyedropper {
                    kind,
                    x: coordinate("x")?,
                    y: coordinate("y")?,
                    reach: match pick.get("reach").filter(|v| !v.is_null()) {
                        None => 0,
                        Some(v) => v
                            .as_u64()
                            .filter(|v| *v <= 2)
                            .ok_or("eyedropper reach must be 0, 1 or 2")?
                            as u32,
                    },
                    options,
                })
            }
        };
        Ok(Self {
            source,
            auto,
            eyedropper,
        })
    }
}

/// The four L planes and their total mass.
struct Planes {
    bins: Vec<u64>,
    total: u64,
}

impl Planes {
    fn new() -> Result<Self, String> {
        let mut bins = reserved_vec(4 * BINS)?;
        bins.resize(4 * BINS, 0u64);
        Ok(Self { bins, total: 0 })
    }

    /// One straight ink sample at `weight` (alpha × coverage, `UNIT` = one
    /// whole pixel).
    fn add(&mut self, ink: [u16; 4], weight: u64) {
        if weight == 0 {
            return;
        }
        self.total += weight;
        for (c, v) in ink.iter().enumerate() {
            self.bins[c * BINS + usize::from(65535 - *v)] += weight;
        }
    }

    fn plane(&self, c: usize) -> &[u64] {
        &self.bins[c * BINS..(c + 1) * BINS]
    }

    fn summed(&self) -> Result<Vec<u64>, String> {
        let mut out = reserved_vec(BINS)?;
        out.extend((0..BINS).map(|i| (0..4).map(|c| self.bins[c * BINS + i]).sum::<u64>()));
        Ok(out)
    }
}

/// A plane in 256 display bins, in pixel units.
fn display(plane: &[u64]) -> Vec<f64> {
    let mut out = vec![0f64; 256];
    for (v, count) in plane.iter().enumerate() {
        // `ops_auto`'s `bin8` at 16 bits: round(v / 257), half up.
        out[(2 * v + 257) / 514] += *count as f64 / UNIT;
    }
    out
}

/// `alpha × coverage` as a mass in [0, UNIT].
fn weight(alpha: u16, coverage: u16) -> u64 {
    (u64::from(alpha) * u64::from(coverage) + 32767) / 65535
}

impl PrecisionDocument {
    /// The analysis's working memory (module doc) and its refusal.
    fn cmyk_tone_scratch(&self, request: &Request) -> (u64, Option<String>) {
        let (w, _) = self.dimensions();
        let stack = self.cmyk_stack_scratch_bytes().unwrap_or(u64::MAX / 4);
        let base = ANALYSIS_FIXED_BYTES + u64::from(w) * 20 + stack;
        if let Err(e) = self.require_cmyk_policy("The CMYK histogram") {
            return (base, Some(e));
        }
        let refusal = match request.source {
            Source::Layer(index) => self
                .layer(index)
                .and_then(|layer| layer.cmyk_source())
                .is_none()
                .then(|| format!("record {index} is not a CMYK raster layer")),
            Source::Backdrop(below) => {
                (below > self.layer_count()).then(|| format!("record {below} is out of range"))
            }
            Source::Composite => None,
        };
        (base, refusal)
    }

    /// `dr_precision_doc_cmyk_tone_analysis_plan`'s answer:
    /// `{"working_memory_plan_bytes","refusal"}`.
    pub fn cmyk_tone_analysis_plan(&self, request: &str) -> Result<Value, String> {
        let request = Request::parse(request)?;
        let (bytes, refusal) = self.cmyk_tone_scratch(&request);
        Ok(json!({"working_memory_plan_bytes":bytes,"refusal":refusal}))
    }

    /// The histograms and, as asked, Auto's endpoints and an eyedropper's
    /// sample and target (module doc). `coverage` is empty or canvas-sized.
    pub fn cmyk_tone_analysis(
        &self,
        request: &str,
        coverage: &[u16],
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Value, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let request = Request::parse(request)?;
        validate_coverage(self, coverage)?;
        let (scratch, refusal) = self.cmyk_tone_scratch(&request);
        if let Some(reason) = refusal {
            return Err(reason);
        }
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let planes = self.cmyk_planes(&request.source, coverage, job)?;
        let summed = planes.summed()?;
        let mut out = json!({"histogram":{
            "cmyk": display(&summed),
            "c": display(planes.plane(0)),
            "m": display(planes.plane(1)),
            "y": display(planes.plane(2)),
            "k": display(planes.plane(3)),
        }});
        if let Some(auto) = &request.auto {
            out["auto"] = auto_endpoints(&planes, &summed, auto);
        }
        if let Some(pick) = &request.eyedropper {
            out["eyedropper"] = self.cmyk_eyedropper(pick, limit, other)?;
        }
        job.report(1.0, 5)?;
        Ok(out)
    }

    fn cmyk_planes(
        &self,
        source: &Source,
        coverage: &[u16],
        job: JobContext<'_>,
    ) -> Result<Planes, String> {
        let canvas = self.dimensions();
        let mut planes = Planes::new()?;
        if let Source::Layer(index) = source {
            let layer = self.layer(*index).ok_or("record index is out of range")?;
            let Content::CmykRaster { image, offset } = &layer.content else {
                return Err(format!("record {index} is not a CMYK raster layer"));
            };
            let (w, h) = image.dimensions();
            for y in 0..h {
                job.check()?;
                let mut x = 0usize;
                for (_, run) in image.row_segments(y, 0, w) {
                    for s in run {
                        let i = y as usize * w as usize + x;
                        let cov = coverage_at(coverage, canvas, (w, h), *offset, i);
                        planes.add([s[0], s[1], s[2], s[3]], weight(s[4], cov));
                        x += 1;
                    }
                }
            }
            return Ok(planes);
        }
        let below;
        let document = match source {
            Source::Backdrop(0) => return Ok(planes),
            // The postorder records below `n`, enclosing groups kept
            // (`cmyk_select::cmyk_backdrop`, `AnalysisSource`'s contract).
            Source::Backdrop(n) => {
                below = self.cmyk_backdrop(*n)?;
                &below
            }
            _ => self,
        };
        let (w, h) = canvas;
        let mut stack = document.cmyk_stack(None, job)?;
        let mut acc = vec![[0f32; 5]; w as usize];
        for y in 0..h {
            job.check()?;
            stack.row(i64::from(y), 0, &mut acc);
            for (x, pixel) in acc.iter().enumerate() {
                let cov = if coverage.is_empty() {
                    u16::MAX
                } else {
                    coverage[y as usize * w as usize + x]
                };
                let ink = [pixel[0], pixel[1], pixel[2], pixel[3]].map(code);
                planes.add(ink, weight(code(pixel[4]), cov));
            }
        }
        Ok(planes)
    }

    fn cmyk_eyedropper(&self, pick: &Eyedropper, limit: u64, other: u64) -> Result<Value, String> {
        let policy = self.require_cmyk_policy("The CMYK eyedropper")?;
        let sample = self.cmyk_sample(pick.x, pick.y, pick.reach, limit, other)?;
        let ink16: Vec<u16> = sample["ink16"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_u64())
                    .map(|v| v as u16)
                    .collect()
            })
            .unwrap_or_default();
        let ink: [f64; 4] =
            std::array::from_fn(|c| f64::from(unit(ink16.get(c).copied().unwrap_or(0))));
        let relative = RenderingIntent::RelativeColorimetric;
        let target: [f64; 4] = match pick.kind {
            Kind::White => [0.0; 4],
            Kind::Black => srgb_to_ink(&policy.profile, [0.0; 3], pick.options)?,
            Kind::Gray => {
                let xyz = policy.profile.to_xyz(&ink, relative).unwrap_or([0.0; 3]);
                let lab = xyz_to_lab(xyz);
                match policy
                    .profile
                    .from_xyz(lab_to_xyz([lab[0], 0.0, 0.0]), relative)
                {
                    Some((device, _)) => std::array::from_fn(|i| device[i].clamp(0.0, 1.0)),
                    None => {
                        return Err(crate::cmyk_color::require_encodable(&policy.profile)
                            .err()
                            .unwrap_or_else(|| "the grey target could not be separated".into()))
                    }
                }
            }
        };
        Ok(json!({"sample":ink,"target":target}))
    }

    /// The clipping display's plan bytes.
    pub(crate) fn cmyk_clip_scratch(&self) -> Result<u64, String> {
        let (w, h) = self.dimensions();
        Ok(pixel_count(w, h)? as u64 * CLIP_BYTES_PER_PIXEL
            + u64::from(w) * 20
            + self.cmyk_stack_scratch_bytes()?
            + BOOKKEEPING_BYTES)
    }

    /// Curves' ⌥ clipping display of this (candidate) document (module doc):
    /// request `{"mode":"shadows"|"highlights","plates":["c","m","y","k"]}`.
    pub fn cmyk_clip_display(
        &self,
        request: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<DrImage, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        self.require_cmyk_policy("The CMYK clipping display")?;
        let value: Value = serde_json::from_str(request)
            .map_err(|e| format!("the clipping display request is not valid JSON: {e}"))?;
        let object = value
            .as_object()
            .ok_or("the clipping display request must be a JSON object")?;
        keys(object, &["mode", "plates"], "the clipping display request")?;
        let shadows = match object.get("mode").and_then(Value::as_str) {
            Some("shadows") => true,
            Some("highlights") => false,
            _ => return Err("the clipping display mode is shadows or highlights".into()),
        };
        let plates: Vec<usize> = match object.get("plates").filter(|v| !v.is_null()) {
            None => vec![0, 1, 2, 3],
            Some(value) => value
                .as_array()
                .ok_or("the clipping display plates are a list of c, m, y and k")?
                .iter()
                .map(|v| match v.as_str() {
                    Some("c") => Ok(0),
                    Some("m") => Ok(1),
                    Some("y") => Ok(2),
                    Some("k") => Ok(3),
                    _ => Err("the clipping display plates are a list of c, m, y and k".to_string()),
                })
                .collect::<Result<_, _>>()?,
        };
        if plates.is_empty() {
            return Err("the clipping display needs at least one plate".into());
        }
        let scratch = self.cmyk_clip_scratch()?;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let (w, h) = self.dimensions();
        let mut stack = self.cmyk_stack(None, job)?;
        let mut acc = vec![[0f32; 5]; w as usize];
        let mut bytes = reserved_vec(pixel_count(w, h)? * 4)?;
        for y in 0..h {
            job.check()?;
            stack.row(i64::from(y), 0, &mut acc);
            for pixel in &acc {
                let ink = on_paper(*pixel).map(code);
                let white = if shadows {
                    !plates.iter().any(|&p| ink[p] == 65535)
                } else {
                    plates.iter().all(|&p| ink[p] == 0)
                };
                let v = if white { 255 } else { 0 };
                bytes.extend_from_slice(&[v, v, v, 255]);
            }
        }
        job.report(1.0, 5)?;
        DrImage::from_rgba(w, h, bytes).ok_or_else(|| "the clipping display has no pixels".into())
    }
}

/// Auto's endpoints (module doc).
fn auto_endpoints(planes: &Planes, summed: &[u64], auto: &AutoRequest) -> Value {
    if planes.total == 0 {
        return json!({"empty":true});
    }
    let total = planes.total as f64;
    let (black, white): ([f32; 4], [f32; 4]) = if auto.monochromatic {
        // The summed plane holds each pixel four times.
        let mass = 4.0 * total;
        match contrast_endpoints_clipped(summed, auto.shadows * mass, auto.highlights * mass, UNIT)
        {
            Some((lo, hi)) => ([lo; 4], [hi; 4]),
            None => ([0.0; 4], [1.0; 4]),
        }
    } else {
        // K (plate 3) keeps (0, 1) (module doc).
        let mut black = [0f32; 4];
        let mut white = [1f32; 4];
        for c in 0..3 {
            if let Some((lo, hi)) = contrast_endpoints_clipped(
                planes.plane(c),
                auto.shadows * total,
                auto.highlights * total,
                UNIT,
            ) {
                black[c] = lo;
                white[c] = hi;
            }
        }
        (black, white)
    };
    json!({"black":black,"white":white,
        "algorithm": if auto.monochromatic { "monochromatic" } else { "per_channel" }})
}
