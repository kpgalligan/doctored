//! The windowed per-component artwork contract (`artwork_window`): a
//! `coverage` or `intrinsic_rgba8` component may declare `"window": [x, y, w,
//! h]`, its plane then covering only those raster pixels, and the bound is
//! 64,000,000 samples per component with no whole-layer total. Through
//! `dr_artwork_render`, `dr_artwork_render_float` and
//! `dr_precision_doc_cmyk_artwork`.
//!
//! The oracles: a windowed list renders exactly as the same list given as
//! whole-raster planes holding each window's samples embedded in zeros (the
//! contract's own definition, built here by hand), at 16 bits, at 32 bits and
//! in CMYK; one pixel inside a window is the colour at the coverage's alpha
//! (sRGB into sRGB, within one code) and one outside it is transparent. The
//! lifted total: two whole-raster intrinsic planes whose bytes sum past 64M,
//! each under it, render. A caller's count is never trusted: one the windows
//! do not name refuses by the layout sentence before admission.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_artwork::{dr_artwork_render, dr_artwork_render_float};
use doctored_core::ffi_cmyk_artwork::dr_precision_doc_cmyk_artwork;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

use crate::cmyk_structure_fixture::{cmyk_layers, free, varied, RgbLayer, LIMIT};
use crate::common::take_err_string;

const WINDOW_REFUSAL: &str = "artwork component window must have a positive size inside the raster";
const LAYOUT_REFUSAL: &str = "artwork samples do not match their bounded component layout";

#[derive(Clone, Copy)]
enum Depth {
    Sixteen,
    Float,
}

fn buffer<T>(values: &[T]) -> *const T {
    if values.is_empty() {
        ptr::null()
    } else {
        values.as_ptr()
    }
}

/// `dr_artwork_render` / `_float` into the sRGB profile.
fn render(
    depth: Depth,
    size: (u32, u32),
    components: &Value,
    samples: &[u16],
    intrinsic: &[u8],
) -> Result<Box<PrecisionImage>, String> {
    render_declared(
        depth,
        size,
        components,
        (samples, samples.len()),
        (intrinsic, intrinsic.len()),
    )
}

