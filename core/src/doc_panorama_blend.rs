//! Auto-Blend Layers ▸ Panorama's write-back on the byte document:
//! after Seamless Tones and Colors, each frame's own raster holds the
//! multi-band blend where its mask reveals it and its gained picture
//! elsewhere (`doc_auto_blend`'s module doc, "Panorama").
//!
//! Each frame keeps its size, offset, mask and alpha bit for bit; its RGB is
//! `(v.clamp(0, 1) · 255).round()` of the blend at a canvas pixel whose label
//! is that frame, else of its own sample through `panorama_gain::gained`
//! (inside the canvas or not). No layer is added. `precision_panorama_blend`
//! is the same at 16 bits and float.

use image::RgbaImage;

use crate::doc::DrDocument;
use crate::job_control::JobContext;
use crate::panorama_gain::gained;
use crate::precision_image::reserved_vec;

/// A unit value as the byte it stores.
fn byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

impl DrDocument {
    /// The frames `frames` (layer indices, ascending; label `k` is
    /// `frames[k]`) rewritten from `blended` (`focus_stack::blend` through
    /// `labels`) and `gains` (module doc). `Err` for planes that are not the
    /// canvas, a refused allocation, a frame the document cannot rewrite, or a
    /// cancel.
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
        let mut doc = self.clone();
        for (k, &index) in frames.iter().enumerate() {
            let layer = &self.layers[index];
            let (fw, fh) = layer.pixels.dimensions();
            let (ox, oy) = (i64::from(layer.offset.0), i64::from(layer.offset.1));
            let mut bytes: Vec<u8> = reserved_vec(fw as usize * fh as usize * 4)?;
            let mut row: Vec<[f32; 4]> = reserved_vec(fw as usize)?;
            row.resize(fw as usize, [0.0; 4]);
            let mut alpha: Vec<u8> = reserved_vec(fw as usize)?;
            alpha.resize(fw as usize, 0);
            for y in 0..fh {
                job.check()?;
                for (x0, run) in layer.pixels.row_segments(y, 0, fw) {
                    let x0 = x0 as usize;
                    for (i, p) in run.iter().enumerate() {
                        row[x0 + i] = p.map(|v| f32::from(v) / 255.0);
                        alpha[x0 + i] = p[3];
                    }
                }
                gained(gains[k], &mut row);
                let cy = i64::from(y) + oy;
                for (x, (p, &a)) in row.iter().zip(&alpha).enumerate() {
                    let cx = x as i64 + ox;
                    let inside = (0..cw as i64).contains(&cx) && (0..ch as i64).contains(&cy);
                    let at = if inside {
                        Some(cy as usize * cw + cx as usize)
                    } else {
                        None
                    };
                    let rgb = match at {
                        Some(at) if usize::from(labels[at]) == k => blended[at],
                        _ => *p,
                    };
                    bytes.extend([byte(rgb[0]), byte(rgb[1]), byte(rgb[2]), a]);
                }
            }
            let image = RgbaImage::from_raw(fw, fh, bytes)
                .ok_or("Auto-Blend Layers could not rebuild a frame")?;
            doc = doc.with_layer_pixels(index, image).ok_or_else(|| {
                format!(
                    "Auto-Blend Layers could not rewrite the pixels of “{}”.",
                    layer.name
                )
            })?;
        }
        Ok(doc)
    }
}
