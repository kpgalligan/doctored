//! 13B-9c: the app rasterizes a Clone, Heal or Spot Heal stroke's footprint
//! over the stroke's rect only (`NativeBrushStroke.footprint(in:)`), with
//! every dab moved by the rect's origin and that origin handed to the
//! textured and dual exports as the texture's anchor. That is only the
//! canvas footprint when the rasterizers are translation-exact, so each is
//! held to it here through the C ABI: for `dr_brush_u16`,
//! `dr_brush_u16_shaped`, `dr_brush_u16_textured` and `dr_brush_u16_dual`,
//! over soft, hard, aliased and pixel tips (each where the export takes
//! it), the rect-sized output equals the canvas output cropped to the rect,
//! sample for sample.

use crate::native_scalar::{error, text, LIMIT};
use doctored_core::ffi_brush::*;
use doctored_core::ffi_brush_dual::*;
use doctored_core::ffi_brush_texture::*;
use serde_json::{json, Value};
use std::ptr;

const W: u32 = 157;
const H: u32 = 121;
/// The stroke's rect: the dabs' extent grown by 2 px, as `healBounds` makes it.
const RECT: (u32, u32, u32, u32) = (31, 27, 97, 71);

/// A stroke of fractional positions, diameters and deposits inside `RECT`.
fn dabs() -> Vec<f64> {
    let mut out = Vec::new();
    for k in 0..9 {
        let t = f64::from(k);
        out.extend([
            48.37 + t * 7.913,
            52.61 + (t * 0.7).sin() * 11.29,
            19.0 + t * 1.37,
            0.35 + 0.07 * t,
        ]);
    }
    out
}

/// One (angle, roundness) pair per dab.
fn shapes(count: usize) -> Vec<f64> {
    (0..count)
        .flat_map(|k| [-150.0 + 37.3 * k as f64, 0.25 + 0.08 * k as f64])
        .collect()
}

/// `dabs` moved by the rect's origin.
fn moved(dabs: &[f64]) -> Vec<f64> {
    dabs.chunks(4)
        .flat_map(|d| {
            [
                d[0] - f64::from(RECT.0),
                d[1] - f64::from(RECT.1),
                d[2],
                d[3],
            ]
        })
        .collect()
}

fn crop(plane: &[u16]) -> Vec<u16> {
    let (x, y, w, h) = RECT;
    let mut out = Vec::new();
    for row in y..y + h {
        let at = (row * W + x) as usize;
        out.extend_from_slice(&plane[at..at + w as usize]);
    }
    out
}

struct Texture(*mut DrBrushTexture);

impl Drop for Texture {
    fn drop(&mut self) {
        unsafe { dr_brush_texture_free(self.0) }
    }
}

fn texture() -> Texture {
    let (w, h) = (5u32, 7u32);
    let rgba: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            let v = ((i * 71) % 256) as u8;
            [
                v,
                255 - v,
                (i * 13) as u8,
                if i % 4 == 1 { 120 } else { 255 },
            ]
        })
        .collect();
    let mut e = ptr::null_mut();
    let t = unsafe { dr_brush_texture_create(rgba.as_ptr(), w, h, 1.7, 0.8, &mut e) };
    assert!(!t.is_null(), "{}", unsafe { error(e) });
    Texture(t)
}

/// Which export a case runs.
enum Kind<'a> {
    Plain,
    Shaped(&'a [f64]),
    Textured(Option<&'a [f64]>, &'a Texture),
    Dual(Option<&'a [f64]>, Option<&'a Texture>, &'a Value, &'a [f64]),
}

