//! Bitmap mode's 1-bit math (#1 Bitmap phases 1a and 1b, KERNEL): the
//! five methods that turn an 8-bit gray row into black and white, the
//! packed plane's layout and Grayscale's Size Ratio block average
//! (`designs/bitmap-mode.md`). `bitmap_convert` is the adapter that
//! reads a document's pixels into these functions and builds the document.
//! Phase 2b's Photoshop take (`scripts/photoshop-probe/bitmap_take.py`,
//! take1 on a confirm scene, every gate declared before it) CONFIRMED 50%
//! Threshold at 128, PASSED the Pattern Dither table fitted from phase 1b's
//! smoke (99.20 % of take1's pixels), and found Photoshop's Diffusion Dither
//! in no error-diffusion kernel tried: it varies between sessions, so the
//! shipped [`DIFFUSION`] is the fitted kernel that agreed most (69.9 %),
//! disclosed as not matching. Halftone Screen ships its declared default.
//! A gray `g` is an 8-bit code and a bit 1 is white.
//!
//! * **The plane** is packed: `ceil(width / 8)` bytes a row ([`stride`]),
//!   the leftmost pixel in the most significant bit, 1 white and 0 black,
//!   the pad bits past the width 0 ([`pack_row`], [`is_white`]).
//! * **50% Threshold** makes a pixel white iff `g ≥` [`THRESHOLD_WHITE`]
//!   (128, the middle gray level); take1 agreed on every pixel.
//! * **Pattern Dither** reads the 8 × 8 threshold table
//!   [`PATTERN_THRESHOLDS`] at `T[y mod 8][x mod 8]` from the canvas origin:
//!   white iff `g ≥ T`. The table is the take's PT8-MID: at each position the
//!   midpoint, rounded down, of the gray interval phase 1b's smoke left
//!   between its darkest white and its lightest black. It is not Bayer's
//!   order; [`bayer`] and [`PATTERN_ORDER`] stay for `indexed_dither`.
//! * **Diffusion Dither** is the error diffusion [`DIFFUSION`] declares
//!   ([`DiffusionRule`]: Sierra Lite's three taps `2/4` ahead, `1/4` below
//!   behind and `1/4` below, serpentine, unclamped, white iff `w ≥ 128`),
//!   the take's best kernel, disclosed as not matching Photoshop's (69.9 %;
//!   Photoshop's differs between sessions). Floyd–Steinberg's master copy is
//!   `indexed_palette::Diffusion`; this is a different kernel and scan, not a
//!   second copy of it.
//! * **Halftone Screen** ([`Screen`], under the [`HALFTONE`] rule): pixel
//!   (x, y) sits at `X = (x+½)/ppi.x`, `Y = (y+½)/ppi.y` inches, y down; with
//!   `φ = turn·angle·π/180`, the screen coordinates are `u = f(X cos φ +
//!   Y sin φ)` and `v = f(−X sin φ + Y cos φ)` (each `+½` when `centred`),
//!   and the cell coordinates `s = 2·frac(u) − 1`, `t = 2·frac(v) − 1`. The
//!   pixel is white iff `g > 0.5 + 254·F`. Under AREA, `F` is the fraction
//!   of a [`SPOT_SAMPLES`]² grid of cell points whose spot value is below
//!   the pixel's (the spot function read as an order, as PostScript
//!   halftones read it); under VALUE, the spot value scaled from its
//!   [`SpotShape::range`] to 0…1. A high spot value is a high threshold, so
//!   a light gray leaves a small black dot where the spot peaks.
//! * **Custom Pattern** tiles a pattern's gray ([`ThresholdTile`]) from the
//!   canvas origin as the threshold: white iff `g > gray[y mod h][x mod w]`
//!   (unmeasured, Photoshop's presets being its own).
//! * **Size Ratio** (Bitmap → Grayscale) averages each `ratio × ratio`
//!   block, the partial blocks at the right and bottom edges over their
//!   pixels inside the canvas only: `(510·W + n) / (2n)`, which is `255·W/n`
//!   rounded half up in integers (W whites, n pixels).

