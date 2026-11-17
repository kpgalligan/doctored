//! The dual brush handle (`brush_dual`): a second tip's size, spacing and
//! scatter parsed once, so a stroke's walk resolves each second stamp without
//! parsing again, and the dual twins of `dr_brush_u16` and
//! `dr_brush_overlay_rgba8` that multiply the two tips' coverage before the one
//! quantization (`brush_coverage`, THE DUAL STROKE). Immutable: every query
//! takes `*const` and the handle may be shared across threads.
use crate::brush_coverage::{BrushRequest, MAX_BRUSH_WORK, WORK_REFUSAL};
use crate::brush_dual::{rasterize_dual, DualBrush, DUAL_TIP_REFUSAL, MAX_REQUEST_BYTES};
use crate::brush_overlay::overlay_rgba8;
use crate::brush_texture::BrushTexture;
use crate::ffi_brush::request;
use crate::ffi_brush_texture::DrBrushTexture;
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op, free_handle, handle_get};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_image::admit;
use std::ffi::c_char;
use std::ptr;

/// The opaque handle `dr_brush_dual_create` returns.
pub struct DrBrushDual(DualBrush);

/// Parses a strict dual request (`brush_dual` module doc; at most 1024
/// bytes): `{"version":1,"seed":u32,"size":1…5000,"spacing":1…200,
/// "scatter":{"amount","both_axes","count","count_jitter"}}`. NULL with
/// `err_out` set on a refusal ("native brush dual holds a size from 1 to 5000
/// and a spacing from 1 to 200" for a size or spacing out of range, the
/// scatter's own sentences for a scatter value). Free the handle with
/// `dr_brush_dual_free`.
///
/// # Safety
/// `request` NULL or a NUL-terminated string; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_brush_dual_create(
    request: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut DrBrushDual {
    unsafe {
        fallible_op(
            err_out,
            "parsing native brush dual",
            ptr::null_mut(),
            || {
                let text =
                    borrow_cstr_bounded(request, "native brush dual request", MAX_REQUEST_BYTES)?;
                DualBrush::parse(text).map(DrBrushDual)
            },
            boxed,
        )
    }
}

/// Frees a dual handle; NULL is a no-op.
///
/// # Safety
/// `dual` NULL or an unfreed handle no call is using.
#[no_mangle]
pub unsafe extern "C" fn dr_brush_dual_free(dual: *mut DrBrushDual) {
    unsafe { free_handle(dual) }
}

/// Feeds the path point (x, y) to the second tip's walk (`brush_dual`, THE
/// PATH): `anchor` NULL for the stroke's first point, else the two doubles
/// the previous call left in `walk_out`; `index` the next second stamp's
/// 0-based number. Lays whole stamps while a stamp's most dabs (4 doubles
/// each, the scatter count of them) fit in what is left of `dabs_cap`
/// doubles, writing each dab's x, y, diameter and deposit to `dabs_out`;
/// `walk_out` (3 doubles) takes the anchor's x and y and the stamps laid. A
/// walk the capacity cut short continues with the returned anchor, the same
/// point and the index advanced by the stamps laid. Returns the dabs written;
/// 0 with `dabs_out` and `walk_out` untouched on a NULL handle, buffer or
/// `walk_out`, a capacity below one stamp, or a point or anchor that is not
/// finite. Writes only the caller's two output buffers.
///
/// # Safety
/// `dual` NULL or live; `anchor` NULL or readable for 2 doubles; `dabs_out`
/// NULL or writable for `dabs_cap` doubles; `walk_out` NULL or writable for 3
/// doubles; the outputs alias neither each other nor `anchor`.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_brush_dual_walk(
    dual: *const DrBrushDual,
    index: u64,
    anchor: *const f64,
    x: f64,
    y: f64,
    dabs_out: *mut f64,
    dabs_cap: usize,
    walk_out: *mut f64,
) -> usize {
    if dabs_out.is_null() || walk_out.is_null() {
        return 0;
    }
    let start = if anchor.is_null() {
        None
    } else {
        Some(unsafe { [*anchor, *anchor.add(1)] })
    };
    unsafe {
        handle_get(dual, 0, |d| {
            let per_stamp = 4 * d.0.max_dabs();
            let max_stamps = dabs_cap / per_stamp;
            if max_stamps == 0 {
                return 0;
            }
            let mut dabs = Vec::with_capacity(max_stamps.min(64) * per_stamp);
            let Some(step) = d.0.walk(index, start, [x, y], max_stamps, &mut dabs) else {
                return 0;
            };
            // `walk` laid at most `max_stamps` stamps of at most `per_stamp`
            // doubles each, so the list fits `dabs_cap`.
            ptr::copy_nonoverlapping(dabs.as_ptr(), dabs_out, dabs.len());
            *walk_out = step.anchor[0];
            *walk_out.add(1) = step.anchor[1];
            *walk_out.add(2) = step.stamps as f64;
            dabs.len() / 4
        })
    }
}

/// The first and second requests, both lists, the shapes, the texture and the
/// output's sample count of a dual rasterization: `dr_brush_u16`'s own
/// admission for each list, a pixel or aliased tip on either refused, the
/// work sum checked against `MAX_BRUSH_WORK`, then the two plans' scratch
/// (plus `count * 16` bytes with `shapes`) admitted and reserved, that figure
/// last. The coloured stroke (`ffi_brush_colour`) admits its dual lists here
/// too and adds its own scratch to the figure.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) unsafe fn dual_request<'a>(
    width: u32,
    height: u32,
    text: *const c_char,
    dabs: *const f64,
    count: usize,
    shapes: *const f64,
    texture: *const DrBrushTexture,
    dual_text: *const c_char,
    dual_dabs: *const f64,
    dual_count: usize,
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<
    (
        BrushRequest,
        BrushRequest,
        &'a [f64],
        &'a [f64],
        Option<&'a [f64]>,
        Option<&'a BrushTexture>,
        usize,
        u64,
    ),
    String,
