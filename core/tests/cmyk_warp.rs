//! Warps on a CMYK document (#1 phase 4b): Layer ▸ Warp (pixels with a
//! linked mask, and a mask target), Puppet Warp, Perspective Warp, Liquify
//! (with and without a feathered selection) and Perspective Crop, each plane
//! for plane against its two RGB16 twins (`common/cmyk_geometry_fixture.rs`);
//! the CMYK warp applied at once with no binding (`warp_info` null, Clear and
//! Bake no change); and the mesh warps' adjustment-layer refusal.

use std::ffi::c_char;
use std::ptr;

use doctored_core::ffi_perspective_warp::dr_precision_doc_perspective_warp;
use doctored_core::ffi_puppet::dr_precision_doc_puppet_warp;
use doctored_core::ffi_selected_adjustment::dr_precision_doc_selected_edit;
use doctored_core::ffi_warp::{
    dr_precision_doc_warp_edit, dr_precision_doc_warp_edit_plan, dr_precision_doc_warp_info,
};
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::cmyk_geometry_fixture::{assert_twin_edit, assert_twins, c, each, twins, Twins};
use crate::cmyk_structure_fixture::{
    cmyk_layers, free, json as json_of, ramp, varied, with_levels, with_mask, RgbLayer, LIMIT,
};
use crate::common::take_err_string;

const W: u32 = 37;
const H: u32 = 23;

fn base() -> Twins {
    let doc = cmyk_layers(
        W,
        H,
        vec![RgbLayer {
            name: "Ink",
            w: W,
            h: H,
            offset: (0, 0),
            pixels: varied(W, H, 17),
        }],
    );
    twins(with_mask(doc, 0, &ramp(W, H, 9)))
}

type Call = unsafe extern "C" fn(
    *const PrecisionDocument,
    *const c_char,
    u64,
    u64,
    *const doctored_core::job_control::DrJobControl,
    *mut *mut c_char,
) -> *mut PrecisionDocument;

