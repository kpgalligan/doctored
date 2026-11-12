//! KERNEL: the Pattern Overlay's tile and its sampler — the
//! tile raster, the PYRAMID levels Scale reads, the one sampler, and the four
//! frozen presets. Free functions over plain values; the effect model is
//! `style_pattern`, the renderer `style_fx_pattern_overlay`.
//!
//! # The tile
//!
//! [`PatternTile`] is straight RGBA8, row 0 at the top, each side
//! 1..=[`MAX_TILE_SIDE`]. Its samples are the DOCUMENT's own values: they are
//! never colour-converted, and a 32-bit document reads code / 255 as linear
//! light (take1's FLOAT = VERBATIM).
//!
//! # Scale: PYRAMID (fitted on take1, `pattern_overlay_take.py --fit`)
//!
//! Photoshop does not sample the tile continuously at a non-unit scale. It
//! first box-reduces the tile `L` = max(0, floor(log2(1 / s))) times, then
//! samples that LEVEL bilinearly with the residual scale s' = s · w / level w
//! (per axis), so every scale in (1/2, 1] reads the tile itself and every
//! scale in (1/4, 1/2] reads one halving. Take1's S50 and S200 fit this at 0
//! codes and S37 at max 0.97 / mean 0.41 codes; a continuous resampler misses
//! S37 by 21.5 codes mean under every filter and origin tried.
//!
//! Each halving is a periodic 2 × 2 box in premultiplied unit RGBA: level
//! texel j is the mean of source texels 2j and 2j + 1 (modulo the side) on
//! each axis. An ODD side is tiled twice first, so its level keeps n texels
//! and spans two periods (ODD-DOUBLE): take2's 11 × 7 tile at 50 % repeats
//! every 11 px, pairing texels (10, 0), (1, 2), …, and its 12 % case reads
//! the doubled third halving. The two candidates fixed before take2,
//! ODD-CEIL (area cells of n / ceil(n / 2)) and ODD-FLOOR, missed by 69 and
//! 40 codes; ODD-DOUBLE is the refit on take2's own data (max 0.5 codes over
//! both, 1.14 over every take2 scale case), CONFIRMED by take3 on held-out
//! odd tiles and scales (9 × 5 at 50 % and 22 %, 13 × 10 at 45 % and 15 %,
//! 11 × 7 at 30 %: max 1.43 codes; ODD-CEIL and ODD-FLOOR 28 to 69). s' uses
//! the period the level spans: s · w' / level w, w' the side doubled once
//! per odd halving.
//!
//! # Sampling
//!
//! Pixel (x, y), in the layer's pixel grid, reads level index ((x − ox) /
//! s'x, (y − oy) / s'y) with the texel centres at INTEGER indices
//! (corner-aligned: take1's S200 reads index x / 2, 55 codes off with
//! centres at half-integers), bilinear and wrapping. The colour is the
//! premultiplied sum divided by its alpha, so a transparent texel lends no
//! colour to its neighbours; the renderer multiplies the coverage by alpha.
//!
//! # Angle (`pattern_overlay_take.py` TAKE3 and TAKE4)
//!
//! An angled tile is TWO-PASS on the PYRAMID level (the TAKE4 fit). Pass 1
//! turns the w x h level into its bounding raster in level texels: raster
//! texel q (integers over R(−t) of [0, w] x [0, h]) is the level at R(t) q,
//! bilinear, taps clamped inside the tile. Pass 2 cuts the plane into
//! instances along the exact turned period vectors v1 = (c w s'x, −s w s'x)
//! and v2 = (s h s'y, c h s'y), (c, s) = (cos t, sin t), t = [`ANGLE_SIGN`]
//! · Photoshop's `Angl`, exact at multiples of 90 degrees: pixel (x, y), d =
//! (x, y) − O, is in instance (floor((dx v2y − dy v2x) / det), floor((v1x dy
//! − v1y dx) / det)) at the local point l, and reads the raster at l / s' +
//! R(−t) [`LATTICE_SHIFT`], bilinear, taps clamped inside the raster. The
//! rotation pivots on the FOLLOW origin O itself (take3's moved and widened
//! cases score exactly as the unmoved one; the fitted offset is (0, 0)). At
//! angle 0 [`LevelOf::rotation`] is `None`, the level is the pyramid level
//! and `sample` runs the unrotated divide, so an unrotated tile is
//! bit-identical to before.
//!
//! Each instance is clamped at its own edges, which is what take1's and
//! take2's A90 show (tile column 0 doubled at every seam: 0 codes). The fit
//! ranked bilinear, Keys a = −0.5 and −0.75, Lanczos-2 and -3 on the level,
//! TWO-PASS, a Paeth three-shear and supersampled NEAREST, all with that
//! clamp, on take1 to take3's eighteen rotation rows; TWO-PASS ranks first
//! (worst mean 81.2 codes against the previous ROT-CONTINUOUS's 90.4), and
//! no sharper filter helps. It is still NOT Photoshop: the two A90s are
//! exact and every other angle misses the declared bound (6 codes max, 1.5
//! mean) by 21.7 to 81.2 codes mean. take4, the one confirming take on
//! held-out angles (10, 65, 120 and −20 degrees at 100 % and 45 %, the odd
//! 9 x 5 tile at 40), MISSES too (21.5 to 74.3 codes mean), and the refit on
//! all 28 rows keeps TWO-PASS first. TAKE5's fit (sharper and smoother
//! two-pass kernels, a Catmull-Smith shear, a centred raster, 8-bit
//! intermediates) passed nothing, and its held-out rows (5, 50, 150 and −35
//! degrees, a 13 x 10 tile at 25) read 34.7 to 75.3 codes mean. TAKE6
//! (`pattern_overlay_669_take.py`: one-pass Keys and Automatic, scale then
//! rotate, a supersampled turn, a whole-pixel shear) passed nothing either,
//! and its confirm take on a 112 x 80 canvas (35, 80, 145 and −50 degrees on
//! a 16 x 10 tile, 110 on a 7 x 11 tile, at 100 % and 60 %) reads 19.7 to
//! 58.2 codes mean. The residual is an open item of #3
//! (`designs/phase-10c-constructs-status.md`).
//!
//! # Depths and CMYK
//!
//! The pyramid and the sampler are generic over the channel count
//! ([`LevelOf`]): one halving and one bilinear serve RGBA ([`TileLevel`]) and
//! a CMYK document's INK level ([`InkLevel`], C, M, Y, K, A). A CMYK layer's
//! tile is converted to ink texel by texel BEFORE the pyramid (each texel's
//! colour goes through the same leg a Colour Overlay's colour does, in
//! `cmyk_style`), so the ink pyramid boxes and blends inks the way the RGB
//! pyramid boxes and blends colours. A 32-bit document needs no level of its
//! own: it samples the RGBA level and reads the codes / 255 as linear. A Lab
//! layer's tile is authored sRGB: [`tile_level_mapped`] turns each texel into
//! 8-bit Lab codes BEFORE the pyramid (`lab_style::painted_sampler`), so the
//! halvings and the turn box Lab values, in a level the size of RGB's.