/// The export's output over a `width × height` plane whose pixel (0, 0) is
/// canvas `origin`.
fn run(
    (width, height): (u32, u32),
    origin: (i64, i64),
    request: &Value,
    dabs: &[f64],
    kind: &Kind<'_>,
) -> Result<Vec<u16>, String> {
    let t = text(request.clone());
    let mut out = vec![0u16; (width * height) as usize];
    let mut e = ptr::null_mut();
    let n = dabs.len() / 4;
    let ok = unsafe {
        match kind {
            Kind::Plain => dr_brush_u16(
                width,
                height,
                t.as_ptr(),
                dabs.as_ptr(),
                n,
                out.as_mut_ptr(),
                out.len(),
                LIMIT,
                0,
                ptr::null(),
                &mut e,
            ),
            Kind::Shaped(shapes) => dr_brush_u16_shaped(
                width,
                height,
                t.as_ptr(),
                dabs.as_ptr(),
                n,
                shapes.as_ptr(),
                out.as_mut_ptr(),
                out.len(),
                LIMIT,
                0,
                ptr::null(),
                &mut e,
            ),
            Kind::Textured(shapes, texture) => dr_brush_u16_textured(
                width,
                height,
                t.as_ptr(),
                dabs.as_ptr(),
                n,
                shapes.map_or(ptr::null(), |s| s.as_ptr()),
                texture.0,
                origin.0,
                origin.1,
                out.as_mut_ptr(),
                out.len(),
                LIMIT,
                0,
                ptr::null(),
                &mut e,
            ),
            Kind::Dual(shapes, texture, second, second_dabs) => {
                let second_text = text((*second).clone());
                let second_dabs: Vec<f64> = if origin == (0, 0) {
                    second_dabs.to_vec()
                } else {
                    moved(second_dabs)
                };
                dr_brush_u16_dual(
                    width,
                    height,
                    t.as_ptr(),
                    dabs.as_ptr(),
                    n,
                    shapes.map_or(ptr::null(), |s| s.as_ptr()),
                    texture.map_or(ptr::null(), |t| t.0 as *const _),
                    origin.0,
                    origin.1,
                    second_text.as_ptr(),
                    second_dabs.as_ptr(),
                    second_dabs.len() / 4,
                    out.as_mut_ptr(),
                    out.len(),
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut e,
                )
            }
        }
    };
    if ok {
        Ok(out)
    } else {
        Err(unsafe { error(e) })
    }
}

/// The rect form against the canvas form cropped, for one case.
fn check(failures: &mut Vec<String>, label: &str, request: &Value, kind: Kind<'_>) {
    let all = dabs();
    let canvas = run((W, H), (0, 0), request, &all, &kind);
    let origin = (i64::from(RECT.0), i64::from(RECT.1));
    let rect = run((RECT.2, RECT.3), origin, request, &moved(&all), &kind);
    match (canvas, rect) {
        (Ok(canvas), Ok(rect)) => {
            assert!(
                canvas.iter().any(|v| *v > 0),
                "{label}: the stroke covers nothing"
            );
            let cropped = crop(&canvas);
            let outside = canvas.iter().map(|v| u64::from(*v)).sum::<u64>()
                - cropped.iter().map(|v| u64::from(*v)).sum::<u64>();
            if outside > 0 {
                failures.push(format!("{label}: the stroke leaves the rect"));
            }
            if let Some(i) = (0..rect.len()).find(|&i| rect[i] != cropped[i]) {
                failures.push(format!(
                    "{label}: rect sample {i} is {} where the canvas holds {}",
                    rect[i], cropped[i]
                ));
            }
        }
        (canvas, rect) => failures.push(format!(
            "{label}: canvas {:?}, rect {:?}",
            canvas.map(|_| ()),
            rect.map(|_| ())
        )),
    }
}

fn tip(hardness: f64, pixel: bool, aliased: bool) -> Value {
    let mut request = json!({"version":1,"hardness":hardness,"roundness":0.7,
        "angle_degrees":23.0,"pixel":pixel});
    if aliased {
        request["aliased"] = json!(true);
    }
    request
}

#[test]
fn a_rect_footprint_is_the_canvas_footprint_cropped() {
    let all = dabs();
    let pairs = shapes(all.len() / 4);
    let grain = texture();
    let second = json!({"version":1,"hardness":0.4,"roundness":1.0,"angle_degrees":0.0,
        "pixel":false});
    // The second tip's own dabs along the same path, a smaller size.
    let second_dabs: Vec<f64> = all
        .chunks(4)
        .flat_map(|d| [d[0] + 1.25, d[1] - 0.75, 11.0, 1.0])
        .collect();
    let mut failures = Vec::new();
    let f = &mut failures;
    for (name, hardness, pixel, aliased) in [
        ("soft", 0.3, false, false),
        ("hard", 1.0, false, false),
        ("aliased", 1.0, false, true),
        ("pixel", 1.0, true, false),
    ] {
        let request = tip(hardness, pixel, aliased);
        check(f, &format!("plain {name}"), &request, Kind::Plain);
        // Shapes, textures and the dual take stamped tips that are not
        // aliased only.
        if pixel || aliased {
            continue;
        }
        check(f, &format!("shaped {name}"), &request, Kind::Shaped(&pairs));
        check(
            f,
            &format!("textured {name}"),
            &request,
            Kind::Textured(None, &grain),
        );
        check(
            f,
            &format!("textured shaped {name}"),
            &request,
            Kind::Textured(Some(&pairs), &grain),
        );
        check(
            f,
            &format!("dual {name}"),
            &request,
            Kind::Dual(None, None, &second, &second_dabs),
        );
        check(
            f,
            &format!("dual shaped textured {name}"),
            &request,
            Kind::Dual(Some(&pairs), Some(&grain), &second, &second_dabs),
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
