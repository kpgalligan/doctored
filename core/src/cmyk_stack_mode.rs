//! Stack Modes in a CMYK document: the CMYK arm of
//! `precision_stack_mode`, over `cmyk_passes`' `L` domain.
//!
//! The rule is `stack_stat`'s, read per ink: each member's CMYKA16 row is
//! two RGBA16 rows, `InkPass::Cmy` `[L_c, L_m, L_y, α]` and `InkPass::K`
//! `[L_k, L_k, L_k, α]` with `L = 65535 − ink` (`cmyk_passes`' master
//! mapping), and `stack_row::<[u16; 4]>` runs on each, so every mode reads a
//! plate exactly as it reads a 16-bit RGB channel: Minimum is the lightest
//! (least ink), Maximum the darkest. The two results join through
//! `cmyk_passes::join_sample` on paper: C/M/Y = 65535 − A's R/G/B, K =
//! 65535 − B's G, α = A's (both passes read the same alphas, so they agree);
//! a pixel no member covers is paper at α 0.
//!
//! The result is ONE new CMYK raster at the extent's origin, spliced through
//! `PrecisionDocument::stacked_document` (the members that were visible are
//! hidden in the same rebuild). Refusals mirror the typed arm's: a member
//! out of range, a group, an adjustment layer (either kind) or an RGB
//! record, by index; then `stack_mode_frame`'s; the working limit; a cancel;
//! and members with no pixel anywhere.
//!
//! The plan is `stack_stat::plan_scratch` at 10 bytes a pixel (the
//! 8-bit factor, `OUTPUT_PLANE_COPIES_8`, the `_` arm) with two rows per
//! member, the two passes' rows.

use serde_json::{json, Value};

use crate::cmyk_image::{CmykImage, CmykSample, CMYK_BYTES_PER_PIXEL, PAPER};
use crate::cmyk_passes::{join_sample, InkPass};
use crate::job_control::JobContext;
use crate::layer_insert::InsertPosition;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, pixel_count, reserved_vec};
use crate::stack_stat::{plan_scratch, stack_row, StackModeRequest, StackScratch, NO_PIXELS};

/// What one request resolves to against a CMYK document.
struct CmykTerms<'a> {
    members: Vec<usize>,
    /// Each member's ink and canvas offset, in `members`' order.
    sources: Vec<(&'a CmykImage, (i32, i32))>,
    extent: (i32, i32, u32, u32),
    scratch: u64,
}

/// One member's two extent-wide pass rows for extent row `y`: transparent
/// everywhere, then the member's own row mapped through both passes.
fn fill_pass_rows(
    cmy: &mut [[u16; 4]],
    k: &mut [[u16; 4]],
    image: &CmykImage,
    offset: (i32, i32),
    extent: (i32, i32),
    y: u32,
) {
    cmy.fill([0; 4]);
    k.fill([0; 4]);
    let (lw, lh) = image.dimensions();
    let ly = i64::from(extent.1) + i64::from(y) - i64::from(offset.1);
    let Ok(at) = usize::try_from(i64::from(offset.0) - i64::from(extent.0)) else {
        return;
    };
    if lw == 0 || !(0..i64::from(lh)).contains(&ly) {
        return;
    }
    for (x, run) in image.row_segments(ly as u32, 0, lw) {
        let start = at + x as usize;
        for (i, s) in run.iter().enumerate() {
            if let (Some(a), Some(b)) = (cmy.get_mut(start + i), k.get_mut(start + i)) {
                *a = InkPass::Cmy.sample(*s);
                *b = InkPass::K.sample(*s);
            }
        }
    }
}

