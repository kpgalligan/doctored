//! Don't Convert of a LAYER clipboard of another profile at 8
//! and 16 bits RELABELS it (`doc_paste_layers`' rule 1 and rule 3), through
//! `dr_doc_paste_layers` / `dr_precision_doc_paste_layers` with `preserve`.
//!
//! Oracles, none read off the implementation: every record's samples are the
//! clip's own, bit for bit; the placed layer's retained source is still
//! effective in the result, re-pointed at the destination's profile handle;
//! Convert keeps the profile refusal, written out here.

use crate::icc_spelling;
use crate::live_filter_support;

use std::ffi::{c_char, CStr};
use std::ptr;
use std::sync::Arc;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_layer_clipboard::{dr_doc_paste_layers, dr_precision_doc_paste_layers};
use doctored_core::icc::IccProfile;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;

use live_filter_support::{plasma, Doc};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const ABOVE: u32 = 0;
const PROFILE_REFUSAL: &str = "pasted layers must be in the document's profile; convert them first";
const W: u32 = 12;
const H: u32 = 9;

fn owned(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}

/// A `profile` clip at the document depth: a white `Background` and a placed
/// image (an sRGB master) as layer 1.
fn clip(typed: bool, profile: Arc<IccProfile>) -> Doc {
    let pixels = plasma(W, H, 0);
    Doc::placed_over_white(typed, typed, W, H, &pixels, IccProfile::srgb(), profile)
}

/// The clip pasted above record 0 of `dest`, its refusal as `Err`.
fn paste(dest: &Doc, clip: &Doc, preserve: bool) -> Result<Doc, String> {
    let mut err = ptr::null_mut();
    let out = unsafe {
        match (dest, clip) {
            (Doc::Byte(d), Doc::Byte(c)) => {
                let p = dr_doc_paste_layers(d, c, 0, ABOVE, 0, 0, preserve, &mut err);
                (!p.is_null()).then(|| Doc::Byte(*Box::from_raw(p)))
            }
            (Doc::Typed(d), Doc::Typed(c)) => {
                let p = dr_precision_doc_paste_layers(
                    d,
                    c,
                    0,
                    ABOVE,
                    0,
                    0,
                    preserve,
                    LIMIT,
                    0,
                    ptr::null_mut(),
                    &mut err,
                );
                (!p.is_null()).then(|| Doc::Typed(*Box::from_raw(p)))
            }
            _ => panic!("one depth"),
        }
    };
    match owned(err) {
        Some(message) => {
            assert!(out.is_none());
            Err(message)
        }
        None => Ok(out.expect("a paste is never a silent no-op")),
    }
}

/// Layer `layer`'s retained source is effective under the document's
/// profile and is rendered under `profile`.
fn assert_bound(doc: &Doc, layer: usize, profile: &Arc<IccProfile>) {
    match doc {
        Doc::Byte(d) => {
            assert!(Arc::ptr_eq(&d.profile, profile), "the destination's handle");
            let source = d.layers[layer]
                .effective_source(&d.profile)
                .expect("the placed image stays bound");
            assert!(Arc::ptr_eq(&source.rendered_profile, profile));
        }
        Doc::Typed(d) => {
            assert!(
                Arc::ptr_eq(d.profile(), profile),
                "the destination's handle"
            );
            let record = d.layer(layer).unwrap();
            let source = record
                .effective_source()
                .expect("the placed image stays bound");
            assert!(Arc::ptr_eq(source.rendered.profile(), profile));

            assert!(Arc::ptr_eq(
                d.layer_source(layer).unwrap().profile(),
                profile
            ));
        }
    }
}

fn destination(typed: bool) -> (Doc, Arc<IccProfile>) {
    let dest = Doc::white(typed, W, H, IccProfile::srgb());
    let profile = match &dest {
        Doc::Byte(d) => d.profile.clone(),
        Doc::Typed(d) => d.profile().clone(),
    };
    (dest, profile)
}

#[test]
fn a_display_p3_clip_pasted_unconverted_keeps_its_numbers_and_its_placed_image() {
    for typed in [false, true] {
        let clip = clip(typed, IccProfile::display_p3());
        assert_bound(
            &clip,
            1,
            &match &clip {
                Doc::Byte(d) => d.profile.clone(),
                Doc::Typed(d) => d.profile().clone(),
            },
        );
        let (dest, profile) = destination(typed);
        let out = paste(&dest, &clip, true).unwrap();
        assert_eq!(out.layer_count(), 3, "typed {typed}");
        assert_eq!(out.layer_pixels(1), clip.layer_pixels(0), "Background");
        assert_eq!(
            out.layer_pixels(2),
            clip.layer_pixels(1),
            "the placed image"
        );
        assert_bound(&out, 2, &profile);
        assert_eq!(
            paste(&dest, &clip, false).err().as_deref(),
            Some(PROFILE_REFUSAL),
            "Convert still refuses, typed {typed}"
        );
    }
}

#[test]
fn a_second_spelling_of_srgb_relabels_under_dont_convert() {
    let bytes = icc_spelling::srgb_second_spelling();
    let spelled = Arc::new(IccProfile::parse(&bytes).expect("a matrix/TRC profile"));
    assert_ne!(&bytes[..], &IccProfile::srgb().bytes()[..]);
    for typed in [false, true] {
        let clip = clip(typed, spelled.clone());
        let (dest, profile) = destination(typed);
        let out = paste(&dest, &clip, true).unwrap();
        assert_eq!(out.layer_pixels(1), clip.layer_pixels(0), "typed {typed}");
        assert_eq!(out.layer_pixels(2), clip.layer_pixels(1), "typed {typed}");
        assert_bound(&out, 2, &profile);
        assert_eq!(
            paste(&dest, &clip, false).err().as_deref(),
            Some(PROFILE_REFUSAL)
        );
    }
}
