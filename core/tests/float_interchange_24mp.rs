//! The OpenEXR and Radiance opens and the new 32-bit float
//! exports stay admitted at or above their measured floors at 24 MP.
//!
//! One 6000×4000 float sRGB picture (an x ramp, a y ramp, blue steps —
//! `float_master_24mp`'s), as a document for the exports and written through
//! the seams as the two files the opens read.
//!
//! * An open's plan is what `float_io` admits: the file's bytes, the seam's
//!   scratch (`count·32 + 1 MiB` for OpenEXR, `count·24 + 1 MiB` for
//!   Radiance), 16 B/px of tiles and 1 MiB of bookkeeping.
//! * An export's plan is `export_plan`'s `scratch_bytes` — the existing
//!   32-bit term, charged whatever the container — with the caller's
//!   projection supplied, as the app supplies it.
//! * A float export's PREVIEW decodes the written bytes and charges the open's
//!   plan for them plus the display's (`doc_export_float`, "The preview":
//!   `count·12 + 1 MiB` for the tone-mapped copy and the bytes), held against
//!   the preview's own measured footprint. The default test reads that figure
//!   back from the code: with the process limit one byte under it,
//!   `float_export_preview` refuses with a capacity sentence naming it.
//!
//! Each floor is ⌈1.25 × the largest measured footprint E⌉ of [`measure`]:
//! Release, one case per process, `MallocLargeCache=0`, three runs and a
//! baseline twin (`base`, the document built and nothing run, which reads
//! 0). The record is beside the constants. One 24 MP document at a time:
//! every `#[test]` holds `ONE_AT_A_TIME`.

use doctored_core::doc_export::ExportRequest;
use doctored_core::exr_float;
use doctored_core::float_primaries::REC709;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::set_process_working_limit;
use doctored_core::radiance_hdr;
use doctored_core::tiff_float::FloatSampleType;

#[path = "common/float_fixture.rs"]
mod float_fixture;

const W: u32 = 6000;
const H: u32 = 4000;
const COUNT: u64 = W as u64 * H as u64;
/// The working limit the measurement planned at (a 16 GB Mac's).
const LIMIT: u64 = 8_589_934_592;
/// `precision_image::BOOKKEEPING_BYTES`, restated: the opens' plan adds it.
const BOOKKEEPING: u64 = 1024 * 1024;
/// `float_document::display_scratch` at this size, restated: what a preview
/// charges beside the open's plan.
const DISPLAY_SCRATCH: u64 = COUNT * 12 + BOOKKEEPING;

/// ⌈1.25 × max E⌉ of the measured opens (max E 771,588,696 and 687,047,208
/// against plans of 1,185,700,751 and 974,883,403). Every max here is over
/// two sets of three runs; the OpenEXR open's E spreads about 1 % from run to
/// run, the crate's parallel block decode being the part that varies.
const OPEN_EXR_FLOOR: u64 = 964_485_870;
const OPEN_HDR_FLOOR: u64 = 858_809_010;
/// ⌈1.25 × max E⌉ of the measured exports: OpenEXR at f32 and at half,
/// Radiance, and the half TIFF (max E 421,757,288, 401,736,016, 589,283,760
/// and 607,879,624, each against the one plan of 2,440,410,880).
const EXPORT_EXR32_FLOOR: u64 = 527_196_610;
const EXPORT_EXR16_FLOOR: u64 = 502_170_020;
const EXPORT_HDR_FLOOR: u64 = 736_604_700;
const EXPORT_TIFF16_FLOOR: u64 = 759_849_530;
/// ⌈1.25 × max E⌉ of the measured previews of the OpenEXR (f32) and Radiance
/// exports (max E 770,769,472 and 783,680,088 over two sets of three runs,
/// against preview plans of 1,474,749,327 and 1,263,931,979). The float TIFF
/// preview is measured too (`preview_tiff`; the record is in
/// `doc_export_float`'s module doc), with no floor here: the default test
/// encodes no TIFF.
const PREVIEW_EXR_FLOOR: u64 = 963_461_840;
const PREVIEW_HDR_FLOOR: u64 = 979_600_110;

static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn sample(i: u32) -> [f32; 4] {
    let (x, y) = (i % W, i / W);
    [
        x as f32 / W as f32,
        y as f32 / H as f32,
        ((x / 8) % 8) as f32 * 0.25,
        1.0,
    ]
}

/// The 6000×4000 f32 sRGB document.
fn float_document() -> PrecisionDocument {
    let pixels = (0..W * H).map(sample).collect();
    float_fixture::float_doc_at(LIMIT, W, H, &IccProfile::srgb(), &[pixels])
}

