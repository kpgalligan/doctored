//! Bounded binary camera provenance bridge. Only a caller's explicit attachment
//! captures bytes. Reads never open paths or invoke a decoder.
use crate::doc::DrDocument;
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op, required_handle};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::PrecisionDocument;
use crate::raw_source::{RawDescriptor, MAX_RAW_DESCRIPTOR_BYTES};
use crate::raw_source_edit;
use std::ffi::{c_char, CString};
use std::ptr;

macro_rules! raw_ffi {
    ($doc:ty,$plan:ident,$attach:ident,$from_plan:ident,$from:ident,$length:ident,$copy:ident) => {
        /// Owned RAW attachment plan JSON. No encoded input is inspected or copied.
        /// # Safety
        /// document NULL/live; descriptor NULL/terminated UTF8; err_out NULL/writable.
        #[no_mangle]
        pub unsafe extern "C" fn $plan(
            document: *const $doc,
            layer: usize,
            encoded_length: usize,
            descriptor: *const c_char,
            err_out: *mut *mut c_char,
        ) -> *mut c_char {
            unsafe {
                fallible_op(
                    err_out,
                    "planning RAW attachment",
                    ptr::null_mut(),
                    || {
                        let descriptor = RawDescriptor::parse(borrow_cstr_bounded(
                            descriptor,
                            "RAW descriptor",
                            MAX_RAW_DESCRIPTOR_BYTES,
                        )?)?;
                        CString::new(
                            raw_source_edit::plan(
                                required_handle(document)?,
                                layer,
                                encoded_length,
                                &descriptor,
                                None,
                            )?
                            .json()
                            .to_string(),
                        )
                        .map_err(|e| e.to_string())
                    },
                    CString::into_raw,
                )
            }
        }
        /// Attach an owned, admitted copy of original RAW bytes to a fresh u16 master.
        /// # Safety
        /// Handles/token NULL/live; encoded NULL or readable for encoded_length bytes;
        /// descriptor NULL/terminated UTF8; err_out NULL/writable. Inputs live through return.
        #[no_mangle]
        pub unsafe extern "C" fn $attach(
            document: *const $doc,
            layer: usize,
            encoded: *const u8,
            encoded_length: usize,
            descriptor: *const c_char,
            limit: u64,
            other: u64,
            control: *const DrJobControl,
            err_out: *mut *mut c_char,
        ) -> *mut $doc {
            unsafe {
                fallible_op(
                    err_out,
                    "attaching RAW source",
                    ptr::null_mut(),
                    || {
                        let document = required_handle(document)?;
                        let control = control.as_ref();
                        let _run = control.map(DrJobControl::begin).transpose()?;
                        let job = control.map_or(JobContext::NONE, JobContext::new);
                        crate::raw_source::validate_length(encoded_length)?;
                        if encoded.is_null() {
                            return Err("RAW bytes are NULL".into());
                        }
                        let text = borrow_cstr_bounded(
                            descriptor,
                            "RAW descriptor",
                            MAX_RAW_DESCRIPTOR_BYTES,
                        )?;
                        raw_source_edit::admit_descriptor(
                            document,
                            None,
                            (encoded_length, text.len()),
                            limit,
                            other,
                            job,
                        )?;
                        let descriptor = RawDescriptor::parse(text)?;
                        raw_source_edit::attach(
                            document,
                            layer,
                            std::slice::from_raw_parts(encoded, encoded_length),
                            &descriptor,
                            limit,
                            other,
                            job,
                        )
                    },
                    boxed,
                )
            }
        }
        /// Plan an attachment sharing an original retained RAW payload allocation.
        /// # Safety
        /// Handles NULL/live; descriptor NULL/terminated UTF8; err_out NULL/writable.
        #[no_mangle]
        pub unsafe extern "C" fn $from_plan(
            document: *const $doc,
            layer: usize,
            original: *const $doc,
            original_layer: usize,
            descriptor: *const c_char,
            err_out: *mut *mut c_char,
        ) -> *mut c_char {
            unsafe {
                fallible_op(
                    err_out,
                    "planning RAW redevelopment attachment",
                    ptr::null_mut(),
                    || {
                        let original = required_handle(original)?;
                        let captured = raw_source_edit::raw(original, original_layer)?;
                        let descriptor = RawDescriptor::parse(borrow_cstr_bounded(
                            descriptor,
                            "RAW descriptor",
                            MAX_RAW_DESCRIPTOR_BYTES,
                        )?)?;
                        CString::new(
                            raw_source_edit::plan(
                                required_handle(document)?,
                                layer,
                                captured.encoded.len(),
                                &descriptor,
                                Some((original, original_layer)),
                            )?
                            .json()
                            .to_string(),
                        )
                        .map_err(|e| e.to_string())
                    },
                    CString::into_raw,
                )
            }
        }
        /// Attach the original payload by immutable sharing, with a new effective recipe.
        /// # Safety
        /// Handles/token NULL/live; descriptor NULL/terminated UTF8; err_out NULL/writable.
        #[no_mangle]
        pub unsafe extern "C" fn $from(
            document: *const $doc,
            layer: usize,
            original: *const $doc,
            original_layer: usize,
            descriptor: *const c_char,
            limit: u64,
            other: u64,
            control: *const DrJobControl,
            err_out: *mut *mut c_char,
        ) -> *mut $doc {
            unsafe {
                fallible_op(
                    err_out,
                    "attaching redeveloped RAW source",
                    ptr::null_mut(),
                    || {
                        let document = required_handle(document)?;
                        let original = required_handle(original)?;
                        let control = control.as_ref();
                        let _run = control.map(DrJobControl::begin).transpose()?;
                        let job = control.map_or(JobContext::NONE, JobContext::new);
                        let text = borrow_cstr_bounded(
                            descriptor,
                            "RAW descriptor",
                            MAX_RAW_DESCRIPTOR_BYTES,
                        )?;
                        raw_source_edit::admit_descriptor(
                            document,
                            Some(original),
                            (0, text.len()),
                            limit,
                            other,
                            job,
                        )?;
                        let descriptor = RawDescriptor::parse(text)?;
                        raw_source_edit::attach_from(
                            document,
                            layer,
                            (original, original_layer),
                            &descriptor,
                            limit,
                            other,
                            job,
                        )
                    },
                    boxed,
                )
            }
        }
        /// Exact retained original byte length; zero and err_out on refusal.
        /// # Safety
        /// document NULL/live; err_out NULL/writable.
        #[no_mangle]
        pub unsafe extern "C" fn $length(
            document: *const $doc,
            layer: usize,
            err_out: *mut *mut c_char,
        ) -> usize {
            unsafe {
                fallible_op(
                    err_out,
                    "reading RAW original length",
                    0,
                    || {
                        Ok(raw_source_edit::raw(required_handle(document)?, layer)?
                            .encoded
                            .len())
                    },
                    |v| v,
                )
            }
        }
        /// Copy exact original bytes into caller-owned output. Length mismatch/NULL
        /// refuses before writing. There is no codec or filesystem work.
        /// # Safety
        /// document NULL/live; output NULL or writable for output_length bytes and
        /// nonoverlapping every retained input; err_out NULL/writable.
        #[no_mangle]
        pub unsafe extern "C" fn $copy(
            document: *const $doc,
            layer: usize,
            output: *mut u8,
            output_length: usize,
            err_out: *mut *mut c_char,
        ) -> bool {
            unsafe {
                fallible_op(
                    err_out,
                    "reading RAW original bytes",
                    false,
                    || {
                        let captured = raw_source_edit::raw(required_handle(document)?, layer)?;
                        if output.is_null() || output_length != captured.encoded.len() {
                            return Err(
                                "RAW output buffer must match the exact original byte length"
                                    .into(),
                            );
                        }
                        ptr::copy_nonoverlapping(
                            captured.encoded.as_ptr(),
                            output,
                            captured.encoded.len(),
                        );
                        Ok(())
                    },
                    |()| true,
                )
            }
        }
    };
}
raw_ffi!(
    DrDocument,
    dr_doc_raw_source_plan,
    dr_doc_attach_raw_source,
    dr_doc_raw_source_from_plan,
    dr_doc_attach_raw_source_from,
    dr_doc_raw_source_length,
    dr_doc_raw_source_copy
);
raw_ffi!(
    PrecisionDocument,
    dr_precision_doc_raw_source_plan,
    dr_precision_doc_attach_raw_source,
    dr_precision_doc_raw_source_from_plan,
    dr_precision_doc_attach_raw_source_from,
    dr_precision_doc_raw_source_length,
    dr_precision_doc_raw_source_copy
);
