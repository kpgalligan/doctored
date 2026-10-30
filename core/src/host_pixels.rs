//! Native integer sample packing from explicitly declared platform layouts.
//! This is not a decoder or color transform. Straight latent RGB stays exact;
//! declared premultiplied RGB is rounded once on division and zero at alpha=0.
//! There are two depth expansions: `rgb10_packed_le`'s 10-bit codes widen to
//! u16 exactly as CoreGraphics redraws that packed buffer (see `expand10`),
//! and the `_left12` twins of `rgba16_le` / `rgba16_le_opaque` /
//! `rgba16_le_premultiplied` widen 12-bit codes stored left-aligned in u16 to
//! full scale by bit replication (see `expand12`).
//!
//! explicit integer RGBA8/RGBA16, RGB, BGRA, RGBX8/RGBX16 (`rgba8_opaque` / `rgba16_le_opaque`) and
//! big-endian ARGB packing, plus packed little-endian 10:10:10 (`rgb10_packed_le`),
//! straight/premultiplied/opaque alpha policy, padded rows and exact EXIF permutation. This is an
//! admitted platform-buffer seam, not a decoder or color transform, and it has TWO depth
//! expansions, both output depth 16, typed owner only: `rgb10_packed_le`'s, where each 10-bit code
//! widens to `(v·65535+511)/1023` (CoreGraphics' redraw of the packed buffer), and `expand12`'s,
//! where `rgba16_le_left12` / `rgba16_le_opaque_left12` (`rgba16_le` / `rgba16_le_opaque`
//! with 12-bit RGB stored left-aligned, ImageIO's identity-matrix AVIF decode) widen each `c = v >>
//! 4` to `(c << 4) | (c >> 8)`, alpha never expanded, and an RGB sample with low bits set refuses
//! the intake; `rgba16_le_premultiplied_left12` (ImageIO's decode of a premultiplied
//! 12-bit identity-matrix AVIF once its `prem` reference is detached) checks and widens the same
//! way FIRST and only then divides by alpha. Native decoder/plane caps precede allocation; exact
//! supported ICC is required.
//! Straight latent RGB stays exact; premultiplied division rounds once at native depth and maps
//! alpha0 RGB to0. Since 13B-3e-4a the typed intake (`typed`, `host_pixels.rs:299`) is built
//! `from_rgba16_unseeded`, so it caches no view. No inferred-endian or implicit byte fallback is
//! accepted. The one floating layout family is the HDR still's: `rgba32f_le`,
//! `rgba32f_le_premultiplied` and `rgba32f_le_opaque` (depth 32, 16 bytes a pixel, little-endian
//! f32), read only by the float intake (`float_host_intake`); `byte` and `typed` refuse them.
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::precision_image::{admit, pixel_count, reserved_vec, BOOKKEEPING_BYTES};
use std::sync::Arc;

