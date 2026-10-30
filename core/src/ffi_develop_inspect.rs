//! Read-only development inspection, with depth-correct shared master handles.
//! No filesystem access or camera decode. Every output publishes atomically.
use crate::develop_inspect::{extended_preview_len, DevelopPreview, DevelopRegion};
use crate::doc::DrDocument;
use crate::ffi_util::{boxed, fallible_op, handle_get, required_handle};
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{PrecisionImage, PrecisionStorage};
use crate::DrImage;
use std::ffi::{c_char, CString};
use std::ptr;

macro_rules! master {
    ($doc:ty,$name:ident) => {
        /// Share the original retained u16 master before geometry/retouch. Refuses
        /// byte masters and invalid bindings. Free with dr_precision_image_free.
        /// # Safety
        /// document NULL/live and err_out NULL/writable through return.
        #[no_mangle]
        pub unsafe extern "C" fn $name(
            document: *const $doc,
            layer: usize,
            err_out: *mut *mut c_char,
        ) -> *mut PrecisionImage {
            unsafe {
                fallible_op(
                    err_out,
                    "reading developed source master",
                    ptr::null_mut(),
                    || required_handle(document)?.source_master16(layer),
                    boxed,
                )
            }
        }
    };
}
master!(DrDocument, dr_doc_source_master16);
master!(PrecisionDocument, dr_precision_doc_source_master16);

/// Share the retained developed master at its own depth, before geometry and
/// retouch: a 16-bit SDR development's u16 master or a 32-bit float (linear)
/// development's f32 one (the layer's source info reports `master_depth` 16
/// or 32). Refuses a byte master and a missing or stale binding. Free with
/// dr_precision_image_free.
/// # Safety
/// document NULL/live and err_out NULL/writable through return.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_source_master(
    document: *const PrecisionDocument,
    layer: usize,
    err_out: *mut *mut c_char,
) -> *mut PrecisionImage {
    unsafe {
        fallible_op(
            err_out,
            "reading developed source master",
            ptr::null_mut(),
            || required_handle(document)?.source_master(layer),
            boxed,
        )
    }
}

/// The image the develop inspection reads. A u16 image is shared as it is;
/// an f32 image (a 32-bit float development) answers its SDR-clipped view:
/// RGB clamped to 0…1, encoded by the image's own profile curve and rounded
/// once to u16, alpha unchanged, so a sample at or above white reads 65535.
/// The f32 case charges 16 bytes a pixel. Free with dr_precision_image_free.
/// # Safety
/// image/control NULL/live; control single-use; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_image_develop_inspection(
    image: *const PrecisionImage,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionImage {
    unsafe {
        fallible_op(
            err_out,
            "preparing a developed source for inspection",
            ptr::null_mut(),
            || required_handle(image)?.develop_inspection(limit, other, control.as_ref()),
            boxed,
        )
    }
}

/// Shared exact u16 histogram under explicit live-set admission. Stride1 visits
/// every source pixel; higher strides retain their existing sampled-only labels.
/// # Safety
/// image/control NULL/live; control single-use; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_image_histogram_checked(
    image: *const PrecisionImage,
    stride: u32,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "counting developed source histogram",
            ptr::null_mut(),
            || {
                CString::new(
                    required_handle(image)?
                        .histogram_checked(stride, limit, other, control.as_ref())?
                        .to_string(),
                )
                .map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}

