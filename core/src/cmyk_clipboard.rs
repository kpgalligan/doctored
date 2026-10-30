//! Copy, Copy Merged, Cut, Paste (Paste in Place, Paste Into, Paste
//! Outside) and New from Clipboard on a CMYK document (#547 phase 4c), over
//! [`CmykClipboardPayload`] (`cmyk_clipboard_codec`). The RGB twin is
//! `precision_clipboard`, whose module doc is the contract; every rule here
//! is that one in ink.
//!
//! * **Copy** reads a CMYK raster record's RAW ink through its offset
//!   (opacity, blend mode and mask not applied; zeros where it does not
//!   reach) or, for a group, the group's RAW projection
//!   (`raw_group_document`, composited through `cmyk_stack` over the
//!   window). A CMYK adjustment record refuses with the RGB Copy's sentence.
//!   **Copy Merged** reads the document's composite over the window through
//!   the same stack — `cmyk_merge::cmyk_frame_image`'s row walk. Every read
//!   multiplies alpha by the selection's coverage (`cover_u16`, the rounded
//!   integer product) and carries the document's policy and declared depth.
//! * **Cut** refuses a group by name, then a pixels lock, then Copies and
//!   erases the same coverage through `CmykPaintEdit`'s erase at opacity 1
//!   (exactly Clear in ink), so a transparency lock answers `(payload, None)`.
//! * **Paste** inserts the payload as one `raster_cmyk` record at
//!   `insert_slot`'s slot, with `precision_clipboard`'s placement and frame
//!   rules. A payload of the SAME profile bytes keeps its ink bit for bit at
//!   any declared depth; another CMYK profile converts under Convert and is
//!   relabelled under Preserve (`cmyk_payload_convert`). An RGB payload is
//!   converted into this document before it gets here (`ffi_clipboard`).
//! * **New from Clipboard** builds one "Background" CMYK record at the
//!   payload's own size, policy, depth and PPI.
//!
//! # Admission
//!
//! Every read charges bookkeeping, then the host's stage (three payloads and
//! `PUBLIC_RENDERING_BYTES_PER_PIXEL` a window pixel, `precision_clipboard_host`'s
//! rule over the CMYK payload), then `2 · payload` (the payload and the
//! handle's encoding), a composite read also the stack's scratch and one ink
//! row, before it allocates. A paste charges the conversion, then the
//! insertion — `clipboard_paste`'s structure term plus the display
//! preparation of the new projection, the layers paste's own measured margin
//! (`doc_paste_layers::LAYER_PASTE_MARGIN_PER_PIXEL` a canvas pixel: phase
//! 4c's `paste-16` measured 1.15 GB at 24 MP, which the adjustment plans'
//! 20 B/px display term missed at 484 MB) — with the converted payload
//! counted as other retained bytes. [`PrecisionDocument::cmyk_clipboard_plans`] reports the
//! canvas-sized cases (`get_precision`'s CMYK `plans`,
//! `scripts/cmyk-admission`).

use std::sync::Arc;

use serde_json::{json, Value};

use crate::cmyk_clipboard_codec::{cmyk_payload_bytes, CmykClipboardPayload};
use crate::cmyk_composite::codes;
use crate::cmyk_document::refused_sentence;
use crate::cmyk_image::{CmykImage, PAPER};
use crate::cmyk_paint::CmykPaintEdit;
use crate::doc_lock::EditKind;
use crate::job_control::JobContext;
use crate::metadata::Metadata;
use crate::precision_clipboard::{cover_u16, ClipboardRead, PastePlacement, PasteRequest};
use crate::precision_clipboard_codec::payload_bytes;
use crate::precision_clipboard_host::PUBLIC_RENDERING_BYTES_PER_PIXEL;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{
    admit, pixel_count, reserved_vec, PrecisionStorage, BOOKKEEPING_BYTES,
};

const OVERFLOW: &str = "clipboard size overflow";

/// The host's stage of one CMYK read: `precision_clipboard_host`'s rule
/// (`host_stage_bytes`, the master) over the CMYK payload — three payloads
/// and the fitted rendering cost a window pixel.
fn cmyk_host_stage_bytes(width: u32, height: u32, covered: bool) -> Result<u64, String> {
    let pixels = pixel_count(width, height)? as u64;
    cmyk_payload_bytes(width, height, covered)
        .checked_mul(3)
        .and_then(|n| n.checked_add(PUBLIC_RENDERING_BYTES_PER_PIXEL.checked_mul(pixels)?))
        .ok_or_else(|| OVERFLOW.into())
}