use std::sync::Arc;

/// The largest tile side, in pixels. 512² base64 is 1.4 MB of JSON, which
/// keeps one styled layer's recorded appearance well inside the 8 MiB comps
/// payload (`doc_comps::PAYLOAD_CAP_REFUSAL`).
pub const MAX_TILE_SIDE: u32 = 512;

/// The render angle's sign against Photoshop's `Angl` (module doc's Angle):
/// +1, t = +Angl in the lattice sampler (R(t) maps the instance-local point
/// to the tile), so a positive angle turns the tile
/// counter-clockwise on screen (take1's A90 reads tile x falling as y grows,
/// and every −1 fit of take1 and take2 ranks far worse: worst mean 108.5 to
/// 108.9 codes against +1's 65.0).
pub const ANGLE_SIGN: f64 = 1.0;

/// The fitted index shift of a turned instance, level texels in the tile's
/// own frame (module doc's Angle): the instance reads the tile at R(t) l /
/// s' + this. (−1, 0) is the one shift under which take1's and take2's A90
/// read exactly (0 codes: tile column 0 doubled at every seam, the clamp of
/// index −1), and every family of the TAKE4 fit ranks first with it; the
/// fitted origin offset beside it is (0, 0), so the tile turns about O itself.
pub const LATTICE_SHIFT: (f64, f64) = (-1.0, 0.0);

