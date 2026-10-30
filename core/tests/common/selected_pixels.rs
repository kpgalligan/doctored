//! Fixtures shared by the selected-pixels suites
//! (`move_selected_pixels.rs`, `transform_selected_pixels.rs`,
//! `selected_pixels_float.rs`): one depth-coded raster plane, the byte and
//! typed documents built from it, the FFI seams (edit, plan and the float
//! bounds) and the W3C source-over oracle pixel.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::{DrDocument, LayerRaster};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::ffi_selected_pixels::{
    dr_doc_selected_pixels_bounds, dr_doc_selected_pixels_edit, dr_doc_selected_pixels_edit_plan,
    dr_precision_doc_selected_pixels_bounds,
};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::RgbaImage;
use serde_json::{json, Value};

pub const LIMIT: u64 = REFERENCE_WORKING_BYTES;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Depth {
    D8,
    D16,
}
pub const DEPTHS: [Depth; 2] = [Depth::D8, Depth::D16];

impl Depth {
    pub fn max(self) -> f64 {
        match self {
            Depth::D8 => 255.0,
            Depth::D16 => 65535.0,
        }
    }
    /// A byte fixture value at this depth. At 16 bits RGB gains a jitter so
    /// the codes are not all multiples of 257.
    pub fn code(self, v: u8, jitter: u16) -> u16 {
        match self {
            Depth::D8 => u16::from(v),
            Depth::D16 if v == 255 || v == 0 => u16::from(v) * 257,
            Depth::D16 => u16::from(v) * 257 + jitter % 200,
        }
    }
}

/// One raster layer at this depth: canvas offset, size and depth-coded
/// straight RGBA.
#[derive(Clone, Debug, PartialEq)]
pub struct Plane {
    pub offset: (i32, i32),
    pub size: (u32, u32),
    pub px: Vec<[u16; 4]>,
}

impl Plane {
    pub fn of(
        depth: Depth,
        offset: (i32, i32),
        size: (u32, u32),
        f: impl Fn(u32, u32) -> [u8; 4],
    ) -> Self {
        let mut px = Vec::new();
        for y in 0..size.1 {
            for x in 0..size.0 {
                let v = f(x, y);
                let j = (x * 37 + y * 11) as u16;
                px.push([
                    depth.code(v[0], j),
                    depth.code(v[1], j + 5),
                    depth.code(v[2], j + 9),
                    depth.code(v[3], 0),
                ]);
            }
        }
        Plane { offset, size, px }
    }
    pub fn at(&self, cx: i64, cy: i64) -> Option<[u16; 4]> {
        let (x, y) = (cx - i64::from(self.offset.0), cy - i64::from(self.offset.1));
        if x < 0 || y < 0 || x >= i64::from(self.size.0) || y >= i64::from(self.size.1) {
            return None;
        }
        Some(self.px[y as usize * self.size.0 as usize + x as usize])
    }
}

pub enum Doc {
    Byte(DrDocument),
    Typed(PrecisionDocument),
}

pub fn byte_doc(canvas: (u32, u32), plane: &Plane) -> DrDocument {
    let mut doc = DrDocument::from_pixels(RgbaImage::new(canvas.0, canvas.1));
    let raw = plane.px.iter().flat_map(|p| p.map(|v| v as u8)).collect();
    let image = RgbaImage::from_raw(plane.size.0, plane.size.1, raw).unwrap();
    doc.layers[0].pixels = LayerRaster::new(image);
    doc.layers[0].offset = plane.offset;
    doc
}