> {
    let (first, dabs, plan) =
        unsafe { request(width, height, text, dabs, count, limit, other, job) }?;
    let (second, dual_dabs, dual_plan) = unsafe {
        request(
            width, height, dual_text, dual_dabs, dual_count, limit, other, job,
        )
    }?;
    if !first.stamped() || !second.stamped() {
        return Err(DUAL_TIP_REFUSAL.into());
    }
    if plan.work_units.saturating_add(dual_plan.work_units) > MAX_BRUSH_WORK {
        return Err(WORK_REFUSAL.into());
    }
    let shaped = if shapes.is_null() {
        0
    } else {
        count as u64 * 16
    };
    let scratch = plan
        .scratch_bytes
        .saturating_add(dual_plan.scratch_bytes)
        .saturating_add(shaped);
    admit(0, scratch, other, limit)?;
    job.reserve(scratch)?;
    let shapes = if shapes.is_null() {
        None
    } else {
        // `request` bounded `count` by the dab cap.
        let shapes = unsafe { std::slice::from_raw_parts(shapes, count * 2) };
        first.check_shapes(dabs, shapes)?;
        Some(shapes)
    };
    let texture = unsafe { texture.as_ref() }.map(|t| &t.0);
    Ok((
        first,
        second,
        dabs,
        dual_dabs,
        shapes,
        texture,
        plan.output_samples,
        scratch,
    ))
}

/// `dr_brush_u16`'s coverage of the first list (`dabs`, with `shapes` NULL or
/// `dab_count * 2` doubles and `texture` NULL or live, output pixel (x, y)
/// being canvas pixel (x + origin_x, y + origin_y)) multiplied per pixel by
/// the second list's (`dual_dabs` under `dual_request`, a native brush
/// request whose opacity is not read; no shapes, no texture) before the one
/// quantization at the first request's opacity (`brush_dual` module doc).
/// Refusals (err_out, `out` untouched): every refusal of `dr_brush_u16` for
/// either list (a NULL `dual_request` included); a pixel or aliased tip on
/// either ("native brush dual requires stamped tips that are not aliased");
/// the two plans' work over the cap ("native brush exceeds 2 billion
/// footprint work units; use a shorter stroke or a smaller brush"); a shapes
/// list `dr_brush_u16_shaped` refuses. Scratch is the two plans' sum, plus
/// `dab_count * 16` bytes with shapes. Errors and cancellation leave `out`
/// unchanged.
///
/// # Safety
/// text/dabs and dual_request/dual_dabs follow the plan contract; shapes NULL
/// or readable/aligned for dab_count*2 doubles; texture NULL or live; out NULL
/// or writable/aligned for out_len u16 and nonaliasing inputs; control
/// NULL/live; err_out NULL/writable.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_brush_u16_dual(
    width: u32,
    height: u32,
    text: *const c_char,
    dabs: *const f64,
    count: usize,
    shapes: *const f64,
    texture: *const DrBrushTexture,
    origin_x: i64,
    origin_y: i64,
    dual_text: *const c_char,
    dual_dabs: *const f64,
    dual_count: usize,
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
                let (first, second, dabs, dual_dabs, shapes, texture, samples, _) = dual_request(
                    width, height, text, dabs, count, shapes, texture, dual_text, dual_dabs,
                    dual_count, limit, other, job,
                )?;
                if out.is_null() || out_len != samples {
                    return Err("native brush output length differs from canvas".into());
                }
                let output = rasterize_dual(
                    &first,
                    &second,
                    (width, height),
                    dabs,
                    shapes,
                    texture.map(|t| (t, (origin_x, origin_y))),
                    dual_dabs,
                    samples,
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

/// `dr_brush_overlay_rgba8` over `dr_brush_u16_dual`'s coverage: the same
/// lists, shapes, texture and origin, refusals and admission, then the
/// overlay rule (`brush_overlay`) with the straight ink `rgb` and the
/// optional `selection` (NULL with length 0, or `width*height` bytes) into
/// `out` (`width*height*4` bytes). Errors and cancellation leave `out`
/// unchanged.
///
/// # Safety
/// As `dr_brush_u16_dual`; rgb NULL or readable for 3 bytes; selection NULL
/// or readable for selection_len bytes; out NULL or writable for out_len
/// bytes and not aliasing any input.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_brush_overlay_rgba8_dual(
    width: u32,
    height: u32,
    text: *const c_char,
    dabs: *const f64,
    count: usize,
    shapes: *const f64,
    texture: *const DrBrushTexture,
    origin_x: i64,
    origin_y: i64,
    dual_text: *const c_char,
    dual_dabs: *const f64,
    dual_count: usize,
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
                let (first, second, dabs, dual_dabs, shapes, texture, samples, _) = dual_request(
                    width, height, text, dabs, count, shapes, texture, dual_text, dual_dabs,
                    dual_count, limit, other, job,
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
                let coverage = rasterize_dual(
                    &first,
                    &second,
                    (width, height),
                    dabs,
                    shapes,
                    texture.map(|t| (t, (origin_x, origin_y))),
                    dual_dabs,
                    samples,
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
