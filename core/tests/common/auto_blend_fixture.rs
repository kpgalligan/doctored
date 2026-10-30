//! Auto-Blend Layers' test fixture: the frames, the three depths'
//! stacks, the FFI runners and the shared asserts of `layer_auto_blend.rs`
//! (Stack Images) and `layer_auto_blend_panorama.rs` (Panorama), moved here
//! verbatim. A suite root mounts it with
//! `#[path = "common/auto_blend_fixture.rs"] mod auto_blend_fixture;`.
#![allow(dead_code)]
#![allow(clippy::needless_range_loop)]

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::{DrDocument, LayerRaster};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_auto_blend::{
    dr_doc_auto_blend, dr_doc_auto_blend_plan, dr_precision_doc_auto_blend,
    dr_precision_doc_auto_blend_plan,
};
use doctored_core::ffi_doc::{dr_doc_layer_clipped, dr_doc_layer_count};
use doctored_core::ffi_precision::dr_precision_doc_info;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::float_fixture::float_doc;

pub(crate) const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// One layer: straight unit RGBA of its own size at a canvas offset.
#[derive(Clone)]
pub(crate) struct Frame {
    pub(crate) dims: (u32, u32),
    pub(crate) offset: (i32, i32),
    pub(crate) pixels: Vec<[f64; 4]>,
}

// ---------------------------------------------------------------- the depths --

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Depth {
    Byte,
    Sixteen,
    Float,
}

impl Depth {
    /// What the depth stores for a unit value, as a unit value again.
    pub(crate) fn stored(self, v: f64) -> f64 {
        match self {
            Depth::Byte => (v * 255.0).round() / 255.0,
            Depth::Sixteen => (v * 65535.0).round() / 65535.0,
            Depth::Float => f64::from(v as f32),
        }
    }
}

pub(crate) enum Stack {
    Byte(DrDocument),
    Typed(PrecisionDocument),
}

pub(crate) fn byte_image(frame: &Frame) -> RgbaImage {
    RgbaImage::from_fn(frame.dims.0, frame.dims.1, |x, y| {
        Rgba(frame.pixels[(y * frame.dims.0 + x) as usize].map(|v| (v * 255.0).round() as u8))
    })
}

pub(crate) fn typed_layer(name: &str, frame: &Frame) -> PrecisionDocumentLayer {
    let image = PrecisionImage::from_rgba16(
        frame.dims.0,
        frame.dims.1,
        frame
            .pixels
            .iter()
            .map(|p| p.map(|v| (v * 65535.0).round() as u16))
            .collect(),
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    PrecisionDocumentLayer::raster(name, image, frame.offset, 1.0, true).unwrap()
}

pub(crate) fn typed_document(
    canvas: (u32, u32),
    layers: Vec<PrecisionDocumentLayer>,
) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        canvas.0,
        canvas.1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

pub(crate) fn byte_document(canvas: (u32, u32), frames: &[Frame]) -> DrDocument {
    // `from_pixels` takes its canvas from the image, so the first layer's
    // raster is swapped in afterwards.
    let mut doc = DrDocument::from_pixels(RgbaImage::new(canvas.0, canvas.1));
    doc.layers[0].pixels = LayerRaster::new(byte_image(&frames[0]));
    doc.layers[0].offset = frames[0].offset;
    doc.layers[0].name = "F0".into();
    for (k, frame) in frames.iter().enumerate().skip(1) {
        doc = doc
            .adding_image_layer(k - 1, byte_image(frame), &format!("F{k}"))
            .expect("a frame layer");
        doc.layers[k].offset = frame.offset;
    }
    assert_eq!((doc.width, doc.height), canvas);
    doc
}

pub(crate) fn take(text: *mut c_char) -> String {
    assert!(!text.is_null());
    let owned = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(text) };
    owned
}

