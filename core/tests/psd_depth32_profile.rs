//! A reopened 32-bit PSD takes its gamma source: a file whose embedded
//! profile is the linear (gamma 1.0) twin of one of the four built-ins opens
//! under that built-in, so a later 8-bit export is tagged and encoded as the
//! document was before its save. Any other linear profile stays the
//! document's own. Through the C API; the float documents hold exact samples
//! (`float_fixture`, which carries the profile bytes unchanged), and the 8-bit
//! oracle is the published transfer curves written out here.
use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::ptr;
use std::sync::Arc;

use doctored_core::doc_export::ExportResult;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_export::{
    dr_export_result_bytes, dr_export_result_free, dr_export_result_info,
    dr_precision_doc_export_encode,
};
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_psd::{
    dr_precision_doc_open_psd, dr_precision_doc_save_psd, dr_psd_import_report,
};
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use image::ImageDecoder;
use serde_json::Value;
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::float_fixture::float_doc;

const LAYERS: i32 = 0;
const FLATTENED: i32 = 1;
const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const PNG8: &str = r#"{"format":"png","bit_depth":8}"#;

/// The opaque values of the picture, each in every channel somewhere.
const VALUES: [f32; 6] = [0.0, 1e-3, 0.18, 0.5, 1.0, 2.5];
/// The one translucent pixel, after the six opaque ones.
const TRANSLUCENT: [f32; 4] = [0.18, 0.5, 2.5, 0.5];
const WIDTH: u32 = VALUES.len() as u32 + 1;

// ------------------------------------------------------------ C plumbing --

/// A typed document handle freed through the C API on drop.
struct Owned(*mut PrecisionDocument);
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { dr_precision_doc_free(self.0) };
    }
}
impl std::ops::Deref for Owned {
    type Target = PrecisionDocument;
    fn deref(&self) -> &PrecisionDocument {
        unsafe { &*self.0 }
    }
}

fn import_report(path: &Path) -> Value {
    let mut error = ptr::null_mut();
    let json = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    assert!(!json.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(json)).expect("report JSON")
}

