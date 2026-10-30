//! The ⌥-drag clipping display: a transient threshold
//! image of the edited composite, never persisted. Highlights paints a black
//! ground with every RGB channel at its maximum code lit to 255 in that
//! channel; Shadows paints a white ground with every RGB channel at zero
//! dropped to 0 in that channel. Alpha-0 pixels show the ground (they have
//! no observable colour), and the threshold output is always opaque.
//!
//! The per-channel endpoint test is the one shared predicate
//! [`clipped_channels`], also used by `develop_inspect`'s endpoint markers.
//! The u16 builder tests the exact native codes 0 and 65535, never a byte
//! quantization, so 65534 is not "clipped".
//!
//! Highlights is a black ground lit per RGB channel at the maximum code, Shadows a white ground
//! dropped per channel at zero, alpha-0 pixels ground. Overlay (View ▸ Show
//! Clipping) is a TRANSPARENT layer for drawing over the canvas, with Develop's endpoint markers
//! (`develop_inspect`): any channel at zero blue (0,0,255,255), any at the maximum red
//! (255,0,0,255), both magenta (255,0,255,255), anything else and alpha 0 clear (0,0,0,0). The byte form is a `pure_op`
//! (`dr_image_clip_display`); the typed form (`dr_precision_image_clip_display`) tests exact native
//! 0/65535 through `runs_rgba16` (no view), admits `1MiB + 4·w·h` before allocating and has no
//! job control. Tests: `tests/clip_display.rs`.
//!
//! At f32 (`runs_rgba32`, same admission and layout) there is no maximum code, so the float
//! predicate [`clipped_channels_float`] lights a channel at or above 1.0 (Highlights) and drops one
//! at or below 0.0 (Shadows) of the edited composite, which maps the white point to 1.0 and the
//! black point to 0.0. Float Levels continues past both (`LevelsStage::apply_float`); float Curves
//! continues past a white end point but holds an inside black end point flat at exactly its output
//! (`Spline::value_float`), so the test includes the points themselves, the float counterpart of
//! the u16 endpoint test. A NaN or non-positive alpha shows the ground; a NaN channel is not clipped.
use crate::precision_image::{
    admit, reserved_vec, PrecisionImage, PrecisionStorage, BOOKKEEPING_BYTES,
};
use image::{Rgba, RgbaImage};

/// Which endpoint the display thresholds; the codes are the header's
/// `DR_CLIP_DISPLAY_*` values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipDisplay {
    Highlights = 1,
    Shadows = 2,
    Overlay = 3,
}

impl ClipDisplay {
    /// `None` for any code other than 1, 2 or 3.
    pub fn from_code(code: u32) -> Option<Self> {
        match code {
            1 => Some(Self::Highlights),
            2 => Some(Self::Shadows),
            3 => Some(Self::Overlay),
            _ => None,
        }
    }

    /// The display pixel of one straight sample whose endpoints are `zero`
    /// and `max`. Highlights and Shadows test their one endpoint; Overlay
    /// tests both through the same predicate and marks them as
    /// `develop_inspect` does.
    fn display<T: Copy + PartialEq>(self, rgba: [T; 4], zero: T, max: T) -> [u8; 4] {
        self.display_bits(|high| clipped_channels(rgba, zero, max, high))
    }

    /// The same display of one straight linear float sample, through
    /// [`clipped_channels_float`] instead of the code-endpoint predicate.
    fn display_float(self, rgba: [f32; 4]) -> [u8; 4] {
        self.display_bits(|high| clipped_channels_float(rgba, high))
    }

    /// The display pixel from a predicate answering the clipped-channel bits
    /// at the high (`true`) or low (`false`) endpoint.
    fn display_bits(self, clipped: impl Fn(bool) -> u8) -> [u8; 4] {
        match self {
            Self::Highlights => self.pixel(clipped(true)),
            Self::Shadows => self.pixel(clipped(false)),
            Self::Overlay => {
                let low = clipped(false) != 0;
                let high = clipped(true) != 0;
                match (low, high) {
                    (true, true) => [255, 0, 255, 255],
                    (true, false) => [0, 0, 255, 255],
                    (false, true) => [255, 0, 0, 255],
                    (false, false) => [0, 0, 0, 0],
                }
            }
        }
    }

