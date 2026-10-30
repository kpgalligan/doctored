//! A synthetic ICC profile builder shared by the LUT parser's tests and the
//! LUT-destination conversion tests: a header, a tag table and the `desc`,
//! `wtpt` and `mft2` tag bodies, laid out by hand.
#![allow(dead_code)]

/// Assembles an ICC profile from a class, a data space, a PCS and a tag
/// list. A SECOND implementation of the layout, deliberately (the
/// `color_tests` builder's rule) — if the two ever disagree, one is wrong.
pub fn build(
    version: [u8; 4],
    class: &[u8; 4],
    space: &[u8; 4],
    pcs: &[u8; 4],
    tags: &[(&[u8; 4], Vec<u8>)],
) -> Vec<u8> {
    let payload_start = 132 + 12 * tags.len();
    let mut table = Vec::new();
    let mut payload: Vec<u8> = Vec::new();
    for (sig, data) in tags {
        let offset = (payload_start + payload.len()) as u32;
        payload.extend_from_slice(data);
        while !payload.len().is_multiple_of(4) {
            payload.push(0);
        }
        table.extend_from_slice(sig.as_slice());
        table.extend_from_slice(&offset.to_be_bytes());
        table.extend_from_slice(&(data.len() as u32).to_be_bytes());
    }
    let total = (payload_start + payload.len()) as u32;
    let mut out = vec![0u8; 128];
    out[0..4].copy_from_slice(&total.to_be_bytes());
    out[8..12].copy_from_slice(&version);
    out[12..16].copy_from_slice(class);
    out[16..20].copy_from_slice(space);
    out[20..24].copy_from_slice(pcs);
    out[36..40].copy_from_slice(b"acsp");
    out.extend_from_slice(&(tags.len() as u32).to_be_bytes());
    out.extend_from_slice(&table);
    out.extend_from_slice(&payload);
    out
}

pub fn tag_desc(name: &str) -> Vec<u8> {
    let mut t = b"desc\0\0\0\0".to_vec();
    let bytes = name.as_bytes();
    t.extend_from_slice(&((bytes.len() + 1) as u32).to_be_bytes());
    t.extend_from_slice(bytes);
    t.push(0);
    t
}

pub fn tag_wtpt(xyz: [f64; 3]) -> Vec<u8> {
    let mut t = b"XYZ \0\0\0\0".to_vec();
    for v in xyz {
        t.extend_from_slice(&(((v * 65536.0).round() as i32).to_be_bytes()));
    }
    t
}

/// A `mft2` (lut16Type). `clut(inputs) -> outputs`, sampled on the grid.
#[allow(clippy::type_complexity)]
pub fn tag_mft2(
    inputs: usize,
    outputs: usize,
    grid: usize,
    clut: &dyn Fn(&[f64]) -> Vec<f64>,
) -> Vec<u8> {
    let mut t = b"mft2\0\0\0\0".to_vec();
    t.push(inputs as u8);
    t.push(outputs as u8);
    t.push(grid as u8);
    t.push(0);
    // The identity matrix, which is what every non-XYZ-input table writes.
    for v in [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0] {
        t.extend_from_slice(&(((v * 65536.0).round() as i32).to_be_bytes()));
    }
    t.extend_from_slice(&2u16.to_be_bytes()); // input table entries
    t.extend_from_slice(&2u16.to_be_bytes()); // output table entries
    for _ in 0..inputs {
        t.extend_from_slice(&0u16.to_be_bytes());
        t.extend_from_slice(&65535u16.to_be_bytes());
    }
    let total = grid.pow(inputs as u32);
    for index in 0..total {
        let mut coords = vec![0.0f64; inputs];
        let mut rest = index;
        for axis in (0..inputs).rev() {
            coords[axis] = (rest % grid) as f64 / (grid - 1) as f64;
            rest /= grid;
        }
        for value in clut(&coords) {
            t.extend_from_slice(&((value.clamp(0.0, 1.0) * 65535.0).round() as u16).to_be_bytes());
        }
    }
    for _ in 0..outputs {
        t.extend_from_slice(&0u16.to_be_bytes());
        t.extend_from_slice(&65535u16.to_be_bytes());
    }
    t
}

/// [`tag_mft2`] with an explicit INPUT table: `input` (u16 samples, at least
/// two) is every input channel's curve, sampled uniformly over 0…1; the
/// output tables stay the 2-entry identity.
#[allow(clippy::type_complexity)]
pub fn tag_mft2_with_input(
    inputs: usize,
    outputs: usize,
    grid: usize,
    input: &[u16],
    clut: &dyn Fn(&[f64]) -> Vec<f64>,
) -> Vec<u8> {
    let plain = tag_mft2(inputs, outputs, grid, clut);
    // The header (8 + 4 + 36 bytes), the two entry counts, the input tables
    // (2 entries each), then the CLUT and the output tables.
    let head = 8 + 4 + 36;
    let tables = head + 4;
    let mut t = plain[..head].to_vec();
    t.extend_from_slice(&(input.len() as u16).to_be_bytes());
    t.extend_from_slice(&2u16.to_be_bytes());
    for _ in 0..inputs {
        for v in input {
            t.extend_from_slice(&v.to_be_bytes());
        }
    }
    t.extend_from_slice(&plain[tables + inputs * 4..]);
    t
}
