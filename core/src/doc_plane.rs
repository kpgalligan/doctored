//! Colour planes and the arithmetic over them: reading one plane of the
//! composite, of a layer or of a bare image as a `u8` buffer, writing one
//! plane back into a layer, painting a plane or a channel with the SAME lerp
//! `doc::paint_mask` uses (`channel_lerp` below is that rule's one copy),
//! blending two planes through the existing blend table — the single function
//! behind Apply Image, Calculations and the luminosity masks. The thumbnail
//! reducers (`box_reduced`, `area_split`, `box_reduced_rows`, `gray_resized`)
//! live in `thumbnail_reduce`.
//!
//! A PLANE is a canvas-sized `u8` buffer, row 0 = top, one byte per canvas
//! pixel: exactly the representation selections (`doc_select`), layer masks
//! (`doc`) and alpha channels (`doc_channel`) already share. The one reader
//! that departs from it is [`image_plane`], whose buffer is the image's own
//! size.

use std::sync::Arc;

use image::RgbaImage;

use crate::blend::{composite_source_into, BlendKind, BlendMode, LUMA_B, LUMA_G, LUMA_R};
use crate::doc::{DrDocument, Layer};
use crate::doc_lock::EditKind;
use crate::doc_paint::overlay_rect;
use crate::layer_rect::canvas_clip;
use crate::plane_scale_offset::PlaneBlend;

/// One 8-bit plane of a colour image. Mirrors `DrPlane` in the C header.
///
/// The FFI never takes this type directly: an extern function declares
/// `plane: c_int` and maps it through [`Plane::from_c`], because building an
/// enum from an out-of-range discriminant is undefined behaviour and the
/// header invites callers to pass values (`DR_PLANE_LUMA` where only a
/// writable plane is accepted, say) that a given export refuses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Plane {
    Red = 0,
    Green = 1,
    Blue = 2,
    Alpha = 3,
    /// Rec. 709 luma. Derived, so read-only.
    Luma = 4,
    /// A layer's mask. Read-only, and only layers have one.
    Mask = 5,
}

impl Plane {
    /// Maps a raw `DrPlane` value coming across the FFI.
    pub fn from_c(value: i32) -> Option<Self> {
        match value {
            0 => Some(Plane::Red),
            1 => Some(Plane::Green),
            2 => Some(Plane::Blue),
            3 => Some(Plane::Alpha),
            4 => Some(Plane::Luma),
            5 => Some(Plane::Mask),
            _ => None,
        }
    }

    /// The RGBA byte offset this plane occupies in a pixel, or `None` for the
    /// DERIVED planes (`Luma`, `Mask`) that no pixel byte holds — which is
    /// exactly the test every writer needs, so "is this plane writable" and
    /// "which byte is it" are one question with one answer.
    pub(crate) fn byte(self) -> Option<usize> {
        match self {
            Plane::Red => Some(0),
            Plane::Green => Some(1),
            Plane::Blue => Some(2),
            Plane::Alpha => Some(3),
            Plane::Luma | Plane::Mask => None,
        }
    }
}

/// Rec. 709 luma of a STRAIGHT RGB triple, rounded to a byte — what "take the
/// image's gray" means everywhere in this module. Alpha plays no part: a
/// plane describes colour, not coverage. Coefficients come from
/// `blend::LUMA_*`, the one copy.
fn luma8(r: u8, g: u8, b: u8) -> u8 {
    (LUMA_R * f32::from(r) + LUMA_G * f32::from(g) + LUMA_B * f32::from(b))
        .clamp(0.0, 255.0)
        .round() as u8
}

/// One plane of a bare image, at the IMAGE's dimensions (row 0 = top).
/// `Mask` is `None` — an image has no mask.
///
/// For an OPAQUE GRAYSCALE image `Luma` is the identity: the coefficients sum
/// to 1 and all three inputs are equal, so `luma(v, v, v)` differs from `v` by
/// at most f32 rounding and lands back on `v` after the round. That makes this
/// the lossless reader for every plane image [`gray_image`] hands out — and
/// the definition of "take the result's gray" for an op that un-grays one
/// (sepia, hue rotate).
pub(crate) fn image_plane(img: &RgbaImage, plane: Plane) -> Option<Vec<u8>> {
    if plane == Plane::Luma {
        return Some(img.pixels().map(|p| luma8(p[0], p[1], p[2])).collect());
    }
    let byte = plane.byte()?;
    Some(img.pixels().map(|p| p[byte]).collect())
}

