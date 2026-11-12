//! The 8-bit native reopen plan (`drdc_reopen_plan`). Each fixture is
//! saved through `dr_doc_save_native` and reopened through `dr_doc_open`.
//! (a) the reopened document's authoritative retention fits `retained_bytes`;
//! (b) `retained ≤ serialized ≤ decode` and decode covers the file plus every
//! decoded record; (c) the export answers the method; (d) a placed layer's
//! shared source counts once; (e) the plan does not move when the views are
//! evicted; (f) NULL inputs answer false with an error; (g) the decode term's
//! transient is the reader stage the fixture's dimensions give; (h) pins the
//! reopened documents' decoded bytes, captured before the reader's layer
//! decode moved from `to_rgba8` to `into_rgba8`. Only (e) evicts in this
//! binary.

use doctored_core::doc::{DrDocument, MaskKind};
use doctored_core::drdc_reopen_plan::DECODE_OVERHEAD_BYTES;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::{dr_doc_native_reopen_plan, dr_doc_open, dr_doc_save_native};
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use doctored_core::retained_source::SourceEdit;
use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba, RgbaImage};
use serde_json::json;
use std::ffi::{c_char, CStr, CString};
use std::io::Cursor;
use std::path::Path;
use std::ptr;
use tempfile::TempDir;

use crate::common::sha256;
use crate::resource_report;
use crate::serial;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

unsafe fn text(pointer: *mut c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let value = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(pointer) };
    value
}

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Four-channel noise: every byte of every pixel from the generator.
fn noise(w: u32, h: u32, seed: u64) -> RgbaImage {
    let mut state = seed;
    let mut out = RgbaImage::new(w, h);
    for p in out.pixels_mut() {
        p.0 = (splitmix(&mut state) as u32).to_le_bytes();
    }
    out
}

/// A white `canvas` Background plus a `source`-sized noise asset placed
/// through the `place` op (scaled to the canvas width when larger), so the
/// layer holds its retained 8-bit source.
fn placed_into(canvas: (u32, u32), source: (u32, u32)) -> DrDocument {
    let background =
        DrDocument::from_pixels(RgbaImage::from_pixel(canvas.0, canvas.1, Rgba([255; 4])));
    let scale = f64::from(canvas.0) / f64::from(source.0);
    let source = DrDocument::from_pixels(noise(source.0, source.1, 0x5EED));
    let mut request =
        json!({"op":"place","source_id":"n","revision_id":"a","name":"Noise","x":0,"y":0});
    // A source larger than the canvas is fitted to it, so its asset outsizes
    // every layer plane (fixture (vi)).
    if scale != 1.0 {
        request["source_transform"] = json!([scale, 0.0, 0.0, scale, 0.0, 0.0]);
    }
    let edit = SourceEdit::parse(&request.to_string()).unwrap();
    background
        .source_edit(Some(&source), &edit, LIMIT, 0, None)
        .unwrap()
        .unwrap()
}

/// A white Background plus a placed noise layer of the same size.
fn placed(w: u32, h: u32) -> DrDocument {
    placed_into((w, h), (w, h))
}

/// The six fixtures (i)–(vi).
fn fixtures() -> Vec<(&'static str, DrDocument)> {
    let plain = DrDocument::from_pixels(noise(200, 150, 1));
    let two = placed(256, 256);
    let duplicated = two.duplicate_layers(&[1]).unwrap();
    assert!(duplicated.layers[2].retained_source.is_some());
    let raster = DrDocument::from_pixels(noise(200, 150, 2))
        .duplicate_layers(&[0])
        .unwrap();
    let plane: Vec<u8> = noise(256, 256, 3)
        .into_raw()
        .into_iter()
        .step_by(4)
        .collect();
    let masked = two
        .add_mask(1, MaskKind::RevealAll)
        .unwrap()
        .add_channel("Alpha 1", &plane, 256, 256, [255, 0, 0], 0.5)
        .unwrap();
    let wide = placed_into((64, 48), (256, 192));
    let asset = wide.layers[1]
        .retained_source
        .as_ref()
        .unwrap()
        .asset
        .dimensions();
    let asset_area = u64::from(asset.0) * u64::from(asset.1);
    for layer in &wide.layers {
        let (w, h) = layer.pixels.dimensions();
        assert!(
            asset_area > 2 * u64::from(w) * u64::from(h),
            "fixture (vi): asset {asset:?} vs plane {w}x{h}"
        );
    }
    vec![
        ("i", plain),
        ("ii", two),
        ("iii", duplicated),
        ("iv", raster),
        ("v", masked),
        ("vi", wide),
    ]
}

