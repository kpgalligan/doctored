//! Deliberate RGB depth reduction, shared by byte export and document conversion.
//! Ordered dithering is a fixed canvas-anchored 8 × 8 Bayer threshold. It changes
//! neither alpha nor the captured profile, and an exact byte code promoted by
//! 257 survives every threshold unchanged. There is no random state or feedback.
//!
//! Byte alpha is rounded without dithering.

use image::RgbaImage;

use crate::job_control::{DrJobControl, JobContext};
use crate::precision_image::{
    admit, reserved_vec, PrecisionImage, PrecisionRun, PrecisionStorage, BOOKKEEPING_BYTES,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrecisionDither {
    None,
    Ordered,
}

impl PrecisionDither {
    pub fn from_code(code: u32) -> Result<Self, String> {
        match code {
            0 => Ok(Self::None),
            1 => Ok(Self::Ordered),
            _ => Err("unsupported precision dither; use none (0) or ordered (1)".into()),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Ordered => "ordered_8x8_v1",
        }
    }
}

const BAYER: [u32; 64] = [
    0, 32, 8, 40, 2, 34, 10, 42, 48, 16, 56, 24, 50, 18, 58, 26, 12, 44, 4, 36, 14, 46, 6, 38, 60,
    28, 52, 20, 62, 30, 54, 22, 3, 35, 11, 43, 1, 33, 9, 41, 51, 19, 59, 27, 49, 17, 57, 25, 15,
    47, 7, 39, 13, 45, 5, 37, 63, 31, 55, 23, 61, 29, 53, 21,
];

pub(crate) fn nearest(value: u16) -> u8 {
    ((u32::from(value) + 128) / 257) as u8
}

impl PrecisionImage {
    pub fn quantize_scratch_bytes(&self) -> Result<u64, String> {
        if self.storage() != PrecisionStorage::Rgba16Unorm {
            return Err("byte quantization requires rgba16_unorm".into());
        }
        let (width, height) = self.dimensions();
        Ok(BOOKKEEPING_BYTES + u64::from(width) * u64::from(height) * 4)
    }

    /// Explicit lossy byte output. Alpha rounds to nearest without dithering;
    /// RGB is straight and remains meaningful even when alpha rounds to zero.
    pub fn quantize(
        &self,
        dither: PrecisionDither,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<RgbaImage, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        admit(
            self.retained_bytes(),
            self.quantize_scratch_bytes()?,
            other,
            limit,
        )?;
        job.reserve(self.quantize_scratch_bytes()?)?;
        self.quantize_with_context(dither, job)
    }

    pub(crate) fn quantize_with_context(
        &self,
        dither: PrecisionDither,
        job: JobContext<'_>,
    ) -> Result<RgbaImage, String> {
        if self.storage() != PrecisionStorage::Rgba16Unorm {
            return Err("byte quantization requires rgba16_unorm".into());
        }
        let (width, height) = self.dimensions();
        let count = width as usize * height as usize;
        let mut bytes = reserved_vec(count * 4)?;
        for y in 0..height {
            for (x0, run) in self.row_segments(y, 0, width) {
                // Gated above: a u16 plane answers only u16 runs.
                let PrecisionRun::Rgba16(run) = run else {
                    continue;
                };
                for (dx, pixel) in run.iter().enumerate() {
                    let x = x0 as usize + dx;
                    let i = y as usize * width as usize + x;
                    if i & 4095 == 0 {
                        job.report(i as f64 / count as f64, 6)?;
                    }
                    let rank = BAYER[(y as usize & 7) * 8 + (x & 7)];
                    for value in &pixel[..3] {
                        bytes.push(match dither {
                            PrecisionDither::None => nearest(*value),
                            // floor(value / 257 + (rank + 0.5) / 64), exactly in integers.
                            PrecisionDither::Ordered => {
                                ((u32::from(*value) * 128 + (2 * rank + 1) * 257) / (257 * 128))
                                    .min(255) as u8
                            }
                        });
                    }
                    bytes.push(nearest(pixel[3]));
                }
            }
        }
        job.report(1.0, 5)?;
        RgbaImage::from_raw(width, height, bytes).ok_or("invalid quantized image dimensions".into())
    }
}