/// A request's window, refused unless non-empty and inside the canvas —
/// `precision_clipboard::validated_window`'s sentence (the master copy).
fn validated_window(
    doc: &PrecisionDocument,
    read: &ClipboardRead,
) -> Result<(u32, u32, u32, u32), String> {
    let (x, y, width, height) = match *read {
        ClipboardRead::Copy {
            x,
            y,
            width,
            height,
            ..
        }
        | ClipboardRead::CopyMerged {
            x,
            y,
            width,
            height,
        } => (x, y, width, height),
    };
    let (w, h) = doc.dimensions();
    if width == 0
        || height == 0
        || u64::from(x) + u64::from(width) > u64::from(w)
        || u64::from(y) + u64::from(height) > u64::from(h)
    {
        return Err(format!(
            "clipboard window {width}×{height} at ({x}, {y}) is empty or leaves the {w}×{h} canvas"
        ));
    }
    Ok((x, y, width, height))
}

/// The placement and frame checks every paste makes before anything else
/// (`precision_clipboard::clipboard_paste`'s, the master copy): `into`
/// requires a positive-size frame, and a frame is valid only with `into`.
pub(crate) fn paste_request_refusal(request: &PasteRequest) -> Result<(), String> {
    match (request.placement, request.frame) {
        (PastePlacement::Into, Some([_, _, fw, fh])) if fw > 0 && fh > 0 => Ok(()),
        (PastePlacement::Into, _) => {
            Err("placement into requires a frame [x, y, width, height] with a positive size".into())
        }
        (_, Some(_)) => Err("a paste frame is only valid with placement into".into()),
        (_, None) => Ok(()),
    }
}

/// Where a `w × h` payload lands on a `canvas` (`clipboard_paste`'s rule,
/// the master copy): its own offset in place, centred on the canvas, or
/// centred on the request's frame with the same floor.
fn paste_offset(
    request: &PasteRequest,
    canvas: (u32, u32),
    own: (i32, i32),
    (w, h): (u32, u32),
) -> Result<(i32, i32), String> {
    let centred = |origin: i64, span: i64, size: u32| {
        span.checked_sub(i64::from(size))
            .map(|d| d.div_euclid(2))
            .and_then(|d| origin.checked_add(d))
            .and_then(|v| i32::try_from(v).ok())
            .ok_or_else(|| "paste offset overflow".to_string())
    };
    match request.placement {
        PastePlacement::InPlace => Ok(own),
        PastePlacement::Centered => Ok((
            centred(0, i64::from(canvas.0), w)?,
            centred(0, i64::from(canvas.1), h)?,
        )),
        PastePlacement::Into => {
            let [fx, fy, fw, fh] = request.frame.ok_or("placement into requires a frame")?;
            Ok((centred(fx, fw, w)?, centred(fy, fh, h)?))
        }
    }
}

/// One window of the canvas and its optional canvas-sized coverage.
struct Window<'a> {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    canvas_width: u32,
    coverage: Option<&'a [u16]>,
}