/// (g)'s oracle: the largest reader stage beside the end state, from each
/// fixture's dimensions (layer planes and 8-bit assets at 4 B/px, channels at
/// 2 B/px of the canvas).
fn transient(name: &str) -> u64 {
    match name {
        "i" | "iv" => 4 * 200 * 150,
        "ii" | "iii" => 4 * 256 * 256,
        // The layer, channel and asset stages; the largest wins.
        "v" => [4 * 256 * 256, 2 * 256 * 256, 4 * 256 * 256]
            .into_iter()
            .max()
            .unwrap(),
        "vi" => 4 * 256 * 192,
        _ => unreachable!("{name}"),
    }
}

/// The export's two figures, which must answer exactly the method.
fn ffi_plan(doc: &DrDocument) -> (u64, u64) {
    let (mut retained, mut decode) = (0u64, 0u64);
    let mut error = ptr::null_mut();
    let ok = unsafe { dr_doc_native_reopen_plan(doc, &mut retained, &mut decode, &mut error) };
    assert!(ok, "{}", unsafe { text(error) });
    assert!(error.is_null());
    (retained, decode)
}

/// The authoritative bytes a document retains: unique minus caches.
fn held(doc: &DrDocument) -> u64 {
    let report = resource_report::report(&[doc as *const DrDocument], &[], &[], &[], 0, 0).unwrap();
    report["unique_retained_bytes"].as_u64().unwrap() - report["cache_bytes"].as_u64().unwrap()
}

/// Saves through the export and answers the file's path.
fn save(doc: &DrDocument, dir: &Path, name: &str) -> String {
    let path = dir.join(name).to_str().unwrap().to_string();
    let c = CString::new(path.clone()).unwrap();
    let mut error = ptr::null_mut();
    let ok = unsafe { dr_doc_save_native(doc, c.as_ptr(), std::ptr::null(), &mut error) };
    assert!(ok, "{name}: {}", unsafe { text(error) });
    path
}

/// Reopens through the export, or answers its error sentence.
fn open(path: &str) -> Result<Box<DrDocument>, String> {
    let c = CString::new(path).unwrap();
    let mut error = ptr::null_mut();
    let doc = unsafe { dr_doc_open(c.as_ptr(), std::ptr::null(), &mut error) };
    if doc.is_null() {
        return Err(unsafe { text(error) });
    }
    Ok(unsafe { Box::from_raw(doc) })
}

/// (h)'s digest of a reopened document (the spec's byte order).
fn digest(doc: &DrDocument) -> String {
    let mut bytes = Vec::new();
    for layer in &doc.layers {
        let (w, h) = layer.pixels.dimensions();
        bytes.extend(w.to_le_bytes());
        bytes.extend(h.to_le_bytes());
        bytes.extend(layer.pixels.to_contiguous().unwrap().as_raw());
        match &layer.mask {
            Some(mask) => {
                bytes.push(1);
                bytes.extend(mask.as_raw());
            }
            None => bytes.push(0),
        }
        if let Some(source) = &layer.retained_source {
            if let Some(pixels) = source.asset.rgba8() {
                bytes.extend(pixels.to_contiguous().unwrap().as_raw());
            }
        }
    }
    for channel in &doc.channels {
        bytes.extend(channel.data.as_raw());
    }
    sha256::sha256_hex(&bytes)
}

fn encode(image: DynamicImage) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    image.write_to(&mut out, ImageFormat::Png).unwrap();
    out.into_inner()
}