/// A pattern's raster: straight RGBA8, row 0 at the top, `width * height *
/// 4` bytes. DOCUMENT values, never colour-converted (module doc).
#[derive(Clone, Debug, PartialEq)]
pub struct PatternTile {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

/// One PYRAMID level: `N` premultiplied unit channels per texel, the last
/// one alpha, row 0 at the top, the residual scale s' per axis that maps
/// level texels to pixels, and the rotation the sampler applies (module
/// doc's Angle). An angled level holds the TURNED raster (pass 1), not the
/// pyramid level it was turned from. RGBA is [`TileLevel`]; a CMYK
/// document's ink level is [`InkLevel`].
#[derive(Clone, Debug, PartialEq)]
pub struct LevelOf<const N: usize> {
    pub width: u32,
    pub height: u32,
    pub premultiplied: Arc<[[f32; N]]>,
    pub scale: (f64, f64),
    /// The turned tile's lattice ([`rotation_of`]); `None` exactly when the
    /// angle is 0 (or not finite), which samples with the unrotated divide.
    pub rotation: Option<Lattice>,
}

/// An angled level's lattice (module doc's Angle), everything the rotation
/// sampler needs that depends on the angle, built once in `level_of`: (cos
/// t, sin t) ([`rotation_of`]), the exact rotated period vectors in layer px
/// v1 = (c w s'x, −s w s'x) and v2 = (s h s'y, c h s'y) (w x h the pyramid
/// level the raster was turned from), their determinant, and the turned
/// raster's grid origin in level texels (its texel (0, 0) is q = this).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lattice {
    pub cos: f64,
    pub sin: f64,
    pub v1: (f64, f64),
    pub v2: (f64, f64),
    pub det: f64,
    pub raster_origin: (f64, f64),
}

/// A level of premultiplied unit RGBA.
pub type TileLevel = LevelOf<4>;

/// A level of premultiplied unit C, M, Y, K, A (module doc's CMYK).
pub type InkLevel = LevelOf<5>;

/// How many halvings Scale `scale` reads: max(0, floor(log2(1 / scale))).
/// A non-finite or non-positive scale reads the tile itself.
pub fn mip_level(scale: f64) -> u32 {
    if !scale.is_finite() || scale <= 0.0 {
        return 0;
    }
    let l = (1.0 / scale).log2().floor();
    if l <= 0.0 {
        0
    } else {
        // Scale is clamped to 0.01 by the parser (L <= 6); the cap keeps a
        // direct construction total.
        l.min(31.0) as u32
    }
}

/// One halving's side (ODD-DOUBLE, module doc): n / 2 on an even side; an
/// odd side is tiled twice first, so it keeps n texels.
fn halved(n: u32) -> u32 {
    if n.is_multiple_of(2) {
        n / 2
    } else {
        n.max(1)
    }
}

/// The level's dimensions for a `width` x `height` tile at `scale` and
/// `angle` degrees, without building it (the scratch accounting's input):
/// the pyramid level's, or at an angle the turned raster's.
pub fn level_dims(width: u32, height: u32, scale: f64, angle: f64) -> (u32, u32) {
    let (mut w, mut h) = (width.max(1), height.max(1));
    for _ in 0..mip_level(scale) {
        w = halved(w);
        h = halved(h);
    }
    match rotation_of(angle) {
        None => (w, h),
        Some((c, s)) => {
            let (_, _, tw, th) = turned_box(c, s, w, h);
            (tw, th)
        }
    }
}

/// One periodic halving of a `w` x `h` premultiplied raster along one axis
/// (`horizontal` = x): target texel j is the mean of source texels 2j and
/// 2j + 1, modulo the side — the 2 × 2 box on an even side, and on an odd
/// side the box over the tile tiled twice (module doc).
fn halve_axis<const N: usize>(
    src: &[[f32; N]],
    w: u32,
    h: u32,
    horizontal: bool,
) -> (Vec<[f32; N]>, u32, u32) {
    let n = if horizontal { w } else { h };
    let m = halved(n);
    let (nw, nh) = if horizontal { (m, h) } else { (w, m) };
    let mut out = vec![[0f32; N]; nw as usize * nh as usize];
    for y in 0..nh {
        for x in 0..nw {
            let (j, line) = if horizontal { (x, y) } else { (y, x) };
            let mut acc = [0f64; N];
            for k in [(2 * j) % n, (2 * j + 1) % n] {
                let (sx, sy) = if horizontal { (k, line) } else { (line, k) };
                let p = src[(sy * w + sx) as usize];
                for c in 0..N {
                    acc[c] += 0.5 * f64::from(p[c]);
                }
            }
            out[(y * nw + x) as usize] = acc.map(|v| v as f32);
        }
    }
    (out, nw, nh)
}

