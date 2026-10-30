//! Black-box suite for 13B-3e-3b: the raw develop's variant and base reads
//! (`raw_local_develop::develop`) read tiles instead of binding a
//! whole-plane view, with every answer, refusal, checkpoint count and report
//! log unchanged. The develop inspection preview (`develop_preview`) MISSED
//! its warm bound on tile reads, so it keeps HEAD's source view and the RAW
//! source geometry output (its feeding seed) stays seeded. Since 13B-3e-4a
//! the geometry SOURCE is ONE transient copy (no view) and the develop
//! OUTPUT (`develop`'s `PrecisionImage::from_rgba16_unseeded` call in
//! `raw_local_develop.rs`) is unseeded, so a develop answer
//! holds no view and a geometry answer only its output's seed.
//!
//! Oracles are independent of the implementation: a preview cell is the
//! centre sample's `(v + 128) / 257` with the blue/red/magenta markers ORed
//! over the cell's alpha-positive samples, computed HERE over `to_rgba16()`;
//! an EXIF orientation is the permutation written HERE.
//!
//! `record_raw_develop_reads` prints one line per case and fixture —
//! `raw <case> <WxH> <sha256> checkpoints=<n> reports=<sha256>` (the hash of
//! `null`, `err:<message>` or the answer's bytes; every call runs under a
//! recording `DrJobControl`, a multi-step case sums its steps' checkpoints
//! and concatenates their logs) — so the same file run against HEAD and the
//! tree diffs byte-identical answers and checkpoint/report sequences. It
//! uses only APIs present at `58f8304`.
//!
//! `no_view_raw_develop_reads` fails at `58f8304` by design (there the
//! develop also holds its inputs' views) and at `6b97c48` (there the develop
//! output and the geometry source hold views); the other tests pass there. The
//! view cache is process-wide, so every test holds `serial()`.

use std::ffi::{c_char, CStr};
use std::ptr;
use std::sync::Arc;

use doctored_core::contiguous_views::contiguous_view_bytes;
use doctored_core::ffi::{
    dr_image_free, dr_image_height, dr_image_pixels_rgba, dr_image_width, dr_string_free,
};
use doctored_core::ffi_develop_inspect::dr_precision_image_develop_preview;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
use doctored_core::raw_local_develop::develop;
use doctored_core::raw_local_mask::RawLocalMask;
use serde_json::json;

use crate::common::sha256;
use crate::resource_report;
use crate::serial;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 600;
const H: u32 = 520;
/// 600×520 has four tile shapes, 512×512 four tiles of one shape, 257×3 two
/// shapes and 1×1 one.
const FIXTURES: [(u32, u32); 4] = [(W, H), (512, 512), (257, 3), (1, 1)];
const FLOAT_FIXTURE: &str = "tests/fixtures/float_11f/scene.tiff";

const CASES: [&str; 18] = [
    "develop/vignette",
    "develop/vignette_reference",
    "develop/local",
    "develop/local_feather_two",
    "develop/unchanged",
    "develop/disabled",
    "geometry/exif6",
    "geometry/exif3",
    "geometry/affine_translate",
    "geometry/affine_bicubic",
    "geometry/lens_bilinear",
    "geometry/identity",
    "preview/fit3",
    "preview/fit3_clipping",
    "preview/region2_clipping1",
    "preview/detail_clipping2",
    "preview/one_cell",
    "pipeline/develop_geometry_preview",
];

const REFUSALS: [&str; 4] = [
    "refuse/float_base",
    "refuse/float_geometry",
    "refuse/float_variant",
    "refuse/float_variant_enabled",
];

// ------------------------------------------------------------- plumbing --