fn open(path: &Path, mode: i32) -> Owned {
    let report = import_report(path);
    let expected = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let document = unsafe {
        dr_precision_doc_open_psd(
            cpath(path).as_ptr(),
            mode,
            expected.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!document.is_null(), "{}", take_err_string(error));
    Owned(document)
}

fn save(document: &PrecisionDocument, path: &Path) {
    let projection = document.render(LIMIT, 0, None).expect("projection");
    let mut error = ptr::null_mut();
    let saved = unsafe {
        dr_precision_doc_save_psd(
            document,
            &projection,
            cpath(path).as_ptr(),
            0,
            LAYERS,
            LIMIT,
            0,
            &mut error,
        )
    };
    assert!(saved, "{}", take_err_string(error));
}

/// One 8-bit PNG export: the bytes and the info JSON.
fn export_png8(document: &PrecisionDocument) -> (Vec<u8>, Value) {
    let request = CString::new(PNG8).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let result: *mut ExportResult = unsafe {
        dr_precision_doc_export_encode(
            document,
            ptr::null(),
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!result.is_null(), "{}", take_err_string(err));
    let mut length = 0usize;
    let pointer = unsafe { dr_export_result_bytes(result, &mut length) };
    let bytes = unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec();
    let mut err: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_export_result_info(result, &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    let text = unsafe { CStr::from_ptr(json) }.to_str().unwrap().to_owned();
    unsafe { dr_string_free(json) };
    unsafe { dr_export_result_free(result) };
    (bytes, serde_json::from_str(&text).unwrap())
}

/// A PNG's `iCCP` profile and its RGBA8 samples, through the `image` crate.
fn decode_png(bytes: &[u8]) -> (Option<Vec<u8>>, Vec<u8>) {
    let mut decoder = image::codecs::png::PngDecoder::new(std::io::Cursor::new(bytes)).unwrap();
    let icc = decoder.icc_profile().unwrap();
    let rgba = image::DynamicImage::from_decoder(decoder)
        .unwrap()
        .to_rgba8()
        .into_raw();
    (icc, rgba)
}

// --------------------------------------------------------------- fixtures --

/// A float document holding one canvas-sized raster of `picture()` under
/// `profile`, its samples exact (`float_fixture`).
fn float_document(profile: &Arc<IccProfile>) -> PrecisionDocument {
    let pixels: Vec<[f32; 4]> = picture()
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect();
    float_doc(WIDTH, 1, profile, &[pixels])
}

/// Six opaque pixels, pixel `i` carrying `VALUES[i]`, `VALUES[i + 2]` and
/// `VALUES[i + 4]` (mod 6), then [`TRANSLUCENT`].
fn picture() -> Vec<f32> {
    let n = VALUES.len();
    let mut samples: Vec<f32> = (0..n)
        .flat_map(|i| [VALUES[i], VALUES[(i + 2) % n], VALUES[(i + 4) % n], 1.0])
        .collect();
    samples.extend_from_slice(&TRANSLUCENT);
    samples
}

/// The IEC 61966-2.1 encoding (linear → encoded), which Display P3 shares.
fn iec_encode(v: f64) -> f64 {
    if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// A built-in, its name and its encoding curve (linear → encoded).
type Builtin = (Arc<IccProfile>, &'static str, fn(f64) -> f64);

/// The four built-ins, their names and their encoding curves: the IEC
/// curve, then the inverse of the declared u8Fixed8 gammas, 563/256 and
/// 461/256.
fn builtins() -> Vec<Builtin> {
    vec![
        (IccProfile::srgb(), "sRGB IEC61966-2.1", iec_encode),
        (IccProfile::display_p3(), "Display P3", iec_encode),
        (IccProfile::adobe_rgb(), "Adobe RGB (1998)", |v| {
            v.powf(256.0 / 563.0)
        }),
        (IccProfile::prophoto_rgb(), "ProPhoto RGB", |v| {
            v.powf(256.0 / 461.0)
        }),
    ]
}

/// `bytes` are exactly `profile`'s, named in the failure rather than dumped.
fn same_profile(bytes: &[u8], profile: &IccProfile, label: &str) {
    let got = IccProfile::parse(bytes).map(|p| p.name().to_owned());
    assert!(
        bytes == &profile.bytes()[..],
        "{label}: {got:?} ({} bytes) is not {} ({} bytes)",
        bytes.len(),
        profile.name(),
        profile.bytes().len()
    );
}

fn observation(file: &str, builtin: &str) -> String {
    format!(
        "The embedded profile “{file}” is the linear (gamma 1.0) twin of {builtin} that a 32-bit PSD carries; the document takes {builtin}, and its float samples are unchanged."
    )
}

fn observations(report: &Value) -> Vec<String> {
    report["observations"]
        .as_array()
        .expect("observations")
        .iter()
        .map(|o| o.as_str().unwrap().to_owned())
        .collect()
}

// ------------------------------------------------------------------ tests --

#[test]
fn a_reopened_32_bit_psd_exports_8_bits_under_its_gamma_source() {
    for (profile, name, encode) in builtins() {
        let directory = TempDir::new().unwrap();
        let original = float_document(&profile);
        same_profile(&original.profile().bytes()[..], &profile, name);
        let path = directory.path().join("picture32.psd");
        save(&original, &path);

        let report = import_report(&path);
        let twin = format!("{name} (Linear RGB Profile)");
        assert!(
            observations(&report).contains(&observation(&twin, name)),
            "{name}: {report}"
        );

        let (want_png, _) = export_png8(&original);
        let (want_icc, want_rgba) = decode_png(&want_png);
        same_profile(&want_icc.expect("an iCCP chunk"), &profile, name);

        // The oracle: each opaque code is within 1 of the gamma source's own
        // encoding of the clipped linear value, so 0.18 is about 118 at sRGB,
        // never the linear 46.
        for (i, pixel) in picture().chunks_exact(4).take(VALUES.len()).enumerate() {
            for c in 0..3 {
                let v = f64::from(pixel[c]).clamp(0.0, 1.0);
                let oracle = (255.0 * encode(v)).round();
                let got = f64::from(want_rgba[4 * i + c]);
                assert!(
                    (got - oracle).abs() <= 1.0,
                    "{name}: pixel {i} channel {c}: {got} against {oracle}"
                );
            }
            assert_eq!(want_rgba[4 * i + 3], 255, "{name}: pixel {i} alpha");
        }

        for mode in [LAYERS, FLATTENED] {
            let back = open(&path, mode);
            let label = format!("{name} mode {mode}");
            assert!(back.is_float(), "{label}");
            same_profile(&back.profile().bytes()[..], &profile, &label);
            assert_eq!(back.profile().name(), name, "{label}");
            let (png, _) = export_png8(&back);
            let (icc, rgba) = decode_png(&png);
            same_profile(&icc.expect("an iCCP chunk"), &profile, &label);
            assert!(rgba == want_rgba, "{label}: {rgba:?} against {want_rgba:?}");
        }
    }
}

/// Resource 1039 of a PSD. Mirrors `psd_depth32.rs`'s `read_header`, the
/// master copy of this walk; repeated here because a test file shares no
/// private helper with another.
fn embedded_profile(bytes: &[u8]) -> Vec<u8> {
    let be_u32 = |b: &[u8], at: usize| u32::from_be_bytes(b[at..at + 4].try_into().unwrap());
    let color_mode = be_u32(bytes, 26) as usize;
    let at = 30 + color_mode;
    let resources = &bytes[at + 4..at + 4 + be_u32(bytes, at) as usize];
    let mut r = 0usize;
    while r < resources.len() {
        let id = u16::from_be_bytes([resources[r + 4], resources[r + 5]]);
        r += 6;
        r += (resources[r] as usize + 2) & !1;
        let len = be_u32(resources, r) as usize;
        if id == 1039 {
            return resources[r + 4..r + 4 + len].to_vec();
        }
        r += 4 + len + len % 2;
    }
    panic!("no resource 1039");
}

/// The offset and size of tag `sig` in `icc`'s tag table.
fn tag_span(icc: &[u8], sig: &[u8; 4]) -> (usize, usize) {
    let be_u32 = |at: usize| u32::from_be_bytes(icc[at..at + 4].try_into().unwrap()) as usize;
    (0..be_u32(128))
        .map(|k| 132 + 12 * k)
        .find(|&entry| &icc[entry..entry + 4] == sig)
        .map(|entry| (be_u32(entry + 4), be_u32(entry + 8)))
        .expect("the tag")
}

#[test]
fn an_unknown_linear_profile_stays_the_document_profile() {
    let directory = TempDir::new().unwrap();
    let p3 = float_document(&IccProfile::display_p3());
    let first = directory.path().join("p3.psd");
    save(&p3, &first);
    let twin = embedded_profile(&std::fs::read(&first).unwrap());
    assert_eq!(
        IccProfile::parse(&twin).unwrap().name(),
        "Display P3 (Linear RGB Profile)"
    );

    // (a) another name, the same space: "Display P3" → "Unknown P3" in place.
    let mut renamed = twin.clone();
    let at = renamed
        .windows(10)
        .position(|w| w == b"Display P3")
        .expect("the desc text");
    renamed[at..at + 10].copy_from_slice(b"Unknown P3");
    // (b) the twin's name, another space: rXYZ and gXYZ swapped.
    let mut swapped = twin.clone();
    let (red, red_len) = tag_span(&twin, b"rXYZ");
    let (green, green_len) = tag_span(&twin, b"gXYZ");
    assert_eq!(red_len, green_len);
    swapped[red..red + red_len].copy_from_slice(&twin[green..green + green_len]);
    swapped[green..green + green_len].copy_from_slice(&twin[red..red + red_len]);

    for (label, variant) in [("renamed", renamed), ("swapped", swapped)] {
        let document = float_document(&Arc::new(IccProfile::parse(&variant).expect("parses")));
        assert!(document.profile().bytes()[..] == variant[..], "{label}");
        let path = directory.path().join(format!("{label}.psd"));
        save(&document, &path);
        let report = import_report(&path);
        assert!(
            !observations(&report)
                .iter()
                .any(|o| o.contains("linear (gamma 1.0) twin")),
            "{label}: {report}"
        );
        for mode in [LAYERS, FLATTENED] {
            let back = open(&path, mode);
            assert!(
                back.profile().bytes()[..] == variant[..],
                "{label} mode {mode}: {}",
                back.profile().name()
            );
        }
    }
}