pub fn typed_layer(plane: &Plane) -> PrecisionDocumentLayer {
    let image = PrecisionImage::from_rgba16(
        plane.size.0,
        plane.size.1,
        plane.px.clone(),
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    PrecisionDocumentLayer::raster("L", image, plane.offset, 1.0, true).unwrap()
}

pub fn typed_doc(canvas: (u32, u32), layers: Vec<PrecisionDocumentLayer>) -> PrecisionDocument {
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

pub fn doc(depth: Depth, canvas: (u32, u32), plane: &Plane) -> Doc {
    match depth {
        Depth::D8 => Doc::Byte(byte_doc(canvas, plane)),
        Depth::D16 => Doc::Typed(typed_doc(canvas, vec![typed_layer(plane)])),
    }
}

pub fn take_err(err: *mut c_char) -> Option<String> {
    if err.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(err) };
    Some(text)
}

pub fn edit8(
    doc: &DrDocument,
    text: &str,
    coverage: &[u16],
    control: Option<&DrJobControl>,
) -> Result<Option<DrDocument>, String> {
    let text = CString::new(text).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_selected_pixels_edit(
            doc,
            text.as_ptr(),
            coverage.as_ptr(),
            coverage.len(),
            LIMIT,
            0,
            control.map_or(ptr::null(), |c| c as *const _),
            &mut err,
        )
    };
    if let Some(message) = take_err(err) {
        assert!(out.is_null());
        return Err(message);
    }
    Ok((!out.is_null()).then(|| *unsafe { Box::from_raw(out) }))
}

pub fn edit16(
    doc: &PrecisionDocument,
    text: &str,
    coverage: &[u16],
    control: Option<&DrJobControl>,
) -> Result<Option<PrecisionDocument>, String> {
    let text = CString::new(text).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            coverage.as_ptr(),
            coverage.len(),
            LIMIT,
            0,
            control.map_or(ptr::null(), |c| c as *const _),
            &mut err,
        )
    };
    if let Some(message) = take_err(err) {
        assert!(out.is_null());
        return Err(message);
    }
    Ok((!out.is_null()).then(|| *unsafe { Box::from_raw(out) }))
}

impl Doc {
    pub fn edit(&self, text: &str, coverage: &[u16]) -> Result<Option<Doc>, String> {
        match self {
            Doc::Byte(d) => edit8(d, text, coverage, None).map(|o| o.map(Doc::Byte)),
            Doc::Typed(d) => edit16(d, text, coverage, None).map(|o| o.map(Doc::Typed)),
        }
    }
    pub fn plane(&self, index: usize) -> Plane {
        match self {
            Doc::Byte(d) => {
                let layer = &d.layers[index];
                let (w, h) = layer.pixels.dimensions();
                let px = layer.pixels.read_rect(0, 0, w, h).unwrap();
                Plane {
                    offset: layer.offset,
                    size: (w, h),
                    px: px.iter().map(|p| p.map(u16::from)).collect(),
                }
            }
            Doc::Typed(d) => {
                let layer = d.layer(index).unwrap();
                let image = layer.source().unwrap();
                Plane {
                    offset: layer.offset(),
                    size: image.dimensions(),
                    px: image.to_rgba16().unwrap(),
                }
            }
        }
    }
}

/// W3C Normal source-over of the float onto the holed pixel.
pub fn oracle_pixel(orig: [f64; 4], c: f64, float: Option<([f64; 4], f64)>) -> Option<[f64; 4]> {
    let float = float.filter(|(p, fc)| p[3] * fc > 0.0);
    if c == 0.0 && float.is_none() {
        return None;
    }
    let ab = orig[3] * (1.0 - c);
    let mut out = [orig[0], orig[1], orig[2], ab];
    if let Some((s, fc)) = float {
        let sa = s[3] * fc;
        let ao = sa + ab * (1.0 - sa);
        for k in 0..3 {
            out[k] = (s[k] * sa + orig[k] * ab * (1.0 - sa)) / ao;
        }
        out[3] = ao;
    }
    Some(out)
}

pub fn coverage(canvas: (u32, u32), f: impl Fn(u32, u32) -> u16) -> Vec<u16> {
    let mut out = Vec::new();
    for y in 0..canvas.1 {
        for x in 0..canvas.0 {
            out.push(f(x, y));
        }
    }
    out
}

