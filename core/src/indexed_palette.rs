//! Indexed Color's palette math (#1 Indexed phase 1a, KERNEL): the
//! deterministic palettes, the forced colours, Transparency and Matte, the
//! nearest-entry map and Floyd–Steinberg error diffusion, all on 8-bit codes
//! (`designs/indexed-mode.md`). `indexed_convert` is the adapter that
//! reads a document's pixels into these functions and writes the index plane.
//!
//! * **Web** is the 216-colour cube, entry `36r + 6g + b` at level `51i`.
//! * **Uniform** is the `k`-level cube (`cube`: levels `round(255i/(k−1))`,
//!   r slowest), `k` the largest in 2..=6 whose cube, joined with the forced
//!   colours, fits the colour budget (`uniform`); the forced colours come first.
//!   Forced Custom (phase 3c) is the options' `forced_table` in order, a
//!   repeated colour dropped (`Forced::colors`).
//! * **System (Mac OS)** and **System (Windows)** are the platforms' fixed
//!   256-entry tables, typed out in `system_mac` and `system_windows`.
//! * **Exact** is the image's distinct opaque colours in first-occurrence
//!   raster order (`ExactColors`), refused past the budget.
//! * **Adaptive** (#1 phase 1b) is median cut after Heckbert, "Color
//!   Image Quantization for Frame Buffer Display" (SIGGRAPH 1982), made
//!   deterministic: the opaque colours are counted in one 2²⁴ histogram
//!   (`ColorHistogram`, [`ADAPTIVE_BYTES`]) and the distinct ones start as
//!   one box in key order. While there are fewer boxes than the budget, the
//!   box of ≥ 2 colours with the largest total count (ties to the lowest
//!   box) is sorted on its widest channel (max − min; ties r, g, b; then by
//!   the packed key) and cut where the lower part first holds half the
//!   count; the lower part keeps the box's place and the upper is appended.
//!   Each entry is its box's count-weighted mean, rounded half up. It is the
//!   published algorithm. The forced colours lead, and cut
//!   entries equal to one of them are dropped.
//!
//! The nearest entry is the least squared RGB distance, ties to the lowest
//! index, never the transparent entry, memoized per 24-bit colour in one
//! `u16` table of [`MEMO_BYTES`] (`NearestMap`). Diffusion is classic
//! Floyd–Steinberg (1976) in raster order with its error scaled by Amount
//! (`Diffusion`); no Photoshop take measured it (phase 3's). Pattern and
//! Noise are `indexed_dither`'s.

/// One 8-bit RGB colour.
pub(crate) type Rgb = [u8; 3];

/// The fewest and most entries an Indexed Color palette holds.
pub(crate) const MIN_COLORS: usize = 2;
pub(crate) const MAX_COLORS: usize = 256;

/// The nearest-entry memo: one `u16` per 24-bit colour (32 MiB), charged
/// exactly by every plan that builds a [`NearestMap`].
pub(crate) const MEMO_BYTES: u64 = (1 << 24) * 2;

/// The Exact palette's seen-colour bit set: one bit per 24-bit colour (2 MiB).
pub(crate) const EXACT_BYTES: u64 = (1 << 24) / 8;

/// The memo's "not yet computed" value; a palette holds at most 256 entries.
const UNKNOWN: u16 = u16::MAX;

/// The Mode ▸ Indexed Color palettes phases 1a, 1b and 1c ship (1c's
/// Custom takes its table from the options, `indexed_act::custom`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Palette {
    Exact,
    Web,
    Uniform,
    Adaptive,
    SystemMac,
    SystemWindows,
    Custom,
}

impl Palette {
    /// The options' wire name (`indexed_convert`).
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "exact" => Palette::Exact,
            "web" => Palette::Web,
            "uniform" => Palette::Uniform,
            "adaptive" => Palette::Adaptive,
            "system_mac" => Palette::SystemMac,
            "system_windows" => Palette::SystemWindows,
            "custom" => Palette::Custom,
            _ => return None,
        })
    }

    /// Whether the palette is one of the fixed 256-entry System tables, whose
    /// entry 255 becomes the transparent index when the image needs one.
    pub(crate) fn is_system(self) -> bool {
        matches!(self, Palette::SystemMac | Palette::SystemWindows)
    }
}

