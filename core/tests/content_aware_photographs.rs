//! Independent photographic quality gate for constrained fill at both depths.
//! The same licensed/pinned corpus as 9D; original decoded pixels are the
//! reference. The unwanted source patch is changed twice, and a source exclusion
//! outside the fill's target-patch context must make the repair invariant.
use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_content_aware::dr_doc_content_aware_fill_edit;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::ptr;

fn error_message(error: *mut c_char) -> String {
    if error.is_null() {
        return "no error".into();
    }
    let value = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(error) };
    value
}

fn fill(
    w: u32,
    h: u32,
    depth: u8,
    pixels: &[[u16; 4]],
    mask: &[u8],
    request: &Value,
) -> Vec<[u16; 4]> {
    let request = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    if depth == 8 {
        let raw: Vec<u8> = pixels.iter().flatten().map(|v| *v as u8).collect();
        let source = DrDocument::from_pixels(image::RgbaImage::from_raw(w, h, raw).unwrap());
        let output = unsafe {
            dr_doc_content_aware_fill_edit(
                &source,
                request.as_ptr(),
                mask.as_ptr(),
                ptr::null(),
                &mut error,
            )
        };
        assert!(!output.is_null(), "{}", error_message(error));
        assert!(error.is_null());
        let output = unsafe { Box::from_raw(output) };
        output.layers[0]
            .pixels
            .contiguous_image()
            .pixels()
            .map(|p| p.0.map(u16::from))
            .collect()
    } else {
        let image =
            PrecisionImage::from_rgba16(w, h, pixels.to_vec(), IccProfile::srgb(), None).unwrap();
        let source =
            PrecisionDocument::from_image(image, Metadata::default(), Resolution::default())
                .unwrap();
        let coverage: Vec<_> = mask.iter().map(|v| u16::from(*v) * 257).collect();
        let output = unsafe {
            dr_precision_doc_edit(
                &source,
                request.as_ptr(),
                coverage.as_ptr(),
                coverage.len(),
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut error,
            )
        };
        assert!(!output.is_null(), "{}", error_message(error));
        assert!(error.is_null());
        let output = unsafe { Box::from_raw(output) };
        output.layer_source(0).unwrap().to_rgba16().unwrap()
    }
}

#[test]
fn excluded_photographic_contaminants_do_not_change_native_repairs() {
    let Ok(root) = std::env::var("DR_RETOUCH_PHOTOGRAPHS") else {
        eprintln!("SKIPPED photographic source-exclusion gate: set DR_RETOUCH_PHOTOGRAPHS");
        return;
    };
    for (name, hash, cx, cy) in [
        (
            "astronaut.png",
            "88431cd9653ccd539741b555fb0a46b61558b301d4110412b5bc28b5e3ea6cb5",
            197u32,
            122u32,
        ),
        (
            "fur.png",
            "596aa1e7cb875eb79f437e310381d26b338a81c2da23439704a73c4651e8c4bb",
            220u32,
            68u32,
        ),
    ] {
        let path = std::path::Path::new(&root).join(name);
        let digest = std::process::Command::new("shasum")
            .args(["-a", "256"])
            .arg(&path)
            .output()
            .unwrap();
        assert!(digest.status.success());
        assert_eq!(
            std::str::from_utf8(&digest.stdout)
                .unwrap()
                .split_whitespace()
                .next(),
            Some(hash)
        );
        let photo = image::load_from_memory(&std::fs::read(path).unwrap())
            .unwrap()
            .to_rgba8();
        let (w, h) = photo.dimensions();
        for depth in [8u8, 16] {
            let scale = if depth == 16 { 257u16 } else { 1u16 };
            let maximum = if depth == 16 { 65535 } else { 255 };
            // The disclosed generated native carrier is the 9D fixture's; the
            // camera photograph itself is byte depth, never relabelled u16.
            let reference: Vec<_> = photo
                .pixels()
                .enumerate()
                .map(|(i, p)| {
                    let extra = if depth == 16 {
                        [i % 31, (i * 7) % 31, (i * 11) % 31]
                    } else {
                        [0; 3]
                    };
                    [
                        (u16::from(p[0]) * scale).saturating_add(extra[0] as u16),
                        (u16::from(p[1]) * scale).saturating_add(extra[1] as u16),
                        (u16::from(p[2]) * scale).saturating_add(extra[2] as u16),
                        maximum,
                    ]
                })
                .collect();
            let mut mask = vec![0u8; (w * h) as usize];
            let request = json!({"op":"content_aware_fill", "layer":0,
            "x":cx-4,"y":cy-4,"width":9,"height":9,
            "ring":0,"seed":12345,"sample":"current","output":"layer",
            "source_mask":{"version":1,"initial":"all","operations":[
                {"op":"exclude_rect","x":cx+12,"y":cy-8,"width":8,"height":16}
            ]}});
            let code = |v: u16| {
                if depth == 16 {
                    v
                } else {
                    (f64::from(v) / 257.).round() as u16
                }
            };
            let mut damaged = reference.clone();
            for y in cy - 4..=cy + 4 {
                for x in cx - 4..=cx + 4 {
                    if (x as i32 - cx as i32).pow(2) + (y as i32 - cy as i32).pow(2) > 16 {
                        continue;
                    }
                    let i = (y * w + x) as usize;
                    mask[i] = 255;
                    damaged[i][..3].copy_from_slice(&[code(60001), code(3001), code(50003)]);
                }
            }
            let mut variants = Vec::new();
            for contaminant in [[60001, 3001, 50003], [3001, 55003, 9001]] {
                let mut input = damaged.clone();
                // Beyond the hole plus the six-pixel target-patch fringe, but
                // still within the automatic source ring: changing this area
                // cannot legitimately alter target/boundary context.
                for y in cy - 8..cy + 8 {
                    for x in cx + 12..cx + 20 {
                        input[(y * w + x) as usize][..3].copy_from_slice(&contaminant.map(code));
                    }
                }
                let result = fill(w, h, depth, &input, &mask, &request);
                for (i, p) in result.iter().enumerate() {
                    assert_eq!(p[3], input[i][3]);
                    if mask[i] == 0 {
                        assert_eq!(*p, input[i], "{name} rgba{depth}, outside pixel{i}");
                    }
                }
                variants.push(result);
            }
            let (mut before, mut after, mut count) = (0f64, 0f64, 0u64);
            for (i, &coverage) in mask.iter().enumerate() {
                if coverage == 0 {
                    continue;
                }
                assert_eq!(
                    variants[0][i], variants[1][i],
                    "excluded source altered repair: {name} rgba{depth} pixel{i}"
                );
                for (c, &expected) in reference[i][..3].iter().enumerate() {
                    before += f64::from(damaged[i][c].abs_diff(expected));
                    after += f64::from(variants[0][i][c].abs_diff(expected));
                    count += 1;
                }
            }
            let mae = after / count as f64 / f64::from(scale);
            // Frozen before running, matching the original small-spot gate.
            assert!(mae < 15., "{name} rgba{depth}: MAE{mae} byte codes");
            assert!(
                after < before * 0.25,
                "{name} rgba{depth}: corruption fraction{}",
                after / before
            );
            eprintln!("{name} rgba{depth}: constrained MAE {mae:.4}/255, {:.3}% corruption remaining; excluded-source variants exact",100.*after/before);
        }
    }
}
