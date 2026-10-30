//! Quick Mask's raw preview and recorded replay use one pixel operation.
//! Independent integer/f64 references and late-malformed-record fixtures
//! verify numeric equivalence and all-or-nothing caller-buffer mutation.

use std::ptr;

use doctored_core::ffi_scope::*;

#[test]
fn raw_preview_and_run_replay_match_independent_source_over() {
    let before: Vec<u8> = (0..256).map(|i| i as u8).collect();
    let overlay: Vec<u8> = (0..256)
        .flat_map(|i| {
            let alpha = (i * 37 % 256) as u8;
            let gray = (u16::from(alpha) * (i * 13 % 256) as u16 / 255) as u8;
            [gray, gray, gray, alpha]
        })
        .collect();
    for scale in [0, 1, 63, 128, 201, 255] {
        let mut raw = before.clone();
        assert!(unsafe {
            dr_selection_paint_quick_mask(raw.as_mut_ptr(), overlay.as_ptr(), 16, 16, scale)
        });
        let mut runs = Vec::new();
        let want: Vec<u8> = before
            .iter()
            .zip(overlay.chunks_exact(4))
            .map(|(&old, source)| {
                let a = (u32::from(source[3]) * u32::from(scale) / 255) as u8;
                let g = (u32::from(source[0]) * u32::from(scale) / 255) as u8;
                runs.extend_from_slice(&[1, 0, 0, 0, g, a]);
                (f64::from(g) + (f64::from(old) * (1.0 - f64::from(a) / 255.0)).round()) as u8
            })
            .collect();
        assert_eq!(raw, want, "raw scale {scale}");
        let mut replay = before.clone();
        assert!(unsafe {
            dr_selection_paint_quick_mask_rle(
                replay.as_mut_ptr(),
                16,
                16,
                runs.as_ptr(),
                runs.len(),
            )
        });
        assert_eq!(replay, want, "replay scale {scale}");
    }
    let mut repeated = [41; 8];
    let runs = [8, 0, 0, 0, 64, 128];
    assert!(unsafe {
        dr_selection_paint_quick_mask_rle(repeated.as_mut_ptr(), 4, 2, runs.as_ptr(), runs.len())
    });
    assert_eq!(repeated, [84; 8]);
}

#[test]
fn malformed_strokes_leave_every_selection_byte_untouched() {
    let original = [17, 41, 139, 251];
    let invalid_runs: &[&[u8]] = &[
        &[],
        &[1, 0, 0, 0, 255],                                // truncated records
        &[3, 0, 0, 0, 255, 255],                           // does not cover canvas
        &[5, 0, 0, 0, 255, 255],                           // runs past canvas
        &[0, 0, 0, 0, 255, 255],                           // zero-length run
        &[2, 0, 0, 0, 255, 255, 2, 0, 0, 0, 2, 1],         // late invalid premultiplication
        &[2, 0, 0, 0, 255, 255, 255, 255, 255, 255, 0, 0], // late overshoot
    ];
    for runs in invalid_runs {
        let mut mask = original;
        assert!(!unsafe {
            dr_selection_paint_quick_mask_rle(mask.as_mut_ptr(), 2, 2, runs.as_ptr(), runs.len())
        });
        assert_eq!(mask, original);
    }
    let invalid_overlay = [
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 2, 0, 0, 1,
    ];
    let mut mask = original;
    assert!(!unsafe {
        dr_selection_paint_quick_mask(mask.as_mut_ptr(), invalid_overlay.as_ptr(), 2, 2, 255)
    });
    assert_eq!(
        mask, original,
        "late invalid overlay partially painted selection"
    );
}

#[test]
fn quick_mask_exports_refuse_null_zero_and_oversized_inputs() {
    let original = [17, 41, 139, 251];
    let mut mask = original;
    let overlay = [0; 16];
    let runs = [4, 0, 0, 0, 0, 0];
    unsafe {
        assert!(!dr_selection_paint_quick_mask(
            ptr::null_mut(),
            overlay.as_ptr(),
            2,
            2,
            255
        ));
        assert!(!dr_selection_paint_quick_mask(
            mask.as_mut_ptr(),
            ptr::null(),
            2,
            2,
            255
        ));
        assert!(!dr_selection_paint_quick_mask(
            mask.as_mut_ptr(),
            overlay.as_ptr(),
            0,
            2,
            255
        ));
        assert!(!dr_selection_paint_quick_mask(
            mask.as_mut_ptr(),
            overlay.as_ptr(),
            u32::MAX,
            u32::MAX,
            255
        ));
        assert!(!dr_selection_paint_quick_mask_rle(
            ptr::null_mut(),
            2,
            2,
            runs.as_ptr(),
            runs.len()
        ));
        assert!(!dr_selection_paint_quick_mask_rle(
            mask.as_mut_ptr(),
            2,
            2,
            ptr::null(),
            runs.len()
        ));
        assert!(!dr_selection_paint_quick_mask_rle(
            mask.as_mut_ptr(),
            0,
            2,
            runs.as_ptr(),
            runs.len()
        ));
        assert!(!dr_selection_paint_quick_mask_rle(
            mask.as_mut_ptr(),
            u32::MAX,
            u32::MAX,
            runs.as_ptr(),
            runs.len()
        ));
    }
    assert_eq!(mask, original);
}