use std::sync::Arc;

/// The least 8-bit gray 50% Threshold makes white (module doc).
pub(crate) const THRESHOLD_WHITE: u8 = 128;

/// Pattern Dither's threshold table (module doc), rows `y mod 8`, columns
/// `x mod 8`: white iff `g ≥ T[y mod 8][x mod 8]` (256 would be never
/// white). Phase 2b's PT8-MID (`bitmap_take.py`'s fit1, PASS on take1 at
/// 16253 of 16384 pixels): each entry is `⌊(lo + hi)/2⌋` of the interval
/// phase 1b's smoke left at that position, its widths at most 7 grays.
pub(crate) const PATTERN_THRESHOLDS: [[u16; 8]; 8] = [
    [252, 124, 222, 95, 240, 121, 210, 83],
    [124, 221, 94, 247, 120, 209, 90, 59],
    [220, 93, 246, 119, 208, 89, 66, 187],
    [92, 245, 118, 215, 88, 65, 186, 35],
    [244, 117, 214, 87, 64, 185, 34, 155],
    [116, 213, 86, 63, 192, 33, 154, 51],
    [212, 85, 62, 191, 32, 153, 58, 179],
    [84, 61, 190, 31, 160, 57, 178, 27],
];

/// An error diffusion's declared rule (module doc): each pixel's `w = g +`
/// its carried error (clamped to 0…255 when `clamp`) is white iff `w ≥
/// white_at`; its error `w − {0, 255}` goes to each tap `(dx, dy, weight)`
/// as `e·weight/divisor`, `dx` pixels ahead along the scan and `dy` rows
/// below, dropped past the edges. `serpentine` walks odd rows right to left
/// with the taps mirrored. Every product and sum is f64 in that order, so
/// `bitmap_take.py`'s `diffuse` reproduces it bit for bit. Floyd–Steinberg
/// is `indexed_palette::Diffusion`'s (the master copy, Indexed Color's).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DiffusionRule {
    pub(crate) divisor: u32,
    pub(crate) taps: &'static [(i32, u32, u32)],
    pub(crate) serpentine: bool,
    pub(crate) clamp: bool,
    pub(crate) white_at: f64,
}

impl DiffusionRule {
    /// The error rows the rule keeps: the current row and every row a tap
    /// reaches below it.
    fn rows(&self) -> usize {
        self.taps
            .iter()
            .map(|tap| tap.1 as usize)
            .max()
            .unwrap_or(0)
            + 1
    }

    /// The error rows' bytes for rows `width` wide (one f64 a pixel a row).
    pub(crate) fn scratch_bytes(&self, width: u32) -> u64 {
        self.rows() as u64 * u64::from(width) * 8
    }
}

/// Diffusion Dither's shipped rule (module doc): phase 2b's MISS-FITTED
/// kernel, LITE-SERP-FREE-128 (Sierra Lite, serpentine, unclamped, white at
/// 128), which agreed with take1's Photoshop bits on 11460 of 16384 pixels
/// (69.9 %) against Floyd–Steinberg's 11250. Photoshop's own diffusion is
/// in no kernel tried and changes between sessions, so this is disclosed as
/// not matching it.
pub(crate) const DIFFUSION: DiffusionRule = DiffusionRule {
    divisor: 4,
    taps: &[(1, 0, 2), (-1, 1, 1), (0, 1, 1)],
    serpentine: true,
    clamp: false,
    white_at: 128.0,
};

