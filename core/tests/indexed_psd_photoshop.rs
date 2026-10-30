//! #547's Indexed Color PSD and PSB against Photoshop's own files
//! (`tests/fixtures/psd_photoshop/indexed_547`, authored by
//! `scripts/photoshop-probe/indexed_psd_take.py`'s take1 from the runner's
//! scenes). Each fixture's length and this suite's parse (`indexed_psd::parse`)
//! hold `photoshop.json`; its palette and indices hold the SCENE it was
//! converted from (every opaque pixel's entry is its colour, every clear one
//! the transparent index); Doctored's open keeps the palette, the
//! transparent index and every row; and Doctored's re-save of it matches
//! Photoshop's file field by field: the header, resources 1046 and 1047, the
//! layer section's shape and the composite's indices. SKIP when the folder is
//! absent.

use std::path::{Path, PathBuf};

use serde_json::Value;
use tempfile::TempDir;

use crate::indexed_fixture::{free, palette_of};
use crate::indexed_psd::{open_psd, parse, plane_of, save, Parsed};

const BANDS: [[u8; 3]; 6] = [
    [200, 30, 30],
    [30, 160, 60],
    [40, 60, 200],
    [230, 200, 40],
    [20, 20, 20],
    [128, 128, 128],
];

fn folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/psd_photoshop/indexed_547")
}

/// `photoshop.json`, or `None` (SKIP) when the folder is absent.
fn manifest() -> Option<Value> {
    let path = folder().join("photoshop.json");
    if !path.is_file() {
        eprintln!("SKIP: {} is absent", path.display());
        return None;
    }
    Some(serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap())
}

/// The runner's scene (`indexed_psd_take.scene`): (width, height, RGBA).
fn scene(name: &str) -> (u32, u32, Vec<[u8; 4]>) {
    if name == "full256" {
        let px = (0..16u32)
            .flat_map(|y| (0..16u32).map(move |x| [(17 * x) as u8, (17 * y) as u8, 128, 255]))
            .collect();
        return (16, 16, px);
    }
    let clear = name == "exact_t";
    let px = (0..16u32)
        .flat_map(|_| {
            (0..24u32).map(move |x| {
                let [r, g, b] = BANDS[(x / 4) as usize];
                [r, g, b, if clear && x >= 20 { 0 } else { 255 }]
            })
        })
        .collect();
    (24, 16, px)
}

fn rows_of(entry: &Value) -> Vec<u8> {
    entry["rows"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|row| {
            row.as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u8)
        })
        .collect()
}

fn palette_entry(entry: &Value) -> Vec<[u8; 3]> {
    entry["palette"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| [0, 1, 2].map(|i| c[i].as_u64().unwrap() as u8))
        .collect()
}

fn files(manifest: &Value) -> Vec<Value> {
    manifest["files"].as_array().unwrap().clone()
}

#[test]
fn each_fixture_holds_its_record_and_its_scene() {
    let Some(manifest) = manifest() else { return };
    assert!(!files(&manifest).is_empty(), "no AUTHORED fixture recorded");
    for entry in files(&manifest) {
        let name = entry["name"].as_str().unwrap();
        let bytes = std::fs::read(folder().join(name)).unwrap();
        assert_eq!(
            bytes.len() as u64,
            entry["bytes"].as_u64().unwrap(),
            "{name}"
        );
        let p = parse(&bytes);
        assert_eq!(
            (p.version, p.depth, p.mode),
            (if name.ends_with(".psb") { 2 } else { 1 }, 8, 2),
            "{name}"
        );
        let (palette, t) = p.table();
        assert_eq!(
            palette.len() as u64,
            entry["count"].as_u64().unwrap(),
            "{name}"
        );
        assert_eq!(palette, palette_entry(&entry), "{name}");
        assert_eq!(
            t.map(u64::from),
            entry["transparent_index"].as_u64(),
            "{name}"
        );
        assert_eq!(p.indices, rows_of(&entry), "{name}");
        let (w, h, px) = scene(entry["scene"].as_str().unwrap());
        assert_eq!((p.width, p.height), (w, h), "{name}");
        for (pixel, &index) in px.iter().zip(&p.indices) {
            if pixel[3] == 0 {
                assert_eq!(Some(index), t, "{name}: a clear pixel holds t");
            } else {
                assert_eq!(
                    palette[usize::from(index)],
                    [pixel[0], pixel[1], pixel[2]],
                    "{name}: an opaque pixel's entry is its colour"
                );
            }
        }
    }
}

#[test]
fn rasterize_opens_each_fixture_exactly() {
    let Some(manifest) = manifest() else { return };
    for entry in files(&manifest) {
        let name = entry["name"].as_str().unwrap();
        let doc = open_psd(&folder().join(name)).unwrap();
        let t = entry["transparent_index"].as_u64().map(|t| t as u8);
        assert_eq!(palette_of(doc), (palette_entry(&entry), t), "{name}");
        assert_eq!(plane_of(doc), rows_of(&entry), "{name}");
        free(doc);
    }
}

/// One record's name, bounds, opacity, flags and channel ids in order.
type RecordShape = (String, [i32; 4], u8, u8, Vec<i16>);

/// The layer section's shape: the stored count, and each record's
/// [`RecordShape`].
fn shape(p: &Parsed) -> (i16, Vec<RecordShape>) {
    (
        p.count,
        p.records
            .iter()
            .map(|r| {
                (
                    r.name.clone(),
                    r.bounds,
                    r.opacity,
                    r.flags,
                    r.channels.iter().map(|c| c.0).collect(),
                )
            })
            .collect(),
    )
}

#[test]
fn rasterizes_resave_matches_photoshops_file_field_by_field() {
    let Some(manifest) = manifest() else { return };
    let dir = TempDir::new().unwrap();
    for entry in files(&manifest) {
        let name = entry["name"].as_str().unwrap();
        let theirs = parse(&std::fs::read(folder().join(name)).unwrap());
        let doc = open_psd(&folder().join(name)).unwrap();
        let out = dir.path().join(name);
        save(doc, &out, i32::from(name.ends_with(".psb")), 1).unwrap();
        free(doc);
        let ours = parse(&std::fs::read(&out).unwrap());
        assert_eq!(
            (
                ours.version,
                ours.channels,
                ours.width,
                ours.height,
                ours.depth,
                ours.mode
            ),
            (
                theirs.version,
                theirs.channels,
                theirs.width,
                theirs.height,
                theirs.depth,
                theirs.mode
            ),
            "{name}: the header"
        );
        assert_eq!(
            ours.mode_data, theirs.mode_data,
            "{name}: the 768-byte table"
        );
        for id in [1046, 1047] {
            assert_eq!(
                ours.resource(id),
                theirs.resource(id),
                "{name}: resource {id}"
            );
        }
        assert_eq!(
            shape(&ours),
            shape(&theirs),
            "{name}: the layer section's shape"
        );
        assert_eq!(
            ours.indices, theirs.indices,
            "{name}: the composite's indices"
        );
    }
}