/// The Forced colours (the Uniform and Adaptive palettes'). `Custom` (phase
/// 3c) is the options' `forced_table`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Forced {
    None,
    BlackWhite,
    Primaries,
    Web,
    Custom(Vec<Rgb>),
}

impl Forced {
    /// The four named sets; `custom` needs its table and is built by the
    /// caller (`indexed_convert`).
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "none" => Forced::None,
            "black_white" => Forced::BlackWhite,
            "primaries" => Forced::Primaries,
            "web" => Forced::Web,
            _ => return None,
        })
    }

    /// The forced colours, in the order they lead the palette: Black and
    /// White; the Primaries order (red, green, blue, cyan,
    /// magenta, yellow, black, white); the Web cube; Custom's entries in
    /// order, a repeated colour dropped (the first kept).
    pub(crate) fn colors(&self) -> Vec<Rgb> {
        match self {
            Forced::Custom(table) => {
                let mut out: Vec<Rgb> = Vec::with_capacity(table.len());
                for c in table {
                    if !out.contains(c) {
                        out.push(*c);
                    }
                }
                out
            }
            Forced::None => Vec::new(),
            Forced::BlackWhite => vec![[0, 0, 0], [255, 255, 255]],
            Forced::Primaries => vec![
                [255, 0, 0],
                [0, 255, 0],
                [0, 0, 255],
                [0, 255, 255],
                [255, 0, 255],
                [255, 255, 0],
                [0, 0, 0],
                [255, 255, 255],
            ],
            Forced::Web => web(),
        }
    }
}

/// The 216-colour Web palette: entry `36r + 6g + b`, each level `51i`.
pub(crate) fn web() -> Vec<Rgb> {
    cube(6)
}

/// The `k`-level colour cube, `k` in 2..=6: levels `round(255i/(k−1))`,
/// entry `(r·k + g)·k + b` (r slowest). `cube(6)` is the Web palette.
pub(crate) fn cube(k: usize) -> Vec<Rgb> {
    let k = k.clamp(2, 6);
    // Integer round-half-up of 255i/(k−1): exact, no float involved.
    let level = |i: usize| ((255 * i * 2 + (k - 1)) / (2 * (k - 1))) as u8;
    let mut out = Vec::with_capacity(k * k * k);
    for r in 0..k {
        for g in 0..k {
            for b in 0..k {
                out.push([level(r), level(g), level(b)]);
            }
        }
    }
    out
}

/// The forced colours followed by `cube` minus them, in cube order.
fn joined(forced: &[Rgb], cube: Vec<Rgb>) -> Vec<Rgb> {
    let mut out = forced.to_vec();
    out.extend(cube.into_iter().filter(|c| !forced.contains(c)));
    out
}

/// The Uniform palette (module doc): the largest `k` in 2..=6 with
/// `|forced ∪ cube_k| + t ≤ colors`, `t` 1 when a transparent entry will be
/// appended. No such `k` refuses, naming `N = |forced ∪ cube_2| + t`.
pub(crate) fn uniform(
    colors: usize,
    forced: &Forced,
    transparent: bool,
) -> Result<Vec<Rgb>, String> {
    let forced = forced.colors();
    let t = usize::from(transparent);
    for k in (2..=6).rev() {
        let palette = joined(&forced, cube(k));
        if palette.len() + t <= colors {
            return Ok(palette);
        }
    }
    let needed = joined(&forced, cube(2)).len() + t;
    Err(format!(
        "The Uniform palette needs at least {needed} colours with these options."
    ))
}

/// System (Mac OS): the 6-level cube FF→00, r slowest, without 000000
/// (entries 0–214); the EE, DD, BB, AA, 88, 77, 55, 44, 22 and 11 ramps of
/// red, green, blue and gray (215–254); 000000 (255).
pub(crate) fn system_mac() -> Vec<Rgb> {
    const LEVELS: [u8; 6] = [0xFF, 0xCC, 0x99, 0x66, 0x33, 0x00];
    const RAMP: [u8; 10] = [0xEE, 0xDD, 0xBB, 0xAA, 0x88, 0x77, 0x55, 0x44, 0x22, 0x11];
    let mut out = Vec::with_capacity(MAX_COLORS);
    for r in LEVELS {
        for g in LEVELS {
            for b in LEVELS {
                if [r, g, b] != [0, 0, 0] {
                    out.push([r, g, b]);
                }
            }
        }
    }
    out.extend(RAMP.iter().map(|&v| [v, 0, 0]));
    out.extend(RAMP.iter().map(|&v| [0, v, 0]));
    out.extend(RAMP.iter().map(|&v| [0, 0, v]));
    out.extend(RAMP.iter().map(|&v| [v, v, v]));
    out.push([0, 0, 0]);
    out
}