/// One of the request-only document exports: `Ok(null)` for no change.
fn call(
    f: Call,
    doc: *const PrecisionDocument,
    request: &Value,
) -> Result<*mut PrecisionDocument, String> {
    let text = c(&request.to_string());
    let mut err = ptr::null_mut();
    let out = unsafe { f(doc, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    if out.is_null() && !err.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

fn changed(f: Call, base: &Twins, request: &Value) -> Twins {
    let out =
        each(base, |doc| call(f, doc, request)).unwrap_or_else(|| panic!("{request}: no change"));
    assert!(unsafe { &*out.cmyk }.is_cmyk(), "{request}: still CMYK");
    assert_twins(&out, &request.to_string());
    out
}

fn warp(columns: u32, rows: u32, target: &str, moved: [f64; 2]) -> Value {
    let mut displacements = vec![[0.0, 0.0]; (columns * rows) as usize];
    displacements[(columns + 1) as usize] = moved;
    json!({"op":"set","layer":0,"target":target,"columns":columns,"rows":rows,
        "displacements":displacements,"filter":"bicubic"})
}

fn info(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    json_of(unsafe { dr_precision_doc_warp_info(doc, 0, &mut err) }, err)
}

#[test]
fn layer_warp_is_its_twins_and_keeps_no_binding() {
    let base = base();
    // Pixels (the linked mask follows) and the mask alone.
    for target in ["pixels", "mask"] {
        let out = changed(
            dr_precision_doc_warp_edit,
            &base,
            &warp(3, 3, target, [2.0, 1.5]),
        );
        // Applied at once: no field to re-edit, Clear and Bake change nothing.
        assert_eq!(info(out.cmyk)["field"], Value::Null, "{target}");
        for request in [
            json!({"op":"clear","layer":0}),
            json!({"op":"bake","layer":0}),
        ] {
            let next = call(dr_precision_doc_warp_edit, out.cmyk, &request).unwrap();
            assert!(
                next.is_null(),
                "{request} after a CMYK warp changes nothing"
            );
        }
        out.free();
    }
    // A translation field moves the record exactly; an identity changes nothing.
    let mut shift = json!({"op":"set","layer":0,"target":"pixels","columns":2,"rows":2,
        "displacements":[[3.0,-2.0],[3.0,-2.0],[3.0,-2.0],[3.0,-2.0]],"filter":"bilinear"});
    changed(dr_precision_doc_warp_edit, &base, &shift).free();
    shift["displacements"] = json!([[0.0, 0.0], [0.0, 0.0], [0.0, 0.0], [0.0, 0.0]]);
    assert!(call(dr_precision_doc_warp_edit, base.cmyk, &shift)
        .unwrap()
        .is_null());
    let mut err = ptr::null_mut();
    let plan = json_of(
        unsafe {
            dr_precision_doc_warp_edit_plan(base.cmyk, c(&shift.to_string()).as_ptr(), &mut err)
        },
        err,
    );
    assert_eq!(plan["scratch_bytes"], 0, "{plan}");
    base.free();
}

#[test]
fn puppet_and_perspective_warp_are_their_twins() {
    let base = base();
    changed(
        dr_precision_doc_puppet_warp,
        &base,
        &json!({"op":"puppet_warp","layer":0,"pins":[
            {"from":[12.0,11.0],"to":[14.0,12.0]},{"from":[26.0,11.0],"to":[26.0,10.0]}]}),
    )
    .free();
    changed(
        dr_precision_doc_perspective_warp,
        &base,
        &json!({"op":"perspective_warp","layer":0,"quads":[{
            "from":[5.0,4.0,31.0,4.0,31.0,19.0,5.0,19.0],
            "to":[7.0,5.0,32.0,3.0,30.0,20.0,4.0,18.0]}]}),
    )
    .free();
    base.free();
}

fn liquify(
    doc: *const PrecisionDocument,
    coverage: &[u16],
) -> Result<*mut PrecisionDocument, String> {
    let request = json!({"layer":0,"op":"liquify","params":{"strokes":[
        {"tool":"forward_warp","size":18,"strength":0.8,"points":[[14.0,11.0],[22.0,13.0]]},
        {"tool":"twirl_clockwise","size":12,"points":[[25.0,8.0],[25.0,8.0]]}]}});
    let text = c(&request.to_string());
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_selected_edit(
            doc,
            text.as_ptr(),
            if coverage.is_empty() {
                ptr::null()
            } else {
                coverage.as_ptr()
            },
            coverage.len(),
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

#[test]
fn liquify_is_its_twins_with_and_without_a_feathered_selection() {
    let base = base();
    let feathered: Vec<u16> = (0..W * H)
        .map(|i| (((i % W) * 65535) / (W - 1)) as u16)
        .collect();
    for coverage in [Vec::new(), feathered] {
        let out = each(&base, |doc| liquify(doc, &coverage)).expect("liquify changes the layer");
        assert!(unsafe { &*out.cmyk }.is_cmyk());
        assert_twins(&out, "liquify");
        out.free();
    }
    base.free();
}

#[test]
fn perspective_crop_is_its_twins() {
    let base = base();
    for filter in ["bilinear", "bicubic"] {
        assert_twin_edit(
            &base,
            &json!({"op":"perspective_crop","quad":[3.0,2.0,34.0,4.0,33.0,21.0,2.0,20.0],
                "width":30,"height":18,"filter":filter})
            .to_string(),
            &[],
        )
        .free();
    }
    base.free();
}

#[test]
fn the_mesh_warps_refuse_an_adjustment_record_as_the_rgb_twin_does() {
    let doc = with_levels(
        cmyk_layers(
            W,
            H,
            vec![RgbLayer {
                name: "Ink",
                w: W,
                h: H,
                offset: (0, 0),
                pixels: varied(W, H, 2),
            }],
        ),
        1.0,
    );
    assert!(unsafe { &*doc }.layer(1).unwrap().cmyk_source().is_none());
    let puppet =
        json!({"op":"puppet_warp","layer":1,"pins":[{"from":[12.0,11.0],"to":[14.0,12.0]}]});
    assert_eq!(
        call(dr_precision_doc_puppet_warp, doc, &puppet).unwrap_err(),
        "Puppet Warp needs a pixel layer, and layer 1 is an adjustment layer"
    );
    let perspective = json!({"op":"perspective_warp","layer":1,"quads":[{
        "from":[5.0,4.0,31.0,4.0,31.0,19.0,5.0,19.0],"to":[7.0,5.0,32.0,3.0,30.0,20.0,4.0,18.0]}]});
    assert_eq!(
        call(dr_precision_doc_perspective_warp, doc, &perspective).unwrap_err(),
        "Perspective Warp needs a pixel layer, and layer 1 is an adjustment layer"
    );
    // A warp of an adjustment record's pixels refuses as on RGB.
    let mut pixels = warp(3, 3, "pixels", [2.0, 1.5]);
    pixels["layer"] = json!(1);
    assert_eq!(
        call(dr_precision_doc_warp_edit, doc, &pixels).unwrap_err(),
        "warp pixels require a raster layer"
    );
    free(doc);
}
