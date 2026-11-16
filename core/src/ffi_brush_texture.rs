//! The brush texture handle (`brush_texture`): a tile's PYRAMID level built
//! once at its scale, so a stroke's rasterizations (the canvas lays one per
//! dab while it draws) never rebuild it, and the textured twins of
//! `dr_brush_u16` and `dr_brush_overlay_rgba8` that read it
//! (`brush_coverage`, THE TEXTURED STROKE). Immutable: the rasterizers take
//! `*const` and the handle may be shared across threads.
use crate::brush_overlay::overlay_rgba8;
use crate::brush_texture::{BrushTexture, TEXTURE_REFUSAL};
use crate::ffi_brush::request;
use crate::ffi_util::{boxed, fallible_op, free_handle};
use crate::job_control::{DrJobControl, JobContext};
use crate::pattern_tile::{PatternTile, MAX_TILE_SIDE};
use crate::precision_image::admit;
use std::ffi::c_char;
use std::ptr;

/// The opaque handle `dr_brush_texture_create` returns.
pub struct DrBrushTexture(BrushTexture);

/// A texture of the straight RGBA8 tile `tile_rgba` (`tile_width` x
/// `tile_height`, each side 1..=512, `tile_width*tile_height*4` bytes, row 0
/// at the top) at `scale` (0.01..=10) and `depth` (0..=1), anchored at the
/// canvas origin (`brush_texture` module doc). NULL with `err_out` set to
/// "native brush texture holds a tile of 1 to 512 pixels a side, a scale from
/// 0.01 to 10 and a depth from 0 to 1" on a NULL tile, a side, scale or depth
/// out of range; the sides are checked before the tile is read. Free with
/// `dr_brush_texture_free`.
///
/// # Safety
/// `tile_rgba` NULL or readable for `tile_width*tile_height*4` bytes;
/// `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_brush_texture_create(
    tile_rgba: *const u8,
    tile_width: u32,
    tile_height: u32,
    scale: f64,
    depth: f64,
    err_out: *mut *mut c_char,
) -> *mut DrBrushTexture {
    unsafe {
        fallible_op(
            err_out,
            "creating brush texture",
            ptr::null_mut(),
            || {
                let sides = 1..=MAX_TILE_SIDE;
                if tile_rgba.is_null()
                    || !sides.contains(&tile_width)
                    || !sides.contains(&tile_height)
                {
                    return Err(TEXTURE_REFUSAL.into());
                }
                // The tile's length is recomputed from its validated sides.
                let bytes = tile_width as usize * tile_height as usize * 4;
                let rgba = std::slice::from_raw_parts(tile_rgba, bytes);
                let tile = PatternTile {
                    width: tile_width,
                    height: tile_height,
                    rgba: rgba.into(),
                    samples: None,
                };
                BrushTexture::new(&tile, scale, depth).map(DrBrushTexture)
            },
            boxed,
        )
    }
}

/// Frees a texture handle; NULL is a no-op.
///
/// # Safety
/// `texture` NULL or an unfreed handle no call is using.
#[no_mangle]
pub unsafe extern "C" fn dr_brush_texture_free(texture: *mut DrBrushTexture) {
    unsafe { free_handle(texture) }
}

/// The request, dabs and plan of a textured rasterization, after the NULL
/// texture refusal, `dr_brush_u16`'s own admission and, when `shapes` is
/// given, its `count * 16` bytes; `shapes` is the list or `None`.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
unsafe fn textured_request<'a>(
    width: u32,
    height: u32,
    text: *const c_char,
    dabs: *const f64,
    count: usize,
    shapes: *const f64,
    texture: *const DrBrushTexture,
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<
    (
        crate::brush_coverage::BrushRequest,
        &'a [f64],
        Option<&'a [f64]>,
        &'a BrushTexture,
        usize,
    ),
    String,
> {
    let Some(texture) = (unsafe { texture.as_ref() }) else {
        return Err(TEXTURE_REFUSAL.into());
    };
    let (request, dabs, plan) =
        unsafe { request(width, height, text, dabs, count, limit, other, job) }?;
    let shapes = if shapes.is_null() {
        None
    } else {
        let scratch = plan.scratch_bytes + count as u64 * 16;
        admit(0, scratch, other, limit)?;
        job.reserve(scratch)?;
        // `request` bounded `count` by the dab cap.
        Some(unsafe { std::slice::from_raw_parts(shapes, count * 2) })
    };
    Ok((request, dabs, shapes, &texture.0, plan.output_samples))
}