/// [`render`] with each buffer's count as DECLARED to the FFI, which may
/// differ from the buffer's own length. A declared count above the buffer's
/// is safe only while the export refuses before it reads the buffer.
fn render_declared(
    depth: Depth,
    size: (u32, u32),
    components: &Value,
    (samples, sample_count): (&[u16], usize),
    (intrinsic, intrinsic_count): (&[u8], usize),
) -> Result<Box<PrecisionImage>, String> {
    let text = CString::new(components.to_string()).unwrap();
    let srgb = IccProfile::srgb();
    let profile = srgb.bytes();
    let mut err = ptr::null_mut();
    let out = unsafe {
        match depth {
            Depth::Sixteen => dr_artwork_render(
                size.0,
                size.1,
                text.as_ptr(),
                buffer(samples),
                sample_count,
                buffer(intrinsic),
                intrinsic_count,
                profile.as_ptr(),
                profile.len(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            ),
            Depth::Float => dr_artwork_render_float(
                size.0,
                size.1,
                text.as_ptr(),
                buffer(samples),
                sample_count,
                buffer(intrinsic),
                intrinsic_count,
                profile.as_ptr(),
                profile.len(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            ),
        }
    };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(unsafe { Box::from_raw(out) })
    }
}

/// `dr_precision_doc_cmyk_artwork`: a new layer above layer 0 of `doc`.
fn publish(
    doc: *const PrecisionDocument,
    size: (u32, u32),
    components: &Value,
    samples: &[u16],
    intrinsic: &[u8],
) -> Result<*mut PrecisionDocument, String> {
    let components = CString::new(components.to_string()).unwrap();
    let name = CString::new("Art").unwrap();
    let description = CString::new(r#"{"type":"shape","version":6}"#).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_cmyk_artwork(
            doc,
            0,
            false,
            0,
            size.0,
            size.1,
            components.as_ptr(),
            buffer(samples),
            samples.len(),
            buffer(intrinsic),
            intrinsic.len(),
            0,
            0,
            name.as_ptr(),
            description.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

/// One component of a fixture list: its JSON without a window, the window
/// its plane covers (`None`: the raster; planeless parts ignore it), and
/// whether it owns a coverage plane, an RGBA8 plane or none.
struct Part {
    component: Value,
    window: Option<[u32; 4]>,
    plane: Plane,
}

#[derive(Clone, Copy, PartialEq)]
enum Plane {
    None,
    Coverage,
    Rgba,
}

/// A list's two spellings: (components, samples, intrinsic) with windows,
/// and the same list as whole-raster planes holding each window's samples
/// embedded in zeros.
type Spelling = (Value, Vec<u16>, Vec<u8>);

fn spellings(parts: &[Part], (w, h): (u32, u32)) -> (Spelling, Spelling) {
    let (mut windowed, mut whole) = (
        (Vec::new(), Vec::new(), Vec::new()),
        (Vec::new(), Vec::new(), Vec::new()),
    );
    for (index, part) in parts.iter().enumerate() {
        let mut declared = part.component.clone();
        whole.0.push(part.component.clone());
        let [x, y, ww, wh] = part.window.unwrap_or([0, 0, w, h]);
        if part.plane != Plane::None {
            if let Some(window) = part.window {
                declared["window"] = json!(window);
            }
        }
        windowed.0.push(declared);
        let seed = index as u32 * 977 + 13;
        match part.plane {
            Plane::None => {}
            Plane::Coverage => {
                let mut plane = vec![0u16; (w * h) as usize];
                for k in 0..ww * wh {
                    let value = match (k + seed) % 7 {
                        0 => 0,
                        1 => 65535,
                        _ => ((k * 7919 + seed * 31) % 65536) as u16,
                    };
                    windowed.1.push(value);
                    plane[((y + k / ww) * w + x + k % ww) as usize] = value;
                }
                whole.1.extend(plane);
            }
            Plane::Rgba => {
                let mut plane = vec![0u8; (w * h * 4) as usize];
                for k in 0..ww * wh {
                    let pixel = [
                        ((k * 131 + seed) % 256) as u8,
                        ((k * 71 + seed * 3) % 256) as u8,
                        ((k * 29 + seed * 7) % 256) as u8,
                        [0u8, 255, 90, 200][((k + seed) % 4) as usize],
                    ];
                    windowed.2.extend(pixel);
                    let at = (((y + k / ww) * w + x + k % ww) * 4) as usize;
                    plane[at..at + 4].copy_from_slice(&pixel);
                }
                whole.2.extend(plane);
            }
        }
    }
    (
        (Value::Array(windowed.0), windowed.1, windowed.2),
        (Value::Array(whole.0), whole.1, whole.2),
    )
}

/// The fixture: a fill under windowed and whole-raster coverage and glyph
/// planes, colours in `key` (`color` RGBA, or `ink` CMYKA).
fn fixture(key: &str, color: impl Fn(usize) -> Value) -> Vec<Part> {
    let part = |component: Value, window: Option<[u32; 4]>, plane: Plane| Part {
        component,
        window,
        plane,
    };
    vec![
        part(json!({"kind": "fill", key: color(0)}), None, Plane::None),
        part(
            json!({"kind": "coverage", key: color(1)}),
            Some([3, 4, 10, 7]),
            Plane::Coverage,
        ),
        part(
            json!({"kind": "intrinsic_rgba8", "opacity": 0.7}),
            Some([20, 10, 9, 8]),
            Plane::Rgba,
        ),
        part(
            json!({"kind": "coverage", key: color(2)}),
            None,
            Plane::Coverage,
        ),
        part(
            json!({"kind": "coverage", key: color(3)}),
            Some([0, 0, 37, 1]),
            Plane::Coverage,
        ),
        part(
            json!({"kind": "intrinsic_rgba8"}),
            Some([36, 22, 1, 1]),
            Plane::Rgba,
        ),
        part(
            json!({"kind": "coverage", key: color(4)}),
            Some([30, 0, 7, 23]),
            Plane::Coverage,
        ),
    ]
}

const SIZE: (u32, u32) = (37, 23);

#[test]
fn a_windowed_list_renders_as_its_zero_embedded_whole_raster_twin() {
    let colors = [
        [0.1, 0.2, 0.3, 0.25],
        [0.9, 0.1, 0.2, 0.8],
        [0.2, 0.7, 0.4, 1.0],
        [0.3, 0.3, 0.9, 0.6],
        [0.65, 0.5, 0.05, 0.9],
    ];
    let parts = fixture("color", |k| json!(colors[k]));
    let (windowed, whole) = spellings(&parts, SIZE);
    assert!(windowed.1.len() < whole.1.len() && windowed.2.len() < whole.2.len());
    let sixteen = |s: &Spelling| {
        render(Depth::Sixteen, SIZE, &s.0, &s.1, &s.2)
            .unwrap_or_else(|e| panic!("16-bit render refused: {e}"))
            .to_rgba16()
            .unwrap()
    };
    let float = |s: &Spelling| {
        render(Depth::Float, SIZE, &s.0, &s.1, &s.2)
            .unwrap_or_else(|e| panic!("float render refused: {e}"))
            .to_rgba32()
            .unwrap()
    };
    let expected = sixteen(&whole);
    assert!(
        sixteen(&windowed) == expected,
        "16-bit windowed render differs"
    );
    assert!(expected.iter().any(|p| p[3] != 0) && expected.iter().any(|p| p != &expected[0]));
    assert!(
        float(&windowed) == float(&whole),
        "float windowed render differs"
    );
}

#[test]
fn a_windowed_cmyk_list_publishes_as_its_zero_embedded_whole_raster_twin() {
    let inks = [
        [0.1, 0.2, 0.3, 0.05, 0.25],
        [0.9, 0.1, 0.2, 0.3, 0.8],
        [0.2, 0.7, 0.4, 0.0, 1.0],
        [0.3, 0.3, 0.9, 0.6, 0.6],
        [0.0, 0.5, 0.05, 0.2, 0.9],
    ];
    let parts = fixture("ink", |k| json!(inks[k]));
    let (windowed, whole) = spellings(&parts, SIZE);
    let base = cmyk_layers(
        SIZE.0,
        SIZE.1,
        vec![RgbLayer {
            name: "Bottom",
            w: SIZE.0,
            h: SIZE.1,
            offset: (0, 0),
            pixels: varied(SIZE.0, SIZE.1, 3),
        }],
    );
    let samples = |s: &Spelling| {
        let doc = publish(base, SIZE, &s.0, &s.1, &s.2)
            .unwrap_or_else(|e| panic!("CMYK publication refused: {e}"));
        let out = unsafe { &*doc }.cmyk_layer_samples(1).unwrap();
        free(doc);
        out
    };
    let expected = samples(&whole);
    assert!(
        samples(&windowed) == expected,
        "CMYK windowed publication differs"
    );
    let mut past = windowed.0.clone();
    past[1]["window"] = json!([30, 4, 10, 7]);
    assert_eq!(
        publish(base, SIZE, &past, &windowed.1, &windowed.2)
            .err()
            .as_deref(),
        Some(WINDOW_REFUSAL)
    );
    free(base);
}

/// One pixel inside a window is the colour at its coverage's alpha; one
/// outside it, on either side, is transparent.
#[test]
fn inside_a_window_is_the_colour_and_outside_it_is_transparent() {
    let color = [0.2f32, 0.4, 0.6, 1.0];
    let components = json!([{"kind": "coverage", "color": color, "window": [2, 1, 3, 2]}]);
    let image = render(Depth::Sixteen, (6, 4), &components, &[65535; 6], &[])
        .unwrap()
        .to_rgba16()
        .unwrap();
    for (x, y) in [(2, 1), (4, 2), (3, 2)] {
        let pixel = image[y * 6 + x];
        for channel in 0..3 {
            let expected = (f64::from(color[channel]) * 65535.0).round() as u16;
            assert!(
                pixel[channel].abs_diff(expected) <= 1,
                "({x}, {y}) channel {channel}: {} vs {expected}",
                pixel[channel]
            );
        }
        assert_eq!(pixel[3], 65535, "({x}, {y}) alpha");
    }
    for (x, y) in [(0, 0), (1, 1), (5, 1), (3, 0), (3, 3)] {
        assert_eq!(image[y * 6 + x], [0; 4], "({x}, {y}) is outside the window");
    }
}

#[test]
fn windows_and_their_buffers_are_checked_before_any_render() {
    let coverage = |window: Value| json!([{"kind": "coverage", "color": [1.0, 0.0, 0.0, 1.0], "window": window}]);
    for (what, window, samples) in [
        ("zero width", json!([0, 0, 0, 3]), 0usize),
        ("past the right edge", json!([5, 0, 4, 2]), 8),
        ("past the bottom edge", json!([0, 5, 2, 2]), 4),
        ("a negative origin", json!([-1, 0, 2, 2]), 4),
    ] {
        assert_eq!(
            render(
                Depth::Sixteen,
                (8, 6),
                &coverage(window),
                &vec![1; samples],
                &[]
            )
            .err()
            .as_deref(),
            Some(WINDOW_REFUSAL),
            "{what}"
        );
    }
    assert_eq!(
        render(
            Depth::Sixteen,
            (8, 6),
            &coverage(json!([1, 1, 3, 3])),
            &[1; 8],
            &[]
        )
        .err()
        .as_deref(),
        Some(LAYOUT_REFUSAL),
        "one sample short"
    );
    // 4,001 × 4,000 RGBA8 is 64,016,000 bytes: over the per-component bound,
    // refused before any plane is read.
    let big = json!([{"kind": "intrinsic_rgba8", "window": [0, 0, 4001, 4000]}]);
    assert_eq!(
        render(Depth::Float, (4001, 4000), &big, &[], &[0; 4])
            .err()
            .as_deref(),
        Some("artwork component exceeds 64,000,000 samples")
    );
}

/// A count the windows do not name is refused by the layout sentence before
/// admission counts it or any slice is built from it: a declared count of
/// 2^40 (whose bytes are far past the 2 GiB limit, so an admission that read
/// it would refuse first) over a buffer holding the windows' exact length.
#[test]
fn a_count_the_windows_do_not_name_is_refused_before_admission() {
    const DECLARED: usize = 1 << 40;
    let coverage =
        json!([{"kind": "coverage", "color": [1.0, 0.0, 0.0, 1.0], "window": [1, 1, 3, 3]}]);
    let glyph = json!([{"kind": "intrinsic_rgba8", "window": [1, 1, 3, 3]}]);
    let samples = [40_000u16; 9];
    let pixels = [200u8; 36];
    for depth in [Depth::Sixteen, Depth::Float] {
        assert_eq!(
            render_declared(depth, (8, 6), &coverage, (&samples, DECLARED), (&[], 0))
                .err()
                .as_deref(),
            Some(LAYOUT_REFUSAL),
            "a declared sample count"
        );
        assert_eq!(
            render_declared(depth, (8, 6), &glyph, (&[], 0), (&pixels, DECLARED))
                .err()
                .as_deref(),
            Some(LAYOUT_REFUSAL),
            "a declared intrinsic count"
        );
        // The same buffers at their own counts render.
        assert!(render(depth, (8, 6), &coverage, &samples, &[]).is_ok());
        assert!(render(depth, (8, 6), &glyph, &[], &pixels).is_ok());
    }
}

/// Two whole-raster RGBA8 planes of 32,016,000 bytes each: 64,032,000 in
/// all, past the old whole-layer total, each under the per-component bound.
/// The top plane is opaque red, so every pixel is.
#[test]
fn components_past_the_old_whole_layer_total_render() {
    const W: u32 = 4000;
    const H: u32 = 2001;
    let n = (W * H) as usize;
    let mut intrinsic = vec![0u8; n * 4];
    intrinsic.extend(std::iter::repeat_n([255u8, 0, 0, 255], n).flatten());
    assert!(intrinsic.len() > 64_000_000);
    let components = json!([{"kind": "intrinsic_rgba8"}, {"kind": "intrinsic_rgba8"}]);
    let image = render(Depth::Sixteen, (W, H), &components, &[], &intrinsic)
        .unwrap_or_else(|e| panic!("the lifted total refused: {e}"))
        .to_rgba16()
        .unwrap();
    assert_eq!(image.len(), n);
    assert_eq!(image[0], [65535, 0, 0, 65535]);
    assert_eq!(image[n - 1], [65535, 0, 0, 65535]);
}
