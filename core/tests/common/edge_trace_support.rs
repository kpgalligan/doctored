//! The Magnetic Lasso tracer's test helpers (`dr_selection_trace_edge`):
//! an owned image handle, the raw call with every output observed (with no
//! edge map, or through one), the traced vertices and the polyline sampled
//! every 0.25 px. Shared by `edge_trace.rs` and `edge_trace_cost.rs`.

use doctored_core::{ffi::*, ffi_selection_tools::dr_selection_trace_edge, DrEdgeMap, DrImage};
use std::ffi::{c_char, CStr, CString};
use std::ptr;

pub(crate) struct Image(pub(crate) *mut DrImage);

impl Drop for Image {
    fn drop(&mut self) {
        unsafe { dr_image_free(self.0) };
    }
}

pub(crate) fn image(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Image {
    let mut bytes = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            bytes.extend_from_slice(&f(x, y));
        }
    }
    let img = unsafe { dr_image_from_rgba(bytes.as_ptr(), w, h) };
    assert!(!img.is_null());
    Image(img)
}

pub(crate) struct Traced {
    pub(crate) points: Vec<[f32; 2]>,
    pub(crate) fasten: Option<usize>,
    pub(crate) corridor: u64,
}

pub(crate) fn raw_call(
    img: &Image,
    json: &str,
    capacity: usize,
) -> (bool, usize, i32, u64, Vec<f32>, String) {
    raw_call_through(img, ptr::null_mut(), json, capacity)
}

/// [`raw_call`] through the edge map `map` (NULL for none).
pub(crate) fn raw_call_through(
    img: &Image,
    map: *mut DrEdgeMap,
    json: &str,
    capacity: usize,
) -> (bool, usize, i32, u64, Vec<f32>, String) {
    let params = CString::new(json).unwrap();
    let mut points = vec![0f32; capacity * 2];
    let (mut count, mut fasten, mut corridor) = (usize::MAX, i32::MIN, u64::MAX);
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_selection_trace_edge(
            img.0,
            map,
            params.as_ptr(),
            if capacity == 0 {
                ptr::null_mut()
            } else {
                points.as_mut_ptr()
            },
            capacity,
            &mut count,
            &mut fasten,
            &mut corridor,
            &mut err,
        )
    };
    let message = if err.is_null() {
        String::new()
    } else {
        let text = unsafe { CStr::from_ptr(err) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(err) };
        text
    };
    (ok, count, fasten, corridor, points, message)
}

pub(crate) fn trace(img: &Image, json: &str) -> Result<Traced, String> {
    trace_through(img, ptr::null_mut(), json)
}

/// [`trace`] through the edge map `map` (NULL for none).
pub(crate) fn trace_through(
    img: &Image,
    map: *mut DrEdgeMap,
    json: &str,
) -> Result<Traced, String> {
    let (ok, count, fasten, corridor, points, message) = raw_call_through(img, map, json, 65_536);
    if !ok {
        return Err(message);
    }
    assert!(message.is_empty());
    Ok(Traced {
        points: points[..count * 2]
            .chunks_exact(2)
            .map(|p| [p[0], p[1]])
            .collect(),
        fasten: (fasten >= 0).then_some(fasten as usize),
        corridor,
    })
}

pub(crate) fn refusal(img: &Image, json: &str) -> String {
    let (ok, count, fasten, corridor, _, message) = raw_call(img, json, 16);
    assert!(!ok, "{json} was accepted");
    assert_eq!(
        (count, fasten, corridor),
        (usize::MAX, i32::MIN, u64::MAX),
        "a refusal wrote outputs"
    );
    message
}

pub(crate) fn segment_distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((p[0] - a[0] - t * dx).powi(2) + (p[1] - a[1] - t * dy).powi(2)).sqrt()
}

/// Every point every 0.25 px along the traced polyline.
pub(crate) fn samples(points: &[[f32; 2]]) -> Vec<[f64; 2]> {
    let mut out = vec![[f64::from(points[0][0]), f64::from(points[0][1])]];
    for pair in points.windows(2) {
        let a = [f64::from(pair[0][0]), f64::from(pair[0][1])];
        let b = [f64::from(pair[1][0]), f64::from(pair[1][1])];
        let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
        let n = (len / 0.25).ceil().max(1.0) as usize;
        for k in 1..=n {
            let t = k as f64 / n as f64;
            out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }
    out
}

pub(crate) fn as_f64(p: [f32; 2]) -> [f64; 2] {
    [f64::from(p[0]), f64::from(p[1])]
}