fn take(p: *mut c_char) -> String {
    if p.is_null() {
        return "<no error message>".into();
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    text
}

fn evict() {
    resource_report::evict(&[]).expect("an eviction report");
}

/// Runs each step under a fresh `DrJobControl` (a control is single-use)
/// when recording; sums the checkpoint counts and concatenates the raw
/// report logs.
struct Recorder {
    recording: bool,
    checkpoints: u64,
    log: String,
}

impl Recorder {
    fn new(recording: bool) -> Self {
        Self {
            recording,
            checkpoints: 0,
            log: String::new(),
        }
    }
    fn step<T>(&mut self, call: impl FnOnce(Option<&DrJobControl>) -> T) -> T {
        if !self.recording {
            return call(None);
        }
        let control = DrJobControl::default();
        control.record_reports();
        let out = call(Some(&control));
        for r in control.reports() {
            self.log += &format!("{} {} {}\n", r.checkpoints, r.progress.to_bits(), r.stage);
        }
        self.checkpoints += control.checkpoints();
        out
    }
}

// ------------------------------------------------------------- fixtures --

/// A deterministic u16 pattern: alpha 0 (hidden RGB including 0 / 65535),
/// 65535 and partial samples, with 0 / 65535 components on alpha-positive
/// samples too.
fn pattern(width: u32, height: u32, seed: u32) -> Vec<[u16; 4]> {
    (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            let alpha = match (x + 3 * y + seed) % 13 {
                0 => 0,
                1..=4 => u16::MAX,
                k => (k * 4099) as u16,
            };
            let mut p = [
                ((x * 2711 + y * 911 + seed * 1234) % 65536) as u16,
                ((x * 523 + y * 3301 + seed * 77) % 65536) as u16,
                (((x ^ y) * 1999 + seed) % 65536) as u16,
                alpha,
            ];
            match (x * 7 + y * 5 + seed) % 17 {
                0 => p[0] = 0,
                1 => p[1] = u16::MAX,
                2 => {
                    p[2] = 0;
                    p[0] = u16::MAX;
                }
                3 => p = [0, 0, 0, alpha],
                4 => p = [u16::MAX, u16::MAX, u16::MAX, alpha],
                _ => {}
            }
            p
        })
        .collect()
}

fn document(pixels: Vec<[u16; 4]>, (w, h): (u32, u32)) -> PrecisionDocument {
    let image = PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap();
    PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap()
}