    /// The opaque threshold pixel for a set of clipped-channel bits
    /// (Highlights and Shadows only).
    fn pixel(self, bits: u8) -> [u8; 4] {
        let lit = |c: u8| bits & (1 << c) != 0;
        let channel = |c: u8| match (self, lit(c)) {
            (Self::Highlights, true) | (Self::Shadows, false) => 255,
            _ => 0,
        };
        [channel(0), channel(1), channel(2), 255]
    }
}

/// Bit c set when RGB channel c == (want_max ? max : zero); 0 whenever rgba[3] == zero.
pub(crate) fn clipped_channels<T: Copy + PartialEq>(
    rgba: [T; 4],
    zero: T,
    max: T,
    want_max: bool,
) -> u8 {
    if rgba[3] == zero {
        return 0;
    }
    let target = if want_max { max } else { zero };
    (0..3).fold(0, |bits, c| {
        if rgba[c] == target {
            bits | 1 << c
        } else {
            bits
        }
    })
}

/// Bit c set when alpha > 0 and channel c is >= 1.0 (want_high) or <= 0.0 (otherwise).
pub(crate) fn clipped_channels_float(rgba: [f32; 4], want_high: bool) -> u8 {
    // A NaN alpha shows the ground, as a non-positive one does; a NaN
    // channel fails both comparisons and is never clipped.
    if rgba[3].is_nan() || rgba[3] <= 0.0 {
        return 0;
    }
    (0..3).fold(0, |bits, c| {
        let clipped = if want_high {
            rgba[c] >= 1.0
        } else {
            rgba[c] <= 0.0
        };
        if clipped {
            bits | 1 << c
        } else {
            bits
        }
    })
}

/// The byte display of `image` (straight RGBA8) in `mode`.
pub fn clip_display_rgba8(image: &RgbaImage, mode: ClipDisplay) -> RgbaImage {
    RgbaImage::from_fn(image.width(), image.height(), |x, y| {
        let p = image.get_pixel(x, y).0;
        Rgba(mode.display(p, 0, u8::MAX))
    })
}

impl PrecisionImage {
    /// The u16 display in `mode`, read through the tiles (`runs_rgba16`), as
    /// display bytes (opaque, or transparent where Overlay marks nothing). Admits the output plane plus bookkeeping
    /// against `limit` before allocating. f32 storage reads `runs_rgba32` through
    /// the float predicate (`>= 1.0` / `<= 0.0`). No job
    /// control: the host runs it after its own cancellation check.
    pub fn clip_display(
        &self,
        mode: ClipDisplay,
        limit: u64,
        other: u64,
    ) -> Result<RgbaImage, String> {
        let (width, height) = self.dimensions();
        let scratch = BOOKKEEPING_BYTES + u64::from(width) * u64::from(height) * 4;
        admit(self.retained_bytes(), scratch, other, limit)?;
        let mut out = reserved_vec(width as usize * height as usize * 4)?;
        if self.storage() == PrecisionStorage::Rgba32Float {
            let runs = self
                .runs_rgba32()
                .ok_or("clipping display requires rgba32_float samples")?;
            for run in runs {
                for &p in run {
                    out.extend_from_slice(&mode.display_float(p));
                }
            }
        } else {
            let runs = self
                .runs_rgba16()
                .ok_or("clipping display requires rgba16_unorm samples")?;
            for run in runs {
                for &p in run {
                    out.extend_from_slice(&mode.display(p, 0, u16::MAX));
                }
            }
        }
        RgbaImage::from_raw(width, height, out)
            .ok_or_else(|| "invalid clipping display dimensions".into())
    }
}
