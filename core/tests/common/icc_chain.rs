//! An independent matrix/TRC colour chain over a profile's own DATA — its
//! three colorant columns and its `curv` table — with every piece of
//! arithmetic written here in f64 and never read off the implementation.
//! Moved out of `precision_clipboard_boundaries.rs` so the
//! clipboard's promotion tests share it.

use doctored_core::icc::IccProfile;

pub fn be32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

pub fn tag<'a>(bytes: &'a [u8], key: &[u8; 4]) -> &'a [u8] {
    for n in 0..be32(bytes, 128) as usize {
        let entry = 132 + n * 12;
        if &bytes[entry..entry + 4] == key {
            let start = be32(bytes, entry + 4) as usize;
            return &bytes[start..start + be32(bytes, entry + 8) as usize];
        }
    }
    panic!("missing tag");
}

/// The profile's DATA — its three colorant columns and its `curv` table —
/// with every piece of arithmetic over it written here.
pub struct Chain {
    pub to_pcs: [[f64; 3]; 3],
    pub curve: Vec<f64>,
}

impl Chain {
    pub fn of(profile: &IccProfile) -> Self {
        let bytes = profile.bytes();
        let columns: [[f64; 3]; 3] = [b"rXYZ", b"gXYZ", b"bXYZ"].map(|key| {
            let data = tag(bytes, key);
            std::array::from_fn(|r| f64::from(be32(data, 8 + r * 4) as i32) / 65536.0)
        });
        let trc = tag(bytes, b"rTRC");
        assert_eq!(&trc[..4], b"curv", "the built-in TRC is a sampled curv");
        let n = be32(trc, 8) as usize;
        assert!(n >= 2);
        let curve = (0..n)
            .map(|i| f64::from(u16::from_be_bytes([trc[12 + 2 * i], trc[13 + 2 * i]])) / 65535.0)
            .collect();
        Chain {
            to_pcs: std::array::from_fn(|r| std::array::from_fn(|c| columns[c][r])),
            curve,
        }
    }

    /// Encoded → linear: linear interpolation of the table.
    pub fn decode(&self, x: f64) -> f64 {
        let n = self.curve.len();
        let position = x.clamp(0.0, 1.0) * (n - 1) as f64;
        let i = (position.floor() as usize).min(n - 2);
        let t = position - i as f64;
        self.curve[i] + (self.curve[i + 1] - self.curve[i]) * t
    }

    /// Linear → encoded: the exact inverse of that piecewise-linear curve.
    pub fn encode(&self, y: f64) -> f64 {
        let n = self.curve.len();
        let y = y.clamp(0.0, 1.0);
        if y <= self.curve[0] {
            return 0.0;
        }
        if y >= self.curve[n - 1] {
            return 1.0;
        }
        let i = self
            .curve
            .partition_point(|&v| v <= y)
            .saturating_sub(1)
            .min(n - 2);
        let (a, b) = (self.curve[i], self.curve[i + 1]);
        let t = if b > a { (y - a) / (b - a) } else { 0.0 };
        (i as f64 + t) / (n - 1) as f64
    }
}

/// Gauss-Jordan elimination.
pub fn inverse(matrix: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut rows: [[f64; 6]; 3] = std::array::from_fn(|r| {
        std::array::from_fn(|c| {
            if c < 3 {
                matrix[r][c]
            } else {
                f64::from(u8::from(c - 3 == r))
            }
        })
    });
    for col in 0..3 {
        let pivot = (col..3)
            .max_by(|&a, &b| rows[a][col].abs().total_cmp(&rows[b][col].abs()))
            .unwrap();
        rows.swap(col, pivot);
        let scale = rows[col][col];
        for value in &mut rows[col] {
            *value /= scale;
        }
        for row in 0..3 {
            if row == col {
                continue;
            }
            let scale = rows[row][col];
            let pivot_row = rows[col];
            for (value, pivot_value) in rows[row].iter_mut().zip(pivot_row) {
                *value -= scale * pivot_value;
            }
        }
    }
    std::array::from_fn(|r| std::array::from_fn(|c| rows[r][c + 3]))
}

pub fn apply(matrix: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|r| (0..3).map(|c| matrix[r][c] * v[c]).sum())
}
