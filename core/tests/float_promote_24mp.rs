//! The conversions into float (`precision_conversion_float`) refuse a
//! result too large to save and reopen at 32 bits IN THEIR PLAN, and stay
//! admitted at or above their measured floors.
//!
//! * The capacity row: a 1024×1024 opaque source, promoted once at the
//!   reference limit; the smallest process limit `L` at which the float
//!   result's own `native_plan()` passes is found by bisection. One byte under
//!   it, the source's own plan (16-bit: its native plan; 8-bit: its 8→16 plan)
//!   is still clean, the float plan has exactly one blocked row — the document
//!   row whose note is the result's own `native_plan()` sentence at `L − 1` —
//!   and the op refuses with it before reporting any progress.
//! * The floors: each case's plan (scratch + output) is at least ⌈1.25 × max
//!   E⌉ of [`measure`]: Release, one case per process, `MallocLargeCache=0`,
//!   the process limit 8,589,934,592, three runs and a baseline twin (`base`,
//!   the source built and nothing run, which reads 0). The record is
//!   `precision_conversion_float`'s module doc.
//!
//! One large document at a time: every `#[test]` holds `ONE_AT_A_TIME`, and
//! the binary is its own because it sets the process working limit.

use doctored_core::doc::DrDocument;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_conversion_structure::StructureConversionPlan;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{set_process_working_limit, PrecisionImage};
use image::{Rgba, RgbaImage};

/// The working limit the measurement planned at (a 16 GB Mac's).
const LIMIT: u64 = 8_589_934_592;

/// ⌈1.25 × max E⌉ of the measured conversions (max E 768,393,768,
/// 1,955,513,736, 957,268,664 and 2,441,742,016; `precision_conversion_float`'s
/// module doc holds the runs).
const P16_24_FLOOR: u64 = 960_492_210;
const P16_61_FLOOR: u64 = 2_444_392_170;
const P8_24_FLOOR: u64 = 1_196_585_830;
const P8_61_FLOOR: u64 = 3_052_177_520;

static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A `w × h` opaque 16-bit sRGB ramp: x in red, y in green, a mix in blue.
fn ramp16(w: u32, h: u32) -> PrecisionDocument {
    let samples: Vec<[u16; 4]> = (0..h)
        .flat_map(|y| {
            (0..w).map(move |x| {
                [
                    (u64::from(x) * 65535 / u64::from(w.max(2) - 1)) as u16,
                    (u64::from(y) * 65535 / u64::from(h.max(2) - 1)) as u16,
                    ((x * 37 + y * 11) % 65536) as u16,
                    65535,
                ]
            })
        })
        .collect();
    let image = PrecisionImage::from_rgba16(w, h, samples, IccProfile::srgb(), None).unwrap();
    PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap()
}

/// The same picture at 8 bits.
fn ramp8(w: u32, h: u32) -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(w, h, |x, y| {
        Rgba([
            (u64::from(x) * 255 / u64::from(w.max(2) - 1)) as u8,
            (u64::from(y) * 255 / u64::from(h.max(2) - 1)) as u8,
            ((x * 37 + y * 11) % 256) as u8,
            255,
        ])
    }))
}

enum Source {
    Sixteen(PrecisionDocument),
    Eight(DrDocument),
}

impl Source {
    fn of(case: &str) -> Self {
        let (w, h) = if case.ends_with("_61") {
            (9568, 6376)
        } else {
            (6000, 4000)
        };
        if case.starts_with("p8") {
            Self::Eight(ramp8(w, h))
        } else {
            Self::Sixteen(ramp16(w, h))
        }
    }

    fn plan(&self) -> StructureConversionPlan {
        match self {
            Self::Sixteen(doc) => doc.promote_to_float_plan(),
            Self::Eight(doc) => doc.promote_to_float_plan(),
        }
        .expect("the plan")
    }

    fn promote(&self, control: Option<&DrJobControl>) -> Result<PrecisionDocument, String> {
        match self {
            Self::Sixteen(doc) => doc.promote_to_float(LIMIT, 0, control),
            Self::Eight(doc) => doc.promote_to_float(LIMIT, 0, control),
        }
    }