/// The one rule that puts an edited pixel back on the plane (#1 Bitmap
/// phase 2a-i, `bitmap_edit`): white iff `gray·alpha + bg·(255 − alpha) ≥
/// 128·255`, `bg` 255 when `background_white`, else 0. The pixel's gray is
/// laid over the background colour at its own alpha, then read by
/// 50% Threshold's boundary ([`THRESHOLD_WHITE`], the middle gray),
/// compared in integers so no rounding moves the boundary. An opaque pixel
/// is 50% Threshold itself; a clear one is the background. A paint kernel's
/// antialiasing is thresholded, not turned off: an edge pixel painted at
/// coverage c holds `c·ink + (1 − c)·backdrop`, so it turns where its
/// coverage reaches about 50 % and a soft brush lands hard-edged.
pub(crate) fn white_over(gray: u8, alpha: u8, background_white: bool) -> bool {
    let bg = if background_white { 255u32 } else { 0 };
    let alpha = u32::from(alpha);
    u32::from(gray) * alpha + bg * (255 - alpha) >= u32::from(THRESHOLD_WHITE) * 255
}

/// The Bayer order `indexed_dither`'s Pattern reads (phase 1b's declared
/// Pattern Dither, which phase 2b's take replaced with
/// [`PATTERN_THRESHOLDS`] in Bitmap mode).
pub(crate) const PATTERN_ORDER: u32 = 8;

/// Halftone Screen's shipped rule (module doc): AREA order, the screen
/// turned counterclockwise on screen by the angle, cells at the corner.
pub(crate) const HALFTONE: HalftoneRule = HalftoneRule {
    area: true,
    turn: -1.0,
    centred: false,
};

/// The AREA order's grid side: 256 × 256 cell points, sorted once a
/// conversion (512 KiB of f64).
pub(crate) const SPOT_SAMPLES: u32 = 256;

/// Halftone Screen's frequency range in lines per inch.
pub(crate) const MIN_FREQUENCY: f64 = 1.0;
pub(crate) const MAX_FREQUENCY: f64 = 999.999;

/// The largest Size Ratio Bitmap → Grayscale takes.
pub(crate) const MAX_SIZE_RATIO: u32 = 16;

/// How a halftone cell's spot becomes a threshold (module doc): `area`
/// (AREA order, else VALUE), `turn` (−1 counterclockwise on screen, +1
/// clockwise) and `centred` (cells offset by half a cell).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HalftoneRule {
    pub(crate) area: bool,
    pub(crate) turn: f64,
    pub(crate) centred: bool,
}

/// Halftone Screen's six shapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SpotShape {
    Round,
    Ellipse,
    Line,
    Square,
    Cross,
    Diamond,
}

impl SpotShape {
    /// The shape of an option name (`round`, `ellipse`, `line`, `square`,
    /// `cross`, `diamond`).
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "round" => SpotShape::Round,
            "ellipse" => SpotShape::Ellipse,
            "line" => SpotShape::Line,
            "square" => SpotShape::Square,
            "cross" => SpotShape::Cross,
            "diamond" => SpotShape::Diamond,
            _ => return None,
        })
    }

    /// The spot value at cell point (s, t), each in −1…1: Adobe's predefined
    /// spot functions (ISO 32000-1:2008 §10.5.3, Table 128: SimpleDot for
    /// Round, EllipseC's 0.75 aspect for Ellipse, Line, Square, Cross and
    /// Diamond), with `a = |s|`, `b = |t|`.
    pub(crate) fn spot(self, s: f64, t: f64) -> f64 {
        let (a, b) = (s.abs(), t.abs());
        let outer = (a - 1.0).powi(2) + (b - 1.0).powi(2) - 1.0;
        match self {
            SpotShape::Round => {
                if a + b <= 1.0 {
                    1.0 - (s * s + t * t)
                } else {
                    outer
                }
            }
            SpotShape::Ellipse => {
                let w = 3.0 * a + 4.0 * b - 3.0;
                if w < 0.0 {
                    1.0 - (s * s + (b / 0.75).powi(2)) / 4.0
                } else if w > 1.0 {
                    ((1.0 - a).powi(2) + ((1.0 - b) / 0.75).powi(2)) / 4.0 - 1.0
                } else {
                    0.5 - w
                }
            }
            SpotShape::Line => -b,
            SpotShape::Square => -a.max(b),
            SpotShape::Cross => -a.min(b),
            SpotShape::Diamond => {
                if a + b <= 0.75 {
                    1.0 - (s * s + t * t)
                } else if a + b <= 1.23 {
                    1.0 - (0.85 * a + b)
                } else {
                    outer
                }
            }
        }
    }

    /// The spot values' range over the cell, VALUE's scale.
    pub(crate) fn range(self) -> (f64, f64) {
        match self {
            SpotShape::Round | SpotShape::Ellipse | SpotShape::Diamond => (-1.0, 1.0),
            SpotShape::Line | SpotShape::Square | SpotShape::Cross => (-1.0, 0.0),
        }
    }
}