/// `dr_brush_u16`, or with `shapes` (NULL, or `dab_count * 2` doubles)
/// `dr_brush_u16_shaped`, with each written pixel's dab coverage multiplied
/// by the texture's `1 − depth·(1 − t)` at its canvas pixel: output pixel (x,
/// y) is canvas pixel (x + origin_x, y + origin_y). The plan is unchanged;
/// `dab_count * 16` bytes are admitted beyond it when `shapes` is given.
/// Refusals (err_out, `out` untouched): every refusal of the plain or shaped
/// call; a NULL `texture` ("native brush texture holds a tile of 1 to 512
/// pixels a side, a scale from 0.01 to 10 and a depth from 0 to 1"); a pixel
/// or aliased request ("native brush texture requires a stamped tip that is
/// not aliased"). Errors and cancellation leave `out` unchanged.
///
/// # Safety
/// text/dabs follow the plan contract; shapes NULL or readable/aligned for
/// count*2 doubles; texture NULL or live; out NULL or writable/aligned for
/// out_len u16 and nonaliasing inputs; control NULL/live; err_out
/// NULL/writable.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_brush_u16_textured(
    width: u32,
    height: u32,
    text: *const c_char,
    dabs: *const f64,
    count: usize,
    shapes: *const f64,
    texture: *const DrBrushTexture,
    origin_x: i64,
    origin_y: i64,
    out: *mut u16,
    out_len: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "rasterizing native brush",
            false,
            || {
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                let (request, dabs, shapes, texture, samples) = textured_request(
                    width, height, text, dabs, count, shapes, texture, limit, other, job,
                )?;
                if out.is_null() || out_len != samples {
                    return Err("native brush output length differs from canvas".into());
                }
                let output = request.rasterize_textured(
                    (width, height),
                    dabs,
                    shapes,
                    Some((texture, (origin_x, origin_y))),
                    job,
                )?;
                job.report(1., 5)?;
                ptr::copy_nonoverlapping(output.as_ptr(), out, samples);
                Ok(())
            },
            |()| true,
        )
    }
}

/// `dr_brush_overlay_rgba8` over `dr_brush_u16_textured`'s coverage: the same
/// shapes, texture and origin, refusals and admission, then the overlay rule
/// (`brush_overlay`) with the straight ink `rgb` and the optional `selection`
/// (NULL with length 0, or `width*height` bytes) into `out`
/// (`width*height*4` bytes). Errors and cancellation leave `out` unchanged.
///
/// # Safety
/// text/dabs follow the plan contract; shapes NULL or readable/aligned for
/// count*2 doubles; texture NULL or live; rgb NULL or readable for 3 bytes;
/// selection NULL or readable for selection_len bytes; out NULL or writable
/// for out_len bytes and not aliasing any input; control NULL/live; err_out
/// NULL/writable.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_brush_overlay_rgba8_textured(
    width: u32,
    height: u32,
    text: *const c_char,
    dabs: *const f64,
    count: usize,
    shapes: *const f64,
    texture: *const DrBrushTexture,
    origin_x: i64,
    origin_y: i64,
    rgb: *const u8,
    selection: *const u8,
    selection_len: usize,
    out: *mut u8,
    out_len: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "rasterizing native brush overlay",
            false,
            || {
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                let (request, dabs, shapes, texture, samples) = textured_request(
                    width, height, text, dabs, count, shapes, texture, limit, other, job,
                )?;
                if rgb.is_null() {
                    return Err("native brush overlay requires an RGB ink".into());
                }
                let mask_ok = if selection.is_null() {
                    selection_len == 0
                } else {
                    selection_len == samples
                };
                if !mask_ok {
                    return Err("native brush overlay selection length differs from canvas".into());
                }
                if out.is_null() || Some(out_len) != samples.checked_mul(4) {
                    return Err("native brush overlay output length differs from canvas".into());
                }
                let ink = [*rgb, *rgb.add(1), *rgb.add(2)];
                let coverage = request.rasterize_textured(
                    (width, height),
                    dabs,
                    shapes,
                    Some((texture, (origin_x, origin_y))),
                    job,
                )?;
                job.report(1., 5)?;
                let mask = if selection.is_null() {
                    None
                } else {
                    Some(std::slice::from_raw_parts(selection, samples))
                };
                let out = std::slice::from_raw_parts_mut(out, samples * 4);
                overlay_rgba8(&coverage, ink, mask, out);
                Ok(())
            },
            |()| true,
        )
    }
}