/// The render angle's rotation (module doc's Angle): `None` at 0 or a
/// non-finite angle, else (cos t, sin t) with t = [`ANGLE_SIGN`] · the angle
/// in radians — EXACT (0 or ±1) at a multiple of 90 degrees, so a quarter
/// turn's seams fall on exact integers and the core, the runner and the
/// oracles pick the same instance there.
fn rotation_of(angle: f64) -> Option<(f64, f64)> {
    if !angle.is_finite() || angle == 0.0 {
        return None;
    }
    let t = ANGLE_SIGN * angle;
    if t % 90.0 == 0.0 {
        let quarter = ((t / 90.0).round() as i64).rem_euclid(4);
        return Some([(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)][quarter as usize]);
    }
    let t = t.to_radians();
    Some((t.cos(), t.sin()))
}

/// The turned raster's integer grid for a `w` x `h` level at (c, s) =
/// (cos t, sin t): R(−t) of [0, w] x [0, h] in level texels, as (x0, y0,
/// width, height) with texel (i, j) at q = (x0 + i, y0 + j). The runner's
/// `box4`, term for term, so both cut the same raster.
fn turned_box(c: f64, s: f64, w: u32, h: u32) -> (f64, f64, u32, u32) {
    let (w, h) = (f64::from(w), f64::from(h));
    let corners = [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)];
    let xs = corners.map(|(u, v)| c * u + s * v);
    let ys = corners.map(|(u, v)| -s * u + c * v);
    let lo = |a: [f64; 4]| a.iter().copied().fold(f64::INFINITY, f64::min).floor();
    let hi = |a: [f64; 4]| a.iter().copied().fold(f64::NEG_INFINITY, f64::max).ceil();
    let (x0, y0) = (lo(xs), lo(ys));
    let side = |lo: f64, hi: f64| (hi - lo).clamp(0.0, f64::from(u32::MAX - 1)) as u32 + 1;
    (x0, y0, side(x0, hi(xs)), side(y0, hi(ys)))
}

/// The premultiplied bilinear sum of a `w` x `h` raster at index (u, v),
/// texel j at index j, the taps CLAMPED inside the raster: the rotation
/// sampler's one filter, for pass 1 (the level into its turned raster) and
/// pass 2 (the raster at the pixel). The unrotated path's bilinear in
/// [`sample`] is the same weights with the taps wrapping instead.
fn clamped_bilinear<const N: usize>(px: &[[f32; N]], w: u32, h: u32, u: f64, v: f64) -> [f64; N] {
    let (w, h) = (i64::from(w.max(1)), i64::from(h.max(1)));
    let (fu, fv) = (u.floor(), v.floor());
    let (tu, tv) = (u - fu, v - fv);
    let (i0, j0) = (fu as i64, fv as i64);
    let mut acc = [0f64; N];
    for (di, dj, k) in [
        (0, 0, (1.0 - tu) * (1.0 - tv)),
        (1, 0, tu * (1.0 - tv)),
        (0, 1, (1.0 - tu) * tv),
        (1, 1, tu * tv),
    ] {
        let i = (i0 + di).clamp(0, w - 1);
        let j = (j0 + dj).clamp(0, h - 1);
        let p = px.get((j * w + i) as usize).copied().unwrap_or([0.0; N]);
        for c in 0..N {
            acc[c] += k * f64::from(p[c]);
        }
    }
    acc
}