/// One Halftone Screen: `frequency` lines per inch, `angle` degrees, the
/// spot `shape` and the output resolution `ppi` (x, y) it is sampled at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Screen {
    pub(crate) frequency: f64,
    pub(crate) angle: f64,
    pub(crate) shape: SpotShape,
    pub(crate) ppi: (f64, f64),
}

/// Custom Pattern's threshold map: one 8-bit gray a texel, row-major.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ThresholdTile {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) gray: Arc<[u8]>,
}

/// The five Bitmap methods.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Method {
    Threshold,
    Pattern,
    Diffusion,
    Halftone(Screen),
    Custom(ThresholdTile),
}

impl Method {
    /// The method's own scratch for a row `width` wide: Diffusion's error
    /// rows ([`DiffusionRule::scratch_bytes`]); Halftone's sorted spot grid;
    /// Custom's tile; Threshold and Pattern none.
    pub(crate) fn scratch_bytes(&self, width: u32) -> u64 {
        match self {
            Method::Threshold | Method::Pattern => 0,
            Method::Diffusion => DIFFUSION.scratch_bytes(width),
            Method::Halftone(_) => u64::from(SPOT_SAMPLES * SPOT_SAMPLES) * 8,
            Method::Custom(tile) => u64::from(tile.width) * u64::from(tile.height),
        }
    }
}

/// The Bayer matrix of `order` (a power of two) at (x, y), each taken mod
/// `order` (module doc's recursion).
pub(crate) fn bayer(order: u32, x: u32, y: u32) -> u32 {
    if order <= 1 {
        return 0;
    }
    let half = order / 2;
    let (x, y) = (x % order, y % order);
    let quadrant = match (x >= half, y >= half) {
        (false, false) => 0,
        (true, false) => 2,
        (false, true) => 3,
        (true, true) => 1,
    };
    4 * bayer(half, x % half, y % half) + quadrant
}

/// One method over an image fed row by row, top to bottom.
pub(crate) struct Binarizer {
    method: Method,
    /// Diffusion's error rows ([`DiffusionRule::rows`] of them, the current
    /// row first).
    errors: Vec<Vec<f64>>,
    /// Halftone's AREA order: every grid point's spot value, ascending.
    spots: Vec<f64>,
}

impl Binarizer {
    /// A binarizer for rows `width` wide; Diffusion allocates its error
    /// rows and Halftone sorts its spot grid.
    pub(crate) fn new(method: Method, width: u32) -> Result<Self, String> {
        let errors = match method {
            Method::Diffusion => error_rows(DIFFUSION.rows(), width as usize)?,
            _ => Vec::new(),
        };
        let spots = match &method {
            Method::Halftone(screen) if HALFTONE.area => spot_order(screen.shape)?,
            _ => Vec::new(),
        };
        Ok(Self {
            method,
            errors,
            spots,
        })
    }