#[derive(Clone, Copy)]
pub(crate) struct HostLayout {
    pub depth: u8,
    channels: usize,
    order: [usize; 4],
    big_endian: bool,
    premultiplied: bool,
    opaque: bool,
    packed10: bool,
    left12: bool,
    /// Little-endian f32 samples (depth 32): the HDR float intake only.
    float: bool,
}
impl HostLayout {
    pub fn parse(name: &str) -> Result<Self, String> {
        let mut layout = Self {
            depth: 8,
            channels: 4,
            order: [0, 1, 2, 3],
            big_endian: false,
            premultiplied: false,
            opaque: false,
            packed10: false,
            left12: false,
            float: false,
        };
        match name {
            "rgba8" => {}
            "rgba8_premultiplied" => layout.premultiplied = true,
            "rgb8" => {
                layout.channels = 3;
                layout.opaque = true;
            }
            "bgra8" => layout.order = [2, 1, 0, 3],
            "bgra8_opaque" => {
                layout.order = [2, 1, 0, 3];
                layout.opaque = true;
            }
            "rgba8_opaque" => layout.opaque = true,
            "rgba16_le" => layout.depth = 16,
            "rgba16_le_premultiplied" => {
                layout.depth = 16;
                layout.premultiplied = true;
            }
            "rgb16_le" => {
                layout.depth = 16;
                layout.channels = 3;
                layout.opaque = true;
            }
            "rgba16_le_opaque" => {
                layout.depth = 16;
                layout.opaque = true;
            }
            // `rgba16_le` / `rgba16_le_opaque` whose RGB samples are 12-bit
            // codes stored left-aligned; each widens through `expand12`.
            // Alpha is never expanded; only the typed owner accepts these.
            "rgba16_le_left12" => {
                layout.depth = 16;
                layout.left12 = true;
            }
            "rgba16_le_opaque_left12" => {
                layout.depth = 16;
                layout.opaque = true;
                layout.left12 = true;
            }
            // `rgba16_le_left12` whose RGB is premultiplied: the typed owner
            // widens the 12-bit codes first and divides by alpha after.
            "rgba16_le_premultiplied_left12" => {
                layout.depth = 16;
                layout.premultiplied = true;
                layout.left12 = true;
            }
            // Depth is the OUTPUT depth: the 10-bit codes widen to u16, so only
            // the typed owner accepts this layout.
            "rgb10_packed_le" => {
                layout.depth = 16;
                layout.opaque = true;
                layout.packed10 = true;
            }
            // Linear-light f32 from a host HDR render (Core Image's RGBAf);
            // only `float_host_intake` reads them.
            "rgba32f_le" => {
                layout.depth = 32;
                layout.float = true;
            }
            "rgba32f_le_premultiplied" => {
                layout.depth = 32;
                layout.float = true;
                layout.premultiplied = true;
            }
            "rgba32f_le_opaque" => {
                layout.depth = 32;
                layout.float = true;
                layout.opaque = true;
            }
            "argb16_be" | "argb16_be_premultiplied" => {
                layout.depth = 16;
                layout.big_endian = true;
                layout.order = [1, 2, 3, 0];
                layout.premultiplied = name.ends_with("_premultiplied");
            }
            _ => return Err("unsupported host integer sample layout".into()),
        }
        Ok(layout)
    }
    /// True for the `rgba32f_*` layouts.
    pub(crate) fn is_float(self) -> bool {
        self.float
    }
    /// One `rgba32f_*` pixel as stored: four little-endian f32, alpha 1.0
    /// for the opaque layout. NOT yet straightened, so the caller can check
    /// the samples the host actually wrote before dividing.
    pub(crate) fn float_raw(self, pixel: &[u8]) -> [f32; 4] {
        let read = |c: usize| {
            f32::from_le_bytes([
                pixel[c * 4],
                pixel[c * 4 + 1],
                pixel[c * 4 + 2],
                pixel[c * 4 + 3],
            ])
        };
        let alpha = if self.opaque { 1.0 } else { read(3) };
        [read(0), read(1), read(2), alpha]
    }
    /// A raw `rgba32f_*` pixel as straight samples: a premultiplied layout
    /// divides RGB by alpha (one f32 division) and alpha 0 has no colour
    /// to recover, so its RGB is the canonical zero.
    pub(crate) fn float_straight(self, raw: [f32; 4]) -> [f32; 4] {
        if !self.premultiplied {
            return raw;
        }
        let a = raw[3];
        if a > 0.0 {
            [raw[0] / a, raw[1] / a, raw[2] / a, a]
        } else {
            [0.0, 0.0, 0.0, a]
        }
    }
    /// Stored bytes per pixel: one little-endian u32 for packed 10-bit,
    /// otherwise one sample per channel at the stored depth.
    fn stride(self) -> usize {
        if self.packed10 {
            4
        } else {
            self.channels * usize::from(self.depth / 8)
        }
    }
    /// Widens one 10-bit code to u16 as `round(v * 65535 / 1023)` in integer
    /// arithmetic. 1023 is odd, so `v * 65535 / 1023` never lands on an exact
    /// half and adding 511 (the floor of half the divisor) is round-to-nearest.
    /// This equals CoreGraphics' redraw of the packed buffer into 16 bits for
    /// every sample measured (12288/12288); ImageIO's own expansion on its
    /// alpha path differs at one code (820 -> 52531, not 52530), so it is not
    /// the reference.
    fn expand10(code: u32) -> u16 {
        ((code * 65535 + 511) / 1023) as u16
    }
    /// Widens one left-aligned 12-bit sample (`v = c << 4`) to full scale
    /// as `(c << 4) | (c >> 8)`, bit replication, so 4095 becomes 65535.
    /// macOS's ImageIO hands back a 12-bit AVIF whose primary item declares
    /// the identity matrix (`nclx` matrix 0) and no premultiplied alpha with
    /// its RGB left-aligned in u16 (white 65520): measured on macOS 26.5.1
    /// over lossless, lossy identity, alpha and grid files, every RGB sample
    /// had its low four bits clear and `v >> 4` equalled `avifdec`'s codes
    /// exactly. Every other 12-bit AVIF decodes full scale, and its
    /// alpha always does, so only RGB widens. The host decides per file from
    /// the container (`heif_container`); the caller refuses a sample whose
    /// low bits are set before this runs.
    fn expand12(sample: u16) -> u16 {
        let code = sample >> 4;
        (code << 4) | (code >> 8)
    }
    fn sample(self, pixel: &[u8], channel: usize) -> u16 {
        if self.depth == 8 {
            u16::from(pixel[channel])
        } else {
            let data = [pixel[channel * 2], pixel[channel * 2 + 1]];
            if self.big_endian {
                u16::from_be_bytes(data)
            } else {
                u16::from_le_bytes(data)
            }
        }
    }
    fn pixel(self, pixel: &[u8]) -> [u16; 4] {
        if self.packed10 {
            // Bits 31..30 are padding and ignored; R = 29..20, G = 19..10,
            // B = 9..0.
            let word = u32::from_le_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]);
            return [
                Self::expand10((word >> 20) & 0x3ff),
                Self::expand10((word >> 10) & 0x3ff),
                Self::expand10(word & 0x3ff),
                65535,
            ];
        }
        let maximum = if self.depth == 8 { 255 } else { 65535 };
        let alpha = if self.opaque {
            maximum
        } else {
            self.sample(pixel, self.order[3])
        };
        let mut out = [0, 0, 0, alpha];
        for (c, output) in out.iter_mut().take(3).enumerate() {
            let value = self.sample(pixel, self.order[c]);
            // A left-aligned 12-bit sample is not at full scale yet, so its
            // division waits for `HostPixelPlan::typed`, after `expand12`.
            *output = if !self.premultiplied || self.left12 {
                value
            } else {
                Self::straight(value, alpha, maximum)
            };
        }
        out
    }
    /// One premultiplied colour sample divided by its alpha: rounded once,
    /// clamped to `maximum`, and 0 where alpha is 0. The one division every
    /// premultiplied layout uses.
    fn straight(value: u16, alpha: u16, maximum: u16) -> u16 {
        if alpha == 0 {
            0
        } else {
            ((u64::from(value) * u64::from(maximum) + u64::from(alpha) / 2) / u64::from(alpha))
                .min(u64::from(maximum)) as u16
        }
    }
}