pub fn rect(x0: u32, y0: u32, w: u32, h: u32) -> impl Fn(u32, u32) -> u16 {
    move |x, y| {
        if x >= x0 && x < x0 + w && y >= y0 && y < y0 + h {
            65535
        } else {
            0
        }
    }
}

pub fn pattern(x: u32, y: u32) -> [u8; 4] {
    [
        (x * 29 + 13) as u8,
        (y * 41 + 7) as u8,
        ((x + y) * 17 + 101) as u8,
        255,
    ]
}

pub fn adjustment_meta() -> String {
    json!({"type":"adjust","evaluation":"precision_v1","op":"invert","params":{}}).to_string()
}

pub fn plan8(doc: &DrDocument, text: &str) -> Result<Value, String> {
    let text = CString::new(text).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_doc_selected_pixels_edit_plan(doc, text.as_ptr(), &mut err) };
    if let Some(message) = take_err(err) {
        return Err(message);
    }
    let value = serde_json::from_str(&unsafe { CStr::from_ptr(out) }.to_string_lossy()).unwrap();
    unsafe { dr_string_free(out) };
    Ok(value)
}

pub fn plan16(doc: &PrecisionDocument, text: &str) -> Result<Value, String> {
    let text = CString::new(text).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut err) };
    if let Some(message) = take_err(err) {
        return Err(message);
    }
    let value = serde_json::from_str(&unsafe { CStr::from_ptr(out) }.to_string_lossy()).unwrap();
    unsafe { dr_string_free(out) };
    Ok(value)
}

/// `dr_doc_selected_pixels_bounds`: `Ok(Some([x, y, w, h]))`, `Ok(None)` for
/// an empty float, `Err` for a refusal.
pub fn bounds8(
    doc: &DrDocument,
    layer: usize,
    coverage: &[u16],
    control: Option<&DrJobControl>,
) -> Result<Option<[u32; 4]>, String> {
    let mut out = [0u32; 4];
    let mut err = ptr::null_mut();
    let found = unsafe {
        dr_doc_selected_pixels_bounds(
            doc,
            layer,
            coverage.as_ptr(),
            coverage.len(),
            control.map_or(ptr::null(), |c| c as *const _),
            out.as_mut_ptr(),
            &mut err,
        )
    };
    if let Some(message) = take_err(err) {
        assert!(!found);
        return Err(message);
    }
    Ok(found.then_some(out))
}

/// `dr_precision_doc_selected_pixels_bounds`, as [`bounds8`].
pub fn bounds16(
    doc: &PrecisionDocument,
    layer: usize,
    coverage: &[u16],
    control: Option<&DrJobControl>,
) -> Result<Option<[u32; 4]>, String> {
    let mut out = [0u32; 4];
    let mut err = ptr::null_mut();
    let found = unsafe {
        dr_precision_doc_selected_pixels_bounds(
            doc,
            layer,
            coverage.as_ptr(),
            coverage.len(),
            control.map_or(ptr::null(), |c| c as *const _),
            out.as_mut_ptr(),
            &mut err,
        )
    };
    if let Some(message) = take_err(err) {
        assert!(!found);
        return Err(message);
    }
    Ok(found.then_some(out))
}

impl Doc {
    /// The float-bounds query at this document's depth.
    pub fn float_bounds(&self, layer: usize, coverage: &[u16]) -> Result<Option<[u32; 4]>, String> {
        match self {
            Doc::Byte(d) => bounds8(d, layer, coverage, None),
            Doc::Typed(d) => bounds16(d, layer, coverage, None),
        }
    }
    /// The selected-pixels plan at this document's depth.
    pub fn plan(&self, text: &str) -> Result<Value, String> {
        match self {
            Doc::Byte(d) => plan8(d, text),
            Doc::Typed(d) => plan16(d, text),
        }
    }
}
