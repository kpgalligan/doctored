#![allow(dead_code)]
use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::dr_doc_open;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::retained_source::{SourceEdit, SourceInput};
use image::RgbaImage;
use serde_json::{json, Value};
use std::{
    ffi::{c_char, CStr, CString},
    path::Path,
    ptr,
    sync::Arc,
};
pub const LIMIT: u64 = REFERENCE_WORKING_BYTES;
/// A layer's (offset, dimensions, u16 samples): `Doc::layer_pixels`.
pub type LayerPixels = ((i32, i32), (u32, u32), Vec<[u16; 4]>);
pub unsafe fn text(p: *mut c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    let s = unsafe { CStr::from_ptr(p) }.to_str().unwrap().to_string();
    unsafe { dr_string_free(p) };
    s
}
pub unsafe fn json_result(p: *mut c_char, e: *mut c_char) -> Result<Value, String> {
    let error = unsafe { text(e) };
    if p.is_null() {
        return Err(error);
    }
    assert!(error.is_empty(), "{error}");
    Ok(serde_json::from_str(&unsafe { text(p) }).unwrap())
}
pub fn source(w: u32, h: u32, pixels: &[[u16; 4]], profile: Arc<IccProfile>) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, pixels.to_vec(), profile, None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}
#[derive(Clone)]
pub enum Doc {
    Byte(DrDocument),
    Typed(PrecisionDocument),
}
impl Doc {
    pub fn retained(
        typed: bool,
        actual16: bool,
        w: u32,
        h: u32,
        pixels: &[[u16; 4]],
        source_profile: Arc<IccProfile>,
        working: Arc<IccProfile>,
    ) -> Self {
        let master = source(w, h, pixels, source_profile.clone());
        let mut byte = DrDocument::from_pixels(
            RgbaImage::from_raw(
                w,
                h,
                pixels
                    .iter()
                    .flat_map(|v| v.map(|v| ((u32::from(v) + 128) / 257) as u8))
                    .collect(),
            )
            .unwrap(),
        );
        byte.profile = source_profile;
        let canvas = if typed {
            Self::Typed(source(w, h, &vec![[0; 4]; pixels.len()], working))
        } else {
            let mut canvas = byte.clone();
            canvas.profile = working;
            Self::Byte(canvas)
        };
        let edit=SourceEdit::parse(&json!({"op":"capture_raster","layer":0,"source_id":"original","revision_id":"a","source_transform":[1,0,0,1,0,0],"depth_conversion":"convert_fallback"}).to_string()).unwrap();
        let input = if actual16 {
            SourceInput::Typed(&master)
        } else {
            SourceInput::Byte(&byte)
        };
        match canvas {
            Self::Byte(d) => Self::Byte(
                d.source_edit_input(Some(input), &edit, LIMIT, 0, None)
                    .unwrap()
                    .unwrap(),
            ),
            Self::Typed(d) => Self::Typed(
                d.source_edit_input(Some(input), &edit, LIMIT, 0, None)
                    .unwrap()
                    .unwrap(),
            ),
        }
    }
    pub fn typed(&self) -> bool {
        matches!(self, Self::Typed(_))
    }
    pub fn pixels(&self) -> Vec<[u16; 4]> {
        match self {
            Self::Byte(d) => d.layers[0]
                .pixels
                .contiguous_image()
                .pixels()
                .map(|p| p.0.map(|v| u16::from(v) * 257))
                .collect(),
            Self::Typed(d) => d.layer_source(0).unwrap().to_rgba16().unwrap(),
        }
    }
    pub fn dimensions(&self) -> (u32, u32) {
        match self {
            Self::Byte(d) => d.layers[0].pixels.dimensions(),
            Self::Typed(d) => d.layer_source(0).unwrap().dimensions(),
        }
    }
    pub fn source_edit(
        &self,
        request: Value,
        input: Option<&PrecisionDocument>,
    ) -> Result<Option<Self>, String> {
        let edit = SourceEdit::parse(&request.to_string())?;
        match self {
            Self::Byte(d) => d
                .source_edit_input(input.map(SourceInput::Typed), &edit, LIMIT, 0, None)
                .map(|v| v.map(Self::Byte)),
            Self::Typed(d) => d
                .source_edit_input(input.map(SourceInput::Typed), &edit, LIMIT, 0, None)
                .map(|v| v.map(Self::Typed)),
        }
    }
    /// An opaque white `w`×`h` document at the document depth (typed:
    /// `precision_blank::create_blank`'s uniform plane).
    pub fn white(typed: bool, w: u32, h: u32, working: Arc<IccProfile>) -> Self {
        if typed {
            Self::Typed(
                PrecisionDocument::create_blank(
                    w,
                    h,
                    [65535; 4],
                    working.bytes(),
                    "Background",
                    LIMIT,
                    0,
                )
                .unwrap(),
            )
        } else {
            let mut d = DrDocument::from_pixels(RgbaImage::from_pixel(w, h, image::Rgba([255; 4])));
            d.profile = working;
            Self::Byte(d)
        }
    }
    /// A `w`×`h` master (`retained`'s construction: a typed master when
    /// `actual16`, else its byte codes) placed (`place`, x 0 y 0,
    /// convert_fallback) as a new top layer.
    pub fn place(
        &self,
        actual16: bool,
        w: u32,
        h: u32,
        pixels: &[[u16; 4]],
        source_profile: Arc<IccProfile>,
    ) -> Self {
        let id = format!("placed-{}", self.layer_count());
        let edit = SourceEdit::parse(
            &json!({"op":"place","source_id":id,"revision_id":"a","name":id,"x":0,"y":0,
                "depth_conversion":"convert_fallback"})
            .to_string(),
        )
        .unwrap();
        let typed_master;
        let byte_master;
        let input = if actual16 {
            typed_master = source(w, h, pixels, source_profile);
            SourceInput::Typed(&typed_master)
        } else {
            let mut byte = DrDocument::from_pixels(
                RgbaImage::from_raw(
                    w,
                    h,
                    pixels
                        .iter()
                        .flat_map(|v| v.map(|v| ((u32::from(v) + 128) / 257) as u8))
                        .collect(),
                )
                .unwrap(),
            );
            byte.profile = source_profile;
            byte_master = byte;
            SourceInput::Byte(&byte_master)
        };
        match self {
            Self::Byte(d) => Self::Byte(
                d.source_edit_input(Some(input), &edit, LIMIT, 0, None)
                    .unwrap()
                    .unwrap(),
            ),
            Self::Typed(d) => Self::Typed(
                d.source_edit_input(Some(input), &edit, LIMIT, 0, None)
                    .unwrap()
                    .unwrap(),
            ),
        }
    }
    /// `retained`'s master placed (`place`, x 0 y 0, convert_fallback) as
    /// layer 1 over a white opaque `w`×`h` layer 0 at the document depth.
    pub fn placed_over_white(
        typed: bool,
        actual16: bool,
        w: u32,
        h: u32,
        pixels: &[[u16; 4]],
        source_profile: Arc<IccProfile>,
        working: Arc<IccProfile>,
    ) -> Self {
        Self::white(typed, w, h, working).place(actual16, w, h, pixels, source_profile)
    }
    pub fn layer_count(&self) -> usize {
        match self {
            Self::Byte(d) => d.layers.len(),
            Self::Typed(d) => d.layer_count(),
        }
    }
    /// Layer `layer`'s offset, dimensions and samples as u16 (byte ×257).
    pub fn layer_pixels(&self, layer: usize) -> LayerPixels {
        match self {
            Self::Byte(d) => {
                let entry = &d.layers[layer];
                let image = entry.pixels.contiguous_image();
                (
                    entry.offset,
                    image.dimensions(),
                    image
                        .pixels()
                        .map(|p| p.0.map(|v| u16::from(v) * 257))
                        .collect(),
                )
            }
            Self::Typed(d) => {
                let image = d.layer_source(layer).unwrap();
                (
                    d.layer(layer).unwrap().offset(),
                    image.dimensions(),
                    image.to_rgba16().unwrap(),
                )
            }
        }
    }
    pub fn open(bytes: &[u8], typed: bool, path: &Path) -> Result<Self, String> {
        if typed {
            return PrecisionDocument::parse_native(bytes, LIMIT, 0, None).map(Self::Typed);
        }
        std::fs::write(path, bytes).unwrap();
        let path = CString::new(path.to_str().unwrap()).unwrap();
        let mut e = ptr::null_mut();
        let p = unsafe { dr_doc_open(path.as_ptr(), &mut e) };
        let error = unsafe { text(e) };
        if p.is_null() {
            Err(error)
        } else {
            assert!(error.is_empty());
            Ok(Self::Byte(unsafe { *Box::from_raw(p) }))
        }
    }
}
/// The footprint master: an opaque sinusoid plasma (`seed` shifts
/// its phases, so seed 1 is a different picture of the same statistics).
pub fn plasma(w: u32, h: u32, seed: u32) -> Vec<[u16; 4]> {
    let phase = f64::from(seed) * 1.7;
    (0..w * h)
        .map(|i| {
            let (x, y) = (f64::from(i % w), f64::from(i / w));
            let v = |a: f64, b: f64| {
                (((x * a + phase).sin() + (y * b - phase).cos() + ((x + y) * 0.0007).sin())
                    * 10922.0
                    + 32767.0) as u16
            };
            [v(0.003, 0.002), v(0.0021, 0.0037), v(0.0013, 0.0009), 65535]
        })
        .collect()
}
