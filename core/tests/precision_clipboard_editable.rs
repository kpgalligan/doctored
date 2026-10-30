//! Phase 12G-3: RZCB v3's EDITABLE SECTION — the codec's new header word,
//! flag bit2 and trailing section, `ClipboardPayload::with_editable` and its
//! admission, `info()["editable"]`, the section's passage through
//! `converted` (and its absence from `from_rgba8` and Copy), and the two new
//! FFI exports.
//!
//! Every reference is written here and never read off the implementation:
//! the test's own v1/v2/v3 RZCB writer (the boundaries file's `drcb()` shape
//! extended to v3), the section JSON built by hand, and the admission sums
//! spelled out from the contract (retained = payload + profile + 2·len,
//! scratch = bookkeeping + 2·len). The codec is EXACT (bit for bit).

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_clipboard::{
    dr_clipboard_payload_bytes, dr_clipboard_payload_decode, dr_clipboard_payload_editable,
    dr_clipboard_payload_free, dr_clipboard_payload_from_rgba8, dr_clipboard_payload_with_editable,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_clipboard::{ClipboardPayload, ClipboardRead};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const BOOKKEEPING: u64 = 1024 * 1024;
/// `MAX_DRDC_BLOB_LEN`, restated: the section's size cap.
const SECTION_CAP: u32 = 16 * 1024 * 1024;

// ------------------------------------------------------------- plumbing --

fn take_error(error: *mut c_char) -> Option<String> {
    if error.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(error) };
    Some(text)
}

enum Samples {
    U16(Vec<[u16; 4]>),
    F32(Vec<[f32; 4]>),
}

/// The test's own RZCB writer. `version` 1 writes the flag byte and no
/// tone-map words; 2 appends `operator` and `white`; 3 appends the
/// `section_len` word as well, and the `section` bytes after the coverage.
/// `section_len` is written as given, so a stream can lie about it.
#[allow(clippy::too_many_arguments)]
fn drcb(
    version: u32,
    w: u32,
    h: u32,
    profile: &[u8],
    samples: &Samples,
    flags: u8,
    tone: (u32, f32),
    coverage: Option<&[u16]>,
    section_len: u32,
    section: &[u8],
) -> Vec<u8> {
    let mut out = b"RZCB".to_vec();
    out.extend(version.to_le_bytes());
    out.extend(
        match samples {
            Samples::U16(_) => 1u32,
            Samples::F32(_) => 2u32,
        }
        .to_le_bytes(),
    );
    out.extend(w.to_le_bytes());
    out.extend(h.to_le_bytes());
    out.extend((-2i32).to_le_bytes());
    out.extend(7i32.to_le_bytes());
    out.extend(300.0f32.to_le_bytes());
    out.extend(150.0f32.to_le_bytes());
    out.extend((profile.len() as u32).to_le_bytes());
    out.extend(profile);
    out.push(flags);
    if version >= 2 {
        out.extend(tone.0.to_le_bytes());
        out.extend(tone.1.to_le_bytes());
    }
    if version == 3 {
        out.extend(section_len.to_le_bytes());
    }
    match samples {
        Samples::U16(p) => p.iter().flatten().for_each(|v| out.extend(v.to_le_bytes())),
        Samples::F32(p) => p.iter().flatten().for_each(|v| out.extend(v.to_le_bytes())),
    }
    if let Some(c) = coverage {
        c.iter().for_each(|v| out.extend(v.to_le_bytes()));
    }
    out.extend(section);
    out
}

/// A well-formed v3 u16 stream carrying `section` (empty = none).
fn u16_v3(profile: &[u8], section: &[u8]) -> Vec<u8> {
    let flags = if section.is_empty() { 0 } else { 4 };
    drcb(
        3,
        2,
        1,
        profile,
        &Samples::U16(u16_pattern(2)),
        flags,
        (0, 4.0),
        None,
        section.len() as u32,
        section,
    )
}

fn decode(bytes: &[u8]) -> Result<ClipboardPayload, String> {
    ClipboardPayload::decode(bytes, LIMIT, 0)
}

