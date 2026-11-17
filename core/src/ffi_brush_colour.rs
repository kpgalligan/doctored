//! The brush colour handle (`brush_colour`): a stroke's colour dynamics parsed
//! once, so each laid dab's ink resolves without parsing again, and the
//! coloured twins of `dr_brush_u16` and `dr_brush_overlay_rgba8` that carry
//! each dab's ink per pixel beside the coverage (`brush_coverage`, THE
//! COLOURED STROKE). Immutable: every query takes `*const` and the handle may
//! be shared across threads.
use crate::brush_colour::{
    check_inks, rasterize_coloured, ColourDynamics, ColouredStroke, INKS_REFUSAL, MAX_REQUEST_BYTES,
};
use crate::brush_coverage::{footprint, BrushRequest};
use crate::brush_texture::{BrushTexture, TEXTURED_TIP_REFUSAL};
use crate::ffi_brush::request;
use crate::ffi_brush_dual::dual_request;
use crate::ffi_brush_texture::DrBrushTexture;
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op, free_handle, handle_get};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_image::admit;
use std::ffi::c_char;
use std::ptr;

/// The opaque handle `dr_brush_colour_create` returns.
pub struct DrBrushColour(ColourDynamics);

/// Parses a strict colour request (`brush_colour` module doc; at most 1024
/// bytes): `{"version":1,"model":"rgb"|"lab","seed":u32,"foreground":[x,y,z],
/// "background":[x,y,z],"mix","hue","saturation","brightness"}`. NULL with
/// `err_out` set on a refusal ("native brush colour holds a foreground and a
/// background of three values from 0 to 1 and a mix, hue, saturation and
/// brightness from 0 to 1" for a value out of range; "native brush colour in
/// the lab model takes a mix and a brightness; its hue and saturation are 0"
/// for `lab` with a hue or saturation). Free with `dr_brush_colour_free`.
///
/// # Safety
/// `request` NULL or a NUL-terminated string; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_brush_colour_create(
    request: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut DrBrushColour {
    unsafe {
        fallible_op(
            err_out,
            "parsing native brush colour",
            ptr::null_mut(),
            || {
                let text =
                    borrow_cstr_bounded(request, "native brush colour request", MAX_REQUEST_BYTES)?;
                ColourDynamics::parse(text).map(DrBrushColour)
            },
            boxed,
        )
    }
}

/// Frees a colour handle; NULL is a no-op.
///
/// # Safety
/// `colour` NULL or an unfreed handle no call is using.
#[no_mangle]
pub unsafe extern "C" fn dr_brush_colour_free(colour: *mut DrBrushColour) {
    unsafe { free_handle(colour) }
}

/// The ink of dab `dab` of stamp `index` (`brush_colour` module doc), three
/// doubles in 0…1 written to `rgb_out`. False, `rgb_out` untouched, on a NULL
/// handle or `rgb_out`.
///
/// # Safety
/// `colour` NULL or live; `rgb_out` NULL or writable for 3 doubles.
#[no_mangle]
pub unsafe extern "C" fn dr_brush_colour_ink(
    colour: *const DrBrushColour,
    index: u64,
    dab: u32,
    rgb_out: *mut f64,
) -> bool {
    if rgb_out.is_null() {
        return false;
    }
    unsafe {
        handle_get(colour, false, |c| {
            let ink = c.0.ink(index, dab);
            ptr::copy_nonoverlapping(ink.as_ptr(), rgb_out, 3);
            true
        })
    }
}

/// A coloured rasterization's admitted inputs.
struct Coloured<'a> {
    first: BrushRequest,
    dabs: &'a [f64],
    shapes: Option<&'a [f64]>,
    texture: Option<&'a BrushTexture>,
    dual: Option<(BrushRequest, &'a [f64])>,
    inks: &'a [f64],
    samples: usize,
}

/// The admission of a coloured rasterization: with no dual (`dual_text` NULL
/// and `dual_count` 0) `dr_brush_u16`'s, plus `count * 16` bytes with shapes
/// (checked by `check_shapes`) and the textured tip's refusal; with one,
/// `ffi_brush_dual`'s `dual_request`. Then the inks (`count * 3` doubles,
/// `check_inks`) and `P`'s 24 bytes per footprint pixel on top of that
/// figure, admitted and reserved.
#[allow(clippy::too_many_arguments)]
unsafe fn coloured_request<'a>(
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
    inks: *const f64,
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<Coloured<'a>, String> {
    let (first, dabs, shapes, texture, dual, samples, scratch) =
        if dual_text.is_null() && dual_count == 0 {
            let (first, dabs, plan) =
                unsafe { request(width, height, text, dabs, count, limit, other, job) }?;
            let texture = unsafe { texture.as_ref() }.map(|t| &t.0);
            if texture.is_some() && !first.stamped() {
                return Err(TEXTURED_TIP_REFUSAL.into());
            }
            let (shapes, scratch) = if shapes.is_null() {
                (None, plan.scratch_bytes)
            } else {
                let scratch = plan.scratch_bytes.saturating_add(count as u64 * 16);
                admit(0, scratch, other, limit)?;
                job.reserve(scratch)?;
                // `request` bounded `count` by the dab cap.
                let shapes = unsafe { std::slice::from_raw_parts(shapes, count * 2) };
                first.check_shapes(dabs, shapes)?;
                (Some(shapes), scratch)
            };
            (
                first,
                dabs,
                shapes,
                texture,
                None,
                plan.output_samples,
                scratch,
            )
        } else {
            let (first, second, dabs, second_dabs, shapes, texture, samples, scratch) = unsafe {
                dual_request(
                    width, height, text, dabs, count, shapes, texture, dual_text, dual_dabs,
                    dual_count, limit, other, job,
                )
            }?;
            let dual = Some((second, second_dabs));
            (first, dabs, shapes, texture, dual, samples, scratch)
        };
    if count > 0 && inks.is_null() {
        return Err(INKS_REFUSAL.into());
    }
    let inks: &[f64] = if count == 0 {
        &[]
    } else {
        // `request` bounded `count` by the dab cap.
        unsafe { std::slice::from_raw_parts(inks, count * 3) }
    };
    check_inks(dabs, inks)?;
    let area = footprint((width, height), dabs)
        .map_or(0, |[x0, y0, x1, y1]| (x1 - x0) as u64 * (y1 - y0) as u64);
    let scratch = scratch.saturating_add(area.saturating_mul(24));
    admit(0, scratch, other, limit)?;
    job.reserve(scratch)?;
    Ok(Coloured {
        first,
        dabs,
        shapes,
        texture,
        dual,
        inks,
        samples,
    })
}

