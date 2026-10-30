//! Thin C entry points for profile-aware selection generation and reversible
//! coverage refinement. Caller output is published only after successful work;
//! cancellation/refusal never leaves a partially written selection behind.

use crate::{
    doc::DrDocument,
    doc_transform::{resample_canvas_plane_with_context, Affine},
    ffi_util::{fallible_op, filter_from_c, read_cstr},
    job_control::{DrJobControl, JobContext},
    DrImage,
};
use image::GrayImage;
use std::ffi::{c_char, c_int, CStr};

/// Bound the parameter copy before allocating the owned UTF8 string.
unsafe fn parameters(ptr: *const c_char) -> Result<String, String> {
    if unsafe { CStr::from_ptr(ptr) }.to_bytes().len() > 128 * 1024 {
        return Err("selection parameters exceed 128 KiB".into());
    }
    unsafe { read_cstr(ptr, "selection parameters") }
}

unsafe fn output(
    out: *mut u8,
    control: *const DrJobControl,
    err: *mut *mut c_char,
    body: impl FnOnce(JobContext<'_>) -> Result<Vec<u8>, String>,
) -> bool {
    if out.is_null() {
        return false;
    }
    unsafe {
        fallible_op(
            err,
            "selection operation failed",
            false,
            || {
                let token = control.as_ref();
                let _run = token.map(DrJobControl::begin).transpose()?;
                let job = token.map_or(JobContext::NONE, JobContext::new);
                let result = body(job)?;
                job.check()?;
                job.report(1.0, 5)?;
                std::ptr::copy_nonoverlapping(result.as_ptr(), out, result.len());
                Ok(())
            },
            |()| true,
        )
    }
}

/// Generates canvas coverage from sampled profile-aware ΔE76 colors or an L*
/// tonal interval, or a hue band. Strict JSON: sampled
/// {mode,samples,tolerance,softness,points,cluster_range}, tonal
/// {mode,lower,upper,softness} or hue {mode,band}, each with invert
/// (`selection_tools` module doc). RGB samples are 1..32 byte triples;
/// tolerance0..200,softness0..100; tonal bounds0..100. Source alpha multiplies
/// selection coverage. Unsupported LUT profiles refuse without sRGB fallback.
/// NULL inputs return false. A NULL control disables cooperative cancellation.
/// Identical output is success; the host compares coverage before creating undo.
///
/// # Safety
/// Handles must be live for the call, params a NUL-terminated UTF8 string,
/// out_mask writable for document width*height bytes. Source dimensions must
/// match the document. Control may be cancelled concurrently but not freed.
/// err_out must be NULL or writable for an owned error string (dr_string_free).
#[no_mangle]
pub unsafe extern "C" fn dr_selection_color_range(
    doc: *const DrDocument,
    source: *const DrImage,
    params: *const c_char,
    out_mask: *mut u8,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    if doc.is_null() || source.is_null() || params.is_null() {
        return false;
    }
    unsafe {
        output(out_mask, control, err_out, |job| {
            crate::selection_tools::color_range(&*doc, &*source, &parameters(params)?, job)
        })
    }
}

/// Produces the union of brush-seeded four-connected regions. Strict JSON:
/// {seeds:[[x,y],...],tolerance:25,edge:12,radius:0,smooth:0}; 1..4096 integer
/// canvas seeds, both thresholds ΔE76 in0..200, radius (the brush radius in
/// canvas pixels) 0..256, smooth an integer 0..4. Every opaque pixel within
/// radius of a seed centre is selected and anchors its own growth; local
/// neighbor edges and each originating anchor must both pass. smooth
/// pre-averages the Lab plane over a (2·smooth+1)² window without crossing a
/// step above edge. With radius above 0, an unselected opaque region of at
/// most ⌈π·radius²⌉ pixels that touches no canvas border and whose neighbours
/// are all selected is filled. Seeds × the brush's box must stay within 64
/// million pixels, else an error names the brush size and seed count. radius 0
/// and smooth 0 select from the seed pixels alone. No implicit interpolation
/// between seeds; host rasterizes stroke centers. Source alpha is retained as
/// selected coverage. NULL control disables cancellation. Host applies
/// add/subtract/intersect.
///
/// # Safety
/// Same handle/string/output/control/error lifetimes as dr_selection_color_range.
#[no_mangle]
pub unsafe extern "C" fn dr_selection_quick(
    doc: *const DrDocument,
    source: *const DrImage,
    params: *const c_char,
    out_mask: *mut u8,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    if doc.is_null() || source.is_null() || params.is_null() {
        return false;
    }
    unsafe {
        output(out_mask, control, err_out, |job| {
            crate::selection_tools::quick(&*doc, &*source, &parameters(params)?, job)
        })
    }
}

/// The Magnetic Lasso's live-wire tracer (`edge_trace`): the cheapest
/// edge-following 8-connected path from `from` inside the corridor within
/// `width` of `from → trail… → (explicit end)`. Strict JSON `{from:[x,y],
/// trail:[[x,y],…] 1..4096, end:"snap"|[x,y], width 1..256 (10), contrast
/// 1..100 (10), frequency 0..100 (57)}`; coordinates finite inside
/// `[0,w]×[0,h]`. On success writes `*out_count` vertices to `out_points` as
/// x,y pairs at pixel centres, `*out_fasten` (the vertex to fasten at, or −1)
/// and `*out_corridor` (the corridor's pixel count). A trail that leaves
/// `2·width` of `from`, comes back within it and encloses at least
/// `width ×` its farthest distance in area is a loop, at any frequency:
/// two legs joined near its farthest point, the junction a fasten candidate
/// and the corridor the legs' sum (`edge_trace` module doc, step 6);
/// otherwise frequency 0 never fastens. `capacity` counts
/// vertices; a short buffer returns false with `*out_count` set to the count
/// needed and an error naming both. Every other failure leaves all outputs
/// untouched. NULL source/params/out_count/out_fasten/out_corridor, or NULL
/// out_points with a nonzero capacity, return false with err_out untouched.
///
/// # Safety
/// `source` must be a live image for the call, `params` a NUL-terminated UTF8
/// string, `out_points` NULL (capacity 0) or writable for `2 * capacity`
/// floats, the three scalar outputs writable, and `err_out` NULL or writable
/// for an owned error string (dr_string_free).
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_selection_trace_edge(
    source: *const DrImage,
    params: *const c_char,
    out_points: *mut f32,
    capacity: usize,
    out_count: *mut usize,
    out_fasten: *mut i32,
    out_corridor: *mut u64,
    err_out: *mut *mut c_char,
) -> bool {
    if source.is_null()
        || params.is_null()
        || out_count.is_null()
        || out_fasten.is_null()
        || out_corridor.is_null()
        || (out_points.is_null() && capacity != 0)
    {
        return false;
    }
    unsafe {
        fallible_op(
            err_out,
            "edge trace failed",
            false,
            || {
                let image = &(*source).pixels;
                let request = crate::edge_trace::parse(&parameters(params)?, image.dimensions())?;
                let trace = crate::edge_trace::trace(image, &request)?;
                let needed = trace.vertices.len();
                if needed > capacity {
                    *out_count = needed;
                    return Err(format!(
                        "edge trace needs room for {needed} vertices but the buffer holds {capacity}"
                    ));
                }
                let fasten = match trace.fasten {
                    Some(k) => i32::try_from(k).map_err(|_| "edge trace fasten index overflows")?,
                    None => -1,
                };
                let flat: Vec<f32> = trace.vertices.iter().flatten().copied().collect();
                std::ptr::copy_nonoverlapping(flat.as_ptr(), out_points, flat.len());
                *out_count = needed;
                *out_fasten = fasten;
                *out_corridor = trace.corridor_px;
                Ok(())
            },
            |()| true,
        )
    }
}

/// Refines captured coverage with strict JSON smooth/feather0..64px,
/// shift-64..64px,contrast0..1,edge_radius0..64px,smart_radius true/false.
/// Order is edge,smooth,shift,feather,contrast. Zero controls reproduce bytes
/// exactly. Source is unchanged. The edge stage re-estimates coverage from the
/// source's colours inside edge_radius of the 50% contour; smart_radius fades
/// it where the picture has no edge. `brush` is the AREA plane, one byte per
/// pixel: 0 never refined, 128 automatic (the radius band), 255 always
/// refined; NULL means all automatic, and any other value is an error. The
/// edge stage runs when edge_radius is above 0 or the plane holds a 255, and
/// then reserves 64 bytes a pixel; edge_radius above 0 with a NULL plane is
/// valid. It is not color decontamination (dr_image_estimate_foreground).
/// NULL control disables cancellation. Output is untouched on error.
///
/// # Safety
/// Source must be live, mask and optional brush readable for source width*height
/// bytes, out_mask writable for the same count, params NUL-terminated UTF8.
/// Output may alias mask/brush: publication happens after every source read.
/// Control/error lifetimes match dr_selection_color_range.
#[no_mangle]
pub unsafe extern "C" fn dr_selection_refine(
    source: *const DrImage,
    mask: *const u8,
    brush: *const u8,
    params: *const c_char,
    out_mask: *mut u8,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    if source.is_null() || mask.is_null() || params.is_null() {
        return false;
    }
    unsafe {
        output(out_mask, control, err_out, |job| {
            let image = &*source;
            let n = image.pixels.width() as usize * image.pixels.height() as usize;
            let input = std::slice::from_raw_parts(mask, n);
            let brush = if brush.is_null() {
                None
            } else {
                Some(std::slice::from_raw_parts(brush, n))
            };
            crate::selection_refine::refine(image, input, brush, &parameters(params)?, job)
        })
    }
}

/// Resamples `width`×`height` coverage to `out_width`×`out_height` through the
/// document resize's own plane kernel (`doc_channel::resized_plane`), filter
/// 0 nearest / 1 bilinear / 2 bicubic / 3 Lanczos — the host's reduced
/// selection for a preview proxy. NULL mask/out_mask return false with
/// err_out untouched; a zero dimension, either product above
/// `doc::MAX_PIXELS` or an unknown filter returns false with a message. The
/// output is written only on success.
///
/// # Safety
/// Mask must be readable for width*height bytes and out_mask writable for
/// out_width*out_height bytes; they must not overlap. err_out must be NULL or
/// writable for an owned error string (dr_string_free).
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_selection_resize(
    mask: *const u8,
    width: u32,
    height: u32,
    out_width: u32,
    out_height: u32,
    filter: c_int,
    out_mask: *mut u8,
    err_out: *mut *mut c_char,
) -> bool {
    if mask.is_null() {
        return false;
    }
    unsafe {
        output(out_mask, std::ptr::null(), err_out, |_| {
            let n = u64::from(width) * u64::from(height);
            let out = u64::from(out_width) * u64::from(out_height);
            if n == 0 || n > crate::doc::MAX_PIXELS || out == 0 || out > crate::doc::MAX_PIXELS {
                return Err(
                    "selection resize dimensions are outside the supported canvas range".into(),
                );
            }
            let filter = filter_from_c(filter).ok_or("unsupported selection resize filter")?;
            let input = GrayImage::from_raw(
                width,
                height,
                std::slice::from_raw_parts(mask, n as usize).to_vec(),
            )
            .ok_or("invalid selection dimensions")?;
            Ok(crate::doc_channel::resized_plane(&input, out_width, out_height, filter).into_raw())
        })
    }
}

/// Affine-transforms coverage on a fixed canvas through the shared inverse
/// sampler. Matrix is [a,b,c,d,tx,ty], filter0nearest/1bilinear/2bicubic/3Lanczos.
/// Off-canvas samples are zero. Singular/nonfinite matrix, invalid dimensions
/// or filter refuse atomically. Neither source mask nor layer pixels change.
/// NULL control disables cancellation; identical output succeeds.
///
/// # Safety
/// Mask must be readable and out_mask writable for width*height bytes; matrix6
/// readable for six doubles. Output may alias mask. Control/error lifetime as
/// dr_selection_color_range. Dimensions must describe the actual buffers.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_selection_transform(
    mask: *const u8,
    width: u32,
    height: u32,
    matrix6: *const f64,
    filter: c_int,
    out_mask: *mut u8,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    if mask.is_null() || matrix6.is_null() {
        return false;
    }
    unsafe {
        output(out_mask, control, err_out, |job| {
            let n = u64::from(width) * u64::from(height);
            if n == 0 || n > crate::doc::MAX_PIXELS {
                return Err(
                    "selection transform dimensions are outside the supported canvas range".into(),
                );
            }
            let filter = filter_from_c(filter).ok_or("unsupported selection transform filter")?;
            let matrix = Affine::from_array(
                std::slice::from_raw_parts(matrix6, 6)
                    .try_into()
                    .expect("six elements"),
            );
            job.reserve(2 * n + 4096)?;
            let input = GrayImage::from_raw(
                width,
                height,
                std::slice::from_raw_parts(mask, n as usize).to_vec(),
            )
            .ok_or("invalid selection dimensions")?;
            Ok(resample_canvas_plane_with_context(&input, &matrix, filter, job)?.into_raw())
        })
    }
}