/// `plane` as an OPAQUE GRAYSCALE RGBA image (r == g == b, alpha 255) — the
/// form every existing flat-image op (filters, adjustments, fill, gradient)
/// can consume, and the form a host draws a plane from. `None` for a zero
/// dimension or a buffer that is not exactly `w * h` bytes.
pub(crate) fn gray_image(plane: &[u8], w: u32, h: u32) -> Option<RgbaImage> {
    if w == 0 || h == 0 {
        return None;
    }
    let expected = (w as usize).checked_mul(h as usize)?;
    if plane.len() != expected {
        return None;
    }
    let mut out = RgbaImage::new(w, h);
    for (px, &v) in out.pixels_mut().zip(plane.iter()) {
        px.0 = [v, v, v, 255];
    }
    Some(out)
}

/// The mask-painting lerp — the ONE copy of the rule `doc::paint_mask`
/// documents and `dr_doc_painting_layer_mask` publishes:
///
/// ```text
/// v' = round(clamp(v + (luma - v) * a, 0, 255))
/// ```
///
/// with `a = sp[3] / 255` and `luma` the STRAIGHT colour's Rec. 709 luma —
/// the premultiplied bytes divided by their own alpha, which is ONE division
/// because luma is linear (`luma(c / a) == luma(c) / a`). Painting white
/// therefore moves the value toward 255 and black toward 0, with the stroke's
/// own anti-aliasing carried by the alpha.
///
/// `None` when `a <= 0`: the pixel is untouched, and no caller may treat that
/// as a write.
pub(crate) fn channel_lerp(v: u8, sp: [u8; 4]) -> Option<u8> {
    let a = f32::from(sp[3]) / 255.0;
    if a <= 0.0 {
        return None;
    }
    let luma =
        (LUMA_R * f32::from(sp[0]) + LUMA_G * f32::from(sp[1]) + LUMA_B * f32::from(sp[2])) / a;
    let luma = luma.clamp(0.0, 255.0);
    let m = f32::from(v);
    Some((m + (luma - m) * a).clamp(0.0, 255.0).round() as u8)
}

/// The offset that maps a layer pixel back to canvas coordinates, admitted
/// only when the layer meets the canvas — the clip
/// `doc_paint::source_rect_in` makes (the master copy of this clipping),
/// factored out because three ops in this module map a canvas-frame buffer
/// through a layer offset. The clipped rect itself is
/// `layer_rect::canvas_clip` (13B-2c-2a).
struct LayerClip {
    off_x: i64,
    off_y: i64,
}

impl LayerClip {
    /// `None` when the layer's extent does not intersect the canvas at all —
    /// no byte could change, so every caller refuses rather than returning an
    /// unchanged copy (which would mint a phantom undo step in the host).
    fn new(doc: &DrDocument, layer: &Layer) -> Option<Self> {
        let (lw, lh) = layer.pixels.dimensions();
        let (off_x, off_y) = (i64::from(layer.offset.0), i64::from(layer.offset.1));
        let lx0 = (-off_x).max(0);
        let ly0 = (-off_y).max(0);
        let lx1 = (i64::from(doc.width) - off_x).min(i64::from(lw));
        let ly1 = (i64::from(doc.height) - off_y).min(i64::from(lh));
        if lx0 >= lx1 || ly0 >= ly1 {
            return None;
        }
        Some(LayerClip { off_x, off_y })
    }

    /// Index of the canvas pixel under layer pixel (`lx`, `ly`), in pixels.
    fn canvas_index(&self, doc: &DrDocument, lx: i64, ly: i64) -> usize {
        let cx = (lx + self.off_x) as u64;
        let cy = (ly + self.off_y) as u64;
        (cy * u64::from(doc.width) + cx) as usize
    }
}

/// Writes `v` into byte `byte` of ONE straight-RGBA pixel `dp`, and answers
/// whether a byte actually moved — the latch the plane writers (below and in
/// `doc_scope`) depend on. The ONE copy of the plane-write rule, including its
/// straight-alpha half: a pixel whose new alpha is 0 carries no colour
/// either, so nothing can fringe when it is composited or resampled (the rule
/// `ops::apply_mask` and `doc_select::clear_selection` already state).
pub(crate) fn write_plane_byte(dp: &mut [u8], byte: usize, v: u8) -> bool {
    let mut changed = false;
    if dp[byte] != v {
        dp[byte] = v;
        changed = true;
    }
    if byte == 3 && v == 0 && dp[..3] != [0, 0, 0] {
        dp[0] = 0;
        dp[1] = 0;
        dp[2] = 0;
        changed = true;
    }
    changed
}