/// An RZLM coverage ramp along `x + y` (plus a `seed` shift).
fn ramp((w, h): (u32, u32), seed: u32) -> RawLocalMask {
    let span = (w + h).saturating_sub(2).max(1);
    let mut bytes = Vec::with_capacity(16 + w as usize * h as usize * 2);
    bytes.extend_from_slice(b"RZLM");
    for v in [1, w, h] {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    for i in 0..w * h {
        let (x, y) = (i % w, i / w);
        let v = ((u64::from((x + y + seed) % (span + 1)) * 65535) / u64::from(span)) as u16;
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    RawLocalMask::from_blob(Arc::new(bytes)).unwrap()
}

fn vignette_text((w, h): (u32, u32)) -> String {
    json!({"stops":-1.0,"center":[f64::from(w) / 2.0, f64::from(h) / 2.0],"falloff":1.0,
        "reference_size":null,"reference_from_source":null})
    .to_string()
}

fn vignette_reference_text((w, h): (u32, u32)) -> String {
    json!({"stops":0.8,"center":[f64::from(w), f64::from(h) * 0.9],"falloff":1.5,
        "reference_size":[w * 2, h * 2],"reference_from_source":[1.5, 0.1, -0.1, 1.5, 3.0, 2.0]})
    .to_string()
}

fn geometry_request(name: &str, (w, h): (u32, u32)) -> String {
    match name {
        "geometry/exif6" => json!({"version":1,"kind":"exif","orientation":6}),
        "geometry/exif3" => json!({"version":1,"kind":"exif","orientation":3}),
        "geometry/identity" => json!({"version":1,"kind":"exif","orientation":1}),
        "geometry/affine_translate" => json!({"version":1,"kind":"affine","width":w + 5,
            "height":h + 4,"inverse":[1,0,0,1,-3,-2],"filter":"bilinear"}),
        "geometry/affine_bicubic" => {
            let (s, c) = 7.0_f64.to_radians().sin_cos();
            let (cx, cy) = (f64::from(w) / 2.0, f64::from(h) / 2.0);
            json!({"version":1,"kind":"affine","width":w,"height":h,
                "inverse":[c, s, -s, c, cx - c * cx + s * cy, cy - s * cx - c * cy],
                "filter":"bicubic"})
        }
        "geometry/lens_bilinear" => json!({"version":1,"kind":"lens","profile_id":"reads",
            "center":[0.5,0.5],"radial":[0.02,0.0,0.0],"red_scale":0.997,"blue_scale":1.003,
            "filter":"bilinear"}),
        other => panic!("unknown geometry case {other}"),
    }
    .to_string()
}

/// `(region [x, y, w, h], divisor, clipping)` of a preview case.
fn preview_params(name: &str, (w, h): (u32, u32)) -> ([u32; 4], u32, u32) {
    let whole = [0, 0, w, h];
    match name {
        "preview/fit3" => (whole, 3, 0),
        "preview/fit3_clipping" => (whole, 3, 3),
        "preview/region2_clipping1" => {
            let (rw, rh) = ((w / 2).max(1), (h / 2).max(1));
            ([(w - rw) / 2, (h - rh) / 2, rw, rh], 2, 1)
        }
        "preview/detail_clipping2" => (whole, 1, 2),
        "preview/one_cell" => (whole, w.max(h), 3),
        other => panic!("unknown preview case {other}"),
    }
}

/// One case's untimed inputs.
struct Built {
    base: PrecisionDocument,
    variants: Vec<PrecisionDocument>,
    masks: Vec<RawLocalMask>,
}

fn build(name: &str, size: (u32, u32)) -> Built {
    let base = document(pattern(size.0, size.1, 1), size);
    let mut built = Built {
        base,
        variants: Vec::new(),
        masks: Vec::new(),
    };
    match name {
        "develop/local" | "develop/disabled" => {
            built.variants = vec![document(pattern(size.0, size.1, 2), size)];
            built.masks = vec![ramp(size, 0)];
        }
        "develop/local_feather_two" => {
            built.variants = vec![
                document(pattern(size.0, size.1, 2), size),
                document(pattern(size.0, size.1, 3), size),
            ];
            built.masks = vec![ramp(size, 0), ramp(size, 7)];
        }
        "develop/unchanged" => {
            built.variants = vec![built.base.clone()];
            built.masks = vec![ramp(size, 0)];
        }
        _ => {}
    }
    built
}

/// A call's answer, alive until dropped.
#[allow(clippy::large_enum_variant)]
enum Answer {
    Doc(PrecisionDocument),
    Preview(Vec<u8>),
    /// The developed and the turned documents, then the preview.
    Pipeline(Box<[Option<PrecisionDocument>; 2]>, Vec<u8>),
}

fn doc_bytes(doc: &PrecisionDocument, out: &mut Vec<u8>) {
    let (w, h) = doc.dimensions();
    out.extend(w.to_le_bytes());
    out.extend(h.to_le_bytes());
    out.extend((doc.layer_count() as u32).to_le_bytes());
    for i in 0..doc.layer_count() {
        let layer = doc.layer(i).unwrap();
        let (x, y) = layer.offset();
        out.extend(x.to_le_bytes());
        out.extend(y.to_le_bytes());
        if let Some(image) = layer.source() {
            let (w, h) = image.dimensions();
            out.extend(w.to_le_bytes());
            out.extend(h.to_le_bytes());
            out.extend_from_slice(image.profile().bytes());
            if image.storage() == PrecisionStorage::Rgba32Float {
                for p in image.to_rgba32().unwrap() {
                    for v in p {
                        out.extend(v.to_le_bytes());
                    }
                }
                continue;
            }
            for p in image.to_rgba16().unwrap() {
                for v in p {
                    out.extend(v.to_le_bytes());
                }
            }
        }
    }
}

impl Answer {
    fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match self {
            Self::Doc(doc) => doc_bytes(doc, &mut out),
            Self::Preview(bytes) => out.extend_from_slice(bytes),
            Self::Pipeline(docs, preview) => {
                for doc in docs.iter() {
                    match doc {
                        Some(doc) => doc_bytes(doc, &mut out),
                        None => out.extend_from_slice(b"null"),
                    }
                }
                out.extend_from_slice(preview);
            }
        }
        out
    }
}

type Outcome = Result<Option<Answer>, String>;

fn doc_answer(out: Result<Option<PrecisionDocument>, String>) -> Outcome {
    out.map(|d| d.map(Answer::Doc))
}

// ---------------------------------------------------------------- sites --

fn run_develop(
    base: &PrecisionDocument,
    variants: &[PrecisionDocument],
    masks: &[RawLocalMask],
    settings: &str,
    vignette: Option<&str>,
    control: Option<&DrJobControl>,
) -> Result<Option<PrecisionDocument>, String> {
    let variants: Vec<Option<&PrecisionDocument>> = variants.iter().map(Some).collect();
    let masks: Vec<&RawLocalMask> = masks.iter().collect();
    develop(
        base, &variants, &masks, settings, vignette, LIMIT, 0, control,
    )
}

/// `dr_precision_image_develop_preview`: width, height (u32 LE) and the
/// RGBA8 bytes.
fn preview(
    image: &PrecisionImage,
    region: [u32; 4],
    divisor: u32,
    clipping: u32,
    control: Option<&DrJobControl>,
) -> Result<Vec<u8>, String> {
    let mut error = ptr::null_mut();
    let result = unsafe {
        dr_precision_image_develop_preview(
            image,
            region[0],
            region[1],
            region[2],
            region[3],
            divisor,
            clipping,
            LIMIT,
            0,
            control.map_or(ptr::null(), |c| c),
            &mut error,
        )
    };
    if result.is_null() {
        return Err(take(error));
    }
    assert!(error.is_null());
    let (width, height) = unsafe { (dr_image_width(result), dr_image_height(result)) };
    let mut out = [width.to_le_bytes(), height.to_le_bytes()].concat();
    out.extend_from_slice(unsafe {
        std::slice::from_raw_parts(
            dr_image_pixels_rgba(result),
            width as usize * height as usize * 4,
        )
    });
    unsafe { dr_image_free(result) };
    Ok(out)
}

/// The case's CALL over its built inputs.
fn call(name: &str, built: &Built, size: (u32, u32), rec: &mut Recorder) -> Outcome {
    let base = &built.base;
    let density =
        |enabled: bool, feather: f64| json!({"enabled":enabled,"density":0.75,"feather":feather});
    match name {
        "develop/vignette" => doc_answer(
            rec.step(|c| run_develop(base, &[], &[], "[]", Some(&vignette_text(size)), c)),
        ),
        "develop/vignette_reference" => doc_answer(rec.step(|c| {
            run_develop(
                base,
                &[],
                &[],
                "[]",
                Some(&vignette_reference_text(size)),
                c,
            )
        })),
        "develop/local" | "develop/unchanged" => {
            let settings = json!([density(true, 0.0)]).to_string();
            doc_answer(
                rec.step(|c| run_develop(base, &built.variants, &built.masks, &settings, None, c)),
            )
        }
        "develop/local_feather_two" => {
            let settings = json!([
                {"enabled":true,"density":0.75,"feather":2.5},
                {"enabled":true,"density":0.4,"feather":1.0}
            ])
            .to_string();
            doc_answer(
                rec.step(|c| run_develop(base, &built.variants, &built.masks, &settings, None, c)),
            )
        }
        "develop/disabled" => {
            let settings = json!([density(false, 0.0)]).to_string();
            doc_answer(
                rec.step(|c| run_develop(base, &built.variants, &built.masks, &settings, None, c)),
            )
        }
        n if n.starts_with("geometry/") => {
            let request = geometry_request(n, size);
            doc_answer(rec.step(|c| base.raw_source_geometry(&request, LIMIT, 0, c)))
        }
        n if n.starts_with("preview/") => {
            let (region, divisor, clipping) = preview_params(n, size);
            let image = base.layer_source(0).unwrap();
            rec.step(|c| preview(image, region, divisor, clipping, c))
                .map(|b| Some(Answer::Preview(b)))
        }
        "pipeline/develop_geometry_preview" => {
            evict();
            let developed =
                rec.step(|c| run_develop(base, &[], &[], "[]", Some(&vignette_text(size)), c))?;
            let turned = {
                let input = developed.as_ref().unwrap_or(base);
                rec.step(|c| {
                    input.raw_source_geometry(
                        &geometry_request("geometry/exif6", size),
                        LIMIT,
                        0,
                        c,
                    )
                })?
            };
            let shown = {
                let input = turned.as_ref().or(developed.as_ref()).unwrap_or(base);
                let image = input.layer_source(0).unwrap();
                let (region, divisor, clipping) =
                    preview_params("preview/fit3_clipping", image.dimensions());
                rec.step(|c| preview(image, region, divisor, clipping, c))?
            };
            Ok(Some(Answer::Pipeline(Box::new([developed, turned]), shown)))
        }
        other => panic!("unknown case {other}"),
    }
}

fn float_scene() -> PrecisionDocument {
    PrecisionDocument::open_preserving(FLOAT_FIXTURE, LIMIT, 0, None).expect("the float fixture")
}

/// A float case: the answer, and for the two that still refuse the sentence
/// the gate must carry.
fn refusal(name: &str, scene: &PrecisionDocument, rec: &mut Recorder) -> Outcome {
    let size = scene.dimensions();
    match name {
        "refuse/float_base" => doc_answer(
            rec.step(|c| run_develop(scene, &[], &[], "[]", Some(&vignette_text(size)), c)),
        ),
        "refuse/float_geometry" => doc_answer(rec.step(|c| {
            scene.raw_source_geometry(&geometry_request("geometry/exif6", size), LIMIT, 0, c)
        })),
        "refuse/float_variant" | "refuse/float_variant_enabled" => {
            let converted = scene
                .layer_source(0)
                .unwrap()
                .convert(PrecisionStorage::Rgba16Unorm, LIMIT, 0, None)
                .unwrap();
            let base = PrecisionDocument::from_image(
                converted,
                Metadata::default(),
                Resolution::default(),
            )
            .unwrap();
            assert_eq!(
                base.layer_source(0).unwrap().profile().bytes(),
                scene.layer_source(0).unwrap().profile().bytes(),
                "the converted base keeps the scene's profile bytes"
            );
            let enabled = name == "refuse/float_variant_enabled";
            let settings = json!([{"enabled":enabled,"density":0.75,"feather":0.0}]).to_string();
            let masks = [ramp(size, 0)];
            let variants: Vec<Option<&PrecisionDocument>> = vec![Some(scene)];
            let masks: Vec<&RawLocalMask> = masks.iter().collect();
            doc_answer(
                rec.step(|c| develop(&base, &variants, &masks, &settings, None, LIMIT, 0, c)),
            )
        }
        other => panic!("unknown refusal {other}"),
    }
}

fn outcome(outcome: &Outcome) -> Vec<u8> {
    match outcome {
        Ok(Some(answer)) => answer.bytes(),
        Ok(None) => b"null".to_vec(),
        Err(e) => format!("err:{e}").into_bytes(),
    }
}

// ---------------------------------------------------------------- tests --

#[test]
fn record_raw_develop_reads() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        for name in CASES {
            let built = build(name, (w, h));
            evict();
            let mut rec = Recorder::new(true);
            let out = call(name, &built, (w, h), &mut rec);
            println!(
                "raw {name} {w}x{h} {} checkpoints={} reports={}",
                sha256::sha256_hex(&outcome(&out)),
                rec.checkpoints,
                sha256::sha256_hex(rec.log.as_bytes())
            );
        }
    }
    let scene = float_scene();
    let (w, h) = scene.dimensions();
    // A float base and a float geometry source develop in f32
    // (`raw_float_develop`, `raw_source_geometry`'s float arm): the first two
    // cases answer a document. A float variant beside a u16 base still
    // refuses.
    for (name, sentence) in REFUSALS.iter().zip([
        None,
        None,
        Some("local variant requires genuine RGBA16 samples"),
        Some("local variant requires genuine RGBA16 samples"),
    ]) {
        evict();
        let mut rec = Recorder::new(true);
        let out = refusal(name, &scene, &mut rec);
        match (&out, sentence) {
            (Err(e), Some(sentence)) => assert_eq!(e, sentence, "{name}"),
            (Ok(Some(_)), None) => {}
            (Err(e), None) => panic!("{name}: {e}"),
            _ => panic!("{name}: a refusal"),
        }
        println!(
            "raw {name} {w}x{h} {} checkpoints={} reports={}",
            sha256::sha256_hex(&outcome(&out)),
            rec.checkpoints,
            sha256::sha256_hex(rec.log.as_bytes())
        );
    }
}