/// Select ▸ Grow (`contiguous`) / Select ▸ Similar: the canvas-sized 0/255
/// region whose colours fall inside the range of every pixel `mask` covers at
/// least half, widened by `tolerance`, sampled from the flattened composite.
/// The host adds it to the current selection. Refuses when `mask_len` is not
/// the canvas pixel count or no pixel is at least half selected ("No pixel
/// is more than half selected, so there is no colour to match."). NULL doc/mask/mask_out return false with
/// err_out untouched. mask_out is written only on success.
///
/// # Safety
/// doc must be live for the call, mask readable for mask_len bytes, mask_out
/// writable for document width*height bytes and not aliasing mask. err_out
/// must be NULL or writable for an owned error string (dr_string_free).
#[no_mangle]
pub unsafe extern "C" fn dr_doc_select_similar(
    doc: *const DrDocument,
    mask: *const u8,
    mask_len: usize,
    tolerance: u8,
    contiguous: bool,
    mask_out: *mut u8,
    err_out: *mut *mut c_char,
) -> bool {
    if doc.is_null() || mask.is_null() {
        return false;
    }
    unsafe {
        output(mask_out, std::ptr::null(), err_out, |_| {
            let doc = &*doc;
            let n = doc.width as usize * doc.height as usize;
            if mask_len != n {
                return Err("selection mask length does not match the canvas".into());
            }
            doc.select_similar(std::slice::from_raw_parts(mask, n), tolerance, contiguous)
        })
    }
}