/// The picture as an OpenEXR file (f32, ZIP), a Radiance file, or (the
/// `preview_tiff` case) the 32-bit float TIFF the export writes.
fn encoded(case: &str) -> Vec<u8> {
    if case == "preview_tiff" {
        let request = ExportRequest::parse("{\"format\":\"tiff\",\"bit_depth\":32}").unwrap();
        return float_document()
            .export_encoded(None, &request, LIMIT, 0, None)
            .expect("the float TIFF")
            .bytes()
            .to_vec();
    }
    if case.ends_with("exr") {
        let rgba: Vec<f32> = (0..W * H).flat_map(sample).collect();
        exr_float::encode(W, H, &rgba, FloatSampleType::F32, REC709).expect("the OpenEXR file")
    } else {
        let rgb: Vec<f32> = (0..W * H)
            .flat_map(|i| {
                let [r, g, b, _] = sample(i);
                [r, g, b]
            })
            .collect();
        radiance_hdr::encode(W, H, &rgb, REC709).expect("the Radiance file")
    }
}

/// What `float_io` admits for the open of `bytes`, and for their preview.
fn open_plan(case: &str, bytes: &[u8]) -> u64 {
    let seam = if case == "preview_tiff" {
        doctored_core::tiff_float::inspect(bytes, "plan")
            .expect("the plan")
            .scratch_bytes
    } else if case.ends_with("exr") {
        exr_float::inspect(bytes, "plan")
            .expect("the plan")
            .scratch_bytes
    } else {
        radiance_hdr::inspect(bytes, "plan")
            .expect("the plan")
            .scratch_bytes
    };
    bytes.len() as u64 + seam + COUNT * 16 + BOOKKEEPING
}

fn request(case: &str) -> ExportRequest {
    let text = match case {
        "export_exr32" => "{\"format\":\"exr\",\"bit_depth\":32}",
        "export_exr16" => "{\"format\":\"exr\",\"bit_depth\":32,\"float_samples\":\"f16\"}",
        "export_hdr" => "{\"format\":\"hdr\",\"bit_depth\":32}",
        "export_tiff16" => "{\"format\":\"tiff\",\"bit_depth\":32,\"float_samples\":\"f16\"}",
        other => panic!("unknown export case {other}"),
    };
    ExportRequest::parse(text).expect("the request")
}

fn admitted(name: &str, figure: u64, floor: u64) {
    println!("{name}: figure {figure} floor {floor}");
    assert!(figure >= floor, "{name}: {figure} < floor {floor}");
}

#[test]
fn the_float_opens_and_exports_are_admitted_at_24mp() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    assert!(set_process_working_limit(LIMIT));
    // The document a preview is shown under (its tone map and profile).
    let tiny = float_fixture::float_doc_at(
        LIMIT,
        1,
        1,
        &IccProfile::srgb(),
        &[vec![[0.5, 0.5, 0.5, 1.0]]],
    );
    for (case, floor, preview, format) in [
        ("open_exr", OPEN_EXR_FLOOR, PREVIEW_EXR_FLOOR, "exr"),
        ("open_hdr", OPEN_HDR_FLOOR, PREVIEW_HDR_FLOOR, "hdr"),
    ] {
        let bytes = encoded(case);
        let plan = open_plan(case, &bytes);
        admitted(case, plan, floor);
        // The preview of the same file charges that plan and the display's.
        let figure = plan + DISPLAY_SCRATCH;
        admitted(&case.replace("open", "preview"), figure, preview);
        // That figure is the code's own charge, not only this file's sum: one
        // byte under it the preview refuses before it decodes anything, and
        // its capacity sentence names the figure.
        let request =
            ExportRequest::parse(&format!("{{\"format\":\"{format}\",\"bit_depth\":32}}"))
                .expect("the request");
        assert!(set_process_working_limit(figure - 1));
        let refusal = tiny.float_export_preview(&bytes, &request).err();
        assert!(set_process_working_limit(LIMIT));
        let refusal = refusal.unwrap_or_else(|| panic!("{case}: a preview past the limit"));
        assert!(
            refusal.contains(" capacity for ")
                && refusal.contains(&format!("needs {figure} bytes"))
                && refusal.contains(&format!("working limit is {} bytes", figure - 1)),
            "{case}: {refusal}"
        );
    }
    let document = float_document();
    for (case, floor) in [
        ("export_exr32", EXPORT_EXR32_FLOOR),
        ("export_exr16", EXPORT_EXR16_FLOOR),
        ("export_hdr", EXPORT_HDR_FLOOR),
        ("export_tiff16", EXPORT_TIFF16_FLOOR),
    ] {
        let plan = document
            .export_plan(None, &request(case))
            .expect("the export plan");
        admitted(case, plan["scratch_bytes"].as_u64().unwrap(), floor);
    }
}

#[repr(C)]
struct RusageInfoV4 {
    uuid: [u8; 16],
    fields: [u64; 35],
}
const PHYS_FOOTPRINT: usize = 7;
const INTERVAL_MAX_PHYS_FOOTPRINT: usize = 33;

extern "C" {
    fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut RusageInfoV4) -> i32;
    fn proc_reset_footprint_interval(pid: i32) -> i32;
}

fn rusage() -> RusageInfoV4 {
    let mut info = RusageInfoV4 {
        uuid: [0; 16],
        fields: [0; 35],
    };
    let rc = unsafe { proc_pid_rusage(std::process::id() as i32, 4, &mut info) };
    assert_eq!(rc, 0, "proc_pid_rusage");
    info
}