    /// The source's own plan is clean at the current process limit.
    fn clean(&self) -> bool {
        match self {
            Self::Sixteen(doc) => doc.native_plan().is_ok(),
            Self::Eight(doc) => doc
                .promote_structure_plan()
                .is_ok_and(|plan| plan.blocked.is_empty()),
        }
    }
}

fn figure(plan: &StructureConversionPlan) -> u64 {
    plan.scratch_bytes + plan.output_bytes
}

#[test]
fn the_float_plans_refuse_a_result_too_large_to_save_and_reopen() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    for source in [
        Source::Sixteen(ramp16(1024, 1024)),
        Source::Eight(ramp8(1024, 1024)),
    ] {
        assert!(set_process_working_limit(LIMIT));
        let float = source
            .promote(None)
            .expect("the conversion at the reference limit");
        assert!(float.native_plan().is_ok());
        // The smallest limit the float result saves and reopens under.
        let (mut low, mut high) = (1_u64, LIMIT);
        assert!(set_process_working_limit(low));
        assert!(float.native_plan().is_err());
        while high - low > 1 {
            let mid = low + (high - low) / 2;
            assert!(set_process_working_limit(mid));
            if float.native_plan().is_ok() {
                high = mid;
            } else {
                low = mid;
            }
        }
        let limit = high;
        assert!(set_process_working_limit(limit - 1));
        let Err(sentence) = float.native_plan() else {
            panic!("one byte under, it refuses");
        };
        let clean = source.clean();
        let plan = source.plan();
        let token = DrJobControl::default();
        let refused = source.promote(Some(&token)).err();
        assert!(set_process_working_limit(LIMIT));
        println!("capacity row: L {limit}: {sentence}");
        assert!(clean, "the source's own plan is clean at L − 1");
        assert_eq!(plan.blocked.len(), 1, "{:?}", plan.refusal());
        assert_eq!(plan.blocked[0].id, 0);
        assert_eq!(plan.blocked[0].note, sentence);
        assert_eq!(refused, plan.refusal());
        assert_eq!(token.progress(), 0.0);
        assert_eq!(token.scratch_bytes(), 0);
    }
}

#[test]
fn the_conversions_into_float_are_admitted_at_their_measured_floors() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    assert!(set_process_working_limit(LIMIT));
    for (case, floor) in [
        ("p16_24", P16_24_FLOOR),
        ("p16_61", P16_61_FLOOR),
        ("p8_24", P8_24_FLOOR),
        ("p8_61", P8_61_FLOOR),
    ] {
        let plan = Source::of(case).plan();
        assert!(plan.blocked.is_empty(), "{case}: {:?}", plan.refusal());
        let figure = figure(&plan);
        println!("{case}: figure {figure} floor {floor}");
        assert!(figure >= floor, "{case}: {figure} < floor {floor}");
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

/// One case per process, selected by `DR_FP_CASE`: `p16_24` and `p16_61` (a
/// 6000×4000 and a 9568×6376 16-bit ramp converted into float), `p8_24` and
/// `p8_61` (the same pictures at 8 bits), and `base` (the 24 MP 16-bit source
/// built and nothing run). The source is built, the footprint read, the
/// interval peak reset and the conversion run with its result held until E =
/// interval peak − pre is read. `plan` is scratch + output, the figure the
/// default test asserts.
#[test]
#[ignore = "measurement: DR_FP_CASE=base|p16_24|p16_61|p8_24|p8_61, Release, MallocLargeCache=0"]
fn measure() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let case = std::env::var("DR_FP_CASE").expect("DR_FP_CASE");
    assert!(set_process_working_limit(LIMIT));
    let source = Source::of(if case == "base" { "p16_24" } else { &case });
    let plan = if case == "base" {
        0
    } else {
        figure(&source.plan())
    };
    let pre = rusage().fields[PHYS_FOOTPRINT];
    unsafe { proc_reset_footprint_interval(std::process::id() as i32) };
    let converted = (case != "base").then(|| source.promote(None).expect("the conversion"));
    let peak = rusage().fields[INTERVAL_MAX_PHYS_FOOTPRINT];
    drop(converted);
    let e = peak.saturating_sub(pre);
    let floor = (e * 5).div_ceil(4);
    println!("FP_MEASURE case={case} plan={plan} pre={pre} peak={peak} E={e} floor={floor}");
    assert!(
        case == "base" || plan >= floor,
        "{case}: plan {plan} < floor {floor}"
    );
}
