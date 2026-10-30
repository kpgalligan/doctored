//! Deliberate depth conversion and authoritative read-only statistics. Typed and
//! byte document owners stay distinct; no export accepts a display surrogate.

use crate::doc::DrDocument;
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op, required_handle};
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::PrecisionImage;
use crate::precision_quantize::PrecisionDither;
use crate::DrImage;
use std::ffi::{c_char, CString};
use std::ptr;

fn string(value: serde_json::Value) -> Result<CString, String> {
    CString::new(value.to_string()).map_err(|e| e.to_string())
}

/// Owned flattened conversion plan/loss JSON. Free with dr_string_free.
/// # Safety
/// document NULL/live; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_promote_flattened_plan(
    document: *const DrDocument,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning flattened promotion",
            ptr::null_mut(),
            || string(required_handle(document)?.promote_flattened_plan()?.json()),
            CString::into_raw,
        )
    }
}

/// Render legacy appearance, then promote every byte by 257 into one u16 raster.
/// Saved channels (alpha and spot) are carried, each coverage value ×257.
/// # Safety
/// document/control NULL/live; control single-use; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_promote_flattened(
    document: *const DrDocument,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "promoting flattened appearance",
            ptr::null_mut(),
            || required_handle(document)?.promote_flattened(limit, other, control.as_ref()),
            boxed,
        )
    }
}

/// Owned structure-preserving 8→16 plan JSON: preserved, discarded,
/// per-record rendering differences and blocked rows. Free with
/// dr_string_free.
/// # Safety
/// document NULL/live; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_promote_structure_plan(
    document: *const DrDocument,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning structure-preserving promotion",
            ptr::null_mut(),
            || string(required_handle(document)?.promote_structure_plan()?.json()),
            CString::into_raw,
        )
    }
}

/// Every supported record carried into a 16-bit document, each byte code
/// widened by 257. Refuses with the plan's first blocked row before any
/// plane is promoted.
/// # Safety
/// document/control NULL/live; control single-use; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_promote_structure(
    document: *const DrDocument,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "promoting document structure",
            ptr::null_mut(),
            || {
                required_handle(document)?.promote_structure(
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}

/// Owned preview-delta JSON: the byte projection ×257 against the converted
/// document's u16 render. Free with dr_string_free.
/// # Safety
/// document/converted/control NULL/live; control single-use; err_out
/// NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_promote_structure_preview(
    document: *const DrDocument,
    converted: *const PrecisionDocument,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "measuring structure-preserving promotion",
            ptr::null_mut(),
            || {
                string(required_handle(document)?.promote_structure_preview(
                    required_handle(converted)?,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )?)
            },
            CString::into_raw,
        )
    }
}

/// Owned 16→float plan JSON: preserved, per-record rendering differences and
/// blocked rows (a refused blend, effect or adjustment, an effective warp, a
/// profile with no matrix/TRC model, a result too large to save and reopen
/// at 32 bits). A float document is an error. Free with dr_string_free.
/// # Safety
/// document NULL/live; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_promote_to_float_plan(
    document: *const PrecisionDocument,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning conversion to 32-bit float",
            ptr::null_mut(),
            || string(required_handle(document)?.promote_to_float_plan()?.json()),
            CString::into_raw,
        )
    }
}

/// Every supported record carried into a 32-bit float document, each u16
/// code decoded through the profile's TRC to linear light. Refuses with the
/// plan's first blocked row before any plane is built, and a float source.
/// # Safety
/// document/control NULL/live; control single-use; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_promote_to_float(
    document: *const PrecisionDocument,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "converting document to 32-bit float",
            ptr::null_mut(),
            || {
                required_handle(document)?.promote_to_float(
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}

/// Owned 8→float plan JSON: the 8→16 plan's rows (less the three its float
/// rows replace), the float rows over its typed shape and the capacity row.
/// Free with dr_string_free.
/// # Safety
/// document NULL/live; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_promote_to_float_plan(
    document: *const DrDocument,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning conversion to 32-bit float",
            ptr::null_mut(),
            || string(required_handle(document)?.promote_to_float_plan()?.json()),
            CString::into_raw,
        )
    }
}

/// Every supported record carried into a 32-bit float document: each byte
/// widened by 257, then decoded through the profile's TRC to linear light.
/// Refuses with the plan's first blocked row before any plane is built.
/// # Safety
/// document/control NULL/live; control single-use; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_promote_to_float(
    document: *const DrDocument,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "converting document to 32-bit float",
            ptr::null_mut(),
            || {
                required_handle(document)?.promote_to_float(
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}

fn narrow_target(depth: u32) -> Result<crate::precision_conversion_narrow::NarrowTarget, String> {
    use crate::precision_conversion_narrow::NarrowTarget;
    match depth {
        8 => Ok(NarrowTarget::Rgba8),
        16 => Ok(NarrowTarget::Rgba16),
        _ => Err("narrowing target depth must be 8 or 16".into()),
    }
}

/// Owned structure-preserving narrowing plan JSON (16→8, float→16,
/// float→8). target_depth is 8 or 16. Free with dr_string_free.
/// # Safety
/// document NULL/live; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_narrow_structure_plan(
    document: *const PrecisionDocument,
    target_depth: u32,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning structure-preserving narrowing",
            ptr::null_mut(),
            || {
                let target = narrow_target(target_depth)?;
                string(
                    required_handle(document)?
                        .narrow_structure_plan(target)?
                        .json(),
                )
            },
            CString::into_raw,
        )
    }
}