/// Exact in-bounds u16 crop, at most1024pixels per side; original ICC retained.
/// Returned image owns its cropped samples and shares the profile.
/// # Safety
/// image/control NULL/live; control single-use; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_image_develop_detail(
    image: *const PrecisionImage,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionImage {
    unsafe {
        fallible_op(
            err_out,
            "reading developed detail",
            ptr::null_mut(),
            || {
                required_handle(image)?.develop_detail(
                    DevelopRegion {
                        x,
                        y,
                        width,
                        height,
                    },
                    limit,
                    other,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}

/// Byte display only, at most1600pixels per output side. Positive divisor1 keeps
/// one source pixel per displaypixel; larger divisors sample the cell center.
/// Flags0/1/2/3 select none/RGB0/RGB65535/both. Markers OR every alpha-positive
/// source sample per cell: blue/red/magenta. The caller carries the source ICC.
/// # Safety
/// image/control NULL/live; control single-use; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_image_develop_preview(
    image: *const PrecisionImage,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    divisor: u32,
    clipping: u32,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    unsafe {
        fallible_op(
            err_out,
            "rendering developed inspection preview",
            ptr::null_mut(),
            || {
                Ok(DrImage {
                    pixels: required_handle(image)?.develop_preview(
                        DevelopPreview {
                            region: DevelopRegion {
                                x,
                                y,
                                width,
                                height,
                            },
                            divisor,
                            clipping,
                        },
                        limit,
                        other,
                        control.as_ref(),
                    )?,
                })
            },
            boxed,
        )
    }
}

/// The extended-range preview of a 32-bit float development (#4) at
/// Fit or 100 % (divisor 1): f32 storage only; output ceil(region/divisor) per
/// side, each at most 1600; each output pixel is the cell-centre sample
/// dr_precision_image_develop_preview picks, through display_encoding's rule
/// X at gain 1, written into `out` as RGBA binary16 little-endian,
/// premultiplied, linear in the image's primaries. Clipping flags 0/1/2/3
/// select none/RGB ≤ 0.0/RGB ≥ 1.0/both over every alpha-positive sample of
/// the cell; a marked cell is opaque binary16 blue/red/magenta. `out_len`
/// must equal out_w·out_h·8 (recomputed by the core). `peak_out` (nullable)
/// receives the largest RGB component of the picture (markers excluded);
/// `marked_out` (nullable) the count of marked output pixels. Admits the
/// extended display plan at the output size under `limit` beside
/// `other_retained_bytes`. false with err_out set for u16 storage, a bad
/// region, divisor, clipping or length, admission and a cancel.
/// # Safety
/// image/control NULL/live; control single-use; `out` NULL or writable for
/// `out_len` bytes; `peak_out` NULL or a writable `float`; `marked_out` NULL
/// or a writable `uint64_t`; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_image_develop_preview_extended(
    image: *const PrecisionImage,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    divisor: u32,
    clipping: u32,
    out: *mut u8,
    out_len: usize,
    peak_out: *mut f32,
    marked_out: *mut u64,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "rendering developed extended preview",
            false,
            || {
                let image = required_handle(image)?;
                if out.is_null() {
                    return Err("the develop extended preview buffer is NULL".into());
                }
                let region = DevelopRegion {
                    x,
                    y,
                    width,
                    height,
                };
                let expected = extended_preview_len(region, divisor)?;
                if out_len != expected {
                    return Err(
                        "the develop extended preview buffer must hold width × height × 8 bytes"
                            .into(),
                    );
                }
                let buffer = std::slice::from_raw_parts_mut(out, expected);
                image.develop_preview_extended(
                    DevelopPreview {
                        region,
                        divisor,
                        clipping,
                    },
                    buffer,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            |(peak, marked)| {
                if !peak_out.is_null() {
                    *peak_out = peak;
                }
                if !marked_out.is_null() {
                    *marked_out = marked;
                }
                true
            },
        )
    }
}

/// The bits per channel of an image's sample storage: 16 for rgba16_unorm,
/// 32 for rgba32_float (the Develop window asks which masters have an
/// extended Fit preview). 0 for NULL. Reads no pixel, allocates nothing.
/// # Safety
/// image NULL/live.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_image_storage(image: *const PrecisionImage) -> u32 {
    unsafe {
        handle_get(image, 0, |i| match i.storage() {
            PrecisionStorage::Rgba16Unorm => 16,
            PrecisionStorage::Rgba32Float => 32,
        })
    }
}