/// TWO-PASS's first pass (module doc's Angle): the `w` x `h` level turned by
/// (c, s) into its bounding raster ([`turned_box`]), raster texel q reading
/// the level at R(t) q. Returns the raster, its width and height, and the
/// lattice the sampler cuts the plane with.
fn turned<const N: usize>(
    level: &[[f32; N]],
    w: u32,
    h: u32,
    scale: (f64, f64),
    (c, s): (f64, f64),
) -> (Vec<[f32; N]>, u32, u32, Lattice) {
    let (x0, y0, tw, th) = turned_box(c, s, w, h);
    let mut out = Vec::with_capacity(tw as usize * th as usize);
    for j in 0..th {
        for i in 0..tw {
            let (qx, qy) = (x0 + f64::from(i), y0 + f64::from(j));
            let (u, v) = (c * qx - s * qy, s * qx + c * qy);
            out.push(clamped_bilinear(level, w, h, u, v).map(|x| x as f32));
        }
    }
    let (wf, hf) = (f64::from(w), f64::from(h));
    let v1 = (c * wf * scale.0, -s * wf * scale.0);
    let v2 = (s * hf * scale.1, c * hf * scale.1);
    let lattice = Lattice {
        cos: c,
        sin: s,
        v1,
        v2,
        det: v1.0 * v2.1 - v1.1 * v2.0,
        raster_origin: (x0, y0),
    };
    (out, tw, th, lattice)
}

/// The PYRAMID level Scale `scale` reads of a `width` x `height`
/// premultiplied raster (module doc): `mip_level(scale)` halvings, then s' =
/// scale · w' / level w per axis, w' the side doubled once for every halving
/// of an odd side. At a non-zero `angle` the returned level is that pyramid
/// level turned into its bounding raster (pass 1, [`turned`]), with its
/// [`Lattice`] in `rotation`. The one pyramid both depths' levels go through.
fn level_of<const N: usize>(
    mut data: Vec<[f32; N]>,
    width: u32,
    height: u32,
    scale: f64,
    angle: f64,
) -> LevelOf<N> {
    let (w0, h0) = (width.max(1), height.max(1));
    data.resize(w0 as usize * h0 as usize, [0f32; N]);
    let (mut w, mut h) = (w0, h0);
    // The tile period the level spans, per axis, in source texels.
    let (mut pw, mut ph) = (f64::from(w0), f64::from(h0));
    for _ in 0..mip_level(scale) {
        if !w.is_multiple_of(2) {
            pw *= 2.0;
        }
        if !h.is_multiple_of(2) {
            ph *= 2.0;
        }
        let (a, aw, ah) = halve_axis(&data, w, h, true);
        let (b, bw, bh) = halve_axis(&a, aw, ah, false);
        data = b;
        w = bw;
        h = bh;
    }
    let s = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let scale = (s * pw / f64::from(w), s * ph / f64::from(h));
    let rotation = match rotation_of(angle) {
        None => None,
        Some(cs) => {
            let (raster, tw, th, lattice) = turned(&data, w, h, scale, cs);
            data = raster;
            w = tw;
            h = th;
            Some(lattice)
        }
    };
    LevelOf {
        width: w,
        height: h,
        premultiplied: data.into(),
        scale,
        rotation,
    }
}

/// The PYRAMID level Scale `scale` reads (module doc): `mip_level(scale)`
/// halvings of the premultiplied tile, then s' = scale · w' / level w per
/// axis, w' the tile side doubled once for every halving of an odd side,
/// sampled turned by `angle` degrees (module doc's Angle).
pub fn tile_level(tile: &PatternTile, scale: f64, angle: f64) -> TileLevel {
    tile_level_mapped(tile, scale, angle, &mut |rgb| rgb)
}

/// [`tile_level`] with each texel's colour passed through `texel` first,
/// alpha kept, BEFORE the pyramid (module doc's Depths and CMYK: a Lab
/// layer's texels become Lab codes), so the halvings and the turn box the
/// mapped values. The identity `texel` is [`tile_level`] byte for byte.
pub fn tile_level_mapped(
    tile: &PatternTile,
    scale: f64,
    angle: f64,
    texel: &mut dyn FnMut([u8; 3]) -> [u8; 3],
) -> TileLevel {
    let (w0, h0) = (tile.width.max(1), tile.height.max(1));
    let count = w0 as usize * h0 as usize;
    let data: Vec<[f32; 4]> = (0..count)
        .map(|i| {
            let px = tile.rgba.get(i * 4..i * 4 + 4).unwrap_or(&[0, 0, 0, 0]);
            let rgb = texel([px[0], px[1], px[2]]);
            let a = f32::from(px[3]) / 255.0;
            [
                f32::from(rgb[0]) / 255.0 * a,
                f32::from(rgb[1]) / 255.0 * a,
                f32::from(rgb[2]) / 255.0 * a,
                a,
            ]
        })
        .collect();
    level_of(data, w0, h0, scale, angle)
}

