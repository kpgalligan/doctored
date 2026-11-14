//! HDR Toning PRESET files (`.hdt`) through `dr_hdr_toning_preset_file_decode`:
//! the values a file holds as `hdr_toning`'s Local Adaptation arguments, the
//! curve's pair order, the gamma clamp and the corner report, a refusal per
//! field, truncation at every length, the 4 KiB cap and the NULL sweep.
//!
//! Every file here is written field by field by `Hdt` below — no Adobe file is
//! committed. The one `#[ignore]` probe reads the presets an installed
//! Photoshop ships, and only those.

use std::ffi::c_char;
use std::path::Path;
use std::ptr;

use doctored_core::ffi_hdr_toning_preset::dr_hdr_toning_preset_file_decode;
use serde_json::{json, Value};

use crate::common::take_err_string;

// ------------------------------------------------------------- C plumbing --

/// The decoded `{hdr_toning, report}`, or the core's message.
fn decode(bytes: &[u8]) -> Result<(Value, Vec<String>), String> {
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe { dr_hdr_toning_preset_file_decode(bytes.as_ptr(), bytes.len(), &mut err) };
    if raw.is_null() {
        return Err(take_err_string(err));
    }
    assert!(err.is_null(), "success must not set err_out");
    let value: Value = serde_json::from_str(&take_err_string(raw)).expect("decode JSON");
    let report = value["report"]
        .as_array()
        .expect("report")
        .iter()
        .map(|line| line.as_str().expect("a report line").to_string())
        .collect();
    Ok((value["hdr_toning"].clone(), report))
}

fn refusal(bytes: &[u8]) -> String {
    match decode(bytes) {
        Ok(decoded) => panic!("decoded unexpectedly: {decoded:?}"),
        Err(message) => message,
    }
}

// ----------------------------------------------------------------- writer --

/// One `.hdt` file, field by field in file order (big-endian), every field
/// public so a case can corrupt exactly one. `Default` is a valid file: Local
/// Adaptation at the core's defaults over the identity curve.
struct Hdt {
    magic: [u8; 4],
    version: u32,
    strength: f32,
    word: u32,
    name: Vec<u16>,
    curve_head: u16,
    /// The point count as stored; `None` writes `points.len()`.
    count: Option<u16>,
    /// `(output, input)`, as stored.
    points: Vec<(u16, u16)>,
    /// One flag per point; `None` writes 1 for every point.
    corners: Option<Vec<u8>>,
    padding: [u8; 8],
    radius: f32,
    method: u32,
    unread: [f32; 2],
    magic2: [u8; 4],
    version2: u32,
    exposure: f32,
    saturation: f32,
    detail: f32,
    shadow: f32,
    highlight: f32,
    gamma: f32,
    vibrance: f32,
    smooth_edges: u8,
    tail: u8,
    trailing: Vec<u8>,
}

impl Default for Hdt {
    fn default() -> Self {
        Self {
            magic: *b"hdrt",
            version: 3,
            strength: 0.52,
            word: 2,
            name: "Default\0".encode_utf16().collect(),
            curve_head: 2,
            count: None,
            points: vec![(0, 0), (255, 255)],
            corners: None,
            padding: [0; 8],
            radius: 16.0,
            method: 3,
            unread: [4.6965, 1.0964],
            magic2: *b"hdra",
            version2: 6,
            exposure: 0.0,
            saturation: 20.0,
            detail: 30.0,
            shadow: 0.0,
            highlight: 0.0,
            gamma: 1.0,
            vibrance: 0.0,
            smooth_edges: 0,
            tail: 0,
            trailing: Vec::new(),
        }
    }
}

