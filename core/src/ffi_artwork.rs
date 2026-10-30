//! Native-depth artwork transfer. All caller buffers remain borrowed, bounded
//! and admitted; path coverage publishes atomically after successful completion.
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op, handle_get, required_handle};
use crate::icc::IccProfile;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, pixel_count, PrecisionImage, BOOKKEEPING_BYTES};
use std::{
    ffi::{c_char, CString},
    ptr,
    sync::Arc,
};

/// Doctored output-coordinate path commands (0 move,1 line,2 quad,3 cubic,4 close)
/// into width*height u16 area coverage. points_count counts xy pairs. Output is
/// unchanged on refusal/cancellation. Caller owns and retains all buffers.
/// # Safety
/// Pointers NULL or valid for the declared lengths; output writable; no overlap;
/// control NULL/live single-use, err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_artwork_path_coverage(
    commands: *const u8,
    command_count: usize,
    points: *const f64,
    point_count: usize,
    width: u32,
    height: u32,
    even_odd: bool,
    output: *mut u16,
    output_count: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "rasterizing native artwork coverage",
            false,
            || {
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                job.check()?;
                let n = pixel_count(width, height)?;
                if (commands.is_null() && command_count != 0)
                    || (points.is_null() && point_count != 0)
                    || output.is_null()
                    || output_count != n
                    || command_count > crate::artwork_coverage::MAX_PATH_POINTS
                    || point_count > crate::artwork_coverage::MAX_PATH_POINTS
                {
                    return Err("invalid native artwork path buffers".into());
                }
                let input = command_count as u64 + point_count as u64 * 16 + n as u64 * 2;
                // Each buffer holds at most MAX_PATH_EDGES (524,288) entries:
                // 16 MiB edges + 16 MiB active edges + 8 MiB intersections +
                // 4 MiB breakpoints + 4 MiB open/spanning u32 index lists;
                // the x-clusters: 2 MiB edge loop indices + 2 MiB
                // top-order permutation + 2 MiB active loop indices + 4 MiB
                // (cluster, index) order + 12 MiB loop extents + 16 MiB
                // cluster edges; 20 MiB covers growth/old allocation overlap.
                // The figure is 172 bytes per edge plus that margin, so it
                // moves with MAX_PATH_EDGES (53 MiB at the earlier
                // 262,144); the host's lease mirrors it
                // (`NativeArtworkEvaluation.coverageBookkeepingBytes`).
                let scratch = 106 * BOOKKEEPING_BYTES + n as u64 * 2 + u64::from(width) * 24;
                admit(input, scratch, other, limit)?;
                job.reserve(scratch)?;
                let commands = if command_count == 0 {
                    &[]
                } else {
                    std::slice::from_raw_parts(commands, command_count)
                };
                let points = if point_count == 0 {
                    &[]
                } else {
                    std::slice::from_raw_parts(points.cast::<[f64; 2]>(), point_count)
                };
                let coverage = crate::artwork_coverage::rasterize(
                    commands, points, width, height, even_odd, job,
                )?;
                job.check()?;
                ptr::copy_nonoverlapping(coverage.as_ptr(), output, n);
                Ok(())
            },
            |()| true,
        )
    }
}
/// Assemble ordered native coverage/continuous sRGB ink and declared intrinsic
/// RGBA8 glyph components into genuine u16 pixels in the supplied exact ICC.
/// Coverage is component-major u16; intrinsic pixels are separate RGBA8 bytes.
/// # Safety
/// Input pointers NULL/readable for lengths, description NUL-terminated;
/// control NULL/live single-use and err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_artwork_render(
    width: u32,
    height: u32,
    description: *const c_char,
    samples: *const u16,
    sample_count: usize,
    intrinsic: *const u8,
    intrinsic_count: usize,
    profile: *const u8,
    profile_length: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionImage {
    unsafe {
        artwork_render(
            ArtworkOutput::Precision,
            width,
            height,
            description,
            samples,
            sample_count,
            intrinsic,
            intrinsic_count,
            profile,
            profile_length,
            limit,
            other,
            control,
            err_out,
        )
    }
}
/// The 32-bit float twin of dr_artwork_render: the same parameters
/// and validation, assembled into an RGBA32 float image of linear light in the
/// supplied exact ICC's primaries. A coverage colour's numbers are taken as
/// linear light (no decode) through the sRGB→profile matrix; intrinsic RGBA8
/// glyph pixels decode through sRGB; components mix in linear light and alpha
/// is coverage/65535. A coverage colour's RGB may be above white, 0..=65504,
/// and its alpha 0..=1. Scratch charges n·16 for the output plane.
/// # Safety
/// Input pointers NULL/readable for lengths, description NUL-terminated;
/// control NULL/live single-use and err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_artwork_render_float(
    width: u32,
    height: u32,
    description: *const c_char,
    samples: *const u16,
    sample_count: usize,
    intrinsic: *const u8,
    intrinsic_count: usize,
    profile: *const u8,
    profile_length: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionImage {
    unsafe {
        artwork_render(
            ArtworkOutput::Float,
            width,
            height,
            description,
            samples,
            sample_count,
            intrinsic,
            intrinsic_count,
            profile,
            profile_length,
            limit,
            other,
            control,
            err_out,
        )
    }
}
/// Which render the two artwork exports run: the u16 `render` or the f32
/// `render_float`. Everything else (validation, admission, ICC) is shared.
#[derive(Clone, Copy)]
enum ArtworkOutput {
    Precision,
    Float,
}
/// The shared body of dr_artwork_render and dr_artwork_render_float.
/// # Safety
/// As the two exports' contracts.
#[allow(clippy::too_many_arguments)]
unsafe fn artwork_render(
    output: ArtworkOutput,
    width: u32,
    height: u32,
    description: *const c_char,
    samples: *const u16,
    sample_count: usize,
    intrinsic: *const u8,
    intrinsic_count: usize,
    profile: *const u8,
    profile_length: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionImage {
    let what = match output {
        ArtworkOutput::Precision => "assembling native artwork",
        ArtworkOutput::Float => "assembling native float artwork",
    };
    unsafe {
        fallible_op(
            err_out,
            what,
            ptr::null_mut(),
            || {
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                job.check()?;
                let n = pixel_count(width, height)?;
                let text = borrow_cstr_bounded(
                    description,
                    "artwork components",
                    crate::artwork_document::MAX_LARGE_DESCRIPTION,
                )?;
                if (samples.is_null() && sample_count != 0)
                    || (intrinsic.is_null() && intrinsic_count != 0)
                    || sample_count.saturating_add(intrinsic_count) > 64_000_000
                    || profile.is_null()
                    || profile_length == 0
                    || profile_length > crate::drdc::MAX_DRDC_BLOB_LEN as usize
                {
                    return Err("invalid artwork component or ICC buffers".into());
                }
                let input = sample_count as u64 * 2
                    + intrinsic_count as u64
                    + profile_length as u64
                    + text.len() as u64;
                let components = crate::artwork_render::components(
                    text,
                    matches!(output, ArtworkOutput::Float),
                )?;
                // A wide fill colour at 8/16 bits builds one high-depth
                // encoder (`artwork_render`'s "Fill layers").
                let plane = match output {
                    ArtworkOutput::Precision => {
                        let wide = if crate::artwork_render::has_wide_fill(&components) {
                            crate::authored_color::wide_scratch_bytes()
                        } else {
                            0
                        };
                        n as u64 * 8 + wide
                    }
                    ArtworkOutput::Float => crate::artwork_render::render_float_scratch(n as u64),
                };
                let scratch = 8 * BOOKKEEPING_BYTES
                    + plane
                    + profile_length as u64 * 64
                    + text.len() as u64 * 32;
                admit(input, scratch, other, limit)?;
                job.reserve(scratch)?;
                let profile = Arc::new(
                    IccProfile::parse(std::slice::from_raw_parts(profile, profile_length))
                        .ok_or("unsupported artwork ICC profile")?,
                );
                if profile.bytes().len() != profile_length {
                    return Err("artwork requires complete exact ICC bytes".into());
                }
                let samples = if sample_count == 0 {
                    &[]
                } else {
                    std::slice::from_raw_parts(samples, sample_count)
                };
                let intrinsic = if intrinsic_count == 0 {
                    &[]
                } else {
                    std::slice::from_raw_parts(intrinsic, intrinsic_count)
                };
                match output {
                    ArtworkOutput::Precision => crate::artwork_render::render(
                        width,
                        height,
                        &components,
                        samples,
                        intrinsic,
                        profile,
                        job,
                    ),
                    ArtworkOutput::Float => crate::artwork_render::render_float(
                        width,
                        height,
                        &components,
                        samples,
                        intrinsic,
                        profile,
                        job,
                    ),
                }
            },
            boxed,
        )
    }
}
/// Publish a typed artwork raster and its description together. Replacing=false
/// appends a top root; true preserves target properties and independent mask.
/// # Safety
/// Handles NULL/live, strings NULL/NUL-terminated, control NULL/live single-use,
/// err_out NULL/writable. Free result with dr_precision_doc_free.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_artwork(
    document: *const PrecisionDocument,
    layer: usize,
    replacing: bool,
    image: *const PrecisionImage,
    x: i32,
    y: i32,
    name: *const c_char,
    description: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "publishing retained artwork",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let image = required_handle(image)?;
                let name = borrow_cstr_bounded(
                    name,
                    "artwork name",
                    crate::drdc::MAX_DRDC_NAME_LEN as usize,
                )?;
                let description = borrow_cstr_bounded(
                    description,
                    "artwork description",
                    crate::artwork_document::MAX_LARGE_DESCRIPTION,
                )?;
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                document.artwork(
                    &crate::artwork_document::ArtworkPublication {
                        index: layer,
                        replacing,
                        offset: (x, y),
                        name,
                        description,
                        position: None,
                    },
                    image,
                    limit,
                    other,
                    job,
                )
            },
            |result| result.map_or(ptr::null_mut(), boxed),
        )
    }
}
/// Publish a typed artwork raster and its description at a CHOSEN stack slot.
/// Identical to dr_precision_doc_artwork except that a NEW record lands where
/// position names relative to anchor (0 above, 1 top, 2 into_group); replacing
/// ignores both and rewrites anchor's own pixels.
/// # Safety
/// Handles NULL/live, strings NULL/NUL-terminated, control NULL/live single-use,
/// err_out NULL/writable. Free result with dr_precision_doc_free.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_artwork_at(
    document: *const PrecisionDocument,
    anchor: usize,
    position: u32,
    image: *const PrecisionImage,
    x: i32,
    y: i32,
    name: *const c_char,
    description: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "publishing retained artwork",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let image = required_handle(image)?;
                let name = borrow_cstr_bounded(
                    name,
                    "artwork name",
                    crate::drdc::MAX_DRDC_NAME_LEN as usize,
                )?;
                let description = borrow_cstr_bounded(
                    description,
                    "artwork description",
                    crate::artwork_document::MAX_LARGE_DESCRIPTION,
                )?;
                let position = crate::layer_insert::InsertPosition::from_code(position)?;
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                document.artwork(
                    &crate::artwork_document::ArtworkPublication {
                        index: anchor,
                        replacing: false,
                        offset: (x, y),
                        name,
                        description,
                        position: Some(position),
                    },
                    image,
                    limit,
                    other,
                    job,
                )
            },
            |result| result.map_or(ptr::null_mut(), boxed),
        )
    }
}
/// Publish a byte artwork fallback from genuine u16 generated output, with an
/// explicit nearest quantization and retained continuous/vector description.
/// Replacing=false inserts above layer; true preserves properties and mask.
/// # Safety
/// Handles NULL/live, strings NULL/NUL-terminated, control NULL/live single-use,
/// err_out NULL/writable. Free result with dr_doc_free.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_artwork(
    document: *const crate::doc::DrDocument,
    layer: usize,
    replacing: bool,
    image: *const PrecisionImage,
    x: i32,
    y: i32,
    name: *const c_char,
    description: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut crate::doc::DrDocument {
    unsafe {
        fallible_op(
            err_out,
            "publishing retained byte artwork",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let image = required_handle(image)?;
                let name = borrow_cstr_bounded(
                    name,
                    "artwork name",
                    crate::drdc::MAX_DRDC_NAME_LEN as usize,
                )?;
                let description = borrow_cstr_bounded(
                    description,
                    "artwork description",
                    crate::artwork_document::MAX_LARGE_DESCRIPTION,
                )?;
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                document.artwork(
                    &crate::artwork_document::ArtworkPublication {
                        index: layer,
                        replacing,
                        offset: (x, y),
                        name,
                        description,
                        position: None,
                    },
                    image,
                    limit,
                    other,
                    job,
                )
            },
            |r| r.map_or(ptr::null_mut(), boxed),
        )
    }
}
/// Publish a byte artwork fallback at a CHOSEN stack slot. Identical to
/// dr_doc_artwork except that the new layer lands where position names relative
/// to anchor (0 above, 1 top, 2 into_group).
/// # Safety
/// Handles NULL/live, strings NULL/NUL-terminated, control NULL/live single-use,
/// err_out NULL/writable. Free result with dr_doc_free.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_artwork_at(
    document: *const crate::doc::DrDocument,
    anchor: usize,
    position: u32,
    image: *const PrecisionImage,
    x: i32,
    y: i32,
    name: *const c_char,
    description: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut crate::doc::DrDocument {
    unsafe {
        fallible_op(
            err_out,
            "publishing retained byte artwork",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let image = required_handle(image)?;
                let name = borrow_cstr_bounded(
                    name,
                    "artwork name",
                    crate::drdc::MAX_DRDC_NAME_LEN as usize,
                )?;
                let description = borrow_cstr_bounded(
                    description,
                    "artwork description",
                    crate::artwork_document::MAX_LARGE_DESCRIPTION,
                )?;
                let position = crate::layer_insert::InsertPosition::from_code(position)?;
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                document.artwork(
                    &crate::artwork_document::ArtworkPublication {
                        index: anchor,
                        replacing: false,
                        offset: (x, y),
                        name,
                        description,
                        position: Some(position),
                    },
                    image,
                    limit,
                    other,
                    job,
                )
            },
            |r| r.map_or(ptr::null_mut(), boxed),
        )
    }
}
/// Read the bounded opaque typed artwork description; NULL means no description.
/// Free the returned string with dr_string_free.
/// # Safety
/// document NULL/live for the call.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_artwork_description(
    document: *const PrecisionDocument,
    layer: usize,
) -> *mut c_char {
    unsafe {
        handle_get(document, ptr::null_mut(), |doc| {
            doc.artwork_description(layer)
                .and_then(|s| CString::new(s).ok())
                .map_or(ptr::null_mut(), CString::into_raw)
        })
    }
}
/// Update or clear a typed raster description after host rerender/geometry.
/// NULL description clears it; never applies to an image-source asset.
/// # Safety
/// document NULL/live, description NULL/NUL-terminated, err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_artwork_metadata(
    document: *const PrecisionDocument,
    layer: usize,
    description: *const c_char,
    limit: u64,
    other: u64,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "updating retained artwork description",
            ptr::null_mut(),
            || {
                let doc = required_handle(document)?;
                let text = if description.is_null() {
                    None
                } else {
                    Some(borrow_cstr_bounded(
                        description,
                        "artwork description",
                        crate::artwork_document::MAX_LARGE_DESCRIPTION,
                    )?)
                };
                doc.artwork_metadata(layer, text, limit, other, JobContext::NONE)
            },
            |r| r.map_or(ptr::null_mut(), boxed),
        )
    }
}
