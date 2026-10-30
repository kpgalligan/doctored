//! Independent f64 harmonic reference for the typed shared membrane solve.
//! The reference uses lexicographic relaxation and directly constructs the
//! finite-difference equation; it never calls the production classifier/solver.
use crate::common;
use common::probe::{cancelled_parked_at, entry_then_cancel, parked_line};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_paint::PaintEdit;
use serde_json::json;

fn document(w: u32, h: u32, dest: Vec<[u16; 4]>, source: Vec<[u16; 4]>) -> PrecisionDocument {
    let p = IccProfile::srgb();
    PrecisionDocument::from_parts(
        w,
        h,
        p.clone(),
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster(
                "Destination",
                PrecisionImage::from_rgba16(w, h, dest, p.clone(), None).unwrap(),
                (0, 0),
                1.0,
                true,
            )
            .unwrap(),
            Layer::raster(
                "Captured source",
                PrecisionImage::from_rgba16(w, h, source, p, None).unwrap(),
                (0, 0),
                0.0,
                false,
            )
            .unwrap(),
        ],
    )
    .unwrap()
}
fn edit(w: u32, h: u32, opacity: f32) -> PaintEdit {
    PaintEdit::parse(&json!({"op":"heal","layer":0,"source_layer":1,"dx":0,"dy":0,"x":0,"y":0,"width":w,"height":h,"opacity":opacity}).to_string()).unwrap()
}
fn neighbors(i: usize, w: usize, h: usize) -> Vec<usize> {
    let (x, y) = (i % w, i / w);
    let mut n = Vec::new();
    if x > 0 {
        n.push(i - 1);
    }
    if x + 1 < w {
        n.push(i + 1);
    }
    if y > 0 {
        n.push(i - w);
    }
    if y + 1 < h {
        n.push(i + w);
    }
    n
}
fn reference(w: usize, h: usize, b: &[f64], covered: &[bool], usable: &[bool]) -> Vec<f64> {
    let n = w * h;
    let boundary: Vec<_> = (0..n)
        .map(|i| {
            covered[i] && usable[i] && neighbors(i, w, h).iter().any(|&j| usable[j] && !covered[j])
        })
        .collect();
    let mut u = vec![0.; n];
    for i in 0..n {
        if boundary[i] {
            u[i] = b[i];
        }
    }
    let mut converged = false;
    for _ in 0..30000 {
        let mut delta = 0f64;
        for i in 0..n {
            if !covered[i] || !usable[i] || boundary[i] {
                continue;
            }
            let ns: Vec<_> = neighbors(i, w, h)
                .into_iter()
                .filter(|&j| covered[j] && usable[j])
                .collect();
            if ns.is_empty() {
                continue;
            }
            let value = ns.iter().map(|&j| u[j]).sum::<f64>() / ns.len() as f64;
            delta = delta.max((value - u[i]).abs());
            u[i] = value;
        }
        if delta < 1e-10 {
            converged = true;
            break;
        }
    }
    assert!(converged);
    u
}