/// A 64×48 noise raster whose layer PNG is replaced by `png`: the outcome
/// of reopening it, a digest or the refusal sentence.
fn spliced(dir: &Path, name: &str, png: impl Fn(&RgbaImage) -> Vec<u8>) -> String {
    let raster = noise(64, 48, 4);
    let path = save(&DrDocument::from_pixels(raster.clone()), dir, name);
    let bytes = std::fs::read(&path).unwrap();
    let start = bytes
        .windows(PNG_MAGIC.len())
        .position(|w| w == PNG_MAGIC)
        .unwrap();
    let len = u32::from_le_bytes(bytes[start - 4..start].try_into().unwrap()) as usize;
    let end = start
        + bytes[start..]
            .windows(4)
            .position(|w| w == b"IEND")
            .unwrap()
        + 8;
    assert_eq!(end - start, len, "{name}: the layer PNG runs png_len bytes");
    let replacement = png(&raster);
    let mut out = bytes[..start - 4].to_vec();
    out.extend((replacement.len() as u32).to_le_bytes());
    out.extend(&replacement);
    out.extend(&bytes[start + len..]);
    std::fs::write(&path, out).unwrap();
    match open(&path) {
        Ok(doc) => digest(&doc),
        Err(message) => format!("refused: {message}"),
    }
}

/// (h) SHA-256 of each reopened document, captured at b941611 before the
/// reader change; never re-captured.
const PINS: [(&str, &str); 8] = [
    (
        "i",
        "ffc81d1908bd4723646630c4f8402ab4c64b9911809bbfb6093afd25b8cf675b",
    ),
    (
        "ii",
        "29ece1b7e4d35037bacd1e3e3002c47b460bd33c9a2a0e221fabdd1fc4d67d3d",
    ),
    (
        "iii",
        "257374e07d09ea3442a798b514bfb7f6a27660089d89c042a8186aa4197b4267",
    ),
    (
        "iv",
        "d508317b22c8c7384df5cce9bef2420ebb4aab0d7d29b6f6a799494b9d357e5f",
    ),
    (
        "v",
        "8af1a80b35a799f4764c46bf7603de9c19f718a6520cd2f0fb316a256f5daf75",
    ),
    (
        "vi",
        "5eb5c65bf2c624266f28e32c2cff6a7c722605fa0e0c3bba4d2d8532ac35cfea",
    ),
    (
        "i-rgb8",
        "f338b65818f8b40c70b2420272a5da9329a0e9a75bbfea3a0432a31aef8facbe",
    ),
    (
        "i-rgba16",
        "2858fac6f3e06ca8082f61baad8206b06a0141b3cb75ea7b57aaf8e85ce44178",
    ),
];

/// (h) The reader's decoded output is bit-identical to the pinned bytes.
#[test]
fn decode_pins() {
    let _serial = serial();
    let dir = TempDir::new().unwrap();
    let mut seen = Vec::new();
    for (name, doc) in fixtures() {
        let path = save(&doc, dir.path(), &format!("{name}.rz"));
        seen.push((name, digest(&open(&path).unwrap())));
    }
    seen.push((
        "i-rgb8",
        spliced(dir.path(), "rgb8.rz", |raster| {
            encode(DynamicImage::ImageRgb8(
                DynamicImage::ImageRgba8(raster.clone()).to_rgb8(),
            ))
        }),
    ));
    seen.push((
        "i-rgba16",
        spliced(dir.path(), "rgba16.rz", |raster| {
            let wide: ImageBuffer<Rgba<u16>, Vec<u16>> = ImageBuffer::from_raw(
                raster.width(),
                raster.height(),
                raster
                    .as_raw()
                    .iter()
                    .map(|&b| u16::from(b) * 257)
                    .collect(),
            )
            .unwrap();
            encode(DynamicImage::ImageRgba16(wide))
        }),
    ));
    for (name, got) in &seen {
        println!("PIN {name} {got}");
    }
    assert_eq!(seen.len(), PINS.len(), "every pinned fixture decodes");
    for ((name, got), (pinned_name, pin)) in seen.iter().zip(PINS) {
        assert_eq!(*name, pinned_name);
        assert_eq!(got, pin, "{name}");
    }
}