impl Window<'_> {
    fn coverage_at(&self, px: u32, py: u32) -> u16 {
        self.coverage.map_or(u16::MAX, |c| {
            c[(self.y + py) as usize * self.canvas_width as usize + (self.x + px) as usize]
        })
    }

    fn cropped_coverage(&self, job: JobContext<'_>) -> Result<Option<Arc<Vec<u16>>>, String> {
        if self.coverage.is_none() {
            return Ok(None);
        }
        let mut out = reserved_vec(pixel_count(self.width, self.height)?)?;
        for py in 0..self.height {
            job.check()?;
            for px in 0..self.width {
                out.push(self.coverage_at(px, py));
            }
        }
        Ok(Some(Arc::new(out)))
    }

    /// `image` placed at `offset`, read raw over the window (paper where it
    /// does not reach), alpha covered — `precision_clipboard`'s
    /// `Window::u16` in ink.
    fn raw(
        &self,
        image: &CmykImage,
        offset: (i32, i32),
        job: JobContext<'_>,
    ) -> Result<CmykImage, String> {
        let (lw, lh) = image.dimensions();
        let a = i64::from(self.x) - i64::from(offset.0);
        let (lx0, lx1) = (
            a.clamp(0, i64::from(lw)),
            (a + i64::from(self.width)).clamp(0, i64::from(lw)),
        );
        CmykImage::from_rows_with_job(
            self.width,
            self.height,
            |py, row| {
                row.fill(PAPER);
                let ly = i64::from(self.y) + i64::from(py) - i64::from(offset.1);
                if (0..i64::from(lh)).contains(&ly) && lx0 < lx1 {
                    for (lx, run) in image.row_segments(ly as u32, lx0 as u32, lx1 as u32) {
                        let at = (i64::from(lx) - a) as usize;
                        row[at..at + run.len()].copy_from_slice(run);
                    }
                }
                for (px, pixel) in row.iter_mut().enumerate() {
                    pixel[4] = cover_u16(pixel[4], self.coverage_at(px as u32, py));
                }
                job.check()
            },
            job,
        )
    }

    /// `source`'s composite over the window through the ONE row compositor
    /// (`cmyk_stack`, the walk `cmyk_frame_image` runs), alpha covered.
    fn composite(
        &self,
        source: &PrecisionDocument,
        job: JobContext<'_>,
    ) -> Result<CmykImage, String> {
        let mut stack = source.cmyk_stack(None, job)?;
        let mut acc = vec![[0f32; 5]; self.width as usize];
        CmykImage::from_rows_with_job(
            self.width,
            self.height,
            |py, row| {
                stack.row(
                    i64::from(self.y) + i64::from(py),
                    i64::from(self.x),
                    &mut acc,
                );
                for (px, (dst, ink)) in row.iter_mut().zip(acc.iter()).enumerate() {
                    *dst = codes(*ink);
                    dst[4] = cover_u16(dst[4], self.coverage_at(px as u32, py));
                }
                job.every(py as usize * self.width as usize)
            },
            job,
        )
    }
}