impl PrecisionDocument {
    fn cmyk_stack_mode_terms(&self, request: &StackModeRequest) -> Result<CmykTerms<'_>, String> {
        let members = request.members()?;
        let mut sources = Vec::with_capacity(members.len());
        for &index in &members {
            let record = self
                .layer(index)
                .ok_or_else(|| format!("stack mode layer {index} is out of range"))?;
            let (image, offset) = match &record.content {
                Content::CmykRaster { image, offset } => (image, *offset),
                Content::Group(_) | Content::PassThrough(_) => {
                    return Err(format!(
                        "Stack Mode requires raster layers; layer {index} is a group"
                    ))
                }
                Content::Adjustment(_) | Content::CmykAdjustment(_) => {
                    return Err(format!(
                        "Stack Mode requires raster layers; layer {index} is an adjustment layer"
                    ))
                }
                Content::Raster { .. } => {
                    return Err(format!(
                        "stack mode layer {index} does not hold the document's own storage"
                    ))
                }
            };
            sources.push((image, offset));
        }
        let (extent, structure) = self.stack_mode_frame(
            request,
            sources.iter().map(|(image, offset)| {
                let (w, h) = image.dimensions();
                (offset.0, offset.1, w, h)
            }),
        )?;
        let scratch = plan_scratch(
            u64::from(extent.2) * u64::from(extent.3),
            u64::from(extent.2),
            CMYK_BYTES_PER_PIXEL,
            2 * members.len() as u64,
            structure,
        )
        .ok_or("stack mode byte calculation overflow")?;
        Ok(CmykTerms {
            members,
            sources,
            extent,
            scratch,
        })
    }

    /// The CMYK `stack_mode_plan` (module doc): the same keys, the same
    /// refusals as the edit before it reads a pixel.
    pub(crate) fn cmyk_stack_mode_plan(&self, request: &StackModeRequest) -> Result<Value, String> {
        let terms = self.cmyk_stack_mode_terms(request)?;
        Ok(json!({"retained_bytes": self.retained_bytes(),
                  "scratch_bytes": terms.scratch, "input_samples": 0}))
    }

    /// A Stack Mode on ink (module doc): plan, admit against `limit` beside
    /// `other`, reserve, check, then one checkpoint and one progress report
    /// per extent row. `Err` for every refusal and for a cancel; never
    /// `Ok(None)`.
    pub(crate) fn cmyk_stack_mode(
        &self,
        request: &StackModeRequest,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let terms = self.cmyk_stack_mode_terms(request)?;
        admit(self.retained_bytes(), terms.scratch, other, limit)?;
        job.reserve(terms.scratch)?;
        job.check()?;
        let slot = self.insert_slot(request.above, InsertPosition::Above)?;
        let (ex, ey, w, h) = terms.extent;
        let width = w as usize;
        let mut plane: Vec<CmykSample> = reserved_vec(pixel_count(w, h)?)?;
        let mut cmy_rows: Vec<Vec<[u16; 4]>> = Vec::new();
        let mut k_rows: Vec<Vec<[u16; 4]>> = Vec::new();
        for _ in &terms.sources {
            for rows in [&mut cmy_rows, &mut k_rows] {
                let mut row = reserved_vec(width)?;
                row.resize(width, [0; 4]);
                rows.push(row);
            }
        }
        let mut out_a: Vec<[u16; 4]> = reserved_vec(width)?;
        out_a.resize(width, [0; 4]);
        let mut out_b: Vec<[u16; 4]> = reserved_vec(width)?;
        out_b.resize(width, [0; 4]);
        let mut scratch = StackScratch::new(terms.sources.len())?;
        let mut any = false;
        for y in 0..h {
            job.check()?;
            for ((cmy, k), (image, offset)) in cmy_rows
                .iter_mut()
                .zip(k_rows.iter_mut())
                .zip(&terms.sources)
            {
                fill_pass_rows(cmy, k, image, *offset, (ex, ey), y);
            }
            let views: Vec<&[[u16; 4]]> = cmy_rows.iter().map(Vec::as_slice).collect();
            any |= stack_row(request.mode, &views, &mut out_a, &mut scratch);
            let views: Vec<&[[u16; 4]]> = k_rows.iter().map(Vec::as_slice).collect();
            stack_row(request.mode, &views, &mut out_b, &mut scratch);
            plane.extend(
                out_a
                    .iter()
                    .zip(&out_b)
                    .map(|(a, b)| join_sample(PAPER, *a, Some(*b), false)),
            );
            job.report(f64::from(y + 1) / f64::from(h) * 0.9, 6)?;
        }
        if !any {
            return Err(NO_PIXELS.into());
        }
        let image = CmykImage::from_contiguous(w, h, &plane, job)?;
        drop(plane);
        let leaf = PrecisionDocumentLayer::raster_cmyk(&request.name, image, (ex, ey), 1.0, true)?;
        let next = self.stacked_document(leaf, &terms.members, slot, job)?;
        job.report(1.0, 5)?;
        Ok(Some(next))
    }
}
