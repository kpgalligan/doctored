//! The display-defined paint tools of a 32-bit float document: the
//! Paint Bucket's region and Dodge/Burn's rule, which `float_paint`'s walk
//! reads. An adapter over `display_encoding` (the kernel that states R1
//! and R4) and `selection_region::grow` (the one anchored flood walk the
//! 8/16-bit bucket and the wand share).
//!
//! - **The bucket (R2, R3).** The region is the master's: `grow` from the
//!   seed over the CANVAS, comparing 16-bit codes with the request's
//!   tolerance (already ×257, as the host sends it at every depth). At float
//!   the codes are the layer's R1 display copy ([`bucket_region`]): the
//!   profile's forward curve over each sample clamped to 0…1, so a value
//!   above white compares as white, and `[0; 4]` off the layer, as the
//!   master reads there. The region is then filled in linear light by the
//!   float walk, in the float modes, with decoded document ink or `linear`
//!   ink as given.
//! - **Dodge/Burn (R4).** [`dodge_burn_rule`] builds
//!   `display_encoding::DodgeBurnLinear` (with Protect Tones' group rule when
//!   the request asks, #547) from the document's own encoder
//!   (the policy's, never the tone map) and decoder; the walk mixes each
//!   covered, non-transparent pixel toward its target.
//!
//! # Admission
//!
//! The bucket's plan adds [`BUCKET_REGION_BYTES_PER_PIXEL`] over the canvas
//! to the float walk's figure (`float_paint::plan_in`): the R1 codes (8 B),
//! the region (2 B) and the flood's queue (at most 8 B). Dodge/Burn plans the
//! walk's figure alone. Measured in Release, one case per process,
//! `MallocLargeCache=0`, a baseline twin and three runs
//! (`examples/float_paint_admission.rs`; 2026-10-12, Apple M2 Max), on a
//! 6000×4000 opaque float layer; `E` is the process's `phys_footprint` peak
//! over the interval that starts just before the call
//! (`proc_reset_footprint_interval`) less the footprint read there:
//! the bucket seeded at (0, 0) with tolerance 65535 over the whole canvas
//! (case `bucket`), Dodge at exposure 0.5, midtones, under canvas-wide
//! coverage of 65535 (case `dodge`), and Dodge on a 6000×4000 layer mask
//! through `scalar_edit` (case `mask_dodge`, `precision_scalar_edit`'s
//! plan):
//!
//! | case | plan A | E (three runs) | ⌈1.25 max E⌉ |
//! |---|---|---|---|
//! | bucket (BK32) | 1,976,388,608 | 818,053,720 / 818,037,336 / 818,053,720 | 1,022,567,150 |
//! | dodge (DB32) | 1,544,388,608 | 769,966,608 / 769,966,608 / 769,966,608 | 962,458,260 |
//! | mask_dodge (MD32) | 824,388,608 | 96,092,232 / 96,092,232 / 96,092,232 | 120,115,290 |
//!
//! Every twin read E 0. `A ≥ ⌈1.25 E⌉` in each, so no term moved; the floors
//! are pinned by `selected_color_planes_24mp.rs` (`FLOOR_BK32`,
//! `FLOOR_DB32`, `FLOOR_MD32`).

use crate::display_encoding::{display_code, DodgeBurnLinear};
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{pixel_count, reserved_vec, PrecisionImage, PrecisionRun};
use crate::precision_paint_tools::NativePaintEdit;

/// The bucket's per-canvas-pixel plan term (module doc, `# Admission`):
/// codes 8 + region 2 + flood queue ≤ 8.
pub(crate) const BUCKET_REGION_BYTES_PER_PIXEL: u64 = 18;

const FLOAT_ONLY: &str = "the float paint tools require a 32-bit float document";