impl DrDocument {
    /// One plane of the FLATTENED composite, canvas-sized. `Mask` is not a
    /// composite plane (`None`).
    ///
    /// A ONE-SHOT op: it runs the whole projection, so a host that draws a
    /// plane repeatedly reads its own cached projection with
    /// `dr_image_plane` instead.
    pub fn composite_plane(&self, plane: Plane) -> Option<Vec<u8>> {
        if plane == Plane::Mask {
            return None;
        }
        image_plane(&self.flattened().ok()?, plane)
    }

    /// One plane of layer `idx`, CANVAS-sized: pixels outside the layer's
    /// rect read 0 for every plane (there are no pixels there, and a
    /// selection never reaches past the canvas either). `Luma` is valid here.
    ///
    /// `Mask` yields `None` when the layer has no mask; when it has one the
    /// plane is again canvas-sized with 0 OUTSIDE the layer's rect — outside
    /// the layer there is nothing for a mask to reveal.
    ///
    /// On a GROUP every plane reads its PROJECTION (`layer_canvas_image`
    /// answers that for a group), and `Mask` is the group's canvas-sized mask
    /// verbatim, since that mask already IS a canvas plane.
    pub fn layer_plane(&self, idx: usize, plane: Plane) -> Option<Vec<u8>> {
        let layer = self.layers.get(idx)?;
        if plane != Plane::Mask {
            // The canvas-sized placement (0 outside the layer) is exactly
            // `layer_canvas_image`; read the plane straight off it rather
            // than restating the offset walk.
            return image_plane(&self.layer_canvas_image(idx)?, plane);
        }
        let mask = layer.mask.as_deref()?;
        Some(
            crate::doc_channel::padded_plane_with_coverage(
                mask,
                self.width,
                self.height,
                layer.mask_properties.offset,
                layer.mask_properties.outside_coverage,
            )
            .into_raw(),
        )
    }

    /// Replaces ONLY `plane` of layer `idx`'s pixels with the same-named
    /// plane of a CANVAS-sized `src`, inside the layer's rect (mapped through
    /// the layer's offset exactly as `painting_layer` maps a stroke).
    ///
    /// `Alpha` writes STRAIGHT alpha, and where the new alpha lands on 0 the
    /// pixel's colour bytes are zeroed too — the rule `ops::apply_mask` and
    /// `doc_select::clear_selection` already state for straight-alpha pixels.
    ///
    /// `None` for an out-of-range `idx`, `Luma`/`Mask` (derived, not
    /// writable), a `src` that is not canvas-sized, a layer extent that
    /// misses the canvas, or when NO BYTE WOULD CHANGE — so a caller writing
    /// several planes in a row must tolerate `None` per plane rather than
    /// `?`-chaining them.
    ///
    /// It writes the canvas ∩ layer rect and nothing else, so the part of an
    /// oversized layer that hangs off the canvas keeps its old plane. That is
    /// right for Apply Image (whose operands are canvas-sized to begin with)
    /// and wrong for a FILTER, which reads and refilters every sample of the
    /// layer.
    pub fn with_layer_plane(&self, idx: usize, plane: Plane, src: &[u8]) -> Option<Self> {
        self.under_locks(idx, EditKind::Pixels, |doc| {
            doc.with_layer_plane_unlocked(idx, plane, src)
        })
    }

    /// The body of [`Self::with_layer_plane`], outside the lock gate. Split
    /// out only so the gate is one line; that method is its only caller.
    fn with_layer_plane_unlocked(&self, idx: usize, plane: Plane, src: &[u8]) -> Option<Self> {
        let byte = plane.byte()?;
        let layer = self.raster_layer(idx)?;
        let expected = (self.width as usize).checked_mul(self.height as usize)?;
        if src.len() != expected {
            return None;
        }
        let clip = LayerClip::new(self, layer)?;
        // Only the canvas ∩ layer rect is read and written back (13B-2c-2a).
        let (rx, ry, rw, rh) = canvas_clip((self.width, self.height), layer)?;
        let mut px = layer.pixels.read_rect(rx, ry, rw, rh)?;
        let mut changed = false;
        for (row, line) in px.chunks_exact_mut(rw as usize).enumerate() {
            let ly = i64::from(ry) + row as i64;
            for (column, dp) in line.iter_mut().enumerate() {
                let lx = i64::from(rx) + column as i64;
                let v = src[clip.canvas_index(self, lx, ly)];
                changed |= write_plane_byte(&mut dp[..], byte, v);
            }
        }
        if !changed {
            return None;
        }
        let pixels = layer.pixels.with_rect(rx, ry, rw, rh, &px).ok()??;
        self.with_layer_raster(idx, pixels)
    }

