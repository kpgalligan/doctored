//! `ClipboardPayload::trim` — New from Clipboard's cut down to the
//! payload's content box (every pixel with alpha > 0) — and its FFI export
//! `dr_clipboard_payload_trim`.
//!
//! Every reference is written here and never read off the implementation:
//! the test's own RZCB v3 writer (the editable file's `drcb()` shape with the
//! offset and PPI as arguments), the box read off the test's own sample
//! vectors, the expected window cut out of those vectors by hand, and the
//! admission sum spelled out from the contract (retained = 2 · (samples +
//! coverage + section) + profile, scratch = bookkeeping). The trim is EXACT:
//! the trimmed payload's encoding is compared byte for byte with the
//! writer's stream for the expected window.

use std::ffi::{c_char, CStr};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_clipboard::{
    dr_clipboard_payload_decode, dr_clipboard_payload_free, dr_clipboard_payload_info,
    dr_clipboard_payload_trim,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::Resolution;
use doctored_core::precision_clipboard::ClipboardPayload;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const BOOKKEEPING: u64 = 1024 * 1024;

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

#[derive(Clone)]
enum Samples {
    U16(Vec<[u16; 4]>),
    F32(Vec<[f32; 4]>),
}

impl Samples {
    /// The `(x, y, w, h)` window of a `width`-wide plane, row-major.
    fn window(&self, width: u32, rect: (u32, u32, u32, u32)) -> Samples {
        match self {
            Samples::U16(p) => Samples::U16(window(p, width, rect)),
            Samples::F32(p) => Samples::F32(window(p, width, rect)),
        }
    }
}

fn window<T: Copy>(plane: &[T], width: u32, rect: (u32, u32, u32, u32)) -> Vec<T> {
    let (x, y, w, h) = rect;
    let mut out = Vec::new();
    for row in y..y + h {
        for col in x..x + w {
            out.push(plane[(row * width + col) as usize]);
        }
    }
    out
}

/// One RZCB stream's header fields besides the samples.
struct Header<'a> {
    w: u32,
    h: u32,
    offset: (i32, i32),
    ppi: (f32, f32),
    profile: &'a [u8],
    byte_origin: bool,
    tone: (u32, f32),
    coverage: Option<&'a [u16]>,
    section: &'a [u8],
}

/// The test's own RZCB v3 writer.
fn drcb(head: &Header<'_>, samples: &Samples) -> Vec<u8> {
    let mut out = b"RZCB".to_vec();
    out.extend(3u32.to_le_bytes());
    out.extend(
        match samples {
            Samples::U16(_) => 1u32,
            Samples::F32(_) => 2u32,
        }
        .to_le_bytes(),
    );
    out.extend(head.w.to_le_bytes());
    out.extend(head.h.to_le_bytes());
    out.extend(head.offset.0.to_le_bytes());
    out.extend(head.offset.1.to_le_bytes());
    out.extend(head.ppi.0.to_le_bytes());
    out.extend(head.ppi.1.to_le_bytes());
    out.extend((head.profile.len() as u32).to_le_bytes());
    out.extend(head.profile);
    let flags = u8::from(head.coverage.is_some())
        | if head.byte_origin { 2 } else { 0 }
        | if head.section.is_empty() { 0 } else { 4 };
    out.push(flags);
    out.extend(head.tone.0.to_le_bytes());
    out.extend(head.tone.1.to_le_bytes());
    out.extend((head.section.len() as u32).to_le_bytes());
    match samples {
        Samples::U16(p) => p.iter().flatten().for_each(|v| out.extend(v.to_le_bytes())),
        Samples::F32(p) => p.iter().flatten().for_each(|v| out.extend(v.to_le_bytes())),
    }
    if let Some(c) = head.coverage {
        c.iter().for_each(|v| out.extend(v.to_le_bytes()));
    }
    out.extend(head.section);
    out
}

fn decode(bytes: &[u8]) -> ClipboardPayload {
    ClipboardPayload::decode(bytes, LIMIT, 0).expect("the test stream decodes")
}

fn section() -> String {
    json!({
        "type": "text",
        "name": "Title",
        "record": "{\"kind\":\"text\",\"string\":\"Hello\"}",
        "anchor": [5, 6],
    })
    .to_string()
}