    /// Row `y` of 8-bit grays into `out`, one byte a pixel: 1 white, 0
    /// black.
    pub(crate) fn row(&mut self, y: u32, gray: &[u8], out: &mut [u8]) {
        let Self {
            method,
            errors,
            spots,
        } = self;
        let pixels = out.iter_mut().zip(gray).enumerate();
        match &*method {
            Method::Diffusion => diffuse_row(&DIFFUSION, errors, y, gray, out),
            Method::Pattern => {
                for (x, (slot, &g)) in pixels {
                    let t = PATTERN_THRESHOLDS[y as usize % 8][x % 8];
                    *slot = u8::from(u16::from(g) >= t);
                }
            }
            Method::Halftone(screen) => {
                let phi = HALFTONE.turn * screen.angle.to_radians();
                // `sin` and `cos` apart, not `sin_cos`: the take's runner and
                // the drive reproduce the rule with the platform's own `sin`
                // and `cos`, bit for bit.
                let (sin, cos) = (phi.sin(), phi.cos());
                let offset = if HALFTONE.centred { 0.5 } else { 0.0 };
                let yy = (f64::from(y) + 0.5) / screen.ppi.1;
                for (x, (slot, &g)) in pixels {
                    let xx = (x as f64 + 0.5) / screen.ppi.0;
                    let u = screen.frequency * (xx * cos + yy * sin) + offset;
                    let v = screen.frequency * (-xx * sin + yy * cos) + offset;
                    let s = 2.0 * (u - u.floor()) - 1.0;
                    let t = 2.0 * (v - v.floor()) - 1.0;
                    let spot = screen.shape.spot(s, t);
                    let fraction = if HALFTONE.area {
                        spots.partition_point(|&p| p < spot) as f64 / spots.len().max(1) as f64
                    } else {
                        let (lo, hi) = screen.shape.range();
                        ((spot - lo) / (hi - lo)).clamp(0.0, 1.0)
                    };
                    *slot = u8::from(f64::from(g) > 0.5 + 254.0 * fraction);
                }
            }
            Method::Custom(tile) => {
                let (w, h) = (tile.width.max(1), tile.height.max(1));
                let line = (y % h) as usize * w as usize;
                for (x, (slot, &g)) in pixels {
                    let threshold = tile.gray.get(line + (x as u32 % w) as usize);
                    *slot = u8::from(threshold.is_some_and(|&t| g > t));
                }
            }
            Method::Threshold => {
                for (_, (slot, &g)) in pixels {
                    *slot = u8::from(g >= THRESHOLD_WHITE);
                }
            }
        }
    }
}

/// `rows` zeroed error rows `width` wide, allocated without aborting.
fn error_rows(rows: usize, width: usize) -> Result<Vec<Vec<f64>>, String> {
    let refused = || "could not allocate the diffusion error rows".to_string();
    let mut out = Vec::new();
    out.try_reserve_exact(rows).map_err(|_| refused())?;
    for _ in 0..rows {
        let mut row = Vec::new();
        row.try_reserve_exact(width).map_err(|_| refused())?;
        row.resize(width, 0.0);
        out.push(row);
    }
    Ok(out)
}

/// Row `y` of `rule` (module doc, [`DiffusionRule`]) into `out`: `errors[0]`
/// is this row's carried error and `errors[dy]` the row `dy` below; after
/// the row they move up one and the last starts at zero. Rows are fed from
/// `y` 0, so a serpentine rule walks odd `y` right to left.
fn diffuse_row(rule: &DiffusionRule, errors: &mut [Vec<f64>], y: u32, gray: &[u8], out: &mut [u8]) {
    let width = gray.len().min(out.len());
    let back = rule.serpentine && y % 2 == 1;
    let ahead: i64 = if back { -1 } else { 1 };
    for step in 0..width {
        let x = if back { width - 1 - step } else { step };
        let carried = errors.first().and_then(|row| row.get(x)).copied();
        let mut w = f64::from(gray[x]) + carried.unwrap_or(0.0);
        if rule.clamp {
            w = w.clamp(0.0, 255.0);
        }
        let white = w >= rule.white_at;
        out[x] = u8::from(white);
        let e = w - if white { 255.0 } else { 0.0 };
        for &(dx, dy, weight) in rule.taps {
            let px = x as i64 + ahead * i64::from(dx);
            if px < 0 || px as usize >= width {
                continue;
            }
            if let Some(slot) = errors
                .get_mut(dy as usize)
                .and_then(|row| row.get_mut(px as usize))
            {
                *slot += e * f64::from(weight) / f64::from(rule.divisor);
            }
        }
    }
    errors.rotate_left(1);
    if let Some(last) = errors.last_mut() {
        last.iter_mut().for_each(|v| *v = 0.0);
    }
}