/// One case per process, selected by `DR_FI_CASE`: `open_exr` and
/// `open_hdr` (the file is written by a child process, `setup_in_child`, so
/// the measured process has never held the picture); `export_exr32`,
/// `export_exr16`, `export_hdr` and `export_tiff16` (the document and its
/// projection built in setup); `preview_exr`, `preview_hdr` and
/// `preview_tiff` (the child's file read and previewed through
/// `float_export_preview` on a 1×1 float document, whose tone map and profile
/// the preview is shown under); `base`, the export shape with nothing run.
/// The footprint is read, the interval peak reset and the measured call run
/// with its result held until E = interval peak − pre is read. `plan` is the
/// figure the default test asserts.
#[test]
#[ignore = "measurement: DR_FI_CASE=base|open_exr|open_hdr|export_exr32|export_exr16|export_hdr|export_tiff16|preview_exr|preview_hdr|preview_tiff, Release, MallocLargeCache=0"]
fn measure() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let case = std::env::var("DR_FI_CASE").expect("DR_FI_CASE");
    assert!(set_process_working_limit(LIMIT));
    let open = case.starts_with("open");
    let preview = case.starts_with("preview");
    if let Ok(path) = std::env::var("DR_FI_SETUP") {
        // The setup child: write the file and its plan figure.
        let bytes = encoded(&case);
        std::fs::write(format!("{path}.plan"), open_plan(&case, &bytes).to_string()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        return;
    }
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join(if case.ends_with("hdr") {
        "measured.hdr"
    } else if case.ends_with("tiff") {
        "measured.tiff"
    } else {
        "measured.exr"
    });
    let path = path.to_str().unwrap();
    let (document, projection, plan) = if preview {
        setup_in_child(path);
        let plan = std::fs::read_to_string(format!("{path}.plan"))
            .unwrap()
            .parse::<u64>()
            .unwrap()
            + DISPLAY_SCRATCH;
        let tiny = float_fixture::float_doc_at(
            LIMIT,
            1,
            1,
            &IccProfile::srgb(),
            &[vec![[0.5, 0.5, 0.5, 1.0]]],
        );
        (Some(tiny), None, plan)
    } else if open {
        setup_in_child(path);
        let plan = std::fs::read_to_string(format!("{path}.plan"))
            .unwrap()
            .parse::<u64>()
            .unwrap();
        (None, None, plan)
    } else {
        let document = float_document();
        let projection = document.render(LIMIT, 0, None).expect("the projection");
        let plan = if case == "base" {
            0
        } else {
            document
                .export_plan(Some(&projection), &request(&case))
                .expect("the export plan")["scratch_bytes"]
                .as_u64()
                .unwrap()
        };
        (Some(document), Some(projection), plan)
    };
    let pre = rusage().fields[PHYS_FOOTPRINT];
    unsafe { proc_reset_footprint_interval(std::process::id() as i32) };
    let previewed = preview.then(|| {
        // The file's bytes are read inside the interval, as the open reads
        // them, so E and the plan count the same things.
        let bytes = std::fs::read(path).expect("the file");
        let format = match case.as_str() {
            "preview_exr" => "exr",
            "preview_hdr" => "hdr",
            _ => "tiff",
        };
        let request =
            ExportRequest::parse(&format!("{{\"format\":\"{format}\",\"bit_depth\":32}}"))
                .expect("the request");
        let image = document
            .as_ref()
            .unwrap()
            .float_export_preview(&bytes, &request)
            .expect("the preview is admitted");
        (bytes, image)
    });
    let (opened, exported) = if preview {
        (None, None)
    } else if open {
        (
            Some(
                PrecisionDocument::open_preserving(path, LIMIT, 0, None)
                    .expect("the open is admitted"),
            ),
            None,
        )
    } else if case == "base" {
        (None, None)
    } else {
        (
            None,
            Some(
                document
                    .as_ref()
                    .unwrap()
                    .export_encoded(projection.as_ref(), &request(&case), LIMIT, 0, None)
                    .expect("the export is admitted"),
            ),
        )
    };
    let peak = rusage().fields[INTERVAL_MAX_PHYS_FOOTPRINT];
    let file = previewed.as_ref().map_or(0, |(bytes, _)| bytes.len());
    drop((opened, exported, previewed));
    let e = peak.saturating_sub(pre);
    let floor = (e * 5).div_ceil(4);
    println!(
        "FI_MEASURE case={case} plan={plan} pre={pre} peak={peak} E={e} floor={floor} file={file}"
    );
    assert!(
        case == "base" || plan >= floor,
        "{case}: plan {plan} < floor {floor}"
    );
}

/// Runs this binary's `measure` again as a child with `DR_FI_SETUP` set to
/// `path`: the child encodes the picture and writes it there with its plan
/// figure beside it (a freed picture's pages stay in the allocator's free
/// lists, counted in the footprint and reused by the open, which would then
/// read short).
fn setup_in_child(path: &str) {
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "measure", "--nocapture"])
        .env("DR_FI_SETUP", path)
        .status()
        .unwrap();
    assert!(status.success(), "the setup child");
}
