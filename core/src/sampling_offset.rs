//! Offset: Filter ▸ Other ▸ Offset. Inside the FRAME
//! (the selection's bounding box in plane pixels, or the whole plane,
//! `ops_spatial::Frame`) each pixel takes the source `(x − horizontal, y −
//! vertical)`, so positive values move content right and down. A source outside
//! the frame is the undefined area:
//! - Repeat clamps it to the frame's edge (Repeat Edge Pixels);
//! - Wrap takes it modulo the frame size (Wrap Around), so any offset, however
//!   large, is defined;
//! - Transparent writes `[0, 0, 0, 0]`, and Fill writes `[f, f, f, 1]`. The host
//!   resolves Set to Background into one of the two per target.
//!
//! Outside the frame a pixel is its own source. The plane copies straight
//! samples and the combine returns them, so codes round-trip exactly at both
//! depths.
//!
//! Offset: exact sample moves within the frame, edges Transparent `[0,0,0,0]`, `Fill(f)`
//! `[f,f,f,1]` (the host resolves Set to Background), Repeat (clamp) and Wrap (`rem_euclid`),
//! offsets bounded only by ±`MAX_PIXELS`; both 0 is the identity.
use crate::job_control::JobContext;
use crate::ops_spatial::{frame_or_plane, Frame};
use crate::precision_image::{pixel_count, reserved_vec, BOOKKEEPING_BYTES};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum OffsetEdge {
    Transparent,
    Fill(f32),
    Repeat,
    Wrap,
}

/// Work units per pixel: an index computation and one sample copy. Frozen from
/// the `#[ignore]`d `radial_clouds_offset_calibration_probe`
/// (`tests/radial_clouds_offset.rs`): at 6000×4000, min of 3, at the
/// reference's r16-unit rate, the larger of the 8-bit run and the whole 16-bit
/// edit, the largest over two runs: Wrap 0.219 s / 0.612 s and 0.221 s /
/// 0.614 s (up to 29.50 units), Repeat 0.218 s / 0.612 s and 0.219 s /
/// 0.612 s; 29.50 × 1.15 = 34, the 16-bit edit's own walk being most of it.
/// 24 MP charges 8.16e8 of 2.4e10. The record is in
/// `designs/phase-9f-selected-tone-status.md`.
pub(crate) const OFFSET_WORK_PER_PIXEL: u64 = 34;

/// `(scratch, work)` refused before any allocation. The offsets are not
/// limited by the frame size: every edge mode is defined for any offset.
pub(crate) fn offset_plan(
    dimensions: (u32, u32),
    frame: Option<Frame>,
    cap: u64,
) -> Result<(u64, u64), String> {
    let n = pixel_count(dimensions.0, dimensions.1)? as u64;
    frame_or_plane(frame, dimensions)?;
    let work = n.saturating_mul(OFFSET_WORK_PER_PIXEL);
    if work > cap {
        return Err("offset exceeds the bounded pixel-work limit".into());
    }
    Ok((n * 16 + 2 * BOOKKEEPING_BYTES, work))
}

/// The plane index pixel `(x, y)` reads, or `None` for an edge fill. A frame
/// outside the plane is treated as the whole plane (`offset_plan` refuses it).
pub(crate) fn offset_source(
    x: u32,
    y: u32,
    dimensions: (u32, u32),
    horizontal: i64,
    vertical: i64,
    edge: OffsetEdge,
    frame: Option<Frame>,
) -> Option<usize> {
    let f = frame_or_plane(frame, dimensions).unwrap_or(Frame {
        x: 0,
        y: 0,
        width: dimensions.0,
        height: dimensions.1,
    });
    let index = |x: i64, y: i64| y as usize * dimensions.0 as usize + x as usize;
    let (fx, fy) = (i64::from(f.x), i64::from(f.y));
    let (fw, fh) = (i64::from(f.width), i64::from(f.height));
    let (x, y) = (i64::from(x), i64::from(y));
    if x < fx || y < fy || x >= fx + fw || y >= fy + fh {
        return Some(index(x, y));
    }
    let (sx, sy) = (x - horizontal, y - vertical);
    if sx >= fx && sy >= fy && sx < fx + fw && sy < fy + fh {
        return Some(index(sx, sy));
    }
    match edge {
        OffsetEdge::Repeat => Some(index(sx.clamp(fx, fx + fw - 1), sy.clamp(fy, fy + fh - 1))),
        OffsetEdge::Wrap => Some(index(
            fx + (sx - fx).rem_euclid(fw),
            fy + (sy - fy).rem_euclid(fh),
        )),
        OffsetEdge::Transparent | OffsetEdge::Fill(_) => None,
    }
}

/// The plane: each pixel's straight source sample, or the edge fill.
pub(crate) fn offset_plane(
    read: impl Fn(usize) -> [f32; 4],
    dimensions: (u32, u32),
    horizontal: i64,
    vertical: i64,
    edge: OffsetEdge,
    frame: Option<Frame>,
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    frame_or_plane(frame, dimensions)?;
    let (w, h) = dimensions;
    let fill = match edge {
        OffsetEdge::Fill(v) => [v, v, v, 1.0],
        _ => [0.0; 4],
    };
    let mut out = reserved_vec(w as usize * h as usize)?;
    let mut i = 0_usize;
    for y in 0..h {
        job.check()?;
        for x in 0..w {
            job.every(i)?;
            i += 1;
            out.push(
                offset_source(x, y, dimensions, horizontal, vertical, edge, frame)
                    .map_or(fill, &read),
            );
        }
    }
    Ok(out)
}