/// The bucket's region over the canvas (module doc): `image`'s R1 display
/// codes placed at `offset` (canvas coordinates), `[0; 4]` off the layer,
/// grown from `seed` by `selection_region::grow`.
pub(crate) fn bucket_region(
    doc: &PrecisionDocument,
    image: &PrecisionImage,
    offset: (i32, i32),
    seed: (u32, u32),
    tolerance: u16,
    contiguous: bool,
    job: JobContext<'_>,
) -> Result<Vec<u16>, String> {
    let policy = doc.float.as_deref().ok_or(FLOAT_ONLY)?;
    let encoder = policy.encoder();
    let (width, height) = doc.dimensions();
    let (w, h) = image.dimensions();
    let a = -i64::from(offset.0);
    let lx0 = a.clamp(0, i64::from(w));
    let lx1 = (a + i64::from(width)).clamp(0, i64::from(w));
    let mut codes = reserved_vec(pixel_count(width, height)?)?;
    for y in 0..height {
        job.check()?;
        let start = codes.len();
        let ly = i64::from(y) - i64::from(offset.1);
        if ly >= 0 && ly < i64::from(h) && lx0 < lx1 {
            codes.resize(start + (lx0 - a) as usize, [0; 4]);
            for (_, run) in image.row_segments(ly as u32, lx0 as u32, lx1 as u32) {
                let PrecisionRun::Rgba32(run) = run else {
                    return Err(
                        "native paint in a 32-bit float document requires f32 storage".into(),
                    );
                };
                codes.extend(run.iter().map(|p| display_code(encoder, *p)));
            }
        }
        codes.resize(start + width as usize, [0; 4]);
    }
    crate::selection_region::grow(
        (width, height),
        |i| codes[i],
        seed,
        tolerance,
        contiguous,
        1,
        job,
    )
}

/// Dodge/Burn's R4 rule in `doc` (module doc): the policy's encoder and the
/// profile's own decode.
pub(crate) fn dodge_burn_rule(
    doc: &PrecisionDocument,
    exposure: f32,
    range: u8,
    burn: bool,
    protect_tones: bool,
) -> Result<DodgeBurnLinear, String> {
    let policy = doc.float.as_deref().ok_or(FLOAT_ONLY)?;
    let model = doc
        .profile()
        .model()
        .ok_or("native paint requires RGB matrix/TRC profile")?;
    Ok(DodgeBurnLinear::new(
        exposure,
        range,
        burn,
        protect_tones,
        policy.encoder().clone(),
        model.clone(),
    ))
}

/// Whether the float walk has nothing to do before it reads anything, as the
/// master (`precision_paint_tools`' `applying_unlocked`) answers: Dodge/Burn
/// at exposure 0, and a bucket whose seed has no stroke or selection
/// coverage. `n` is the frame's pixel count.
pub(crate) fn idle(
    edit: &NativePaintEdit,
    doc: &PrecisionDocument,
    input: &[u16],
    n: usize,
    selected: bool,
) -> bool {
    match *edit {
        NativePaintEdit::DodgeBurn { exposure, .. } => exposure == 0.,
        NativePaintEdit::BucketFill { x, y, .. } => {
            let seed = y as usize * doc.width as usize + x as usize;
            input[seed] == 0 || (selected && input[n + seed] == 0)
        }
        _ => false,
    }
}

/// The float walk's display-defined terms for `edit`: the bucket's region
/// ([`bucket_region`], from the layer at frame-local `offset`) and
/// Dodge/Burn's rule ([`dodge_burn_rule`]); `None` for every other kind.
pub(crate) fn tone_terms(
    edit: &NativePaintEdit,
    doc: &PrecisionDocument,
    image: &PrecisionImage,
    offset: (i32, i32),
    job: JobContext<'_>,
) -> Result<(Option<Vec<u16>>, Option<DodgeBurnLinear>), String> {
    Ok(match *edit {
        NativePaintEdit::BucketFill {
            x,
            y,
            tolerance,
            contiguous,
            ..
        } => (
            Some(bucket_region(
                doc,
                image,
                offset,
                (x as u32, y as u32),
                tolerance,
                contiguous,
                job,
            )?),
            None,
        ),
        NativePaintEdit::DodgeBurn {
            exposure,
            range,
            burn,
            protect_tones,
            ..
        } => (
            None,
            Some(dodge_burn_rule(doc, exposure, range, burn, protect_tones)?),
        ),
        _ => (None, None),
    })
}