impl Coloured<'_> {
    /// The coloured stroke (`brush_colour::rasterize_coloured`).
    fn rasterize(
        &self,
        dimensions: (u32, u32),
        origin: (i64, i64),
        job: JobContext<'_>,
    ) -> Result<ColouredStroke, String> {
        rasterize_coloured(
            &self.first,
            dimensions,
            self.dabs,
            self.shapes,
            self.texture.map(|t| (t, origin)),
            self.dual.as_ref().map(|(second, dabs)| (second, *dabs)),
            self.inks,
            self.samples,
            job,
        )
    }
}

/// The straight ink as a code of `scale` (65535 or 255), rounded half up.
fn code(value: f64, scale: f64) -> f64 {
    (value * scale).round()
}

/// `dr_brush_u16`'s coverage (`dr_brush_u16_dual`'s with a dual: `dual_request`
/// NULL with `dual_count` 0 is none), each dab in its own ink: `inks` holds
/// `dab_count * 3` doubles in 0…1, the document's own unit triple. `out`
/// (`4 * width * height` u16) takes the coverage, then the three planes of
/// the straight ink `round(S * 65535)` (`brush_colour` module doc, THE
/// COLOURED STROKE; 0 where nothing was laid). `shapes`, `texture` and the
/// origin are as `dr_brush_u16_textured` takes them (a NULL texture is none).
/// Refusals (err_out, `out` untouched): every refusal of `dr_brush_u16`,
/// `dr_brush_u16_shaped`, `dr_brush_u16_textured` (but its NULL texture) and,
/// with a dual, `dr_brush_u16_dual`; an inks list that is NULL, or a value
/// not finite in 0…1 ("native brush inks hold three values from 0 to 1 per
/// dab"). Scratch is the plan(s), plus `dab_count * 16` bytes with shapes,
/// plus 24 bytes per footprint pixel. Errors and cancellation leave `out`
/// unchanged.
///
/// # Safety
/// text/dabs and dual_request/dual_dabs follow the plan contract; shapes NULL
/// or readable/aligned for dab_count*2 doubles; texture NULL or live; inks
/// NULL or readable/aligned for dab_count*3 doubles; out NULL or
/// writable/aligned for out_len u16 and nonaliasing inputs; control
/// NULL/live; err_out NULL/writable.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_brush_u16_coloured(
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
    inks: *const f64,
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
                let request = coloured_request(
                    width, height, text, dabs, count, shapes, texture, dual_text, dual_dabs,
                    dual_count, inks, limit, other, job,
                )?;
                let samples = request.samples;
                if out.is_null() || Some(out_len) != samples.checked_mul(4) {
                    return Err("native brush output length differs from canvas".into());
                }
                let stroke = request.rasterize((width, height), (origin_x, origin_y), job)?;
                job.report(1., 5)?;
                let out = std::slice::from_raw_parts_mut(out, samples * 4);
                let (coverage, planes) = out.split_at_mut(samples);
                coverage.copy_from_slice(&stroke.coverage);
                let w = width as usize;
                for i in 0..samples {
                    let ink = stroke.ink_at(i % w, i / w);
                    for (ch, value) in ink.iter().enumerate() {
                        planes[ch * samples + i] = code(*value, 65535.) as u16;
                    }
                }
                Ok(())
            },
            |()| true,
        )
    }
}

/// `dr_brush_overlay_rgba8` over `dr_brush_u16_coloured`'s coverage and ink:
/// the same lists, shapes, texture, origin, dual, inks, refusals and
/// admission, then the overlay rule (`brush_overlay`) with each pixel's ink
/// `round(S * 255)` and the optional `selection` (NULL with length 0, or
/// `width*height` bytes) into `out` (`width*height*4` bytes). Errors and
/// cancellation leave `out` unchanged.
///
/// # Safety
/// As `dr_brush_u16_coloured`; selection NULL or readable for selection_len
/// bytes; out NULL or writable for out_len bytes and not aliasing any input.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_brush_overlay_rgba8_coloured(
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
    inks: *const f64,
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
                let request = coloured_request(
                    width, height, text, dabs, count, shapes, texture, dual_text, dual_dabs,
                    dual_count, inks, limit, other, job,
                )?;
                let samples = request.samples;
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
                let stroke = request.rasterize((width, height), (origin_x, origin_y), job)?;
                job.report(1., 5)?;
                let mask = if selection.is_null() {
                    None
                } else {
                    Some(std::slice::from_raw_parts(selection, samples))
                };
                let out = std::slice::from_raw_parts_mut(out, samples * 4);
                let w = width as usize;
                crate::brush_overlay::overlay_rgba8_inked(
                    &stroke.coverage,
                    |i| stroke.ink_at(i % w, i / w).map(|v| code(v, 255.) as u8),
                    mask,
                    out,
                );
                Ok(())
            },
            |()| true,
        )
    }
}