/// System (Windows): the ten low Win32 static colours (0–9), the 6-level
/// cube ascending, r slowest (10–225), black (226–245), the ten high static
/// colours (246–255).
pub(crate) fn system_windows() -> Vec<Rgb> {
    const LOW: [Rgb; 10] = [
        [0x00, 0x00, 0x00],
        [0x80, 0x00, 0x00],
        [0x00, 0x80, 0x00],
        [0x80, 0x80, 0x00],
        [0x00, 0x00, 0x80],
        [0x80, 0x00, 0x80],
        [0x00, 0x80, 0x80],
        [0xC0, 0xC0, 0xC0],
        [0xC0, 0xDC, 0xC0],
        [0xA6, 0xCA, 0xF0],
    ];
    const HIGH: [Rgb; 10] = [
        [0xFF, 0xFB, 0xF0],
        [0xA0, 0xA0, 0xA4],
        [0x80, 0x80, 0x80],
        [0xFF, 0x00, 0x00],
        [0x00, 0xFF, 0x00],
        [0xFF, 0xFF, 0x00],
        [0x00, 0x00, 0xFF],
        [0xFF, 0x00, 0xFF],
        [0x00, 0xFF, 0xFF],
        [0xFF, 0xFF, 0xFF],
    ];
    let mut out = Vec::with_capacity(MAX_COLORS);
    out.extend_from_slice(&LOW);
    out.extend(cube(6));
    out.extend(std::iter::repeat_n([0, 0, 0], 20));
    out.extend_from_slice(&HIGH);
    out
}

/// The Exact palette's refusal past `budget` colours.
pub(crate) fn exact_refusal(budget: usize) -> String {
    format!("The Exact palette holds at most {budget} colours; this image has more.")
}

/// The Exact palette's collector: distinct colours in first-occurrence order
/// over one bit set of [`EXACT_BYTES`].
pub(crate) struct ExactColors {
    seen: Vec<u64>,
    colors: Vec<Rgb>,
    budget: usize,
    over: bool,
}

impl ExactColors {
    pub(crate) fn new(budget: usize) -> Result<Self, String> {
        let words = (1usize << 24) / 64;
        let mut seen = crate::precision_image::reserved_vec(words)?;
        seen.resize(words, 0u64);
        Ok(Self {
            seen,
            colors: Vec::new(),
            budget,
            over: false,
        })
    }

    /// Records one opaque colour; false once the budget is exceeded.
    pub(crate) fn push(&mut self, rgb: Rgb) -> bool {
        if self.over {
            return false;
        }
        let key = key(rgb);
        let (word, bit) = (key / 64, key % 64);
        if self.seen[word] & (1 << bit) == 0 {
            self.seen[word] |= 1 << bit;
            if self.colors.len() == self.budget {
                self.over = true;
                return false;
            }
            self.colors.push(rgb);
        }
        true
    }

    /// The palette, padded by [`pad_to_min`], or the refusal past `budget`
    /// (at most the budget the collector was made with). A caller that
    /// appends a transparent entry passes a budget one smaller, so the
    /// padded palette plus that entry still fits the colour count (2 colours,
    /// one opaque and one transparent, is the opaque colour and the
    /// transparent entry).
    pub(crate) fn finish(self, budget: usize) -> Result<Vec<Rgb>, String> {
        if self.over || self.colors.len() > budget {
            return Err(exact_refusal(budget));
        }
        let mut colors = self.colors;
        pad_to_min(&mut colors, budget);
        Ok(colors)
    }
}

/// Pads `colors` to [`MIN_COLORS`] (black, or white once black is in it)
/// but never past `budget`: the Exact and Adaptive palettes' padding.
fn pad_to_min(colors: &mut Vec<Rgb>, budget: usize) {
    while colors.len() < MIN_COLORS.min(budget) {
        let pad = if colors.contains(&[0, 0, 0]) {
            [255, 255, 255]
        } else {
            [0, 0, 0]
        };
        colors.push(pad);
    }
}