impl Hdt {
    fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(self.magic);
        out.extend(self.version.to_be_bytes());
        out.extend(self.strength.to_be_bytes());
        out.extend(self.word.to_be_bytes());
        out.extend((self.name.len() as u32).to_be_bytes());
        for unit in &self.name {
            out.extend(unit.to_be_bytes());
        }
        out.extend(self.curve_head.to_be_bytes());
        let count = self.count.unwrap_or(self.points.len() as u16);
        out.extend(count.to_be_bytes());
        for (output, input) in &self.points {
            out.extend(output.to_be_bytes());
            out.extend(input.to_be_bytes());
        }
        match &self.corners {
            Some(flags) => out.extend(flags),
            None => out.extend(vec![1u8; self.points.len()]),
        }
        out.extend(self.padding);
        out.extend(self.radius.to_be_bytes());
        out.extend(self.method.to_be_bytes());
        for value in self.unread {
            out.extend(value.to_be_bytes());
        }
        out.extend(self.magic2);
        out.extend(self.version2.to_be_bytes());
        for value in [
            self.exposure,
            self.saturation,
            self.detail,
            self.shadow,
            self.highlight,
            self.gamma,
            self.vibrance,
        ] {
            out.extend(value.to_be_bytes());
        }
        out.push(self.smooth_edges);
        out.push(self.tail);
        out.extend(&self.trailing);
        out
    }
}

/// `hdr_toning`'s Local Adaptation arguments at the core's defaults, with
/// `changes` over them.
fn arguments(changes: Value) -> Value {
    let mut out = json!({
        "method": "local_adaptation",
        "radius": 16.0,
        "strength": 0.52,
        "smooth_edges": false,
        "gamma": 1.0,
        "exposure": 0.0,
        "detail": 30.0,
        "shadow": 0.0,
        "highlight": 0.0,
        "vibrance": 0.0,
        "saturation": 20.0,
        "toning_curve": [[0, 0], [255, 255]],
    });
    for (key, value) in changes.as_object().expect("changes") {
        out[key] = value.clone();
    }
    out
}

// ------------------------------------------------------------------ values --

#[test]
fn a_full_file_decodes_to_the_values_it_holds() {
    let file = Hdt {
        strength: 1.15,
        radius: 25.56,
        exposure: 0.3,
        saturation: 26.0,
        detail: 300.0,
        shadow: -100.0,
        highlight: -100.0,
        gamma: 0.76,
        vibrance: 22.0,
        smooth_edges: 1,
        points: vec![(0, 0), (77, 50), (126, 125), (182, 199), (255, 255)],
        ..Hdt::default()
    };
    let (decoded, report) = decode(&file.bytes()).expect("decodes");
    assert_eq!(
        decoded,
        json!({
            "method": "local_adaptation",
            "radius": 25.56,
            "strength": 1.15,
            "smooth_edges": true,
            "gamma": 0.76,
            "exposure": 0.3,
            "detail": 300.0,
            "shadow": -100.0,
            "highlight": -100.0,
            "vibrance": 22.0,
            "saturation": 26.0,
            "toning_curve": [[0, 0], [50, 77], [125, 126], [199, 182], [255, 255]],
        })
    );
    assert!(report.is_empty(), "{report:?}");
}

#[test]
fn the_default_file_is_local_adaptation_at_its_defaults() {
    let (decoded, report) = decode(&Hdt::default().bytes()).expect("decodes");
    assert_eq!(decoded, arguments(json!({})));
    assert!(report.is_empty(), "{report:?}");
}

/// A stored f32 is read as the shortest decimal that round-trips to it, so a
/// value entered as 1.15 imports as 1.15, not as the f32's 1.1499999761…
#[test]
fn stored_floats_read_as_their_shortest_decimal() {
    for (stored, read) in [(1.15f32, 1.15f64), (0.1, 0.1), (4.0, 4.0), (0.5296, 0.5296)] {
        let file = Hdt {
            strength: stored,
            ..Hdt::default()
        };
        let (decoded, _) = decode(&file.bytes()).expect("decodes");
        assert_eq!(decoded["strength"], json!(read), "{stored}");
    }
}

/// The curve's pairs are stored output first, the order the crate's other
/// curve files from the same application use (`.acv`); the argument is
/// `[in, out]`.
#[test]
fn curve_pairs_are_stored_output_first() {
    let file = Hdt {
        points: vec![(0, 0), (5, 48), (230, 203), (255, 255)],
        ..Hdt::default()
    };
    let (decoded, _) = decode(&file.bytes()).expect("decodes");
    assert_eq!(
        decoded["toning_curve"],
        json!([[0, 0], [48, 5], [203, 230], [255, 255]])
    );
}

