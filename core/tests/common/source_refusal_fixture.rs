//! Shared builders for `source_profile_refusal.rs`: a base document
//! plus one placed ordinary source carrying NO profile refusal, at each depth.
//!
//! The throwaway generator that wrote the stored 53/55-writer files
//! (`tests/fixtures/rz_byte_53/placed_source.rz`,
//! `tests/fixtures/rz_typed_55/placed_source.rz`) held this file VERBATIM at
//! a commit whose `place` request had no `profile_refusal` key, so nothing
//! here may name that key's Rust surface: a refusal travels only as request
//! JSON, and the builders below never send one.
#![allow(dead_code)]

use doctored_core::doc::DrDocument;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::retained_source::SourceEdit;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

pub const LIMIT: u64 = REFERENCE_WORKING_BYTES;
pub const SOURCE_ID: &str = "refusal-master";
pub const REVISION: &str = "first";
/// The placed layer: bottom-first index at byte depth, postorder at typed.
pub const PLACED: usize = 1;

/// The place request; `refusal` adds the `profile_refusal` key.
pub fn place_request(refusal: Option<&str>) -> Value {
    let mut request = json!({"op":"place","source_id":SOURCE_ID,
        "revision_id":REVISION,"name":"Placed","x":2,"y":1});
    if let Some(text) = refusal {
        request["profile_refusal"] = json!(text);
    }
    request
}

pub fn place(refusal: Option<&str>) -> SourceEdit {
    SourceEdit::parse(&place_request(refusal).to_string()).unwrap()
}

pub fn byte_base() -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(9, 7, |x, y| {
        Rgba([(x * 23 + 5) as u8, (y * 31 + 9) as u8, 77, 255])
    }))
}

pub fn byte_master() -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(5, 4, |x, y| {
        Rgba([
            (x * 41 + y * 7 + 3) as u8,
            (y * 59 + 11) as u8,
            (x * 13 + 200) as u8,
            [0, 1, 128, 255][((x + y) % 4) as usize],
        ])
    }))
}

pub fn typed_base() -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            9,
            7,
            (0..63_u16)
                .map(|v| [v * 997 + 5, 65535 - v * 613, 4099, 65535])
                .collect(),
            IccProfile::srgb(),
            None,
        )
        .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

pub fn typed_master() -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            5,
            4,
            (0..20_u16)
                .map(|v| {
                    [
                        v * 3001 + 7,
                        v * 1009 + 13,
                        65535 - v * 2003,
                        [0, 1, 32769, 65535][(v % 4) as usize],
                    ]
                })
                .collect(),
            IccProfile::srgb(),
            None,
        )
        .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

/// The byte base with the master placed and no refusal sent.
pub fn byte_document() -> DrDocument {
    byte_base()
        .source_edit(Some(&byte_master()), &place(None), LIMIT, 0, None)
        .unwrap()
        .unwrap()
}

/// The typed twin.
pub fn typed_document() -> PrecisionDocument {
    typed_base()
        .source_edit(Some(&typed_master()), &place(None), LIMIT, 0, None)
        .unwrap()
        .unwrap()
}