/// True inside `rect`.
fn inside(x: u32, y: u32, rect: (u32, u32, u32, u32)) -> bool {
    x >= rect.0 && x < rect.0 + rect.2 && y >= rect.1 && y < rect.1 + rect.3
}

/// A `w × h` u16 plane whose RGB is nonzero EVERYWHERE (latent RGB under
/// zero alpha) and whose alpha is nonzero only on `rect`'s four edges (so
/// the box is exactly `rect`) and zero in its interior's odd pixels.
fn u16_plane(w: u32, h: u32, rect: (u32, u32, u32, u32)) -> Vec<[u16; 4]> {
    let mut out = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let k = (y * w + x) as u16;
            let edge =
                x == rect.0 || x == rect.0 + rect.2 - 1 || y == rect.1 || y == rect.1 + rect.3 - 1;
            let alpha = if !inside(x, y, rect) {
                0
            } else if edge {
                1 + k * 997
            } else if (x + y) % 2 == 1 {
                0
            } else {
                65535
            };
            out.push([1 + k * 311, 65535 - k * 13, 40000 + k, alpha]);
        }
    }
    out
}

/// The f32 twin: extended-range RGB everywhere, alpha > 0 only on `rect`'s
/// corners' rows and columns (the smallest positive alpha included).
fn f32_plane(w: u32, h: u32, rect: (u32, u32, u32, u32)) -> Vec<[f32; 4]> {
    let mut out = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let k = (y * w + x) as f32;
            let alpha = if !inside(x, y, rect) {
                0.0
            } else if (x, y) == (rect.0, rect.1) {
                f32::MIN_POSITIVE
            } else if (x, y) == (rect.0 + rect.2 - 1, rect.1 + rect.3 - 1) {
                1.0
            } else if (x + y) % 3 == 0 {
                0.0
            } else {
                0.5
            };
            out.push([-0.25 + k * 0.125, 4.0 - k, 12.0, alpha]);
        }
    }
    out
}

// ------------------------------------------------------------------ trim --

#[test]
fn u16_trim_is_the_exact_box_window() {
    let p3 = IccProfile::display_p3();
    let (w, h) = (7u32, 6u32);
    let rect = (2u32, 1u32, 4u32, 3u32);
    let samples = Samples::U16(u16_plane(w, h, rect));
    let coverage: Vec<u16> = (0..w * h).map(|k| (k * 1543) as u16).collect();
    let text = section();
    let source = drcb(
        &Header {
            w,
            h,
            offset: (-2, 7),
            ppi: (300.0, 150.0),
            profile: p3.bytes(),
            byte_origin: false,
            tone: (0, 4.0),
            coverage: Some(&coverage),
            section: text.as_bytes(),
        },
        &samples,
    );
    let payload = decode(&source);
    let trimmed = payload.trim(LIMIT, 0).unwrap().expect("a real trim");
    let expected_coverage = window(&coverage, w, rect);
    let expected = drcb(
        &Header {
            w: rect.2,
            h: rect.3,
            offset: (-2 + rect.0 as i32, 7 + rect.1 as i32),
            ppi: (300.0, 150.0),
            profile: p3.bytes(),
            byte_origin: false,
            tone: (0, 4.0),
            coverage: Some(&expected_coverage),
            section: text.as_bytes(),
        },
        &samples.window(w, rect),
    );
    assert_eq!(trimmed.dimensions(), (4, 3));
    assert_eq!(trimmed.offset(), (0, 8));
    assert_eq!(trimmed.coverage(), Some(&expected_coverage[..]));
    assert_eq!(trimmed.editable(), Some(text.as_str()));
    assert!(!trimmed.byte_origin());
    assert_eq!(trimmed.resolution().x, 300.0);
    assert_eq!(trimmed.resolution().y, 150.0);
    let Samples::U16(plane) = samples.window(w, rect) else {
        unreachable!()
    };
    assert_eq!(trimmed.image().to_rgba16().unwrap(), plane);
    let encoded = trimmed.encode().unwrap();
    assert_eq!(encoded, expected, "bit-exact against the writer's window");
    assert_eq!(decode(&encoded).encode().unwrap(), expected, "round trip");
    assert_eq!(
        payload.encode().unwrap(),
        source,
        "the source payload is untouched"
    );
}