#[test]
fn smooth_edges_reads_zero_and_one() {
    for (stored, read) in [(0u8, false), (1, true)] {
        let file = Hdt {
            smooth_edges: stored,
            ..Hdt::default()
        };
        let (decoded, _) = decode(&file.bytes()).expect("decodes");
        assert_eq!(decoded["smooth_edges"], json!(read));
    }
}

/// The fields the reader skips — the stored name, the two floats after the
/// method and the last byte — change nothing.
#[test]
fn the_unread_fields_change_nothing() {
    let file = Hdt {
        name: "City Twilight\0".encode_utf16().collect(),
        unread: [0.0, 1.0],
        tail: 7,
        ..Hdt::default()
    };
    let (decoded, report) = decode(&file.bytes()).expect("decodes");
    assert_eq!(decoded, arguments(json!({})));
    assert!(report.is_empty());
}

// ------------------------------------------------------------------ report --

#[test]
fn a_gamma_above_two_imports_as_two_with_a_report_line() {
    for (stored, line) in [
        (3.25f32, "gamma 3.25 is above the maximum 2; imported as 2"),
        (4.64, "gamma 4.64 is above the maximum 2; imported as 2"),
        (2.03, "gamma 2.03 is above the maximum 2; imported as 2"),
    ] {
        let file = Hdt {
            gamma: stored,
            ..Hdt::default()
        };
        let (decoded, report) = decode(&file.bytes()).expect("decodes");
        assert_eq!(decoded, arguments(json!({"gamma": 2.0})), "{stored}");
        assert_eq!(report, [line]);
    }
    let at_the_maximum = Hdt {
        gamma: 2.0,
        ..Hdt::default()
    };
    let (decoded, report) = decode(&at_the_maximum.bytes()).expect("decodes");
    assert_eq!(decoded["gamma"], json!(2.0));
    assert!(report.is_empty(), "{report:?}");
}

/// A flag of 0 marks a corner point (1 is a smooth one, the flag every
/// default curve carries); Doctored's Toning Curve is smooth through every
/// point, so the import names the corners it does not carry.
#[test]
fn corner_points_are_named_in_the_report() {
    let curve = vec![(0, 0), (40, 64), (220, 192), (255, 255)];
    for (flags, line) in [
        (vec![1, 1, 1, 1], None),
        (
            vec![1, 0, 1, 1],
            Some("1 of the toning curve's 4 points is a corner; imported as a smooth point"),
        ),
        (
            vec![1, 0, 0, 1],
            Some("2 of the toning curve's 4 points are corners; imported as smooth points"),
        ),
    ] {
        let file = Hdt {
            points: curve.clone(),
            corners: Some(flags.clone()),
            ..Hdt::default()
        };
        let (decoded, report) = decode(&file.bytes()).expect("decodes");
        assert_eq!(
            decoded["toning_curve"],
            json!([[0, 0], [64, 40], [192, 220], [255, 255]])
        );
        assert_eq!(report, line.into_iter().collect::<Vec<_>>(), "{flags:?}");
    }
    let both = Hdt {
        gamma: 3.0,
        corners: Some(vec![0, 1]),
        ..Hdt::default()
    };
    let (_, report) = decode(&both.bytes()).expect("decodes");
    assert_eq!(
        report,
        [
            "gamma 3 is above the maximum 2; imported as 2",
            "1 of the toning curve's 2 points is a corner; imported as a smooth point",
        ]
    );
}

// ---------------------------------------------------------------- refusals --

#[test]
fn another_method_refuses_by_its_number() {
    for method in [0u32, 1, 2, 4] {
        let file = Hdt {
            method,
            ..Hdt::default()
        };
        assert_eq!(
            refusal(&file.bytes()),
            format!(
                "hdr toning preset method {method} is not supported: only local adaptation (3) is read"
            )
        );
    }
}

