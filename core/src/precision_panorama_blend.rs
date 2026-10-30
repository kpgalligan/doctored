//! Auto-Blend Layers ▸ Panorama's write-back at 16 bits and 32-bit float:
//! `doc_panorama_blend`'s rule in each frame's own storage.
//!
//! The kernel's samples are unit values (16 bits) or retouch codes over the
//! code of white ([`float_white_code`]; `precision_auto_blend`'s module doc,
//! "Panorama reads display units"), so a 16-bit frame stores `unit_to_u16(v)`
//! and a float frame `decode(v · white)`, the Stack Images merged record's
//! decode on Panorama's unit. Alpha is copied from the frame's own samples,
//! bit for bit.

use crate::job_control::JobContext;
use crate::panorama_gain::gained;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{reserved_vec, unit_to_u16, PrecisionRun};
use crate::retouch_encoding::{decode, encode};

/// The retouch code of diffuse white, Panorama's float unit.
pub(crate) fn float_white_code() -> f32 {
    encode(1.0)
}

impl PrecisionDocument {
    /// `DrDocument::panorama_frames` at high depth (module doc): the frames
    /// rewritten from `blended` where their label is theirs and from their
    /// gained samples elsewhere. `Err` for planes that are not the canvas, a
    /// refused allocation, a frame the document cannot rewrite, or a cancel.
    pub(crate) fn panorama_frames(
        &self,
        frames: &[usize],
        labels: &[u8],
        blended: &[[f32; 4]],
        gains: &[[f32; 3]],
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let (cw, ch) = (self.width as usize, self.height as usize);
        if labels.len() != cw * ch || blended.len() != cw * ch || gains.len() != frames.len() {
            return Err("Auto-Blend Layers' blend differs from the canvas".into());
        }
        let float = self.is_float();
        let white = float_white_code();
        let mut doc = self.clone();
        for (k, &index) in frames.iter().enumerate() {
            let layer = self.layer(index).ok_or("invalid typed layer")?;
            let image = layer.source().ok_or("invalid typed layer")?;
            let offset = layer.offset();
            let (fw, fh) = image.dimensions();
            let (ox, oy) = (i64::from(offset.0), i64::from(offset.1));
            let count = fw as usize * fh as usize;
            let mut codes16: Vec<[u16; 4]> = if float {
                Vec::new()
            } else {
                reserved_vec(count)?
            };
            let mut values32: Vec<[f32; 4]> = if float {
                reserved_vec(count)?
            } else {
                Vec::new()
            };
            let mut row: Vec<[f32; 4]> = reserved_vec(fw as usize)?;
            row.resize(fw as usize, [0.0; 4]);
            let mut alpha16: Vec<u16> = reserved_vec(fw as usize)?;
            alpha16.resize(fw as usize, 0);
            let mut alpha32: Vec<f32> = reserved_vec(fw as usize)?;
            alpha32.resize(fw as usize, 0.0);
            for y in 0..fh {
                job.check()?;
                for (x0, run) in image.row_segments(y, 0, fw) {
                    let x0 = x0 as usize;
                    for i in 0..run.len() {
                        let p = run.pixel(i);
                        row[x0 + i] = if float {
                            let code = |v: f32| encode(v) / white;
                            [code(p[0]), code(p[1]), code(p[2]), p[3]]
                        } else {
                            p
                        };
                        match run {
                            PrecisionRun::Rgba16(samples) => {
                                alpha16[x0 + i] = samples[i][3];
                                alpha32[x0 + i] = p[3];
                            }
                            PrecisionRun::Rgba32(samples) => {
                                alpha16[x0 + i] = unit_to_u16(p[3]);
                                alpha32[x0 + i] = samples[i][3];
                            }
                        }
                    }
                }
                gained(gains[k], &mut row);
                let cy = i64::from(y) + oy;
                for (x, p) in row.iter().enumerate() {
                    let cx = x as i64 + ox;
                    let inside = (0..cw as i64).contains(&cx) && (0..ch as i64).contains(&cy);
                    let at = if inside {
                        Some(cy as usize * cw + cx as usize)
                    } else {
                        None
                    };
                    let v = match at {
                        Some(at) if usize::from(labels[at]) == k => blended[at],
                        _ => *p,
                    };
                    if float {
                        values32.push([
                            decode(v[0] * white),
                            decode(v[1] * white),
                            decode(v[2] * white),
                            alpha32[x],
                        ]);
                    } else {
                        codes16.push([
                            unit_to_u16(v[0]),
                            unit_to_u16(v[1]),
                            unit_to_u16(v[2]),
                            alpha16[x],
                        ]);
                    }
                }
            }
            let rebuilt = if float {
                image.rebuilt_rgba32_extended(fw, fh, values32, image.profile().clone())?
            } else {
                image.rebuilt_rgba16(fw, fh, codes16, image.profile().clone())?
            };
            doc = doc
                .replacing_content(
                    index,
                    Content::Raster {
                        image: rebuilt,
                        offset,
                    },
                )?
                .ok_or_else(|| {
                    format!(
                        "Auto-Blend Layers could not rewrite the pixels of “{}”.",
                        layer.name()
                    )
                })?;
        }
        Ok(doc)
    }
}