#[test]
fn no_view_raw_develop_reads() {
    let _serial = serial();
    let plane = u64::from(W) * u64::from(H) * 8;
    for name in CASES {
        let built = build(name, (W, H));
        evict();
        let answer = call(name, &built, (W, H), &mut Recorder::new(false))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        // The views kept since 13B-3e-4a (the develop output unseeded, the
        // geometry source one transient): the geometry output's seed
        // (`raw_source_geometry.rs:470`) and the preview SOURCE's view
        // (`develop_inspect.rs:136`), both 13B-3e-4c's.
        let expected = match (name.split('/').next().unwrap(), &answer) {
            ("develop", Some(_)) => 0,
            ("develop", None) => 0,
            ("geometry", Some(Answer::Doc(doc))) => {
                let (dw, dh) = doc.dimensions();
                u64::from(dw) * u64::from(dh) * 8
            }
            ("geometry", None) => 0,
            ("preview", _) => plane,
            ("pipeline", Some(Answer::Pipeline(docs, _))) if docs.iter().all(Option::is_some) => {
                plane
            }
            _ => panic!("{name}: an unexpected answer"),
        };
        assert_eq!(contiguous_view_bytes(), expected, "{name}: views held");
        drop(answer);
    }
}

/// The preview written HERE: per cell the centre sample `(v + 128) / 257`,
/// or a marker when some alpha-positive sample of the cell has an RGB
/// component at exactly 0 (blue, bit 1) / 65535 (red, bit 2) / both
/// (magenta).
fn expected_preview(
    samples: &[[u16; 4]],
    stride: u32,
    region: [u32; 4],
    divisor: u32,
    clipping: u32,
) -> Vec<u8> {
    let [rx, ry, rw, rh] = region;
    let (width, height) = (rw.div_ceil(divisor), rh.div_ceil(divisor));
    let mut out = [width.to_le_bytes(), height.to_le_bytes()].concat();
    for row in 0..height {
        let y0 = ry + row * divisor;
        let y1 = (y0 + divisor).min(ry + rh);
        for column in 0..width {
            let x0 = rx + column * divisor;
            let x1 = (x0 + divisor).min(rx + rw);
            let at = |x: u32, y: u32| samples[(y * stride + x) as usize];
            let centre = at(x0 + (x1 - x0) / 2, y0 + (y1 - y0) / 2);
            let mut value = centre.map(|v| ((u32::from(v) + 128) / 257) as u8);
            let (mut zero, mut maximum) = (false, false);
            for y in y0..y1 {
                for x in x0..x1 {
                    let p = at(x, y);
                    if p[3] != 0 {
                        zero |= clipping & 1 != 0 && p[..3].contains(&0);
                        maximum |= clipping & 2 != 0 && p[..3].contains(&u16::MAX);
                    }
                }
            }
            if zero && maximum {
                value = [255, 0, 255, 255];
            } else if zero {
                value = [0, 0, 255, 255];
            } else if maximum {
                value = [255, 0, 0, 255];
            }
            out.extend_from_slice(&value);
        }
    }
    out
}