/// The Adaptive palette's histogram: one `u32` count per 24-bit colour
/// (64 MiB), charged exactly by the plan.
pub(crate) const ADAPTIVE_BYTES: u64 = (1 << 24) * 4;

/// What one distinct colour costs the median cut: its key and its count.
pub(crate) const ADAPTIVE_COLOR_BYTES: u64 = 8;

/// The Adaptive palette's colour counts (module doc), over [`ADAPTIVE_BYTES`].
pub(crate) struct ColorHistogram {
    counts: Vec<u32>,
}

impl ColorHistogram {
    pub(crate) fn new() -> Result<Self, String> {
        let mut counts = crate::precision_image::reserved_vec(1 << 24)?;
        counts.resize(1 << 24, 0u32);
        Ok(Self { counts })
    }

    /// Counts one opaque colour.
    pub(crate) fn push(&mut self, rgb: Rgb) {
        let slot = &mut self.counts[key(rgb)];
        *slot = slot.saturating_add(1);
    }
}

/// One median-cut box: a run of the distinct colours and its total count.
struct CutBox {
    start: usize,
    end: usize,
    total: u64,
}

/// Median cut after Heckbert (1982), deterministically (module doc): at
/// most `entries` colours, one a box. An empty histogram answers none.
pub(crate) fn median_cut(histogram: &ColorHistogram, entries: usize) -> Result<Vec<Rgb>, String> {
    let distinct = histogram.counts.iter().filter(|&&c| c > 0).count();
    let mut colors: Vec<(u32, u32)> = crate::precision_image::reserved_vec(distinct)?;
    colors.extend(
        histogram
            .counts
            .iter()
            .enumerate()
            .filter(|(_, &c)| c > 0)
            .map(|(k, &c)| (k as u32, c)),
    );
    if colors.is_empty() || entries == 0 {
        return Ok(Vec::new());
    }
    let channel = |k: u32, c: usize| ((k >> (16 - 8 * c)) & 0xFF) as u8;
    let total = |run: &[(u32, u32)]| run.iter().map(|&(_, c)| u64::from(c)).sum::<u64>();
    let mut boxes = vec![CutBox {
        start: 0,
        end: colors.len(),
        total: total(&colors),
    }];
    while boxes.len() < entries {
        // The splittable box of the largest count, ties to the lowest index.
        let mut chosen: Option<usize> = None;
        for (i, b) in boxes.iter().enumerate() {
            if b.end - b.start >= 2 && chosen.is_none_or(|j| b.total > boxes[j].total) {
                chosen = Some(i);
            }
        }
        let Some(i) = chosen else {
            break;
        };
        let (start, end) = (boxes[i].start, boxes[i].end);
        let run = &mut colors[start..end];
        // The axis of the widest range, ties r, g, b.
        let mut axis = 0;
        let mut widest = 0u8;
        for c in 0..3 {
            let lo = run.iter().map(|&(k, _)| channel(k, c)).min().unwrap_or(0);
            let hi = run.iter().map(|&(k, _)| channel(k, c)).max().unwrap_or(0);
            if c == 0 || hi - lo > widest {
                axis = c;
                widest = hi - lo;
            }
        }
        run.sort_unstable_by_key(|&(k, _)| (channel(k, axis), k));
        let whole = boxes[i].total;
        // The smallest k in 1..n with 2·Σ_{i<k} count ≥ total, else n − 1.
        let n = run.len();
        let mut below = 0u64;
        let cut = run[..n - 1]
            .iter()
            .position(|&(_, count)| {
                below += u64::from(count);
                2 * below >= whole
            })
            .map_or(n - 1, |i| i + 1);
        let lower = total(&run[..cut]);
        boxes[i] = CutBox {
            start,
            end: start + cut,
            total: lower,
        };
        boxes.push(CutBox {
            start: start + cut,
            end,
            total: whole - lower,
        });
    }
    Ok(boxes
        .iter()
        .map(|b| {
            let weight = b.total.max(1);
            let mut sums = [0u64; 3];
            for &(k, count) in &colors[b.start..b.end] {
                for (c, sum) in sums.iter_mut().enumerate() {
                    *sum += u64::from(count) * u64::from(channel(k, c));
                }
            }
            sums.map(|s| ((s + weight / 2) / weight) as u8)
        })
        .collect())
}

