//! ZIP (compression words 2 and 3) at 8 bits and in the stored composite at
//! both depths, through the C API.
//!
//! The oracle is `common/psb_fixture.rs`: its own stored-block zlib writer,
//! its own Adler-32 and its own per-row delta ENCODER. Nothing here is
//! compared against the crate's inflater or its prediction pass.
//!
//! The composite cases are the ones that size an allocation: the stored
//! composite is ONE zlib stream through every channel of the file, and the
//! header may declare up to 56 channels, so the reader must inflate only the
//! planes it keeps.
use doctored_core::psd_zip::unpack_zip8;
use serde_json::Value;
use tempfile::TempDir;

use crate::psb_fixture::{self as fixture, two_layers, File, FLATTENED, LAYERS};

fn bytes8(pixels: &[[u16; 4]]) -> Vec<u8> {
    pixels.iter().flatten().map(|v| *v as u8).collect()
}

fn write(directory: &TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = directory.path().join(name);
    std::fs::write(&path, bytes).expect("write");
    path
}

#[test]
fn eight_bit_zip_layer_channels_open_with_and_without_prediction() {
    let directory = TempDir::new().unwrap();
    for word in [2u16, 3] {
        let file = two_layers(1, 8, word);
        let path = write(&directory, &format!("zip{word}.psd"), &file.build());
        let report = fixture::import_report(&path);
        assert_eq!(report["format"], Value::from("PSD"));
        assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
        let layered = fixture::open8(&path, LAYERS).expect("layers");
        assert_eq!(layered.layers.len(), 2);
        assert_eq!(
            layered.flattened().unwrap().into_raw(),
            bytes8(&file.composite),
            "word {word}"
        );
    }
}

#[test]
fn an_eight_bit_zip_mask_plane_opens_with_its_samples_at_its_own_rectangle() {
    let directory = TempDir::new().unwrap();
    for word in [2u16, 3] {
        let mut file = two_layers(1, 8, word);
        // 6 x 4 at left 2, top 0: neither the layer's 4 x 2 rectangle at
        // (3, 1) nor the canvas, and no two rows alike, so a plane decoded at
        // the layer's size or with the delta undone wrongly cannot match.
        let mask = fixture::Mask {
            bounds: [0, 2, 4, 8],
            samples: (0..24u32).map(|i| (i * 37 + 11) as u8).collect(),
            outside: 255,
        };
        file.layers[1].mask = Some(mask.clone());
        let path = write(&directory, &format!("mask{word}.psd"), &file.build());
        let report = fixture::import_report(&path);
        assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
        let layered = fixture::open8(&path, LAYERS).expect("layers");
        assert_eq!(layered.layers.len(), 2);
        assert!(layered.layers[0].mask.is_none(), "word {word}");
        let upper = &layered.layers[1];
        let plane = upper.mask.as_ref().expect("the mask plane");
        assert_eq!(plane.dimensions(), (6, 4), "word {word}");
        assert_eq!(plane.as_raw(), &mask.samples, "word {word}");
        assert_eq!(
            upper.mask_properties.offset,
            (mask.bounds[1], mask.bounds[0]),
            "word {word}"
        );
        assert_eq!(upper.mask_properties.outside_coverage, 255, "word {word}");
        assert!(upper.mask_enabled, "word {word}");
        // The layer's own pixels are where the record put them.
        assert_eq!(upper.offset, (3, 1), "word {word}");
        assert_eq!(upper.pixels.dimensions(), (4, 2), "word {word}");
    }
}

/// A file declaring 56 channels: the four picture planes and 52 saved ones,
/// each filled with a value no picture plane holds.
fn fifty_six(depth: u16, word: u16) -> File {
    let mut file = two_layers(1, depth, 0);
    let plane = file.width as usize * file.height as usize * file.sample_bytes();
    file.saved = (0..52)
        .map(|index| vec![200 + index as u8; plane])
        .collect();
    file.composite_word = word;
    file
}