    /// Paints channel `i` with a canvas-frame PREMULTIPLIED RGBA8 overlay
    /// (exactly canvas `w * h * 4` bytes — the same buffer `painting_layer`
    /// takes) through [`channel_lerp`]: white paints toward 255, black toward
    /// 0. A channel is canvas-sized, so there is no offset mapping.
    ///
    /// `None` for an out-of-range `i`, a wrong buffer length, a channel that
    /// somehow broke the canvas-sized invariant, or when NO BYTE WOULD CHANGE
    /// — the latch `doc_paint::painting_layer_blend` carries. Nothing else
    /// refuses a stroke on a channel (it covers the whole canvas), so without
    /// it painting white over white would mint a phantom undo step and a
    /// false dirty flag. (`doc::paint_mask` has no such latch; that is a
    /// pre-existing deviation, deliberately left as it is.)
    pub fn painting_channel(&self, i: usize, overlay: &[u8]) -> Option<Self> {
        let channel = self.channels.get(i)?;
        if channel.data.dimensions() != (self.width, self.height) {
            return None;
        }
        let px = (self.width as usize).checked_mul(self.height as usize)?;
        if overlay.len() != px.checked_mul(4)? {
            return None;
        }
        let mut painted = (*channel.data).clone();
        let raw: &mut [u8] = &mut painted;
        let mut changed = false;
        for (di, v) in raw.iter_mut().enumerate() {
            let si = di * 4;
            let sp = [
                overlay[si],
                overlay[si + 1],
                overlay[si + 2],
                overlay[si + 3],
            ];
            if let Some(nv) = channel_lerp(*v, sp) {
                if *v != nv {
                    *v = nv;
                    changed = true;
                }
            }
        }
        if !changed {
            return None;
        }
        let mut doc = self.clone();
        doc.channels[i].data = Arc::new(painted);
        Some(doc)
    }

    /// The same coverage paint into ONE plane of layer `idx`'s pixels, mapped
    /// through the layer's offset. Unlike [`Self::with_layer_plane`], painting
    /// `Alpha` never clears the colour bytes: a stroke is incremental, and
    /// colour must survive an alpha that dips to 0 and is painted back up
    /// (the same reason `blend::paint_pixel` keeps the destination colour at
    /// alpha 0).
    ///
    /// `None` for `Luma`/`Mask`, an out-of-range `idx`, a wrong buffer
    /// length, a layer extent that misses the canvas, or when no byte would
    /// change (the same latch as [`Self::painting_channel`]).
    pub fn painting_layer_plane(&self, idx: usize, plane: Plane, overlay: &[u8]) -> Option<Self> {
        self.under_locks(idx, EditKind::Pixels, |doc| {
            doc.painting_layer_plane_unlocked(idx, plane, overlay)
        })
    }