/// The colours the median cut may make under these options, or the
/// refusal naming `N = |forced| + t + 1`.
pub(crate) fn adaptive_budget(
    colors: usize,
    forced: &Forced,
    transparent: bool,
) -> Result<usize, String> {
    let needed = forced.colors().len() + usize::from(transparent);
    colors
        .checked_sub(needed)
        .filter(|budget| *budget >= 1)
        .ok_or_else(|| {
            format!(
                "The Adaptive palette needs at least {} colours with these options.",
                needed + 1
            )
        })
}

/// The Adaptive palette (module doc): the forced colours, then the median
/// cut of `colors − |forced| − t` entries without those equal to a forced
/// colour, padded by [`pad_to_min`].
pub(crate) fn adaptive(
    histogram: &ColorHistogram,
    colors: usize,
    forced: &Forced,
    transparent: bool,
) -> Result<Vec<Rgb>, String> {
    let budget = adaptive_budget(colors, forced, transparent)?;
    let forced = forced.colors();
    let mut out = forced.clone();
    out.extend(
        median_cut(histogram, budget)?
            .into_iter()
            .filter(|c| !forced.contains(c)),
    );
    pad_to_min(&mut out, colors - usize::from(transparent));
    Ok(out)
}

/// A u16 sample's 8-bit code, `(v + 128) / 257`.
pub(crate) fn code8(v: u16) -> u8 {
    ((u32::from(v) + 128) / 257) as u8
}

/// One straight-alpha 8-bit pixel under Transparency and Matte: `None` is a
/// transparent pixel, else the opaque colour to index. With Transparency and
/// no matte, alpha below 128 is transparent and the rest keeps its colour
/// unmatted; with a matte, alpha 0 is transparent and the rest is composited
/// over the matte. Without Transparency every pixel is composited over the
/// matte (white when none).
pub(crate) fn prepare(rgba: [u8; 4], transparency: bool, matte: Option<Rgb>) -> Option<Rgb> {
    let alpha = rgba[3];
    let over = |m: Rgb| -> Rgb {
        let a = u32::from(alpha);
        [0, 1, 2]
            .map(|c| ((u32::from(rgba[c]) * a + u32::from(m[c]) * (255 - a) + 127) / 255) as u8)
    };
    match (transparency, matte) {
        (true, None) if alpha < 128 => None,
        (true, None) => Some([rgba[0], rgba[1], rgba[2]]),
        (true, Some(_)) if alpha == 0 => None,
        (true, Some(m)) => Some(over(m)),
        (false, m) => Some(over(m.unwrap_or([255, 255, 255]))),
    }
}

fn key(rgb: Rgb) -> usize {
    (usize::from(rgb[0]) << 16) | (usize::from(rgb[1]) << 8) | usize::from(rgb[2])
}

/// The nearest-entry map of one palette (module doc), memoized over
/// [`MEMO_BYTES`].
pub(crate) struct NearestMap {
    palette: Vec<Rgb>,
    transparent: Option<usize>,
    memo: Vec<u16>,
}

impl NearestMap {
    /// `transparent` is never matched. The palette holds 2..=256 entries and
    /// at least one opaque one.
    pub(crate) fn new(palette: Vec<Rgb>, transparent: Option<usize>) -> Result<Self, String> {
        if !(MIN_COLORS..=MAX_COLORS).contains(&palette.len()) {
            return Err("an Indexed Color palette holds 2 to 256 colours".into());
        }
        let mut memo = crate::precision_image::reserved_vec(1 << 24)?;
        memo.resize(1 << 24, UNKNOWN);
        Ok(Self {
            palette,
            transparent,
            memo,
        })
    }

    pub(crate) fn entry(&self, index: u8) -> Rgb {
        self.palette[usize::from(index)]
    }

    /// The nearest opaque entry: least squared RGB distance, ties to the
    /// lowest index.
    pub(crate) fn index(&mut self, rgb: Rgb) -> u8 {
        let key = key(rgb);
        let known = self.memo[key];
        if known != UNKNOWN {
            return known as u8;
        }
        let best = nearest_entry(&self.palette, self.transparent, rgb);
        self.memo[key] = u16::from(best);
        best
    }
}