#[test]
fn f32_trim_keeps_the_tone_map_and_every_bit() {
    let srgb = IccProfile::srgb();
    let (w, h) = (6u32, 5u32);
    let rect = (1u32, 2u32, 3u32, 2u32);
    let samples = Samples::F32(f32_plane(w, h, rect));
    let head = |w, h, offset| Header {
        w,
        h,
        offset,
        ppi: (240.0, 120.0),
        profile: srgb.bytes(),
        byte_origin: false,
        tone: (1, 2.5),
        coverage: None,
        section: &[],
    };
    let payload = decode(&drcb(&head(w, h, (10, -3)), &samples));
    let trimmed = payload.trim(LIMIT, 0).unwrap().expect("a real trim");
    let expected = drcb(&head(rect.2, rect.3, (11, -1)), &samples.window(w, rect));
    assert_eq!(trimmed.offset(), (11, -1));
    assert_eq!(trimmed.tone_map(), payload.tone_map());
    assert_eq!(trimmed.coverage(), None);
    assert_eq!(trimmed.editable(), None);
    let encoded = trimmed.encode().unwrap();
    assert_eq!(encoded, expected, "bit-exact against the writer's window");
    assert_eq!(decode(&encoded).encode().unwrap(), expected, "round trip");
}

#[test]
fn a_byte_origin_payload_stays_byte_origin() {
    let srgb = IccProfile::srgb();
    let (w, h) = (4u32, 3u32);
    // Alpha only at (1, 1) and (2, 1): the box is (1, 1, 2, 1).
    let mut bytes = Vec::new();
    for k in 0..w * h {
        let alpha = if k == 5 || k == 6 { 200 } else { 0 };
        bytes.extend([k as u8 * 20, 255 - k as u8, 7, alpha]);
    }
    let payload = ClipboardPayload::from_rgba8(
        &bytes,
        w,
        h,
        srgb.bytes(),
        (3, 4),
        Resolution { x: 72.0, y: 72.0 },
        LIMIT,
        0,
    )
    .unwrap();
    let trimmed = payload.trim(LIMIT, 0).unwrap().expect("a real trim");
    assert_eq!(trimmed.dimensions(), (2, 1));
    assert_eq!(trimmed.offset(), (4, 5));
    assert!(trimmed.byte_origin());
    let expected: Vec<[u16; 4]> = [5usize, 6]
        .iter()
        .map(|&k| {
            let p = &bytes[k * 4..k * 4 + 4];
            [0, 1, 2, 3].map(|c| u16::from(p[c]) * 257)
        })
        .collect();
    assert_eq!(trimmed.image().to_rgba16().unwrap(), expected);
}

#[test]
fn nothing_to_trim_answers_none() {
    let srgb = IccProfile::srgb();
    let (w, h) = (5u32, 4u32);
    let head = Header {
        w,
        h,
        offset: (0, 0),
        ppi: (72.0, 72.0),
        profile: srgb.bytes(),
        byte_origin: false,
        tone: (0, 4.0),
        coverage: None,
        section: &[],
    };
    // Fully transparent, latent RGB and all.
    let clear = Samples::U16((0..w * h).map(|k| [k as u16, 9, 9, 0]).collect());
    assert!(decode(&drcb(&head, &clear))
        .trim(LIMIT, 0)
        .unwrap()
        .is_none());
    // Already tight: only two opposite corners are visible.
    let tight = Samples::U16(
        (0..w * h)
            .map(|k| [1, 2, 3, if k == 0 || k == w * h - 1 { 1 } else { 0 }])
            .collect(),
    );
    assert!(decode(&drcb(&head, &tight))
        .trim(LIMIT, 0)
        .unwrap()
        .is_none());
    let tight = Samples::F32(
        (0..w * h)
            .map(|k| {
                [
                    1.0,
                    2.0,
                    3.0,
                    if k == w - 1 || k == w * (h - 1) {
                        0.25
                    } else {
                        0.0
                    },
                ]
            })
            .collect(),
    );
    let head = Header {
        tone: (1, 2.5),
        ..head
    };
    assert!(decode(&drcb(&head, &tight))
        .trim(LIMIT, 0)
        .unwrap()
        .is_none());
}