/// Every supported record carried into an 8-bit document; dither 0 none /
/// 1 ordered RGB per record, alpha nearest. Refuses with the plan's first
/// blocked row before any plane is built.
/// # Safety
/// document/control NULL/live; control single-use; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_narrow_structure_to_u8(
    document: *const PrecisionDocument,
    dither: u32,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    unsafe {
        fallible_op(
            err_out,
            "narrowing document structure to 8 bits",
            ptr::null_mut(),
            || {
                required_handle(document)?.narrow_structure_to_u8(
                    PrecisionDither::from_code(dither)?,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}

/// A float document's every supported record carried into a 16-bit
/// document, each raster tone-mapped per record. Refuses a 16-bit source.
/// # Safety
/// document/control NULL/live; control single-use; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_narrow_structure_to_u16(
    document: *const PrecisionDocument,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "narrowing document structure to 16 bits",
            ptr::null_mut(),
            || {
                required_handle(document)?.narrow_structure_to_u16(
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}

/// Owned preview-delta JSON: this document's render (tone-mapped when float)
/// against the narrowed document's — exactly one of converted_u8 /
/// converted_u16 non-NULL. Free with dr_string_free.
/// # Safety
/// document/converted/control NULL/live; control single-use; err_out
/// NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_narrow_structure_preview(
    document: *const PrecisionDocument,
    converted_u8: *const DrDocument,
    converted_u16: *const PrecisionDocument,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    use crate::precision_conversion_narrow::Narrowed;
    unsafe {
        fallible_op(
            err_out,
            "measuring structure-preserving narrowing",
            ptr::null_mut(),
            || {
                let converted = match (converted_u8.as_ref(), converted_u16.as_ref()) {
                    (Some(bytes), None) => Narrowed::Rgba8(bytes),
                    (None, Some(typed)) => Narrowed::Rgba16(typed),
                    _ => return Err("exactly one converted document handle is required".into()),
                };
                string(required_handle(document)?.narrow_structure_preview(
                    converted,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )?)
            },
            CString::into_raw,
        )
    }
}

/// Owned flattened reduction plan/loss JSON. Free with dr_string_free.
/// # Safety
/// document NULL/live; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_downconvert_plan(
    document: *const PrecisionDocument,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning flattened downconversion",
            ptr::null_mut(),
            || string(required_handle(document)?.downconvert_plan()?.json()),
            CString::into_raw,
        )
    }
}

/// One byte raster after authoritative rendering; dither 0 none / 1 ordered RGB.
/// ICC/PPI/metadata and saved channels (nearest) retained; editable tree/masks
/// discarded.
/// # Safety
/// document/control NULL/live; control single-use; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_downconvert(
    document: *const PrecisionDocument,
    dither: u32,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    unsafe {
        fallible_op(
            err_out,
            "downconverting flattened appearance",
            ptr::null_mut(),
            || {
                required_handle(document)?.downconvert(
                    PrecisionDither::from_code(dither)?,
                    limit,
                    other,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}

/// Explicit byte export copy; alpha never dithered. Profile stays on source.
/// # Safety
/// image/control NULL/live; control single-use; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_image_quantize(
    image: *const PrecisionImage,
    dither: u32,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    unsafe {
        fallible_op(
            err_out,
            "quantizing high-depth pixels",
            ptr::null_mut(),
            || {
                Ok(DrImage {
                    pixels: required_handle(image)?.quantize(
                        PrecisionDither::from_code(dither)?,
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

/// Owned exact u16/rational mean sample JSON; no byte intermediary. Reach <= 2.
/// # Safety
/// image NULL/live; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_image_sample(
    image: *const PrecisionImage,
    x: i32,
    y: i32,
    reach: u32,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "sampling high-depth pixels",
            ptr::null_mut(),
            || string(required_handle(image)?.sample(x, y, reach)?),
            CString::into_raw,
        )
    }
}

/// Owned qualified family/target/depth decision. Unknown combinations refuse.
/// # Safety
/// operation/target NULL or terminated UTF-8 strings; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_capability(
    operation: *const c_char,
    target: *const c_char,
    depth: u32,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "checking precision capability",
            ptr::null_mut(),
            || {
                string(crate::precision_capability::capability(
                    borrow_cstr_bounded(operation, "operation", 64)?,
                    borrow_cstr_bounded(target, "target", 64)?,
                    depth,
                ))
            },
            CString::into_raw,
        )
    }
}