/// The ink PYRAMID level of a `width` x `height` tile already converted to
/// premultiplied unit C, M, Y, K, A (row 0 at the top; module doc's CMYK):
/// the same halvings, s' and rotation as [`tile_level`].
pub fn ink_level(
    premultiplied: Vec<[f32; 5]>,
    width: u32,
    height: u32,
    scale: f64,
    angle: f64,
) -> InkLevel {
    level_of(premultiplied, width, height, scale, angle)
}

/// `tile`'s ink sampler at `origin` (module doc's CMYK): each texel's colour
/// converted by `texel_ink` (unit C, M, Y, K), premultiplied by its alpha
/// into the ink PYRAMID level [`ink_level`] at `scale` and `angle`. The one
/// implementation a CMYK Pattern Overlay or Pattern stroke
/// (`cmyk_style::effect_inks`) and a CMYK pattern fill layer
/// (`cmyk_artwork_render`) share; its bytes are [`ink_scratch_bytes`].
pub fn ink_sampler(
    tile: &PatternTile,
    scale: f64,
    angle: f64,
    origin: (f64, f64),
    texel_ink: &mut dyn FnMut([u8; 3]) -> [f32; 4],
) -> InkSampler {
    let texels = tile
        .rgba
        .chunks_exact(4)
        .map(|px| {
            let a = f32::from(px[3]) / 255.0;
            let ink = texel_ink([px[0], px[1], px[2]]);
            [ink[0] * a, ink[1] * a, ink[2] * a, ink[3] * a, a]
        })
        .collect();
    InkSampler {
        level: Arc::new(ink_level(texels, tile.width, tile.height, scale, angle)),
        origin,
    }
}

/// The bytes [`ink_sampler`] allocates for a `width` x `height` tile at
/// `scale` and `angle`: the converted tile and its level, 20 bytes a texel
/// each — an exact term.
pub fn ink_scratch_bytes(width: u32, height: u32, scale: f64, angle: f64) -> u64 {
    let (w, h) = level_dims(width, height, scale, angle);
    let texels = u64::from(w) * u64::from(h) + u64::from(width.max(1)) * u64::from(height.max(1));
    20 * texels
}

/// The level at pixel (x, y) of the layer's grid with the tile's origin at
/// `origin` (layer px): straight unit channels, the last one alpha (module
/// doc's Sampling).
pub fn sample<const N: usize>(level: &LevelOf<N>, origin: (f64, f64), x: f64, y: f64) -> [f32; N] {
    let (w, h) = (
        i64::from(level.width.max(1)),
        i64::from(level.height.max(1)),
    );
    let (u, v) = match &level.rotation {
        None => {
            let u = (x - origin.0) / level.scale.0;
            let v = (y - origin.1) / level.scale.1;
            (u, v)
        }
        // The turned tile's own sampler (module doc's Angle).
        Some(lattice) => return sample_rotated(level, lattice, origin, x, y),
    };
    if !u.is_finite() || !v.is_finite() || N == 0 {
        return [0.0; N];
    }
    let (fu, fv) = (u.floor(), v.floor());
    let (tu, tv) = (u - fu, v - fv);
    let (i0, j0) = (fu as i64, fv as i64);
    let mut acc = [0f64; N];
    for (di, dj, k) in [
        (0, 0, (1.0 - tu) * (1.0 - tv)),
        (1, 0, tu * (1.0 - tv)),
        (0, 1, (1.0 - tu) * tv),
        (1, 1, tu * tv),
    ] {
        let i = (i0 + di).rem_euclid(w);
        let j = (j0 + dj).rem_euclid(h);
        let p = level
            .premultiplied
            .get((j * w + i) as usize)
            .copied()
            .unwrap_or([0.0; N]);
        for c in 0..N {
            acc[c] += k * f64::from(p[c]);
        }
    }
    straight(acc)
}

/// Straight unit channels of a premultiplied sum: the colour divided by its
/// alpha, so a transparent texel lends no colour, each clamped to 0..1.
fn straight<const N: usize>(acc: [f64; N]) -> [f32; N] {
    let alpha = acc[N - 1];
    let a = alpha.clamp(0.0, 1.0);
    if a <= 0.0 {
        return [0.0; N];
    }
    let mut out = [0f32; N];
    for c in 0..N - 1 {
        out[c] = (acc[c] / alpha).clamp(0.0, 1.0) as f32;
    }
    out[N - 1] = a as f32;
    out
}