#[test]
fn trim_admits_the_handle_it_will_become() {
    let p3 = IccProfile::display_p3();
    let (w, h) = (7u32, 6u32);
    let rect = (2u32, 1u32, 4u32, 3u32);
    let coverage: Vec<u16> = vec![65535; (w * h) as usize];
    let text = section();
    let payload = decode(&drcb(
        &Header {
            w,
            h,
            offset: (0, 0),
            ppi: (72.0, 72.0),
            profile: p3.bytes(),
            byte_origin: false,
            tone: (0, 4.0),
            coverage: Some(&coverage),
            section: text.as_bytes(),
        },
        &Samples::U16(u16_plane(w, h, rect)),
    ));
    let n = u64::from(rect.2 * rect.3);
    let retained = 2 * (n * 8 + n * 2 + text.len() as u64) + p3.bytes().len() as u64;
    let need = retained + BOOKKEEPING;
    payload
        .trim(LIMIT, LIMIT - need)
        .expect("exactly the need is admitted")
        .expect("a real trim");
    let err = payload
        .trim(LIMIT, LIMIT - need + 1)
        .err()
        .expect("one byte short is refused");
    assert!(err.contains("working limit"), "{err}");
}

#[test]
fn an_offset_leaving_i32_is_refused() {
    let srgb = IccProfile::srgb();
    let samples = Samples::U16(vec![[1, 2, 3, 0], [4, 5, 6, 65535]]);
    let payload = decode(&drcb(
        &Header {
            w: 2,
            h: 1,
            offset: (i32::MAX, 0),
            ppi: (72.0, 72.0),
            profile: srgb.bytes(),
            byte_origin: false,
            tone: (0, 4.0),
            coverage: None,
            section: &[],
        },
        &samples,
    ));
    let err = payload.trim(LIMIT, 0).err().expect("refused");
    assert!(err.contains("overflow"), "{err}");
}

// ------------------------------------------------------------------- FFI --

#[test]
fn ffi_trim_reports_the_window_and_tolerates_null() {
    let srgb = IccProfile::srgb();
    let (w, h) = (7u32, 6u32);
    let rect = (2u32, 1u32, 4u32, 3u32);
    let head = Header {
        w,
        h,
        offset: (-2, 7),
        ppi: (72.0, 72.0),
        profile: srgb.bytes(),
        byte_origin: false,
        tone: (0, 4.0),
        coverage: None,
        section: &[],
    };
    let loose = drcb(&head, &Samples::U16(u16_plane(w, h, rect)));
    let tight = drcb(&head, &Samples::U16(u16_plane(w, h, (0, 0, w, h))));
    unsafe {
        let mut err = ptr::null_mut();
        let handle = dr_clipboard_payload_decode(loose.as_ptr(), loose.len(), LIMIT, 0, &mut err);
        assert_eq!(take_error(err), None);
        let mut err = ptr::null_mut();
        let trimmed = dr_clipboard_payload_trim(handle, LIMIT, 0, &mut err);
        assert_eq!(take_error(err), None);
        assert!(!trimmed.is_null());
        let mut err = ptr::null_mut();
        let info = dr_clipboard_payload_info(trimmed, &mut err);
        assert_eq!(take_error(err), None);
        let text = CStr::from_ptr(info).to_str().unwrap().to_owned();
        dr_string_free(info);
        let info: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(info["width"], 4);
        assert_eq!(info["height"], 3);
        assert_eq!(info["offset"], json!([0, 8]));

        // Nothing to trim: NULL WITHOUT an error.
        let mut err = ptr::null_mut();
        let whole = dr_clipboard_payload_decode(tight.as_ptr(), tight.len(), LIMIT, 0, &mut err);
        assert_eq!(take_error(err), None);
        let mut err = ptr::null_mut();
        assert!(dr_clipboard_payload_trim(whole, LIMIT, 0, &mut err).is_null());
        assert_eq!(take_error(err), None);

        // A refusal: NULL + message.
        let mut err = ptr::null_mut();
        assert!(dr_clipboard_payload_trim(handle, 0, 0, &mut err).is_null());
        assert!(take_error(err).is_some());

        // The NULL sweep.
        let mut err = ptr::null_mut();
        assert!(dr_clipboard_payload_trim(ptr::null(), LIMIT, 0, &mut err).is_null());
        assert!(take_error(err).is_some());
        assert!(dr_clipboard_payload_trim(ptr::null(), LIMIT, 0, ptr::null_mut()).is_null());
        assert!(dr_clipboard_payload_trim(handle, 0, 0, ptr::null_mut()).is_null());

        dr_clipboard_payload_free(whole);
        dr_clipboard_payload_free(trimmed);
        dr_clipboard_payload_free(handle);
    }
}