impl Stack {
    /// Layers "F0", "F1", … bottom first ("L0", … in a float document, whose
    /// records are canvas-sized with the frame's outside transparent).
    pub(crate) fn build(depth: Depth, canvas: (u32, u32), frames: &[Frame]) -> Self {
        match depth {
            Depth::Byte => Stack::Byte(byte_document(canvas, frames)),
            Depth::Sixteen => Stack::Typed(typed_document(
                canvas,
                frames
                    .iter()
                    .enumerate()
                    .map(|(k, frame)| typed_layer(&format!("F{k}"), frame))
                    .collect(),
            )),
            Depth::Float => {
                let records: Vec<Vec<[f32; 4]>> = frames
                    .iter()
                    .map(|frame| {
                        let mut record = vec![[0.0f32; 4]; (canvas.0 * canvas.1) as usize];
                        for y in 0..frame.dims.1 {
                            for x in 0..frame.dims.0 {
                                let (cx, cy) = (
                                    i64::from(x) + i64::from(frame.offset.0),
                                    i64::from(y) + i64::from(frame.offset.1),
                                );
                                if cx < 0
                                    || cy < 0
                                    || cx >= i64::from(canvas.0)
                                    || cy >= i64::from(canvas.1)
                                {
                                    continue;
                                }
                                let p = frame.pixels[(y * frame.dims.0 + x) as usize];
                                record[(cy * i64::from(canvas.0) + cx) as usize] = if p[3] > 0.0 {
                                    p.map(|v| v as f32)
                                } else {
                                    [0.0; 4]
                                };
                            }
                        }
                        record
                    })
                    .collect();
                Stack::Typed(float_doc(canvas.0, canvas.1, &IccProfile::srgb(), &records))
            }
        }
    }

    pub(crate) fn canvas(&self) -> (u32, u32) {
        match self {
            Stack::Byte(doc) => (doc.width, doc.height),
            Stack::Typed(doc) => doc.dimensions(),
        }
    }

    pub(crate) fn layer_count(&self) -> usize {
        match self {
            Stack::Byte(doc) => unsafe { dr_doc_layer_count(doc) },
            Stack::Typed(doc) => doc.layer_count(),
        }
    }

    pub(crate) fn plan(&self, request: &Value) -> Result<Value, String> {
        let text = CString::new(request.to_string()).unwrap();
        let mut err = ptr::null_mut();
        let plan = match self {
            Stack::Byte(doc) => unsafe { dr_doc_auto_blend_plan(doc, text.as_ptr(), &mut err) },
            Stack::Typed(doc) => unsafe {
                dr_precision_doc_auto_blend_plan(doc, text.as_ptr(), &mut err)
            },
        };
        if plan.is_null() {
            return Err(take(err));
        }
        assert!(err.is_null());
        Ok(serde_json::from_str(&take(plan)).unwrap())
    }

    pub(crate) fn run_with(
        &self,
        request: &Value,
        limit: u64,
        control: *const DrJobControl,
    ) -> Result<(Stack, Value), String> {
        let text = CString::new(request.to_string()).unwrap();
        let (mut report, mut err) = (ptr::null_mut(), ptr::null_mut());
        let out = match self {
            Stack::Byte(doc) => {
                let out = unsafe {
                    dr_doc_auto_blend(doc, text.as_ptr(), limit, 0, control, &mut report, &mut err)
                };
                (!out.is_null()).then(|| Stack::Byte(*unsafe { Box::from_raw(out) }))
            }
            Stack::Typed(doc) => {
                let out = unsafe {
                    dr_precision_doc_auto_blend(
                        doc,
                        text.as_ptr(),
                        limit,
                        0,
                        control,
                        &mut report,
                        &mut err,
                    )
                };
                (!out.is_null()).then(|| Stack::Typed(*unsafe { Box::from_raw(out) }))
            }
        };
        match out {
            Some(out) => {
                assert!(err.is_null(), "a result carries no error");
                Ok((out, serde_json::from_str(&take(report)).unwrap()))
            }
            None => {
                assert!(report.is_null(), "a refusal writes no report");
                assert!(!err.is_null(), "a NULL result always carries an error");
                Err(take(err))
            }
        }
    }

    pub(crate) fn run(&self, request: &Value) -> Result<(Stack, Value), String> {
        self.run_with(request, LIMIT, ptr::null())
    }

    pub(crate) fn has_mask(&self, layer: usize) -> bool {
        match self {
            Stack::Byte(doc) => doc.layers[layer].mask.is_some(),
            Stack::Typed(doc) => doc.layer(layer).unwrap().mask().is_some(),
        }
    }

    /// Layer `layer`'s mask as "reveals this pixel", after checking it is
    /// canvas-sized at (0, 0), binary, linked, enabled, density 1, feather 0
    /// and outside coverage 0.
    pub(crate) fn mask(&self, layer: usize) -> Vec<bool> {
        let canvas = self.canvas();
        let plain = MaskProperties::at((0, 0));
        match self {
            Stack::Byte(doc) => {
                let entry = &doc.layers[layer];
                let mask = entry.mask.as_ref().expect("a layer mask");
                assert_eq!(mask.dimensions(), canvas);
                assert!(entry.mask_enabled);
                assert!(entry.mask_properties == plain);
                mask.as_raw()
                    .iter()
                    .map(|&v| {
                        assert!(v == 0 || v == 255, "a binary mask");
                        v == 255
                    })
                    .collect()
            }
            Stack::Typed(doc) => {
                let mask = doc.layer(layer).unwrap().mask().expect("a layer mask");
                assert_eq!(mask.plane().dimensions(), canvas);
                assert!(mask.enabled());
                assert!(mask.properties() == &plain);
                mask.plane()
                    .samples()
                    .iter()
                    .map(|&v| {
                        assert!(v == 0 || v == 65535, "a binary mask");
                        v == 65535
                    })
                    .collect()
            }
        }
    }