/// The ONE rotation sampler (module doc's Angle), every depth and CMYK ink
/// alike: pixel (x, y) falls in the lattice instance (a, b) of the turned
/// tile, at the local point l = d − a v1 − b v2 (d = (x, y) − `origin`),
/// which reads the turned raster bilinearly, taps clamped inside it, at q =
/// l / s' + R(−t) [`LATTICE_SHIFT`] − its grid origin (TWO-PASS's second
/// pass). The instance index keeps the runner's exact expression form (no
/// reciprocal, no reordering), so a seam pixel picks the same instance in
/// the core, the runner and the oracles.
fn sample_rotated<const N: usize>(
    level: &LevelOf<N>,
    lattice: &Lattice,
    origin: (f64, f64),
    x: f64,
    y: f64,
) -> [f32; N] {
    let (dx, dy) = (x - origin.0, y - origin.1);
    let (v1, v2) = (lattice.v1, lattice.v2);
    let a = ((dx * v2.1 - dy * v2.0) / lattice.det).floor();
    let b = ((v1.0 * dy - v1.1 * dx) / lattice.det).floor();
    let lx = dx - a * v1.0 - b * v2.0;
    let ly = dy - a * v1.1 - b * v2.1;
    let (c, s) = (lattice.cos, lattice.sin);
    let (sx, sy) = LATTICE_SHIFT;
    let qx = lx / level.scale.0 + c * sx + s * sy - lattice.raster_origin.0;
    let qy = ly / level.scale.1 - s * sx + c * sy - lattice.raster_origin.1;
    if !qx.is_finite() || !qy.is_finite() || N == 0 {
        return [0.0; N];
    }
    straight(clamped_bilinear(
        &level.premultiplied,
        level.width,
        level.height,
        qx,
        qy,
    ))
}

/// A level placed on a layer: what a rendered plane's colour evaluates per
/// pixel at composite time (`style_render::PlaneColor::Pattern`).
#[derive(Clone, Debug, PartialEq)]
pub struct PatternSampler {
    pub level: Arc<TileLevel>,
    pub origin: (f64, f64),
}

impl PatternSampler {
    /// Straight unit RGBA at layer pixel (lx, ly).
    pub fn at(&self, lx: i64, ly: i64) -> [f32; 4] {
        sample(&self.level, self.origin, lx as f64, ly as f64)
    }
}

/// `tile` on the CANVAS grid at 100 %, unturned: Edit ▸ Fill…'s Pattern
/// contents, read at a canvas pixel. At scale 1 every bilinear tap lands on
/// an integer index, so pixel (x, y) reads texel (x mod w, y mod h) exactly,
/// the read a Pattern Overlay at 100 % and 0 degrees makes.
pub fn canvas_sampler(tile: &PatternTile) -> PatternSampler {
    PatternSampler {
        level: Arc::new(tile_level(tile, 1.0, 0.0)),
        origin: (0.0, 0.0),
    }
}

/// The bytes a fill's [`canvas_sampler`] (or its ink twin) holds for a
/// `width` x `height` tile beside the parsed tile: the decoded tile, a
/// mapped copy of it (a Gray document's greyed tile, a Lab document's codes)
/// and the level, 20 bytes a texel at most (an ink level) — 28 a texel.
pub fn fill_scratch_bytes(width: u32, height: u32) -> u64 {
    28 * u64::from(width.max(1)) * u64::from(height.max(1))
}

/// An ink level placed on a CMYK layer: what a rendered plane's ink
/// evaluates per pixel (`cmyk_style_row::InkColor::Pattern`).
#[derive(Clone, Debug, PartialEq)]
pub struct InkSampler {
    pub level: Arc<InkLevel>,
    pub origin: (f64, f64),
}

impl InkSampler {
    /// Straight unit C, M, Y, K and alpha at layer pixel (lx, ly).
    pub fn at(&self, lx: i64, ly: i64) -> [f32; 5] {
        sample(&self.level, self.origin, lx as f64, ly as f64)
    }
}

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
