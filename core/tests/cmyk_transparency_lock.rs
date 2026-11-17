//! A transparency-locked CMYK raster under `under_locks`' Pixels writers
//! (`precision_lock::freezing_alpha`'s CMYK arm, `cmyk_lock`), through the
//! FFI: a type or shape re-render (`dr_precision_doc_cmyk_artwork`) and a
//! retouch that grows its target (`dr_precision_doc_edit`).
//!
//! The oracles are independent of the implementation: an UNLOCKED twin's
//! answer to the same request (`new`), the layer's samples before it
//! (`old`), and the frozen-alpha rule written out here ([`frozen`]): the
//! old sample where either alpha is 0, else the new ink at the old alpha.
//! A moved or resized raster is the lock's own sentence, word for word.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_cmyk_artwork::dr_precision_doc_cmyk_artwork;
use doctored_core::precision_document::PrecisionDocument;

use crate::cmyk_structure_fixture::{cmyk_layers, free, then, try_edit, varied, RgbLayer, LIMIT};
use crate::common::take_err_string;

const SHAPE: &str = r#"{"type":"shape","version":6}"#;
const TEXT: &str = r#"{"type":"text","version":5}"#;

/// The described layer's rect on the 12 × 8 canvas.
const AT: (i32, i32) = (2, 1);
const SIZE: (u32, u32) = (6, 4);

/// The first ink: every value a power of two (or 0 or 1), so the assembled
/// straight ink is the ink's own code at every coverage.
const INK: [f32; 5] = [0.5, 0.25, 0.0, 1.0, 1.0];

/// The frozen-alpha rule (module doc).
fn frozen(old: [u16; 5], new: [u16; 5]) -> [u16; 5] {
    if old[4] == 0 || new[4] == 0 {
        old
    } else {
        [new[0], new[1], new[2], new[3], old[4]]
    }
}