pub(crate) struct HostPixelPlan {
    pub width: u32,
    pub height: u32,
    pub stored_width: u32,
    pub stored_height: u32,
    pub row_bytes: usize,
    pub input_length: usize,
    pub layout: HostLayout,
    pub orientation: u8,
    pub scratch_bytes: u64,
}
impl HostPixelPlan {
    pub fn new(
        dimensions: (u32, u32),
        row_bytes: usize,
        input_length: usize,
        layout: HostLayout,
        orientation: u32,
        profile_length: usize,
    ) -> Result<Self, String> {
        Self::plan(
            dimensions,
            row_bytes,
            input_length,
            layout,
            orientation,
            profile_length,
            true,
        )
    }
    /// [`Self::new`] for the HDR float intake, whose PQ and HLG pixels carry
    /// no ICC profile: a zero `profile_length` is admitted, a length past the
    /// blob cap still refuses.
    pub(crate) fn hdr(
        dimensions: (u32, u32),
        row_bytes: usize,
        input_length: usize,
        layout: HostLayout,
        orientation: u32,
        profile_length: usize,
    ) -> Result<Self, String> {
        Self::plan(
            dimensions,
            row_bytes,
            input_length,
            layout,
            orientation,
            profile_length,
            false,
        )
    }
    fn plan(
        dimensions: (u32, u32),
        row_bytes: usize,
        input_length: usize,
        layout: HostLayout,
        orientation: u32,
        profile_length: usize,
        profile_required: bool,
    ) -> Result<Self, String> {
        let count = pixel_count(dimensions.0, dimensions.1)?;
        // A retained master must be reopenable. Enforce the shared native
        // decoder/plane capacity before allocating or reading platform pixels.
        crate::native_sources::single_image_capacity(dimensions, layout.depth)?;
        if !(1..=8).contains(&orientation) {
            return Err("host orientation must be EXIF 1 through 8".into());
        }
        if (profile_required && profile_length == 0)
            || profile_length > crate::drdc::MAX_DRDC_BLOB_LEN as usize
        {
            return Err("host pixels require a bounded original ICC profile".into());
        }
        let row = dimensions.0 as usize * layout.stride();
        let bound = row_bytes
            .checked_mul(dimensions.1 as usize)
            .ok_or("host row byte overflow")?;
        let minimum = bound
            .checked_sub(row_bytes)
            .and_then(|b| b.checked_add(row))
            .ok_or("host row byte overflow")?;
        if row_bytes < row
            || input_length < minimum
            || input_length > bound
            || input_length > isize::MAX as usize
        {
            return Err(
                "host row padding and buffer length disagree with dimensions and layout".into(),
            );
        }
        let (width, height) = crate::pixel_orientation::dimensions(
            dimensions.0 as usize,
            dimensions.1 as usize,
            orientation as u8,
        );
        Ok(Self {
            width: width as u32,
            height: height as u32,
            stored_width: dimensions.0,
            stored_height: dimensions.1,
            row_bytes,
            input_length,
            layout,
            orientation: orientation as u8,
            scratch_bytes: 4 * BOOKKEEPING_BYTES
                + count as u64 * 4 * u64::from(layout.depth / 8)
                + profile_length as u64 * 64,
        })
    }
    pub fn profile(
        &self,
        bytes: &[u8],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Arc<IccProfile>, String> {
        job.check()?;
        admit(
            self.input_length as u64 + bytes.len() as u64,
            self.scratch_bytes,
            other,
            limit,
        )?;
        job.reserve(self.scratch_bytes)?;
        let profile = IccProfile::parse(bytes)
            .ok_or("host pixel profile is not a supported RGB ICC profile")?;
        if profile.bytes().as_ref() != bytes || profile.model().is_none() {
            return Err(
                "host pixel profile requires exact complete RGB matrix/TRC ICC bytes".into(),
            );
        }
        Ok(Arc::new(profile))
    }
    /// The stored bytes of output pixel `index` (row-major, oriented).
    fn stored<'a>(&self, input: &'a [u8], index: usize) -> &'a [u8] {
        let x = index % self.width as usize;
        let y = index / self.width as usize;
        let (sx, sy) = crate::pixel_orientation::source(
            x,
            y,
            self.stored_width as usize,
            self.stored_height as usize,
            self.orientation,
        );
        let stride = self.layout.stride();
        let at = sy * self.row_bytes + sx * stride;
        &input[at..at + stride]
    }
    fn at(&self, input: &[u8], index: usize) -> [u16; 4] {
        self.layout.pixel(self.stored(input, index))
    }
    /// Output pixel `index` of an `rgba32f_*` layout, raw (not straightened).
    pub(crate) fn float_at(&self, input: &[u8], index: usize) -> [f32; 4] {
        self.layout.float_raw(self.stored(input, index))
    }
    /// Output pixel `index` of a 16-bit layout as straight full-scale u16
    /// codes: the layout's read, then a left-aligned 12-bit layout's check,
    /// widening and division. The one per-pixel rule `typed` and the HDR
    /// float intake's PQ/HLG signal read share.
    pub(crate) fn codes(&self, input: &[u8], index: usize) -> Result<[u16; 4], String> {
        let mut pixel = self.at(input, index);
        if self.layout.left12 {
            if pixel[..3].iter().any(|sample| sample & 0xf != 0) {
                return Err(
                    "12-bit left-aligned host samples must have their low 4 bits clear".into(),
                );
            }
            let alpha = pixel[3];
            for sample in &mut pixel[..3] {
                *sample = HostLayout::expand12(*sample);
                if self.layout.premultiplied {
                    *sample = HostLayout::straight(*sample, alpha, 65535);
                }
            }
        }
        Ok(pixel)
    }
    pub fn byte(
        &self,
        input: &[u8],
        profile: Arc<IccProfile>,
        job: JobContext<'_>,
    ) -> Result<crate::doc::DrDocument, String> {
        if self.layout.depth != 8 || input.len() != self.input_length {
            return Err("byte host intake requires a genuine byte layout".into());
        }
        let count = self.width as usize * self.height as usize;
        let mut bytes = reserved_vec(count * 4)?;
        for index in 0..count {
            job.every(index)?;
            bytes.extend(self.at(input, index).map(|c| c as u8));
        }
        let image = image::RgbaImage::from_raw(self.width, self.height, bytes)
            .ok_or("host byte output extent mismatch")?;
        let mut doc = crate::doc::DrDocument::from_pixels(image);
        doc.profile = profile;
        job.check()?;
        Ok(doc)
    }
    pub fn typed(
        &self,
        input: &[u8],
        profile: Arc<IccProfile>,
        job: JobContext<'_>,
    ) -> Result<crate::precision_document::PrecisionDocument, String> {
        if self.layout.depth != 16 || input.len() != self.input_length {
            return Err("typed host intake requires a genuine u16 layout".into());
        }
        let count = self.width as usize * self.height as usize;
        let mut pixels = reserved_vec(count)?;
        for index in 0..count {
            job.every(index)?;
            pixels.push(self.codes(input, index)?);
        }
        let image = crate::precision_image::PrecisionImage::from_rgba16_unseeded(
            self.width,
            self.height,
            pixels,
            profile,
            None,
        )?;
        let doc = crate::precision_document::PrecisionDocument::from_image(
            image,
            crate::metadata::Metadata::default(),
            crate::metadata::Resolution::default(),
        )?;
        job.check()?;
        Ok(doc)
    }
}