/// The AREA order of `shape`: its spot value at every point
/// `S_k = (k + ½)/128 − 1` of the [`SPOT_SAMPLES`]² grid, ascending.
fn spot_order(shape: SpotShape) -> Result<Vec<f64>, String> {
    let n = SPOT_SAMPLES as usize;
    let mut spots = Vec::new();
    spots
        .try_reserve_exact(n * n)
        .map_err(|_| "could not allocate the halftone spot order".to_string())?;
    let sample = |k: usize| (k as f64 + 0.5) / (f64::from(SPOT_SAMPLES) / 2.0) - 1.0;
    for i in 0..n {
        for j in 0..n {
            spots.push(shape.spot(sample(i), sample(j)));
        }
    }
    spots.sort_by(f64::total_cmp);
    Ok(spots)
}

/// The packed plane's bytes a row: `ceil(width / 8)`.
pub(crate) fn stride(width: u32) -> usize {
    (width as usize).div_ceil(8)
}

/// Packs `bits` (one byte a pixel, nonzero white) into `out`, MSB first,
/// the pad bits 0.
pub(crate) fn pack_row(bits: &[u8], out: &mut [u8]) {
    for (byte, chunk) in out.iter_mut().zip(bits.chunks(8)) {
        *byte = chunk
            .iter()
            .enumerate()
            .fold(0u8, |acc, (i, &bit)| acc | (u8::from(bit != 0) << (7 - i)));
    }
}

/// Whether pixel `x` of a packed row is white.
pub(crate) fn is_white(packed_row: &[u8], x: usize) -> bool {
    packed_row
        .get(x / 8)
        .is_some_and(|byte| byte & (0x80 >> (x % 8)) != 0)
}

/// The Grayscale canvas of a `width × height` plane at `ratio`:
/// `(ceil(width / ratio), ceil(height / ratio))`.
pub(crate) fn ratio_dimensions(width: u32, height: u32, ratio: u32) -> (u32, u32) {
    let ratio = ratio.max(1);
    (width.div_ceil(ratio), height.div_ceil(ratio))
}

/// Output row `out_y` of the Size Ratio average (module doc) of a packed
/// `width × height` plane: one 8-bit gray an output pixel into `out`.
pub(crate) fn block_average_row(
    packed: &[u8],
    width: u32,
    height: u32,
    ratio: u32,
    out_y: u32,
    out: &mut [u8],
) {
    let ratio = ratio.max(1);
    let line = stride(width);
    let y0 = out_y.saturating_mul(ratio).min(height);
    let y1 = y0.saturating_add(ratio).min(height);
    for (ox, slot) in out.iter_mut().enumerate() {
        let x0 = (ox as u32).saturating_mul(ratio).min(width);
        let x1 = x0.saturating_add(ratio).min(width);
        let n = u64::from(x1 - x0) * u64::from(y1 - y0);
        if n == 0 {
            *slot = 0;
            continue;
        }
        let mut whites = 0u64;
        for y in y0..y1 {
            let row = &packed[y as usize * line..(y as usize + 1) * line];
            for x in x0..x1 {
                whites += u64::from(is_white(row, x as usize));
            }
        }
        *slot = ((510 * whites + n) / (2 * n)) as u8;
    }
}