#[test]
fn every_exact_field_refuses_by_name() {
    let cases: Vec<(Hdt, &str)> = vec![
        (
            Hdt {
                magic: *b"hdrx",
                ..Hdt::default()
            },
            "hdr toning preset signature is \"hdrx\"; expected \"hdrt\"",
        ),
        (
            Hdt {
                version: 2,
                ..Hdt::default()
            },
            "hdr toning preset version 2 is not 3",
        ),
        (
            Hdt {
                word: 1,
                ..Hdt::default()
            },
            "hdr toning preset header word 1 is not 2",
        ),
        (
            Hdt {
                name: Vec::new(),
                ..Hdt::default()
            },
            "hdr toning preset name holds 0 characters; expected 1 to 256",
        ),
        (
            Hdt {
                name: vec![0x41; 257],
                ..Hdt::default()
            },
            "hdr toning preset name holds 257 characters; expected 1 to 256",
        ),
        (
            Hdt {
                curve_head: 1,
                ..Hdt::default()
            },
            "hdr toning preset curve header 1 is not 2",
        ),
        (
            Hdt {
                points: vec![(0, 0)],
                ..Hdt::default()
            },
            "hdr toning preset toning curve holds 1 points; expected 2 to 16",
        ),
        (
            Hdt {
                points: (0..17).map(|i| (i * 15, i * 15)).collect(),
                ..Hdt::default()
            },
            "hdr toning preset toning curve holds 17 points; expected 2 to 16",
        ),
        (
            Hdt {
                corners: Some(vec![1, 2]),
                ..Hdt::default()
            },
            "hdr toning preset corner flag of point 2 is 2; expected 0 or 1",
        ),
        (
            Hdt {
                padding: [0, 0, 0, 0, 0, 0, 0, 1],
                ..Hdt::default()
            },
            "hdr toning preset padding after the corner flags is not zero",
        ),
        (
            Hdt {
                magic2: *b"HDRA",
                ..Hdt::default()
            },
            "hdr toning preset local adaptation signature is \"HDRA\"; expected \"hdra\"",
        ),
        (
            Hdt {
                version2: 5,
                ..Hdt::default()
            },
            "hdr toning preset local adaptation version 5 is not 6",
        ),
        (
            Hdt {
                smooth_edges: 2,
                ..Hdt::default()
            },
            "hdr toning preset smooth_edges 2 is not 0 or 1",
        ),
        (
            Hdt {
                trailing: vec![0],
                ..Hdt::default()
            },
            "hdr toning preset has 1 bytes after its last field",
        ),
    ];
    for (file, message) in cases {
        assert_eq!(refusal(&file.bytes()), message);
    }
}

#[test]
fn every_truncation_refuses_and_never_panics() {
    let file = Hdt {
        points: vec![(0, 0), (77, 50), (255, 255)],
        ..Hdt::default()
    }
    .bytes();
    for len in 0..file.len() {
        let message = refusal(&file[..len]);
        assert!(
            message.starts_with("hdr toning preset is truncated at "),
            "{len}: {message}"
        );
    }
    assert_eq!(
        refusal(&[]),
        "hdr toning preset is truncated at its signature"
    );
    assert_eq!(
        refusal(&file[..file.len() - 1]),
        "hdr toning preset is truncated at its last byte"
    );
    assert!(decode(&file).is_ok());
}

#[test]
fn a_stored_value_that_is_not_finite_refuses_by_name() {
    let nan = Hdt {
        strength: f32::NAN,
        ..Hdt::default()
    };
    assert_eq!(
        refusal(&nan.bytes()),
        "hdr toning preset strength is not a finite number"
    );
    let infinite = Hdt {
        gamma: f32::INFINITY,
        ..Hdt::default()
    };
    assert_eq!(
        refusal(&infinite.bytes()),
        "hdr toning preset gamma is not a finite number"
    );
}