impl PrecisionDocument {
    /// Copy or Copy Merged of one canvas window on a CMYK document (module
    /// doc). `coverage` is canvas-sized u16 or `None` for a fully covered
    /// window.
    pub(crate) fn cmyk_clipboard_copy(
        &self,
        read: &ClipboardRead,
        coverage: Option<&[u16]>,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<CmykClipboardPayload, String> {
        job.check()?;
        admit(0, BOOKKEEPING_BYTES, other, limit)?;
        job.reserve(BOOKKEEPING_BYTES)?;
        let policy = self.require_cmyk_policy("Copy")?.clone();
        let (x, y, width, height) = validated_window(self, read)?;
        let canvas = pixel_count(self.width, self.height)?;
        if coverage.is_some_and(|c| c.len() != canvas) {
            return Err("clipboard coverage must be canvas-sized".into());
        }
        let covered = coverage.is_some();
        admit(
            0,
            cmyk_host_stage_bytes(width, height, covered)?,
            other,
            limit,
        )?;
        let window = Window {
            x,
            y,
            width,
            height,
            canvas_width: self.width,
            coverage,
        };
        let held = 2 * cmyk_payload_bytes(width, height, covered);
        let image = match *read {
            ClipboardRead::Copy { layer, .. } => {
                let record = self
                    .layer(layer)
                    .ok_or("clipboard layer index is out of range")?;
                match &record.content {
                    Content::CmykRaster { image, offset } => {
                        admit(0, held, other, limit)?;
                        job.reserve(held)?;
                        window.raw(image, *offset, job)?
                    }
                    Content::Group(_) | Content::PassThrough(_) => {
                        let source = self.raw_group_document(layer)?;
                        self.cmyk_read_composite(&source, &window, held, limit, other, job)?
                    }
                    Content::CmykAdjustment(_) | Content::Adjustment(_) => {
                        return Err(format!(
                            "Copy requires a raster layer; layer {layer} is an adjustment layer (use Copy Merged)"
                        ))
                    }
                    Content::Raster { .. } => return Err(refused_sentence("An RGB layer")),
                }
            }
            ClipboardRead::CopyMerged { .. } => {
                self.cmyk_read_composite(self, &window, held, limit, other, job)?
            }
        };
        let coverage = window.cropped_coverage(job)?;
        job.report(1.0, 5)?;
        Ok(CmykClipboardPayload {
            image,
            depth: policy.depth(),
            policy,
            offset: (x as i32, y as i32),
            resolution: self.resolution(),
            coverage,
            editable: None,
        })
    }

    /// A composite read's admission and read: the payload and its encoding,
    /// the stack's scratch over the window's rows and one ink row.
    #[allow(clippy::too_many_arguments)]
    fn cmyk_read_composite(
        &self,
        source: &PrecisionDocument,
        window: &Window<'_>,
        held: u64,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<CmykImage, String> {
        let scratch = held
            .checked_add(source.cmyk_stack_scratch_bytes_over(window.width)?)
            .and_then(|n| n.checked_add(u64::from(window.width) * 20))
            .ok_or(OVERFLOW)?;
        admit(0, scratch, other, limit)?;
        job.reserve(scratch)?;
        window.composite(source, job)
    }

    /// Cut on a CMYK document (module doc): `(payload, None)` when the
    /// erase changed nothing (a transparency lock, a transparent window).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn cmyk_clipboard_cut(
        &self,
        layer: usize,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        coverage: Option<&[u16]>,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<(CmykClipboardPayload, Option<Self>), String> {
        if self
            .layer(layer)
            .is_some_and(|record| record.children().is_some())
        {
            return Err(format!(
                "Cut requires a raster layer; layer {layer} is a group"
            ));
        }
        self.lock_refusal(layer, EditKind::Pixels)?;
        let read = ClipboardRead::Copy {
            layer,
            x,
            y,
            width,
            height,
        };
        let payload =
            self.cmyk_clipboard_copy(&read, coverage, limit, other, job.range(0.0, 0.4))?;
        let held = 2 * payload.payload_bytes();
        admit(0, held, other, limit)?;
        let canvas = pixel_count(self.width, self.height)?;
        let full;
        let erase_coverage = match coverage {
            Some(c) => c,
            None => {
                admit(
                    0,
                    canvas as u64 * 2,
                    other.checked_add(held).ok_or(OVERFLOW)?,
                    limit,
                )?;
                let mut plane = reserved_vec(canvas)?;
                plane.resize(canvas, u16::MAX);
                full = plane;
                &full
            }
        };
        let erase_other = other
            .checked_add(held)
            .and_then(|n| {
                n.checked_add(if coverage.is_none() {
                    canvas as u64 * 2
                } else {
                    0
                })
            })
            .ok_or(OVERFLOW)?;
        let erase = CmykPaintEdit::EraseNative {
            layer,
            opacity: 1.0,
            selection: false,
        };
        let document = erase.apply(
            self,
            erase_coverage,
            limit,
            erase_other,
            job.range(0.4, 1.0),
        )?;
        Ok((payload, document))
    }

    /// Paste of a CMYK payload as one `raster_cmyk` record (module doc).
    pub(crate) fn cmyk_clipboard_paste(
        &self,
        payload: &CmykClipboardPayload,
        request: &PasteRequest,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        job.check()?;
        paste_request_refusal(request)?;
        admit(0, BOOKKEEPING_BYTES, other, limit)?;
        job.reserve(BOOKKEEPING_BYTES)?;
        let policy = self.require_cmyk_policy("Paste")?.clone();
        let mut other = other;
        let converted = payload.converted_with(
            &policy,
            policy.depth(),
            request.profile,
            limit,
            other,
            job.range(0.0, 0.8),
        )?;
        if let Some(next) = &converted {
            other = other
                .checked_add(2 * next.payload_bytes())
                .ok_or(OVERFLOW)?;
        }
        let payload = converted.as_ref().unwrap_or(payload);
        let (w, h) = payload.dimensions();
        let offset = paste_offset(request, self.dimensions(), payload.offset(), (w, h))?;
        let slot = self.insert_slot(request.anchor, request.position)?;
        self.preflight_raster_addition((w, h), &request.name)?;
        let scratch = self.cmyk_paste_insertion(&request.name)?;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let record = PrecisionDocumentLayer::raster_cmyk(
            &request.name,
            payload.image().clone(),
            offset,
            1.0,
            true,
        )?;
        let next = self.inserting_record(record, slot)?;
        job.report(1.0, 5)?;
        Ok(next)
    }

    /// A paste's insertion: `clipboard_paste`'s structure term (every name
    /// twice, a record's bookkeeping each) and the new projection's display
    /// preparation (`LAYER_PASTE_MARGIN_PER_PIXEL` a canvas pixel, module doc).
    fn cmyk_paste_insertion(&self, name: &str) -> Result<u64, String> {
        let names: u64 = (0..self.layer_count())
            .filter_map(|i| self.layer(i))
            .map(|layer| layer.name().len() as u64)
            .sum();
        let canvas = pixel_count(self.width, self.height)? as u64;
        (4 * BOOKKEEPING_BYTES + (names + name.len() as u64) * 2)
            .checked_add(self.layer_count() as u64 * 4096)
            .and_then(|n| {
                n.checked_add(
                    canvas.checked_mul(crate::doc_paste_layers::LAYER_PASTE_MARGIN_PER_PIXEL)?,
                )
            })
            .ok_or_else(|| OVERFLOW.into())
    }

    /// New from Clipboard of a CMYK payload: one "Background" CMYK record at
    /// (0, 0), the payload's own size, policy, declared depth and PPI,
    /// `Metadata::default()` (`from_clipboard`'s rule).
    pub(crate) fn cmyk_from_clipboard(
        payload: &CmykClipboardPayload,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        job.check()?;
        admit(0, 4 * BOOKKEEPING_BYTES, other, limit)?;
        job.reserve(4 * BOOKKEEPING_BYTES)?;
        let (w, h) = payload.dimensions();
        let leaf = PrecisionDocumentLayer::raster_cmyk(
            "Background",
            payload.image().clone(),
            (0, 0),
            1.0,
            true,
        )?;
        let document = Self::build_cmyk(
            w,
            h,
            Metadata::default(),
            payload.resolution(),
            vec![leaf],
            Vec::new(),
            payload.policy().clone(),
        )?;
        job.report(1.0, 5)?;
        Ok(document)
    }

    /// The NEW memory of one CMYK read (module doc): the host stage, or the
    /// read's own charges, whichever is larger.
    pub(crate) fn cmyk_clipboard_read_allocation(
        &self,
        read: &ClipboardRead,
        covered: bool,
    ) -> Result<u64, String> {
        let (_, _, width, height) = validated_window(self, read)?;
        let host = cmyk_host_stage_bytes(width, height, covered)?;
        let held = 2 * cmyk_payload_bytes(width, height, covered);
        let composite = |source: &PrecisionDocument| -> Result<u64, String> {
            held.checked_add(source.cmyk_stack_scratch_bytes_over(width)?)
                .and_then(|n| n.checked_add(u64::from(width) * 20))
                .ok_or_else(|| OVERFLOW.to_string())
        };
        let core = match *read {
            ClipboardRead::Copy { layer, .. } => {
                let record = self
                    .layer(layer)
                    .ok_or("clipboard layer index is out of range")?;
                match &record.content {
                    Content::CmykRaster { .. } => held,
                    Content::Group(_) | Content::PassThrough(_) => {
                        composite(&self.raw_group_document(layer)?)?
                    }
                    _ => {
                        return Err(format!(
                            "Copy requires a raster layer; layer {layer} is an adjustment layer (use Copy Merged)"
                        ))
                    }
                }
            }
            ClipboardRead::CopyMerged { .. } => composite(self)?,
        };
        Ok(BOOKKEEPING_BYTES + host.max(core))
    }

    /// The canvas-sized clipboard and structure plans `get_precision`'s CMYK
    /// `plans` carry (module doc), each `None` where the edit refuses: Copy,
    /// Copy Merged and Cut of the top raster with a selection; a same-profile
    /// Paste of that copy, a converting one (another CMYK profile, or RGB at
    /// 16 bits, whose terms are the same), and that copy pasted into a 16-bit
    /// RGB document of the same canvas; Layer Via Copy and Cut of the top
    /// raster; a converting layers paste of every root; Stamp Selected of
    /// every root and Merge Group of the topmost group.
    pub(crate) fn cmyk_clipboard_plans(&self) -> Value {
        let (w, h) = self.dimensions();
        let top = (0..self.layer_count())
            .rev()
            .find(|&i| self.layer(i).and_then(|l| l.cmyk_source()).is_some());
        let group = (0..self.layer_count())
            .rev()
            .find(|&i| self.layer(i).is_some_and(|l| l.children().is_some()));
        let read = |read: ClipboardRead| self.cmyk_clipboard_read_allocation(&read, true).ok();
        let copy = top.and_then(|layer| {
            read(ClipboardRead::Copy {
                layer,
                x: 0,
                y: 0,
                width: w,
                height: h,
            })
        });
        let merged = read(ClipboardRead::CopyMerged {
            x: 0,
            y: 0,
            width: w,
            height: h,
        });
        let canvas = u64::from(w) * u64::from(h);
        let held = 2 * cmyk_payload_bytes(w, h, true);
        let cut = top.and_then(|layer| {
            let erase = CmykPaintEdit::EraseNative {
                layer,
                opacity: 1.0,
                selection: false,
            }
            .plan(self)
            .ok()?["scratch_bytes"]
                .as_u64()?;
            Some(copy?.max(held + erase))
        });
        let insertion = self.cmyk_paste_insertion("Layer").ok();
        let convert = insertion.and_then(|insertion| {
            let leg = canvas
                .checked_mul(crate::cmyk_convert::TO_CMYK_BYTES_PER_PIXEL)?
                .checked_add(crate::soft_proof::memo_peak_bytes_for::<[f64; 4]>(canvas))?;
            Some((leg + BOOKKEEPING_BYTES).max(insertion + held))
        });
        // The RGB destination's paste: the conversion's leg, or the RGB
        // payload held twice beside the display preparation of a canvas of
        // this size (this document's insertion margin standing for it: the
        // RGB `clipboard_paste` charges no display term at 16 bits).
        let to_rgb = {
            let leg = canvas * crate::cmyk_convert::TO_RGB_BYTES_PER_PIXEL
                + crate::soft_proof::memo_peak_bytes(canvas)
                + BOOKKEEPING_BYTES;
            leg.max(
                2 * payload_bytes(w, h, PrecisionStorage::Rgba16Unorm, true)
                    + insertion.unwrap_or(0),
            )
        };
        let via = |cut: bool| {
            top.and_then(|layer| {
                crate::precision_layer_via::LayerViaEdit::parse(&format!(
                    r#"{{"op":"layer_via","layer":{layer},"cut":{cut},"name":"Layer","selection":true}}"#
                ))
                .and_then(|edit| edit.plan(self))
                .ok()
                .and_then(|plan| plan["scratch_bytes"].as_u64())
            })
        };
        let roots: Vec<usize> = {
            let mut out = Vec::new();
            let mut i = self.layer_count();
            let flat = crate::precision_structure_ops::flatten(self.root_layers());
            while i > 0 {
                i -= 1;
                if flat[i].depth == 0 {
                    out.push(i);
                }
            }
            out.reverse();
            out
        };
        let paste_layers = self.cmyk_paste_layers_convert_plan(&roots).ok();
        let structure = |request: String| {
            crate::precision_structure::StructureEdit::parse(&request)
                .and_then(|edit| edit.scratch_bytes(self))
                .ok()
        };
        let list = roots
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");
        json!({
            "cmyk_copy_scratch_bytes": copy,
            "cmyk_copy_merged_scratch_bytes": merged,
            "cmyk_cut_scratch_bytes": cut,
            "cmyk_paste_scratch_bytes": insertion,
            "cmyk_paste_convert_scratch_bytes": convert,
            "cmyk_paste_to_rgb_scratch_bytes": to_rgb,
            "cmyk_layer_via_copy_scratch_bytes": via(false),
            "cmyk_layer_via_cut_scratch_bytes": via(true),
            "cmyk_paste_layers_convert_scratch_bytes": paste_layers,
            "cmyk_stamp_selected_scratch_bytes": structure(format!(
                r#"{{"op":"stamp_selected","layers":[{list}],"name":"Layer (merged)"}}"#
            )),
            "cmyk_merge_group_scratch_bytes": group.and_then(|g| structure(format!(
                r#"{{"op":"merge_group","layer":{g}}}"#
            ))),
        })
    }
}