/// `dr_precision_doc_cmyk_artwork` with one coverage component of `ink` at
/// `offset`; `Ok(null)` for a no-op.
#[allow(clippy::too_many_arguments)]
fn publish(
    doc: *const PrecisionDocument,
    layer: usize,
    replacing: bool,
    offset: (i32, i32),
    size: (u32, u32),
    ink: [f32; 5],
    coverage: &[u16],
    description: &str,
) -> Result<*mut PrecisionDocument, String> {
    let components = CString::new(format!(r#"[{{"kind":"coverage","ink":{ink:?}}}]"#)).unwrap();
    let description = CString::new(description).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_cmyk_artwork(
            doc,
            layer,
            replacing,
            0,
            size.0,
            size.1,
            components.as_ptr(),
            coverage.as_ptr(),
            coverage.len(),
            ptr::null(),
            0,
            offset.0,
            offset.1,
            c"Artwork".as_ptr(),
            description.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() && !err.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

/// A coverage plane of `size` whose value at pixel `i` is `classes[i % 6]`.
fn coverage(size: (u32, u32), classes: [u16; 6]) -> Vec<u16> {
    (0..size.0 * size.1)
        .map(|i| classes[i as usize % 6])
        .collect()
}

/// The first coverage: every alpha class (0, partial, opaque).
const OLD: [u16; 6] = [0, 65535, 30000, 0, 50000, 65535];

/// A 12 × 8 CMYK document of one varied layer, with a [`SHAPE`] layer of
/// [`INK`] over [`OLD`] at [`AT`] above it (record 1).
fn described() -> *mut PrecisionDocument {
    let base = cmyk_layers(
        12,
        8,
        vec![RgbLayer {
            name: "Bottom",
            w: 12,
            h: 8,
            offset: (0, 0),
            pixels: varied(12, 8, 1),
        }],
    );
    let out = publish(base, 0, false, AT, SIZE, INK, &coverage(SIZE, OLD), SHAPE).unwrap();
    assert!(!out.is_null());
    free(base);
    out
}

fn lock(doc: *mut PrecisionDocument, layer: usize) -> *mut PrecisionDocument {
    then(
        doc,
        &format!(r#"{{"op":"layer_lock","layer":{layer},"locks":1}}"#),
        &[],
    )
}

fn samples_of(doc: *const PrecisionDocument, index: usize) -> Vec<[u16; 5]> {
    unsafe { &*doc }.cmyk_layer_samples(index).unwrap()
}

fn offset_of(doc: *const PrecisionDocument, index: usize) -> (i32, i32) {
    unsafe { &*doc }.layer(index).unwrap().offset()
}

fn description_of(doc: *const PrecisionDocument, index: usize) -> Option<String> {
    unsafe { &*doc }
        .artwork_description(index)
        .map(str::to_string)
}

#[test]
fn a_moved_or_resized_rerender_names_the_transparency_lock() {
    let doc = described();
    let locked = lock(doc, 1);
    let moved = publish(
        locked,
        1,
        true,
        (3, 1),
        SIZE,
        INK,
        &coverage(SIZE, OLD),
        TEXT,
    );
    assert_eq!(
        moved.unwrap_err(),
        "high-depth layer 1 is locked (transparency)"
    );
    let wider = (7, 4);
    let resized = publish(locked, 1, true, AT, wider, INK, &coverage(wider, OLD), TEXT);
    assert_eq!(
        resized.unwrap_err(),
        "high-depth layer 1 is locked (transparency)"
    );
    free(locked);
}

#[test]
fn an_unmoved_rerender_keeps_the_frozen_alpha_and_takes_the_new_description() {
    let doc = described();
    let ink = [0.7f32, 0.1, 0.5, 0.2, 0.9];
    // old α 0 → new α > 0, old α > 0 → new α 0, both partial, both 0,
    // partial → opaque, opaque → opaque.
    let next = coverage(SIZE, [40000, 0, 20000, 0, 65535, 65535]);
    let twin = publish(doc, 1, true, AT, SIZE, ink, &next, TEXT).unwrap();
    assert!(!twin.is_null());
    let old = samples_of(doc, 1);
    let new = samples_of(twin, 1);
    let want: Vec<[u16; 5]> = old.iter().zip(&new).map(|(o, n)| frozen(*o, *n)).collect();
    assert!(want != old, "the re-render changes the ink");
    assert!(want != new, "the lock changes the re-render");
    let locked = lock(doc, 1);
    let out = publish(locked, 1, true, AT, SIZE, ink, &next, TEXT).unwrap();
    assert!(!out.is_null());
    assert_eq!(samples_of(out, 1), want);
    assert_eq!(offset_of(out, 1), AT);
    assert_eq!(description_of(out, 1).as_deref(), Some(TEXT));
    for d in [out, locked, twin] {
        free(d);
    }
}

#[test]
fn an_unmoved_rerender_whose_frozen_pixels_are_the_old_ones_is_null() {
    let doc = described();
    // The same ink; only the alpha amounts move, within each class.
    let next = coverage(SIZE, [0, 40000, 65535, 0, 20000, 30000]);
    let twin = publish(doc, 1, true, AT, SIZE, INK, &next, TEXT).unwrap();
    assert!(!twin.is_null(), "unlocked, the re-render is a new document");
    let (old, new) = (samples_of(doc, 1), samples_of(twin, 1));
    assert!(old != new);
    assert!(
        old.iter().zip(&new).all(|(o, n)| frozen(*o, *n) == *o),
        "the frozen re-render is the old samples"
    );
    let locked = lock(doc, 1);
    let out = publish(locked, 1, true, AT, SIZE, INK, &next, TEXT).unwrap();
    assert!(out.is_null(), "locked, the re-render changes nothing");
    free(twin);
    free(locked);
}

#[test]
fn a_locked_group_names_itself_for_a_moved_rerender() {
    let doc = described();
    let grouped = then(
        doc,
        r#"{"op":"group_layers","layers":[1],"name":"G","pass_through":false}"#,
        &[],
    );
    let locked = lock(grouped, 2);
    let moved = publish(
        locked,
        1,
        true,
        (3, 1),
        SIZE,
        INK,
        &coverage(SIZE, OLD),
        TEXT,
    );
    assert_eq!(
        moved.unwrap_err(),
        "high-depth layer 1 is inside group 2, which is locked (transparency)"
    );
    free(locked);
}

#[test]
fn a_retouch_that_expands_a_transparency_locked_layer_names_the_lock() {
    let (w, h) = (24u32, 20u32);
    let opaque: Vec<[u16; 4]> = varied(w, h, 2)
        .into_iter()
        .map(|p| [p[0], p[1], p[2], 65535])
        .collect();
    let doc = cmyk_layers(
        w,
        h,
        vec![
            RgbLayer {
                name: "Background",
                w,
                h,
                offset: (0, 0),
                pixels: opaque,
            },
            RgbLayer {
                name: "Offset",
                w: 16,
                h: 12,
                offset: (4, 4),
                pixels: varied(16, 12, 6),
            },
        ],
    );
    // A 3 × 3 block inside the layer (x 10..13, y 8..11).
    let block: Vec<u16> = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            if (10..13).contains(&x) && (8..11).contains(&y) {
                65535
            } else {
                0
            }
        })
        .collect();
    let request = r#"{"op":"retouch_clone","layer":1,"sample":"all","matrix":[1,0,0,1,-5,-3],"expand_destination":true}"#;
    let twin = try_edit(doc, request, &block).unwrap();
    assert!(!twin.is_null());
    let grown = unsafe { &*twin }.layer(1).unwrap();
    assert_eq!(grown.cmyk_source().unwrap().dimensions(), (w, h));
    assert_eq!(grown.offset(), (0, 0));
    let locked = lock(doc, 1);
    assert_eq!(
        try_edit(locked, request, &block).unwrap_err(),
        "high-depth layer 1 is locked (transparency)"
    );
    free(twin);
    free(locked);
}