#[test]
fn a_fifty_six_channel_zip_composite_opens_as_its_first_planes() {
    let directory = TempDir::new().unwrap();
    for word in [2u16, 3] {
        let file = fifty_six(8, word);
        let path = write(&directory, &format!("c8-{word}.psd"), &file.build());
        let report = fixture::import_report(&path);
        assert_eq!(
            report["stored_composite_available"],
            Value::Bool(true),
            "{report}"
        );
        let stored = fixture::open8(&path, FLATTENED).expect("composite");
        assert_eq!(
            stored.flattened().unwrap().into_raw(),
            bytes8(&file.composite),
            "8-bit word {word}"
        );

        let file = fifty_six(16, word);
        let path = write(&directory, &format!("c16-{word}.psd"), &file.build());
        let report = fixture::import_report(&path);
        assert_eq!(
            report["stored_composite_available"],
            Value::Bool(true),
            "{report}"
        );
        let stored = fixture::open16(&path, FLATTENED).expect("composite");
        assert_eq!(
            fixture::rendered16(&stored),
            file.composite,
            "16-bit word {word}"
        );
    }
}

#[test]
fn a_zip_composite_whose_stream_ends_inside_the_kept_planes_refuses() {
    let directory = TempDir::new().unwrap();
    for depth in [8u16, 16] {
        let file = two_layers(1, depth, 0);
        let mut bytes = file.build();
        let plane = file.width as usize * file.height as usize * file.sample_bytes();
        // The raw composite is the last `2 + 4 planes` bytes; replace it with
        // a COMPLETE zlib stream that holds only two of the four kept planes.
        bytes.truncate(bytes.len() - (2 + 4 * plane));
        bytes.extend_from_slice(&2u16.to_be_bytes());
        let mut two = fixture::plane_of(&file.composite, 0, file.sample_bytes());
        two.extend(fixture::plane_of(&file.composite, 1, file.sample_bytes()));
        bytes.extend_from_slice(&fixture::zlib_stored(&two));
        let path = write(&directory, &format!("short{depth}.psd"), &bytes);
        let report = fixture::import_report(&path);
        assert_eq!(report["stored_composite_available"], Value::Bool(false));
        assert_eq!(
            report["stored_composite_refusal"],
            Value::from("PSD ZIP composite ends before its color planes"),
            "{report}"
        );
        // The layers are untouched by a broken composite.
        assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
        let refused = if depth == 8 {
            fixture::open8(&path, FLATTENED).err()
        } else {
            fixture::open16(&path, FLATTENED).err()
        };
        assert!(
            refused
                .expect("refused")
                .contains("ends before its color planes"),
            "depth {depth}"
        );
    }
}

#[test]
fn unpack_zip8_undoes_the_byte_delta_and_refuses_what_is_not_the_plane() {
    let (row_bytes, rows) = (7usize, 3usize);
    let plane: Vec<u8> = (0..row_bytes * rows)
        .map(|i| (i * 53 % 251) as u8)
        .collect();
    let plain = fixture::zlib_stored(&plane);
    assert_eq!(unpack_zip8(&plain, row_bytes, rows, false).unwrap(), plane);
    let predicted = fixture::zlib_stored(&fixture::predict(&plane, row_bytes, 1));
    assert_eq!(
        unpack_zip8(&predicted, row_bytes, rows, true).unwrap(),
        plane
    );
    // The delta restarts on every row: decoding the plain stream AS predicted
    // gives each row's running sum, which the test computes itself.
    let summed: Vec<u8> = plane
        .chunks(row_bytes)
        .flat_map(|row| {
            let mut total = 0u8;
            row.iter()
                .map(|v| {
                    total = total.wrapping_add(*v);
                    total
                })
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(unpack_zip8(&plain, row_bytes, rows, true).unwrap(), summed);

    let mut trailing = plain.clone();
    trailing.push(0);
    assert_eq!(
        unpack_zip8(&trailing, row_bytes, rows, false).unwrap_err(),
        "PSD ZIP channel has trailing data"
    );
    assert!(unpack_zip8(&plain, row_bytes, rows + 1, false).is_err());
    assert!(unpack_zip8(&plain, row_bytes, rows - 1, false).is_err());
    assert_eq!(
        unpack_zip8(&[0, 0, 1, 2], row_bytes, rows, false).unwrap_err(),
        "PSD ZIP channel has an invalid zlib header"
    );
    assert!(unpack_zip8(&[], 1, 1, false).is_err());
}