/// The nearest opaque entry of `palette` to `rgb` with no memo: least
/// squared RGB distance, ties to the lowest index, never `transparent`. The
/// one search [`NearestMap::index`] memoizes; a single lookup (the clear
/// pixel's background entry, `indexed_edge::clear_index`) calls it direct,
/// so it builds no [`MEMO_BYTES`] table.
pub(crate) fn nearest_entry(palette: &[Rgb], transparent: Option<usize>, rgb: Rgb) -> u8 {
    let mut best = (u32::MAX, 0usize);
    for (i, entry) in palette.iter().enumerate() {
        if Some(i) == transparent {
            continue;
        }
        let d: u32 = (0..3)
            .map(|c| {
                let delta = i32::from(entry[c]) - i32::from(rgb[c]);
                (delta * delta) as u32
            })
            .sum();
        if d < best.0 {
            best = (d, i);
        }
    }
    best.1 as u8
}

/// Dither None: each opaque pixel's nearest entry, each transparent pixel
/// `transparent`.
pub(crate) fn plain_row(
    map: &mut NearestMap,
    source: &[Option<Rgb>],
    transparent: u8,
    out: &mut [u8],
) {
    for (pixel, slot) in source.iter().zip(out.iter_mut()) {
        *slot = match pixel {
            Some(rgb) => map.index(*rgb),
            None => transparent,
        };
    }
}

/// Floyd–Steinberg diffusion over one image, fed row by row (module doc).
pub(crate) struct Diffusion {
    amount: f64,
    preserve_exact: bool,
    current: Vec<[f64; 3]>,
    next: Vec<[f64; 3]>,
}

impl Diffusion {
    /// The error rows' bytes for an image `width` wide.
    pub(crate) fn scratch_bytes(width: u32) -> u64 {
        2 * u64::from(width) * 24
    }

    /// `amount` in 1..=100 percent.
    pub(crate) fn new(width: u32, amount: u32, preserve_exact: bool) -> Result<Self, String> {
        let width = width as usize;
        let mut current = crate::precision_image::reserved_vec(width)?;
        current.resize(width, [0.0; 3]);
        let mut next = crate::precision_image::reserved_vec(width)?;
        next.resize(width, [0.0; 3]);
        Ok(Self {
            amount: f64::from(amount.clamp(1, 100)),
            preserve_exact,
            current,
            next,
        })
    }

    /// One row, left to right: `w = clamp(src + e, 0, 255)`, the entry
    /// nearest `round(w)`, and `(w − entry)·amount/100` spread 7/16 right,
    /// 3/16 below-left, 5/16 below, 1/16 below-right, dropped past the
    /// edges. A transparent pixel takes and emits none; under Preserve Exact
    /// a source colour equal to an entry takes that entry and emits none.
    pub(crate) fn row(
        &mut self,
        map: &mut NearestMap,
        source: &[Option<Rgb>],
        transparent: u8,
        out: &mut [u8],
    ) {
        let width = source.len().min(self.current.len()).min(out.len());
        for x in 0..width {
            let Some(src) = source[x] else {
                out[x] = transparent;
                continue;
            };
            if self.preserve_exact {
                let exact = map.index(src);
                if map.entry(exact) == src {
                    out[x] = exact;
                    continue;
                }
            }
            let e = self.current[x];
            let w = [0, 1, 2].map(|c| (f64::from(src[c]) + e[c]).clamp(0.0, 255.0));
            let index = map.index(w.map(|v| v.round() as u8));
            out[x] = index;
            let chosen = map.entry(index);
            let err = [0, 1, 2].map(|c| (w[c] - f64::from(chosen[c])) * self.amount / 100.0);
            let spread = |row: &mut Vec<[f64; 3]>, at: Option<usize>, weight: f64| {
                if let Some(slot) = at.and_then(|i| row.get_mut(i)) {
                    for c in 0..3 {
                        slot[c] += err[c] * weight / 16.0;
                    }
                }
            };
            let right = (x + 1 < width).then_some(x + 1);
            spread(&mut self.current, right, 7.0);
            spread(&mut self.next, x.checked_sub(1), 3.0);
            spread(&mut self.next, Some(x), 5.0);
            spread(&mut self.next, right, 1.0);
        }
        std::mem::swap(&mut self.current, &mut self.next);
        self.next.iter_mut().for_each(|e| *e = [0.0; 3]);
    }
}