/// (a), (b), (c) and (g) over every fixture. If (a) fails the plan is fixed
/// upward, never this assertion.
#[test]
fn plan_covers_each_reopen() {
    let _serial = serial();
    let dir = TempDir::new().unwrap();
    for (name, doc) in fixtures() {
        let plan = doc.native_reopen_plan().unwrap();
        let path = save(&doc, dir.path(), &format!("{name}.rz"));
        let file = std::fs::metadata(&path).unwrap().len();
        let reopened = open(&path).unwrap();
        let kept = held(&reopened);
        assert!(
            kept <= plan.retained_bytes,
            "{name}: reopened {kept} > retained {}",
            plan.retained_bytes
        );
        assert!(plan.retained_bytes <= plan.serialized_bytes, "{name}");
        assert!(plan.serialized_bytes <= plan.decode_bytes, "{name}");
        assert!(
            plan.decode_bytes >= file + plan.serialized_bytes,
            "{name}: decode {} < file {file} + serialized {}",
            plan.decode_bytes,
            plan.serialized_bytes
        );
        assert_eq!(
            ffi_plan(&doc),
            (plan.retained_bytes, plan.decode_bytes),
            "{name}"
        );
        let encoded = doc.native_save_plan().unwrap().encoded_bound;
        assert_eq!(
            plan.decode_bytes - encoded - plan.serialized_bytes - DECODE_OVERHEAD_BYTES,
            transient(name),
            "{name}"
        );
    }
}

/// (d) A placed layer and its source share one plane: the plan keeps it once,
/// within 64 KiB of what the reopened document holds, and the decoded fallback
/// it frees is at least the plane.
#[test]
fn placed_source_counts_once() {
    let _serial = serial();
    let dir = TempDir::new().unwrap();
    let doc = placed(256, 256);
    let plan = doc.native_reopen_plan().unwrap();
    let reopened = open(&save(&doc, dir.path(), "placed.rz")).unwrap();
    let kept = held(&reopened);
    assert!(
        plan.retained_bytes <= kept + 64 * 1024,
        "retained {} vs reopened {kept}",
        plan.retained_bytes
    );
    assert!(plan.serialized_bytes - plan.retained_bytes >= 256 * 256 * 4);
}

/// (e) The plan reads metadata and pointer identity only: evicting the
/// views changes nothing.
#[test]
fn plan_does_not_depend_on_views() {
    let _serial = serial();
    let doc = placed(256, 256);
    let live = resource_report::report(&[&doc as *const DrDocument], &[], &[], &[], 0, 0).unwrap();
    assert!(
        live["contiguous_view_bytes"].as_u64().unwrap() > 0,
        "{live}"
    );
    let before = doc.native_reopen_plan().unwrap();
    resource_report::evict(&[&doc as *const DrDocument]).unwrap();
    let after = doc.native_reopen_plan().unwrap();
    assert_eq!(
        (
            before.serialized_bytes,
            before.retained_bytes,
            before.decode_bytes
        ),
        (
            after.serialized_bytes,
            after.retained_bytes,
            after.decode_bytes
        )
    );
}

/// (f) A NULL document or out-pointer answers false with an error and writes
/// nothing; a NULL `err_out` is tolerated.
#[test]
fn null_inputs_answer_false() {
    let _serial = serial();
    let doc = DrDocument::from_pixels(noise(8, 8, 9));
    let (mut retained, mut decode) = (7u64, 7u64);
    let mut error = ptr::null_mut();
    assert!(!unsafe {
        dr_doc_native_reopen_plan(ptr::null(), &mut retained, &mut decode, &mut error)
    });
    assert!(!unsafe { text(error) }.is_empty());
    for (r, d) in [
        (ptr::null_mut(), &mut decode as *mut u64),
        (&mut retained as *mut u64, ptr::null_mut()),
    ] {
        let mut error = ptr::null_mut();
        assert!(!unsafe { dr_doc_native_reopen_plan(&doc, r, d, &mut error) });
        assert!(!unsafe { text(error) }.is_empty());
    }
    assert_eq!((retained, decode), (7, 7));
    assert!(!unsafe {
        dr_doc_native_reopen_plan(
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    });
}
