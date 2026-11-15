//! KERNEL: the four frozen pattern presets — their ids, display names and
//! tiles (`pattern_tile::PatternTile`). A `.rz` stores only a preset's id, so
//! the definitions never change.

use crate::pattern_tile::PatternTile;

/// The four presets, FROZEN: `(id, display name)` in popup order.
pub const PRESETS: [(&str, &str); 4] = [
    ("checkerboard", "Checkerboard"),
    ("stripes", "Stripes"),
    ("dots", "Dots"),
    ("grid", "Grid"),
];

/// A tile drawn from `texel(x, y)` (straight RGBA8).
fn drawn(width: u32, height: u32, texel: impl Fn(u32, u32) -> [u8; 4]) -> PatternTile {
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            rgba.extend_from_slice(&texel(x, y));
        }
    }
    PatternTile {
        width,
        height,
        rgba: rgba.into(),
        samples: None,
    }
}

/// The preset `id`'s tile; `None` for an unknown id. The definitions are
/// frozen (a `.rz` stores only the id).
pub fn preset_tile(id: &str) -> Option<PatternTile> {
    match id {
        // 16 x 16: white where x/8 + y/8 is even, else #c0c0c0.
        "checkerboard" => Some(drawn(16, 16, |x, y| {
            if (x / 8 + y / 8) % 2 == 0 {
                [0xff, 0xff, 0xff, 0xff]
            } else {
                [0xc0, 0xc0, 0xc0, 0xff]
            }
        })),
        // 8 x 8: black for x < 4, else white.
        "stripes" => Some(drawn(8, 8, |x, _| {
            if x < 4 {
                [0, 0, 0, 0xff]
            } else {
                [0xff, 0xff, 0xff, 0xff]
            }
        })),
        // 12 x 12: an opaque black disc of radius 3 about (6, 6), sampled at
        // texel centres; transparent elsewhere.
        "dots" => Some(drawn(12, 12, |x, y| {
            let (dx, dy) = (f64::from(x) + 0.5 - 6.0, f64::from(y) + 0.5 - 6.0);
            if dx * dx + dy * dy <= 9.0 {
                [0, 0, 0, 0xff]
            } else {
                [0, 0, 0, 0]
            }
        })),
        // 10 x 10: opaque #808080 on row 0 and column 0; transparent elsewhere.
        "grid" => Some(drawn(10, 10, |x, y| {
            if x == 0 || y == 0 {
                [0x80, 0x80, 0x80, 0xff]
            } else {
                [0, 0, 0, 0]
            }
        })),
        _ => None,
    }
}