    /// A canvas-sized layer at (0, 0) as unit RGBA (a float sample as it is).
    pub(crate) fn pixels(&self, layer: usize) -> Vec<[f64; 4]> {
        let canvas = self.canvas();
        match self {
            Stack::Byte(doc) => {
                let entry = &doc.layers[layer];
                assert_eq!(entry.pixels.dimensions(), canvas);
                assert_eq!(entry.offset, (0, 0));
                entry
                    .pixels
                    .contiguous_image()
                    .pixels()
                    .map(|p| p.0.map(|v| f64::from(v) / 255.0))
                    .collect()
            }
            Stack::Typed(doc) => {
                let entry = doc.layer(layer).unwrap();
                let image = entry.source().expect("a raster record");
                assert_eq!(image.dimensions(), canvas);
                assert_eq!(entry.offset(), (0, 0));
                if doc.is_float() {
                    image
                        .to_rgba32()
                        .unwrap()
                        .iter()
                        .map(|p| p.map(f64::from))
                        .collect()
                } else {
                    image
                        .to_rgba16()
                        .unwrap()
                        .iter()
                        .map(|p| p.map(|v| f64::from(v) / 65535.0))
                        .collect()
                }
            }
        }
    }

    pub(crate) fn name(&self, layer: usize) -> String {
        match self {
            Stack::Byte(doc) => doc.layers[layer].name.clone(),
            Stack::Typed(doc) => doc.layer(layer).unwrap().name().to_string(),
        }
    }

    pub(crate) fn clipped(&self, layer: usize) -> bool {
        match self {
            Stack::Byte(doc) => unsafe { dr_doc_layer_clipped(doc, layer) },
            Stack::Typed(doc) => {
                let mut err = ptr::null_mut();
                let info = unsafe { dr_precision_doc_info(doc, &mut err) };
                let info: Value = serde_json::from_str(&take(info)).unwrap();
                info["layers"][layer]["clipped"]
                    .as_bool()
                    .expect("a clipped flag")
            }
        }
    }

    /// Whether layer `layer` here holds the very raster layer `other` holds
    /// in `before`: the edit left its pixels alone.
    pub(crate) fn same_raster(&self, layer: usize, before: &Stack, other: usize) -> bool {
        match (self, before) {
            (Stack::Byte(a), Stack::Byte(b)) => {
                LayerRaster::ptr_eq(&a.layers[layer].pixels, &b.layers[other].pixels)
            }
            (Stack::Typed(a), Stack::Typed(b)) => PrecisionImage::ptr_eq(
                a.layer(layer).unwrap().source().unwrap(),
                b.layer(other).unwrap().source().unwrap(),
            ),
            _ => false,
        }
    }
}

pub(crate) fn request(layers: &[usize], seamless: bool) -> Value {
    json!({"layers": layers, "seamless": seamless})
}

/// Every pixel is revealed by exactly one mask when `covered`, by none
/// otherwise.
pub(crate) fn assert_partition(masks: &[Vec<bool>], covered: impl Fn(usize) -> bool) {
    for i in 0..masks[0].len() {
        let revealed = masks.iter().filter(|mask| mask[i]).count();
        assert_eq!(revealed, usize::from(covered(i)), "pixel {i}");
    }
}

/// The edit and the plan both refuse `request` with a sentence containing
/// `expected`, and the edit returns nothing.
pub(crate) fn assert_refused(stack: &Stack, request: &Value, expected: &str) {
    let count = stack.layer_count();
    let masked: Vec<bool> = (0..count).map(|layer| stack.has_mask(layer)).collect();
    let edit = stack.run(request).err().expect("the edit refuses");
    assert!(
        edit.contains(expected),
        "{edit:?} should contain {expected:?}"
    );
    let plan = stack.plan(request).expect_err("the plan refuses");
    assert_eq!(plan, edit, "the plan answers the edit's refusal");
    assert_eq!(stack.layer_count(), count);
    for layer in 0..count {
        assert_eq!(stack.has_mask(layer), masked[layer]);
    }
}