    /// The body of [`Self::painting_layer_plane`], outside the lock gate.
    /// Split out only so the gate is one line; that method is its only
    /// caller.
    fn painting_layer_plane_unlocked(
        &self,
        idx: usize,
        plane: Plane,
        overlay: &[u8],
    ) -> Option<Self> {
        let byte = plane.byte()?;
        let layer = self.raster_layer(idx)?;
        let expected = (self.width as usize)
            .checked_mul(self.height as usize)?
            .checked_mul(4)?;
        if overlay.len() != expected {
            return None;
        }
        let clip = LayerClip::new(self, layer)?;
        // Only the overlay's nonzero rect is read and written back
        // (13B-2c-2a): an all-zero overlay pixel never moves a byte.
        let (rx, ry, rw, rh) = overlay_rect(overlay, (self.width, self.height), layer)?;
        let mut px = layer.pixels.read_rect(rx, ry, rw, rh)?;
        let mut changed = false;
        for (row, line) in px.chunks_exact_mut(rw as usize).enumerate() {
            let ly = i64::from(ry) + row as i64;
            for (column, dp) in line.iter_mut().enumerate() {
                let lx = i64::from(rx) + column as i64;
                let si = clip.canvas_index(self, lx, ly) * 4;
                let sp = [
                    overlay[si],
                    overlay[si + 1],
                    overlay[si + 2],
                    overlay[si + 3],
                ];
                if let Some(nv) = channel_lerp(dp[byte], sp) {
                    if dp[byte] != nv {
                        dp[byte] = nv;
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            return None;
        }
        let pixels = layer.pixels.with_rect(rx, ry, rw, rh, &px).ok()??;
        self.with_layer_raster(idx, pixels)
    }
}

/// True for the four HSL modes — Hue, Saturation, Color and Luminosity —
/// which carry NO information on a single gray plane.
///
/// The W3C defines them over a whole RGB triple, and a gray triple
/// `(v, v, v)` has zero saturation: `set_sat(c, 0)` answers black, so `Hue`,
/// `Saturation` and `Color` all return the base verbatim and `Luminosity`
/// collapses to `Normal`. [`blend_planes`] refuses them rather than pretend
/// to compute something; [`blend_planes_rgb`], where the operands really are
/// triples, is the one place they mean anything.
///
/// Darker Color and Lighter Color are non-separable too, and deliberately NOT
/// listed: on gray triples the whole-pixel luma pick reduces to Darken and
/// Lighten, which is exactly what a user asking for them on one plane means.
pub fn degenerates_on_gray(mode: BlendMode) -> bool {
    matches!(
        mode,
        BlendMode::Hue | BlendMode::Saturation | BlendMode::Color | BlendMode::Luminosity
    )
}

/// One pixel of the blend both plane blenders run: the (already inverted)
/// operands as unit triples through the existing blend table.
///
/// The accumulator's backdrop is OPAQUE, which reduces
/// `composite_source_into` to `out = sa * B(b, s) + (1 - sa) * b` — exactly
/// Apply Image's "blend, then fade by opacity" — so neither blender carries a
/// second copy of the blend math, Dissolve's canvas-absolute dither included.
pub(crate) fn blend_triple(
    b: [f32; 3],
    s: [f32; 3],
    opacity: f32,
    kind: BlendKind,
    xy: (i64, i64),
) -> [f32; 3] {
    let mut acc = [[b[0], b[1], b[2], 1.0]];
    composite_source_into(&mut acc, 0, s, opacity, kind, xy);
    [acc[0][0], acc[0][1], acc[0][2]]
}

/// A blended unit value back as a byte.
fn unit_byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// `base = lerp(base, B(base, source), opacity)` per pixel, IN PLACE — the
/// ONE function behind Apply Image and Calculations on a SINGLE plane (a
/// colour plane, an alpha channel, Calculations' result). Three planes
/// blended as one colour go through [`blend_planes_rgb`] instead.
///
/// `B` is the existing blend table (`blend::blend_kind` +
/// `blend::composite_source_into`), so a gray value is treated as the RGB
/// triple `(v, v, v)` and the result's gray is taken: every separable mode,
/// Dissolve's canvas-absolute dither included, with no second copy of the
/// blend math anywhere. The four HSL modes are refused
/// ([`degenerates_on_gray`]) rather than silently answering the base: on one
/// gray plane they have no meaning to compute.
///
/// `invert_base` / `invert_source` invert an operand BEFORE it is used, so the
/// complement is both the blend input and the value lerped from ("work on the
/// complement", which is what the Invert checkbox means).
///
/// `scale` and `offset` are Apply Image's Scale and Offset for Add and
/// Subtract (`plane_scale_offset`); `(1, 0)` is the table unchanged.
///
/// In place on a CALLER-owned buffer: the documented exception to the purity
/// rule that the `dr_selection_*` family already uses. Returns `false` (and
/// leaves `base` untouched) on a zero dimension, a buffer whose length is not
/// exactly `w * h`, a non-finite opacity, one of the four HSL modes, Pass
/// Through, which is a group's declaration and not a blend function at all
/// ([`crate::blend::BlendMode::is_group_only`]), or a scale and offset
/// `PlaneBlend::new` refuses.
// Apply Image's own vocabulary, one parameter each; bundling them into a
// struct would only move the count somewhere else, and the FFI shim mirrors
// this list one-for-one.
#[allow(clippy::too_many_arguments)]
pub fn blend_planes(
    base: &mut [u8],
    source: &[u8],
    w: u32,
    h: u32,
    mode: BlendMode,
    opacity: f32,
    invert_base: bool,
    invert_source: bool,
    scale: f32,
    offset: f32,
) -> bool {
    if w == 0 || h == 0 || !opacity.is_finite() || degenerates_on_gray(mode) || mode.is_group_only()
    {
        return false;
    }
    let expected = match (w as usize).checked_mul(h as usize) {
        Some(px) => px,
        None => return false,
    };
    if base.len() != expected || source.len() != expected {
        return false;
    }
    let opacity = opacity.clamp(0.0, 1.0);
    let Ok(blend) = PlaneBlend::new(mode, scale, offset) else {
        return false;
    };
    for y in 0..i64::from(h) {
        for x in 0..i64::from(w) {
            let i = (y as u64 * u64::from(w) + x as u64) as usize;
            // INVERSION FIRST: the inverted operand is the one that is both
            // blended AND lerped from.
            let bv = if invert_base { 255 - base[i] } else { base[i] };
            let sv = if invert_source {
                255 - source[i]
            } else {
                source[i]
            };
            let b = f32::from(bv) / 255.0;
            let s = f32::from(sv) / 255.0;
            base[i] = unit_byte(blend.triple([b, b, b], [s, s, s], opacity, (x, y))[0]);
        }
    }
    true
}

/// The RGB-TRIPLE twin of [`blend_planes`]: red, green and blue blended as
/// ONE COLOUR, which is the only formulation in which the four non-separable
/// modes (Hue, Saturation, Color, Luminosity) mean anything — blending the
/// three planes independently would hand each of them a gray triple and make
/// three of the four the identity ([`degenerates_on_gray`] says why).
///
/// This is Apply Image with an RGB source onto an RGB target. Everything else
/// matches [`blend_planes`] exactly, the shared per-pixel `blend_triple`
/// being the one copy of the math: `invert_base`/`invert_source` invert an
/// operand FIRST (so the complement is both blended and lerped from), the
/// backdrop is opaque, and the fade is by `opacity`.
///
/// PURE, unlike its in-place sibling: three fresh planes come back, in
/// red-green-blue order. `None` on a zero dimension, any buffer whose length
/// is not exactly `w * h`, a non-finite opacity, Pass Through (a group's
/// declaration, not a blend function), or a scale and offset
/// `PlaneBlend::new` refuses. Every other mode is accepted — the four HSL
/// ones are what this function exists for.
// Apply Image's own vocabulary, one parameter each; the FFI shim mirrors this
// list one-for-one.
#[allow(clippy::too_many_arguments)]
pub fn blend_planes_rgb(
    base: [&[u8]; 3],
    source: [&[u8]; 3],
    w: u32,
    h: u32,
    mode: BlendMode,
    opacity: f32,
    invert_base: bool,
    invert_source: bool,
    scale: f32,
    offset: f32,
) -> Option<[Vec<u8>; 3]> {
    if w == 0 || h == 0 || !opacity.is_finite() || mode.is_group_only() {
        return None;
    }
    let expected = (w as usize).checked_mul(h as usize)?;
    if base.iter().any(|p| p.len() != expected) || source.iter().any(|p| p.len() != expected) {
        return None;
    }
    let opacity = opacity.clamp(0.0, 1.0);
    let blend = PlaneBlend::new(mode, scale, offset).ok()?;
    let mut out = [
        vec![0u8; expected],
        vec![0u8; expected],
        vec![0u8; expected],
    ];
    for y in 0..i64::from(h) {
        for x in 0..i64::from(w) {
            let i = (y as u64 * u64::from(w) + x as u64) as usize;
            let mut b = [0.0f32; 3];
            let mut s = [0.0f32; 3];
            for c in 0..3 {
                let bv = if invert_base {
                    255 - base[c][i]
                } else {
                    base[c][i]
                };
                let sv = if invert_source {
                    255 - source[c][i]
                } else {
                    source[c][i]
                };
                b[c] = f32::from(bv) / 255.0;
                s[c] = f32::from(sv) / 255.0;
            }
            let blended = blend.triple(b, s, opacity, (x, y));
            for c in 0..3 {
                out[c][i] = unit_byte(blended[c]);
            }
        }
    }
    Some(out)
}