fn u16_pattern(n: u32) -> Vec<[u16; 4]> {
    let codes = [0u16, 1, 0x0101, 1000, 12345, 32768, 40000, 65534, 65535];
    (0..n as usize)
        .map(|k| {
            [
                codes[k % 9],
                codes[(k + 4) % 9],
                codes[(k + 7) % 9],
                codes[(k * 5 + 2) % 9],
            ]
        })
        .collect()
}

fn f32_pattern(n: u32) -> Vec<[f32; 4]> {
    let values = [-0.25f32, 0.0, 0.0031, 0.18, 0.5, 0.99, 1.0, 1.7, 4.0, 12.0];
    let alphas = [1.0f32, 0.25, 0.5, 0.0, 0.75, 1.0, 0.6];
    (0..n as usize)
        .map(|k| {
            [
                values[k % 10],
                values[(k + 3) % 10],
                values[(k + 7) % 10],
                alphas[k % 7],
            ]
        })
        .collect()
}

/// The section the host writes for a text layer, built by hand.
fn text_section() -> String {
    json!({
        "type": "text",
        "name": "Title",
        "record": "{\"kind\":\"text\",\"string\":\"Hello\"}",
        "anchor": [5, 6],
    })
    .to_string()
}

/// Every malformed section the decoder and `with_editable` must refuse, with
/// the word its sentence must contain.
fn bad_sections() -> Vec<(Vec<u8>, &'static str)> {
    vec![
        (vec![0xff, 0xfe, b'{', b'}'], "UTF-8"),
        (b"[1, 2]".to_vec(), "object"),
        (b"not json".to_vec(), "JSON"),
        (br#"{"record":"r"}"#.to_vec(), "`type`"),
        (br#"{"type":7,"record":"r"}"#.to_vec(), "`type`"),
        (br#"{"type":"text"}"#.to_vec(), "`record`"),
        (br#"{"type":"text","record":7}"#.to_vec(), "`record`"),
        (br#"{"type":"text","record":""}"#.to_vec(), "`record`"),
        (
            br#"{"type":"text","record":"r","name":3}"#.to_vec(),
            "`name`",
        ),
        (
            br#"{"type":"text","record":"r","anchor":[1,2,3]}"#.to_vec(),
            "`anchor`",
        ),
        (
            br#"{"type":"text","record":"r","anchor":[1,"2"]}"#.to_vec(),
            "`anchor`",
        ),
        (
            br#"{"type":"text","record":"r","anchor":[NaN,2]}"#.to_vec(),
            "JSON",
        ),
        (
            br#"{"type":"text","record":"r","extra":1}"#.to_vec(),
            "`extra`",
        ),
    ]
}

// ----------------------------------------------------------------- codec --

#[test]
fn v3_round_trips_with_and_without_a_section() {
    let profile = IccProfile::srgb().bytes().to_vec();
    let (w, h) = (3u32, 2u32);
    let coverage = [0u16, 1, 32768, 65534, 65535, 0x0101];
    let section = text_section();
    for floats in [false, true] {
        let samples = if floats {
            Samples::F32(f32_pattern(w * h))
        } else {
            Samples::U16(u16_pattern(w * h))
        };
        let bps = if floats { 16 } else { 8 };
        let tone = if floats { (1, 2.5) } else { (0, 4.0) };
        for cov in [None, Some(&coverage[..])] {
            for carried in [None, Some(section.as_str())] {
                let text = carried.unwrap_or("");
                let flags = u8::from(cov.is_some()) | if carried.is_some() { 4 } else { 0 };
                let bytes = drcb(
                    3,
                    w,
                    h,
                    &profile,
                    &samples,
                    flags,
                    tone,
                    cov,
                    text.len() as u32,
                    text.as_bytes(),
                );
                let payload = decode(&bytes).expect("v3 decodes");
                assert_eq!(payload.editable(), carried);
                assert_eq!(payload.coverage(), cov);
                assert_eq!(payload.encode().unwrap(), bytes, "bit-exact re-encode");
                assert_eq!(
                    payload.payload_bytes(),
                    6 * bps + if cov.is_some() { 12 } else { 0 } + text.len() as u64
                );
                assert_eq!(payload.info()["version"], 3);
            }
        }
    }
}

#[test]
fn v1_and_v2_streams_decode_without_a_section() {
    let profile = IccProfile::srgb().bytes().to_vec();
    let samples = Samples::U16(u16_pattern(2));
    for version in [1u32, 2] {
        let bytes = drcb(version, 2, 1, &profile, &samples, 0, (0, 4.0), None, 0, &[]);
        let payload = decode(&bytes).expect("an older stream decodes");
        assert_eq!(payload.editable(), None);
        assert_eq!(payload.info()["editable"], Value::Null);
        // The encoder writes v3: its bytes are the v3 twin with no section.
        let v3 = drcb(3, 2, 1, &profile, &samples, 0, (0, 4.0), None, 0, &[]);
        assert_eq!(payload.encode().unwrap(), v3);
    }
}

#[test]
fn decode_refuses_every_bad_flag_length_and_section_by_name() {
    let profile = IccProfile::srgb().bytes().to_vec();
    let samples = Samples::U16(u16_pattern(2));
    let section = text_section();
    let refused = |stream: Vec<u8>, needle: &str| {
        let err = decode(&stream).err().expect("refused");
        assert!(err.contains(needle), "{needle}: {err}");
    };
    let stream = |version: u32, flags: u8, len: u32, body: &[u8]| {
        drcb(
            version,
            2,
            1,
            &profile,
            &samples,
            flags,
            (0, 4.0),
            None,
            len,
            body,
        )
    };
    refused(stream(3, 8, 0, &[]), "flags 8");
    refused(stream(3, 4, 0, &[]), "editable flag");
    refused(
        stream(3, 0, section.len() as u32, section.as_bytes()),
        "editable flag",
    );
    // A v2 stream with bit2 set is refused by the v2 flag rule.
    refused(stream(2, 4, 0, &[]), "flags 4");
    // Over the cap: refused from the header word, before any length check.
    refused(stream(3, 4, SECTION_CAP + 1, &[]), "size cap");
    // A length the stream does not hold.
    refused(stream(3, 4, 100, br#"{"type":"t"}"#), "declares");
    for (body, needle) in bad_sections() {
        refused(stream(3, 4, body.len() as u32, &body), needle);
    }
    // The control: the same framing with a good section decodes.
    decode(&stream(3, 4, section.len() as u32, section.as_bytes())).expect("control");
}

// -------------------------------------------------------- with_editable --

#[test]
fn with_editable_validates_as_the_decoder_does_and_replaces() {
    let profile = IccProfile::srgb().bytes().to_vec();
    let payload = decode(&u16_v3(&profile, &[])).unwrap();
    for (body, needle) in bad_sections() {
        let Ok(text) = String::from_utf8(body) else {
            continue; // a &str cannot hold invalid UTF-8
        };
        let err = payload
            .with_editable(&text, LIMIT, 0)
            .err()
            .expect("refused");
        assert!(err.contains(needle), "{needle}: {err}");
    }
    let err = payload
        .with_editable("", LIMIT, 0)
        .err()
        .expect("an empty section is refused");
    assert!(err.contains("JSON"), "{err}");
    let over = format!(
        r#"{{"type":"t","record":"{}"}}"#,
        "x".repeat(SECTION_CAP as usize)
    );
    let err = payload.with_editable(&over, LIMIT, 0).err().unwrap();
    assert!(err.contains("size cap"), "{err}");

    let section = text_section();
    let carried = payload.with_editable(&section, LIMIT, 0).unwrap();
    assert_eq!(carried.editable(), Some(section.as_str()));
    assert_eq!(payload.editable(), None, "the source payload is untouched");
    assert_eq!(
        carried.payload_bytes(),
        payload.payload_bytes() + section.len() as u64
    );
    assert_eq!(
        carried.encode().unwrap(),
        u16_v3(&profile, section.as_bytes())
    );
    let other = json!({"type": "adjust", "record": "{}"}).to_string();
    let replaced = carried.with_editable(&other, LIMIT, 0).unwrap();
    assert_eq!(replaced.editable(), Some(other.as_str()));
}

#[test]
fn with_editable_admits_the_handle_it_will_become() {
    let srgb = IccProfile::srgb();
    let profile = srgb.bytes().len() as u64;
    // A payload whose samples dwarf the section: 100 × 100 u16.
    let image =
        PrecisionImage::from_rgba16(100, 100, u16_pattern(10_000), srgb.clone(), None).unwrap();
    let doc = PrecisionDocument::from_parts(
        100,
        100,
        srgb,
        Metadata::default(),
        Resolution { x: 72.0, y: 72.0 },
        vec![Layer::raster("L", image, (0, 0), 1.0, true).unwrap()],
    )
    .unwrap();
    let read: ClipboardRead =
        serde_json::from_value(json!({"op":"copy","layer":0,"x":0,"y":0,"width":100,"height":100}))
            .unwrap();
    let payload = doc.clipboard_copy(&read, None, LIMIT, 0, None).unwrap();
    let samples = payload.payload_bytes();
    assert_eq!(samples, 80_000);
    let section = text_section();
    let len = section.len() as u64;

    // Objection 1: a limit that admits the section's parts but not the
    // payload the new handle re-encodes is refused.
    let section_only = BOOKKEEPING + 4 * len + profile;
    let err = payload
        .with_editable(&section, LIMIT, LIMIT - section_only)
        .err()
        .expect("the re-encoded payload is not admitted");
    assert!(err.contains("working limit"), "{err}");

    // The straddle: retained payload + profile + 2·len, scratch
    // bookkeeping + 2·len.
    let need = samples + profile + 2 * len + BOOKKEEPING + 2 * len;
    payload
        .with_editable(&section, LIMIT, LIMIT - need)
        .expect("exactly the need is admitted");
    let err = payload
        .with_editable(&section, LIMIT, LIMIT - need + 1)
        .err()
        .expect("one byte short is refused");
    assert!(err.contains("working limit"), "{err}");
}

#[test]
fn info_reports_the_section_without_the_record() {
    let profile = IccProfile::srgb().bytes().to_vec();
    let bare = decode(&u16_v3(&profile, &[])).unwrap();
    assert_eq!(bare.info()["editable"], Value::Null);
    let record = "{\"kind\":\"text\",\"string\":\"Hello\"}";
    let payload = decode(&u16_v3(&profile, text_section().as_bytes())).unwrap();
    assert_eq!(
        payload.info()["editable"],
        json!({"type": "text", "name": "Title", "anchor": [5, 6], "record_bytes": record.len()})
    );
    let adjust = json!({"type": "adjust", "record": "é"}).to_string();
    let payload = bare.with_editable(&adjust, LIMIT, 0).unwrap();
    assert_eq!(
        payload.info()["editable"],
        json!({"type": "adjust", "record_bytes": 2}),
        "record_bytes counts UTF-8 bytes"
    );
}

#[test]
fn converted_carries_the_section_and_fresh_payloads_carry_none() {
    let (srgb, p3) = (IccProfile::srgb(), IccProfile::display_p3());
    let section = text_section();
    let payload = decode(&u16_v3(srgb.bytes(), section.as_bytes())).unwrap();
    let out = payload
        .converted(PrecisionStorage::Rgba16Unorm, &p3, LIMIT, 0, None)
        .unwrap()
        .expect("a real conversion");
    assert_eq!(&**out.image().profile().bytes(), &p3.bytes()[..]);
    assert_eq!(out.editable(), Some(section.as_str()));

    let bytes = [10u8, 20, 30, 255, 40, 50, 60, 128];
    let fresh = ClipboardPayload::from_rgba8(
        &bytes,
        2,
        1,
        srgb.bytes(),
        (0, 0),
        Resolution { x: 72.0, y: 72.0 },
        LIMIT,
        0,
    )
    .unwrap();
    assert_eq!(fresh.editable(), None);

    let image = PrecisionImage::from_rgba16(2, 1, u16_pattern(2), srgb.clone(), None).unwrap();
    let doc = PrecisionDocument::from_parts(
        2,
        1,
        srgb,
        Metadata::default(),
        Resolution { x: 72.0, y: 72.0 },
        vec![Layer::raster("L", image, (0, 0), 1.0, true).unwrap()],
    )
    .unwrap();
    let read: ClipboardRead =
        serde_json::from_value(json!({"op":"copy","layer":0,"x":0,"y":0,"width":2,"height":1}))
            .unwrap();
    let copied = doc.clipboard_copy(&read, None, LIMIT, 0, None).unwrap();
    assert_eq!(copied.editable(), None);
}

#[test]
fn from_clipboard_ignores_the_section() {
    // The core never reads the section: New from Clipboard's document is one
    // "Background" raster holding exactly the payload, and the host restores
    // the record into it.
    let profile = IccProfile::srgb().bytes().to_vec();
    let payload = decode(&u16_v3(&profile, text_section().as_bytes())).unwrap();
    let doc = PrecisionDocument::from_clipboard(&payload, LIMIT, 0, None).unwrap();
    assert_eq!(doc.layer_count(), 1);
    let record = doc.layer(0).unwrap();
    assert_eq!(record.name(), "Background");
    assert_eq!(record.offset(), (0, 0));
    assert_eq!(
        record
            .source()
            .expect("a raster record")
            .to_rgba16()
            .unwrap(),
        u16_pattern(2)
    );
}

// ------------------------------------------------------------------- FFI --

#[test]
fn ffi_with_editable_round_trips_and_tolerates_null() {
    let srgb = IccProfile::srgb();
    let flat = [10u8, 20, 30, 255, 40, 50, 60, 128];
    let section = text_section();
    let c_section = CString::new(section.clone()).unwrap();
    unsafe {
        let mut err = ptr::null_mut();
        let handle = dr_clipboard_payload_from_rgba8(
            flat.as_ptr(),
            flat.len(),
            2,
            1,
            srgb.bytes().as_ptr(),
            srgb.bytes().len(),
            0,
            0,
            72.0,
            72.0,
            LIMIT,
            0,
            &mut err,
        );
        assert_eq!(take_error(err), None);
        // No section: NULL WITHOUT an error.
        let mut err = ptr::null_mut();
        assert!(dr_clipboard_payload_editable(handle, &mut err).is_null());
        assert_eq!(take_error(err), None);

        let mut err = ptr::null_mut();
        let carried =
            dr_clipboard_payload_with_editable(handle, c_section.as_ptr(), LIMIT, 0, &mut err);
        assert_eq!(take_error(err), None);
        assert!(!carried.is_null());
        let mut length = 0usize;
        let bytes = dr_clipboard_payload_bytes(carried, &mut length);
        let stream = std::slice::from_raw_parts(bytes, length).to_vec();
        assert_eq!(stream[4], 3, "version 3");
        assert!(stream.ends_with(section.as_bytes()), "the section is last");
        let mut err = ptr::null_mut();
        let decoded =
            dr_clipboard_payload_decode(stream.as_ptr(), stream.len(), LIMIT, 0, &mut err);
        assert_eq!(take_error(err), None);
        let mut err = ptr::null_mut();
        let text = dr_clipboard_payload_editable(decoded, &mut err);
        assert_eq!(take_error(err), None);
        assert_eq!(CStr::from_ptr(text).to_str().unwrap(), section);
        dr_string_free(text);

        // A malformed section is refused through the shim too.
        let bad = CString::new(r#"{"type":"t"}"#).unwrap();
        let mut err = ptr::null_mut();
        assert!(
            dr_clipboard_payload_with_editable(handle, bad.as_ptr(), LIMIT, 0, &mut err).is_null()
        );
        assert!(take_error(err).unwrap().contains("`record`"));

        // The NULL sweep: each answers NULL with a message, and tolerates a
        // NULL err_out.
        let mut err = ptr::null_mut();
        assert!(dr_clipboard_payload_with_editable(
            ptr::null(),
            c_section.as_ptr(),
            LIMIT,
            0,
            &mut err
        )
        .is_null());
        assert!(take_error(err).is_some());
        let mut err = ptr::null_mut();
        assert!(
            dr_clipboard_payload_with_editable(handle, ptr::null(), LIMIT, 0, &mut err).is_null()
        );
        assert!(take_error(err).is_some());
        assert!(dr_clipboard_payload_with_editable(
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null_mut()
        )
        .is_null());
        let mut err = ptr::null_mut();
        assert!(dr_clipboard_payload_editable(ptr::null(), &mut err).is_null());
        assert!(take_error(err).is_some());
        assert!(dr_clipboard_payload_editable(ptr::null(), ptr::null_mut()).is_null());

        dr_clipboard_payload_free(decoded);
        dr_clipboard_payload_free(carried);
        dr_clipboard_payload_free(handle);
    }
}
