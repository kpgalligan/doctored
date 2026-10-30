//! Retained binary-resource attachment queries. An attachment is written
//! only by the RAW local-mask publication (`ffi_raw_source`); a host-media
//! attachment an older file carries still reads here.
use crate::ffi_util::fallible_op;
use crate::raw_source_edit::RawDocument;
use std::ffi::{c_char, CString};
use std::ptr;

macro_rules! attachment_kind {
    ($doc:ty,$name:ident) => {
        /// Allocation-free kind tag: 0 absent,1 host_media,2 raw_local_masks,3 other.
        /// # Safety
        /// document must be NULL or remain live through return.
        #[no_mangle]
        pub unsafe extern "C" fn $name(document: *const $doc, layer: usize) -> u32 {
            unsafe {
                fallible_op(
                    ptr::null_mut(),
                    "reading source attachment kind",
                    0,
                    || {
                        Ok(document
                            .as_ref()
                            .and_then(|d| d.source_attachment(layer))
                            .map_or(0, |a| match a.descriptor.kind.as_str() {
                                "host_media" => 1,
                                "raw_local_masks" => 2,
                                _ => 3,
                            }))
                    },
                    |v| v,
                )
            }
        }
    };
}
attachment_kind!(crate::doc::DrDocument, dr_doc_source_attachment_kind);
attachment_kind!(
    crate::precision_document::PrecisionDocument,
    dr_precision_doc_source_attachment_kind
);

macro_rules! exports {
    ($doc:ty,$info:ident,$has:ident,$scratch:ident) => {
        /// Allocation-free presence query; NULL/invalid/unbound/absent is false.
        /// # Safety
        /// document is NULL or a live document retained throughout this call.
        #[no_mangle]
        pub unsafe extern "C" fn $has(document: *const $doc, layer: usize) -> bool {
            unsafe {
                fallible_op(
                    ptr::null_mut(),
                    "checking source attachment",
                    false,
                    || {
                        Ok(document
                            .as_ref()
                            .and_then(|d| d.source_attachment(layer))
                            .is_some())
                    },
                    |v| v,
                )
            }
        }
        /// Cheap metadata expansion allowance for core serialization and host parse;
        /// NULL/invalid/absent returns a bounded64KiB default, without parsing JSON.
        /// # Safety
        /// document is NULL or a live document retained throughout this call.
        #[no_mangle]
        pub unsafe extern "C" fn $scratch(document: *const $doc, layer: usize) -> u64 {
            unsafe {
                fallible_op(
                    ptr::null_mut(),
                    "sizing source attachment",
                    65_536,
                    || {
                        Ok(document
                            .as_ref()
                            .and_then(|d| d.source_attachment(layer))
                            .and_then(|a| a.descriptor.encoded_len().ok())
                            .map_or(65_536, |n| n as u64 * 768 + 65_536))
                    },
                    |v| v,
                )
            }
        }
        /// Owned complete envelope JSON; absent is NULL without error. Host must
        /// admit the scratch query before serializing/parsing this metadata.
        /// # Safety
        /// document NULL/live; err_out NULL/writable. Returned string uses dr_string_free.
        #[no_mangle]
        pub unsafe extern "C" fn $info(
            document: *const $doc,
            layer: usize,
            err_out: *mut *mut c_char,
        ) -> *mut c_char {
            unsafe {
                fallible_op(
                    err_out,
                    "reading source attachment",
                    ptr::null_mut(),
                    || {
                        let Some(attachment) =
                            document.as_ref().and_then(|d| d.source_attachment(layer))
                        else {
                            return Ok(None);
                        };
                        Ok(Some(
                            CString::new(attachment.descriptor.to_json()?)
                                .map_err(|e| e.to_string())?,
                        ))
                    },
                    |v| v.map_or(ptr::null_mut(), CString::into_raw),
                )
            }
        }
    };
}
exports!(
    crate::doc::DrDocument,
    dr_doc_source_attachment_info,
    dr_doc_has_source_attachment,
    dr_doc_source_attachment_scratch_bytes
);
exports!(
    crate::precision_document::PrecisionDocument,
    dr_precision_doc_source_attachment_info,
    dr_precision_doc_has_source_attachment,
    dr_precision_doc_source_attachment_scratch_bytes
);