#[test]
fn typed_heal_matches_f64_domains_preserves_tiny_alpha_and_original_storage() {
    let (w, h) = (25u32, 23u32);
    let n = (w * h) as usize;
    let src: Vec<_> = (0..n)
        .map(|i| {
            let x = i % w as usize;
            let y = i / w as usize;
            [
                20000 + ((x * 17 + y * 23) % 1111) as u16,
                30000 + ((x * x * 3 + y * 7) % 977) as u16,
                40000 + ((x * 11 + y * y * 5) % 997) as u16,
                65535,
            ]
        })
        .collect();
    let dest: Vec<_> = (0..n)
        .map(|i| {
            let x = i % w as usize;
            let y = i / w as usize;
            [
                21000 + ((x * x * 31 + y * 79) % 9001) as u16,
                27000 + ((x * 37 + y * y * 97) % 7123) as u16,
                39000 + ((x * y * 13) % 3991) as u16,
                if i == 12 * w as usize + 12 {
                    0
                } else {
                    [1, 2, 3, 256, 65535][i % 5]
                },
            ]
        })
        .collect();
    for shape in 0..4 {
        let coverage: Vec<_> = (0..n)
            .map(|i| {
                let (x, y) = (i % w as usize, i / w as usize);
                let inside = match shape {
                    0 => (3..22).contains(&x) && (3..20).contains(&y),
                    1 => x < 16 && (4..20).contains(&y),
                    2 => (x as i32 - 12).pow(2) + (y as i32 - 11).pow(2) < 81,
                    _ => !(9..=15).contains(&x) && (3..19).contains(&y),
                };
                if inside {
                    if i % 3 == 0 {
                        32768
                    } else {
                        65535
                    }
                } else {
                    32767
                }
            })
            .collect();
        let d = document(w, h, dest.clone(), src.clone());
        let op = edit(w, h, 0.625);
        let out = d
            .paint(&op, &coverage, REFERENCE_WORKING_BYTES, 0, None)
            .unwrap()
            .unwrap();
        let pixels = out.layer_source(0).unwrap().to_rgba16().ok().unwrap();
        let covered: Vec<_> = coverage.iter().map(|&v| v >= 32768).collect();
        let usable: Vec<_> = dest.iter().map(|v| v[3] > 0).collect();
        let mut worst = 0f64;
        for c in 0..3 {
            let b: Vec<_> = (0..n)
                .map(|i| f64::from(dest[i][c]) - f64::from(src[i][c]))
                .collect();
            let u = reference(w as usize, h as usize, &b, &covered, &usable);
            for i in 0..n {
                let expected = if covered[i] && usable[i] {
                    (f64::from(dest[i][c])
                        + 0.625 * (f64::from(src[i][c]) + u[i] - f64::from(dest[i][c])))
                    .clamp(0., 65535.)
                    .round()
                } else {
                    f64::from(dest[i][c])
                };
                worst = worst.max((f64::from(pixels[i][c]) - expected).abs());
            }
        }
        assert!(worst <= 2., "shape{shape} error {worst} u16 codes");
        for i in 0..n {
            assert_eq!(pixels[i][3], dest[i][3]);
            if dest[i][3] == 0 {
                assert_eq!(pixels[i], dest[i]);
            }
        }
        assert_eq!(d.layer_source(0).unwrap().to_rgba16().ok().unwrap(), dest);
        let reopened = PrecisionDocument::parse_native(
            &out.encode_native(REFERENCE_WORKING_BYTES, 0, None).unwrap(),
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap();
        assert_eq!(
            reopened.layer_source(0).unwrap().to_rgba16().ok().unwrap(),
            pixels
        );
        eprintln!("typed healing shape{shape} maximum error {worst} u16 codes");
    }
}

#[test]
fn pure_neumann_heal_uses_shared_mean_gauge_and_retains_subbyte_source_detail() {
    let (w, h) = (9u32, 9u32);
    let n = (w * h) as usize;
    let src: Vec<_> = (0..n)
        .map(|i| [20000 + i as u16, 30000 + i as u16, 40000 + i as u16, 65535])
        .collect();
    let dest: Vec<_> = (0..n)
        .map(|i| {
            [
                25000 + (i % 7) as u16,
                35000 + (i % 11) as u16,
                45000 + (i % 13) as u16,
                1,
            ]
        })
        .collect();
    let d = document(w, h, dest.clone(), src.clone());
    let out = d
        .paint(
            &edit(w, h, 1.0),
            &vec![65535; n],
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let pixels = out.layer_source(0).unwrap().to_rgba16().ok().unwrap();
    for c in 0..3 {
        let mean = (0..n)
            .map(|i| f64::from(dest[i][c]) - f64::from(src[i][c]))
            .sum::<f64>()
            / n as f64;
        for i in 0..n {
            assert!((f64::from(pixels[i][c]) - (f64::from(src[i][c]) + mean).round()).abs() <= 1.);
        }
    }
    assert!(pixels.windows(2).all(|pair| pair[1][0] == pair[0][0] + 1));
    assert!(pixels.iter().all(|p| p[3] == 1));
}

#[test]
fn healing_refusals_noops_and_job_cleanup_are_atomic() {
    let n = 17 * 17;
    let samples = vec![[12001, 23002, 34003, 2]; n];
    let d = document(17, 17, samples.clone(), samples.clone());
    let op = edit(17, 17, 1.0);
    assert!(d
        .paint(&op, &vec![65535; n], REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .is_none());
    let cancelled = DrJobControl::default();
    cancelled.cancel();
    assert!(d
        .paint(
            &op,
            &vec![65535; n],
            REFERENCE_WORKING_BYTES,
            0,
            Some(&cancelled)
        )
        .is_err());
    assert_eq!(cancelled.scratch_bytes(), 0);
    assert!(d.paint(&op, &vec![65535; n], 1, 0, None).is_err());
    // A 24 MP window's solve was past the 2·10⁹ work units the paint plan
    // used to refuse at. The heal runs under a job control, so the plan
    // answers with the window's 160 B/px; the solve window limit still refuses.
    let large = d
        .paint_plan(&edit(6000, 4000, 1.0))
        .expect("the 24 MP heal plan answers");
    assert!(
        large.scratch_bytes >= 24_000_000 * 160,
        "{}",
        large.scratch_bytes
    );
    assert!(d
        .paint_plan(&edit(8000, 4001, 1.0))
        .err()
        .unwrap()
        .contains("healing window exceeds the shared solve limit"));
    assert_eq!(
        d.layer_source(0).unwrap().to_rgba16().ok().unwrap(),
        samples
    );
}

#[test]
fn active_healing_cancels_inside_shared_solver_and_releases_reservation() {
    let (w, h) = (220u32, 220u32);
    let n = (w * h) as usize;
    let source: Vec<_> = (0..n)
        .map(|i| {
            [
                20000 + (i % 137) as u16,
                30000 + (i % 139) as u16,
                40000 + (i % 149) as u16,
                65535,
            ]
        })
        .collect();
    let dest: Vec<_> = (0..n)
        .map(|i| {
            [
                21000 + ((i * 31) % 10007) as u16,
                31000 + ((i * 37) % 10009) as u16,
                41000 + ((i * 41) % 10037) as u16,
                2,
            ]
        })
        .collect();
    let d = document(w, h, dest.clone(), source);
    let coverage: Vec<_> = (0..n)
        .map(|i| {
            if i % w as usize > 1
                && (i % w as usize) < (w - 2) as usize
                && i / w as usize > 1
                && (i / w as usize) < (h - 2) as usize
            {
                65535
            } else {
                0
            }
        })
        .collect();
    let op = edit(w, h, 1.0);
    let render = |token: usize| {
        let control = unsafe { &*(token as *const DrJobControl) };
        match d.paint(&op, &coverage, REFERENCE_WORKING_BYTES, 0, Some(control)) {
            Ok(_) => (true, String::new()),
            Err(error) => (false, error),
        }
    };
    let (entry, next) = entry_then_cancel(&|_, stage, _| stage == 4, render);
    let n0 = entry.checkpoints;
    assert!(
        next.expect("the entry's kernel never reported again")
            .checkpoints
            > n0 + 3,
        "the entry's kernel polls fewer than three checkpoints before its next report"
    );
    let parked = cancelled_parked_at(
        n0 + 3,
        &|progress, stage| stage == entry.stage && progress == entry.progress,
        render,
    );
    eprintln!(
        "typed healing solver cancellation: {}",
        parked_line(&parked)
    );
    assert!(parked.scratch > 0);
    assert_eq!(d.layer_source(0).unwrap().to_rgba16().ok().unwrap(), dest);
}

mod bench {
    use super::*;

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn active_healing_cancels_inside_shared_solver_and_releases_reservation() {
        use std::time::{Duration, Instant};
        let (w, h) = (220u32, 220u32);
        let n = (w * h) as usize;
        let source: Vec<_> = (0..n)
            .map(|i| {
                [
                    20000 + (i % 137) as u16,
                    30000 + (i % 139) as u16,
                    40000 + (i % 149) as u16,
                    65535,
                ]
            })
            .collect();
        let dest: Vec<_> = (0..n)
            .map(|i| {
                [
                    21000 + ((i * 31) % 10007) as u16,
                    31000 + ((i * 37) % 10009) as u16,
                    41000 + ((i * 41) % 10037) as u16,
                    2,
                ]
            })
            .collect();
        let d = document(w, h, dest.clone(), source);
        let coverage: Vec<_> = (0..n)
            .map(|i| {
                if i % w as usize > 1
                    && (i % w as usize) < (w - 2) as usize
                    && i / w as usize > 1
                    && (i / w as usize) < (h - 2) as usize
                {
                    65535
                } else {
                    0
                }
            })
            .collect();
        let control = DrJobControl::default();
        let op = edit(w, h, 1.0);
        std::thread::scope(|scope| {
            let work =
                scope.spawn(|| d.paint(&op, &coverage, REFERENCE_WORKING_BYTES, 0, Some(&control)));
            let start = Instant::now();
            while control.stage() != 4
                && !work.is_finished()
                && start.elapsed() < Duration::from_secs(10)
            {
                std::thread::sleep(Duration::from_micros(100));
            }
            assert_eq!(control.stage(), 4, "solver never reached its healing stage");
            assert!(control.scratch_bytes() > 0);
            control.cancel();
            let stopped = Instant::now();
            let result = work.join().unwrap();
            assert!(result.err().unwrap().contains("cancel"));
            assert!(stopped.elapsed() < Duration::from_secs(2));
        });
        assert_eq!(control.scratch_bytes(), 0);
        assert_eq!(d.layer_source(0).unwrap().to_rgba16().ok().unwrap(), dest);
    }
}