#[test]
fn preview_matches_the_cell_rule() {
    let _serial = serial();
    let mut markers = 0;
    for size in FIXTURES {
        let base = document(pattern(size.0, size.1, 1), size);
        let image = base.layer_source(0).unwrap();
        let samples = image.to_rgba16().unwrap();
        for name in CASES.iter().filter(|n| n.starts_with("preview/")) {
            let (region, divisor, clipping) = preview_params(name, size);
            let actual = preview(image, region, divisor, clipping, None).unwrap();
            let expected = expected_preview(&samples, size.0, region, divisor, clipping);
            assert_eq!(actual, expected, "{name} {size:?}");
            markers += expected[8..]
                .chunks_exact(4)
                .filter(|p| p[1] == 0 && p[3] == 255 && (p[0] == 255 || p[2] == 255))
                .count();
        }
    }
    assert!(markers > 0, "the marker cases were vacuous");
}

/// Destination `(x, y)` of a `w`×`h` source under EXIF orientation `o`
/// reads source `(sx, sy)`.
fn exif_source(o: u32, (w, h): (u32, u32), x: u32, y: u32) -> (u32, u32) {
    match o {
        1 => (x, y),
        2 => (w - 1 - x, y),
        3 => (w - 1 - x, h - 1 - y),
        4 => (x, h - 1 - y),
        5 => (y, x),
        6 => (y, h - 1 - x),
        7 => (w - 1 - y, h - 1 - x),
        8 => (w - 1 - y, x),
        _ => unreachable!(),
    }
}

#[test]
fn exif_geometry_is_the_permutation() {
    let _serial = serial();
    for size in FIXTURES {
        let (w, h) = size;
        let base = document(pattern(w, h, 1), size);
        let input = base.layer_source(0).unwrap().to_rgba16().unwrap();
        for orientation in 1..=8 {
            let request = json!({"version":1,"kind":"exif","orientation":orientation}).to_string();
            let out = base.raw_source_geometry(&request, LIMIT, 0, None).unwrap();
            if orientation == 1 {
                assert!(out.is_none(), "{size:?}: orientation 1 is the identity");
                continue;
            }
            let out = out.unwrap();
            let image = out.layer_source(0).unwrap();
            let (dw, dh) = image.dimensions();
            assert_eq!((dw, dh), if orientation >= 5 { (h, w) } else { (w, h) });
            let output = image.to_rgba16().unwrap();
            for y in 0..dh {
                for x in 0..dw {
                    let (sx, sy) = exif_source(orientation, size, x, y);
                    assert_eq!(
                        output[(y * dw + x) as usize],
                        input[(sy * w + sx) as usize],
                        "{size:?} orientation {orientation} ({x},{y})"
                    );
                }
            }
        }
    }
}
