//! A CMYK layer's SELECTED PIXELS (#547 phase 4b): Move (the layer's extent
//! unchanged and grown), ⌥ duplicate, Transform, Puppet Warp and Perspective
//! Warp of the selection, a mask target and the float bounds, each against
//! the two RGB16 twins (`common/cmyk_geometry_fixture.rs`); the RGB twin's
//! adjustment-layer refusal; and an RGB colour-plane target refused by name
//! (an ink plate moves: `cmyk_selected_plate.rs`).

use std::ptr;

use doctored_core::ffi_selected_pixels::dr_precision_doc_selected_pixels_bounds;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::json;

use crate::cmyk_geometry_fixture::{assert_twin_edit, record, turn, twins, Twins};
use crate::cmyk_structure_fixture::{
    cmyk_layers, free, ramp, refusal, varied, with_levels, with_mask, RgbLayer,
};
use crate::common::take_err_string;

const W: u32 = 37;
const H: u32 = 23;

/// A record of `size` at `offset` on the 37 × 23 canvas, masked.
fn base(offset: (i32, i32), size: (u32, u32)) -> Twins {
    let doc = cmyk_layers(
        W,
        H,
        vec![RgbLayer {
            name: "Ink",
            w: size.0,
            h: size.1,
            offset,
            pixels: varied(size.0, size.1, 23),
        }],
    );
    twins(with_mask(doc, 0, &ramp(W, H, 4)))
}

/// A canvas coverage: 65535 inside `(x, y, w, h)`, a feathered column to its
/// right, 0 elsewhere.
fn selection(x: u32, y: u32, w: u32, h: u32) -> Vec<u16> {
    (0..W * H)
        .map(|i| {
            let (px, py) = (i % W, i / W);
            if py < y || py >= y + h {
                0
            } else if px >= x && px < x + w {
                65535
            } else if px == x + w {
                30000
            } else {
                0
            }
        })
        .collect()
}

#[test]
fn moving_and_duplicating_selected_ink_is_its_twins() {
    let full = base((0, 0), (W, H));
    let coverage = selection(6, 5, 12, 9);
    for request in [
        json!({"op":"move_selected","layer":0,"dx":4,"dy":3}),
        json!({"op":"move_selected","layer":0,"dx":-3,"dy":2,"duplicate":true}),
        json!({"op":"move_selected","layer":0,"dx":5,"dy":-1,"bounds":[0,0,W,H]}),
    ] {
        let out = assert_twin_edit(&full, &request.to_string(), &coverage);
        assert_eq!(
            record(out.cmyk, 0).2,
            (W, H),
            "{request}: the extent is unchanged"
        );
        out.free();
    }
    full.free();
    // A smaller record: a move that lands beyond it grows the extent.
    let small = base((4, 3), (20, 12));
    let out = assert_twin_edit(
        &small,
        &json!({"op":"move_selected","layer":0,"dx":12,"dy":7}).to_string(),
        &selection(6, 5, 10, 6),
    );
    let (_, offset, size) = record(out.cmyk, 0);
    assert!(
        size.0 > 20 && size.1 > 12,
        "grown to {size:?} at {offset:?}"
    );
    out.free();
    small.free();
}

#[test]
fn transforming_and_warping_selected_ink_is_its_twins() {
    let full = base((0, 0), (W, H));
    let small = base((4, 3), (20, 12));
    let coverage = selection(8, 6, 14, 10);
    for request in [
        json!({"op":"transform_selected","layer":0,"matrix":turn(13.0, 1.1, (15.0, 11.0)),
            "filter":"bilinear"}),
        json!({"op":"transform_selected","layer":0,"matrix":turn(-20.0, 1.4, (15.0, 11.0)),
            "filter":"bicubic","duplicate":true}),
        json!({"op":"puppet_selected","layer":0,"pins":[
            {"from":[12.0,10.0],"to":[14.0,11.0]},{"from":[19.0,12.0],"to":[19.0,11.0]}]}),
        json!({"op":"perspective_selected","layer":0,"quads":[{
            "from":[8.0,6.0,22.0,6.0,22.0,16.0,8.0,16.0],
            "to":[9.0,7.0,24.0,5.0,23.0,17.0,7.0,15.0]}]}),
    ] {
        for doc in [&full, &small] {
            assert_twin_edit(doc, &request.to_string(), &coverage).free();
        }
    }
    full.free();
    small.free();
}

#[test]
fn a_mask_target_and_the_float_bounds_are_their_twins() {
    let full = base((0, 0), (W, H));
    let coverage = selection(6, 5, 12, 9);
    assert_twin_edit(
        &full,
        &json!({"op":"move_selected","layer":0,"dx":3,"dy":-2,"target":"mask"}).to_string(),
        &coverage,
    )
    .free();
    let bounds = |doc: *const PrecisionDocument| {
        let mut out = [0u32; 4];
        let mut err = ptr::null_mut();
        let found = unsafe {
            dr_precision_doc_selected_pixels_bounds(
                doc,
                0,
                coverage.as_ptr(),
                coverage.len(),
                ptr::null(),
                out.as_mut_ptr(),
                &mut err,
            )
        };
        assert!(err.is_null(), "{}", take_err_string(err));
        found.then_some(out)
    };
    let found = bounds(full.cmyk);
    assert!(found.is_some());
    assert_eq!(found, bounds(full.cmy));
    full.free();
}

#[test]
fn an_adjustment_record_and_a_colour_plane_refuse_by_name() {
    let doc = with_levels(
        cmyk_layers(
            W,
            H,
            vec![RgbLayer {
                name: "Ink",
                w: W,
                h: H,
                offset: (0, 0),
                pixels: varied(W, H, 5),
            }],
        ),
        1.0,
    );
    let coverage = selection(6, 5, 12, 9);
    assert_eq!(
        refusal(
            doc,
            &json!({"op":"move_selected","layer":1,"dx":2,"dy":1}).to_string(),
            &coverage
        ),
        "Moving selected pixels needs a pixel layer, and layer 1 is an adjustment layer"
    );
    assert_eq!(
        refusal(
            doc,
            &json!({"op":"transform_selected","layer":1,"matrix":[1.0,0.0,0.0,1.0,2.0,1.0],
                "filter":"bilinear"})
            .to_string(),
            &coverage
        ),
        "Transforming selected pixels needs a pixel layer, and layer 1 is an adjustment layer"
    );
    assert_eq!(
        refusal(
            doc,
            &json!({"op":"move_selected","layer":0,"dx":2,"dy":1,"target":"red"}).to_string(),
            &coverage
        ),
        "Moving or transforming the \"red\" channel's selected pixels is unavailable in a CMYK document"
    );
    free(doc);
}