/// The ranges are the request's own (`LocalAdaptation::parse`), refused with
/// its sentence; only a gamma above 2 is clamped.
#[test]
fn a_value_outside_the_request_ranges_refuses_with_its_sentence() {
    let cases: Vec<(Hdt, &str)> = vec![
        (
            Hdt {
                radius: 600.0,
                ..Hdt::default()
            },
            "hdr toning radius must be a number in 1…500 pixels",
        ),
        (
            Hdt {
                gamma: 0.05,
                ..Hdt::default()
            },
            "hdr toning gamma must be a number in 0.1…2",
        ),
        (
            Hdt {
                detail: 301.0,
                ..Hdt::default()
            },
            "hdr toning detail must be a number in -100…300",
        ),
        (
            Hdt {
                points: vec![(0, 0), (300, 128), (255, 255)],
                ..Hdt::default()
            },
            "hdr toning toning_curve must be 2 to 16 [in, out] points in 0…255",
        ),
        (
            Hdt {
                points: vec![(0, 9), (255, 9)],
                ..Hdt::default()
            },
            "hdr toning toning_curve needs at least two points with different inputs",
        ),
    ];
    for (file, reason) in cases {
        assert_eq!(
            refusal(&file.bytes()),
            format!("this hdr toning preset cannot be used: {reason}")
        );
    }
}

#[test]
fn a_file_over_4_kib_is_refused_before_it_is_read() {
    let mut file = Hdt::default().bytes();
    file.resize(4097, 0);
    assert_eq!(
        refusal(&file),
        "hdr toning preset file is 4097 bytes; the limit is 4096"
    );
    // At the cap exactly the file is read, and its padding is trailing bytes.
    file.truncate(4096);
    assert!(refusal(&file).starts_with("hdr toning preset has "));
    // The cap is checked BEFORE the bytes are touched: a NULL pointer with an
    // oversized length still answers the size sentence.
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe { dr_hdr_toning_preset_file_decode(ptr::null(), 1 << 40, &mut err) };
    assert!(raw.is_null());
    assert_eq!(
        take_err_string(err),
        format!(
            "hdr toning preset file is {} bytes; the limit is 4096",
            1u64 << 40
        )
    );
}

// -------------------------------------------------------------- NULL sweep --

#[test]
fn null_arguments_are_tolerated() {
    let file = Hdt::default().bytes();
    // NULL bytes with a non-zero length.
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe { dr_hdr_toning_preset_file_decode(ptr::null(), 4, &mut err) };
    assert!(raw.is_null());
    assert_eq!(take_err_string(err), "bytes is NULL");
    // NULL bytes with zero length is an EMPTY file: the format's own
    // truncation sentence, not a null-argument one.
    err = ptr::null_mut();
    let raw = unsafe { dr_hdr_toning_preset_file_decode(ptr::null(), 0, &mut err) };
    assert!(raw.is_null());
    assert_eq!(
        take_err_string(err),
        "hdr toning preset is truncated at its signature"
    );
    // NULL err_out, on failure and on success.
    let raw = unsafe { dr_hdr_toning_preset_file_decode(ptr::null(), 0, ptr::null_mut()) };
    assert!(raw.is_null());
    let raw =
        unsafe { dr_hdr_toning_preset_file_decode(file.as_ptr(), file.len(), ptr::null_mut()) };
    assert!(!raw.is_null());
    let _ = take_err_string(raw);
}

// ------------------------------------------------------ installed presets --

/// Every HDR Toning preset an installed Photoshop ships decodes. Reads only
/// `/Applications/Adobe Photoshop */Presets/HDR Toning` — never the user's
/// own library.
#[test]
#[ignore = "reads an installed Adobe Photoshop's shipped presets"]
fn installed_photoshop_presets_decode() {
    let mut decoded = 0;
    let mut failures = Vec::new();
    let apps = std::fs::read_dir("/Applications").expect("/Applications");
    for app in apps.flatten() {
        let name = app.file_name().to_string_lossy().into_owned();
        if !name.starts_with("Adobe Photoshop ") {
            continue;
        }
        let dir = app.path().join("Presets").join("HDR Toning");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("hdt") {
                continue;
            }
            let bytes = std::fs::read(&path).expect("preset bytes");
            match decode(&bytes) {
                Ok((arguments, report)) => {
                    println!("{} -> {arguments} {report:?}", display(&path));
                    decoded += 1;
                }
                Err(message) => failures.push(format!("{}: {message}", display(&path))),
            }
        }
    }
    println!("decoded {decoded} installed presets");
    assert!(failures.is_empty(), "{failures:#?}");
    assert!(
        decoded > 0,
        "no installed Photoshop HDR Toning presets found"
    );
}

fn display(path: &Path) -> String {
    path.display().to_string()
}
