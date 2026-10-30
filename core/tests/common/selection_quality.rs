//! Original analytic fixture corpus. Ground-truth alpha is 4x4 area sampling
//! of explicit shapes, independent of any selection kernel under evaluation.
#![allow(dead_code)]
use image::{GrayImage, Luma, Rgba, RgbaImage};

pub const WIDTH: u32 = 320;
pub const HEIGHT: u32 = 256;
pub const SUBJECTS: [&str; 4] = ["portrait_hair", "fur", "translucent_glass", "hard_product"];

fn ellipse(x: f64, y: f64, cx: f64, cy: f64, rx: f64, ry: f64) -> bool {
    ((x - cx) / rx).powi(2) + ((y - cy) / ry).powi(2) < 1.0
}
fn alpha_at(subject: &str, x: f64, y: f64) -> f64 {
    match subject {
        "portrait_hair" => {
            if ellipse(x, y, 160., 126., 47., 67.) || ellipse(x, y, 160., 251., 97., 68.) {
                return 1.0;
            }
            // Curved, subpixel-width strands with varied lengths and coverage.
            for k in 0..38 {
                let z = k as f64;
                let t = (y - 45.) / 130.;
                let strand = 115. + z * 2.45 + (t * 3.3 + z * 0.7).sin() * (7. + z % 4.);
                if y > 42. + z % 8.
                    && y < 155. + z % 27.
                    && (x - strand).abs() < 0.34 + z % 3. * 0.13
                {
                    return 0.45 + z % 6. * 0.09;
                }
            }
            0.0
        }
        "fur" => {
            let nx = (x - 160.) / 90.;
            let ny = (y - 135.) / 77.;
            let angle = ny.atan2(nx);
            let r = (nx * nx + ny * ny).sqrt();
            let edge = 1. + 0.09 * (angle * 57.).sin() + 0.045 * (angle * 103.).sin();
            if r < 0.88 {
                1.0
            } else if r < edge {
                ((edge - r) / 0.065).clamp(0., 1.)
            } else {
                0.
            }
        }
        "translucent_glass" => {
            if !(88.0..232.0).contains(&x) || !(32.0..226.0).contains(&y) {
                return 0.;
            }
            let edge = (x - 88.).min(232. - x).min(y - 32.).min(226. - y);
            if edge < 3. {
                0.7
            } else {
                0.16 + 0.17 * ((x * 0.17).sin().abs()).powi(12) + if y > 142. { 0.18 } else { 0. }
            }
        }
        "hard_product" => {
            let body = (54.0..266.0).contains(&x) && (86.0..211.0).contains(&y);
            let top = (117.0..205.0).contains(&x) && (49.0..90.0).contains(&y);
            let antenna = (74.0..76.0).contains(&x) && (38.0..96.0).contains(&y);
            if (body || top || antenna) && !ellipse(x, y, 91., 107., 5., 5.) {
                1.
            } else {
                0.
            }
        }
        _ => panic!("unknown corpus subject"),
    }
}

pub fn alpha(subject: &str) -> GrayImage {
    GrayImage::from_fn(WIDTH, HEIGHT, |x, y| {
        let mut sum = 0.;
        for sy in 0..4 {
            for sx in 0..4 {
                sum += alpha_at(
                    subject,
                    f64::from(x) + (sx as f64 + 0.5) / 4.,
                    f64::from(y) + (sy as f64 + 0.5) / 4.,
                );
            }
        }
        Luma([(sum / 16. * 255.).round() as u8])
    })
}

/// Authored foreground values behind every analytic composite (encoded RGB).
pub fn foreground_color(x: u32) -> [f32; 3] {
    [0.24 + 0.03 * (x as f32 * 0.13).sin(), 0.18, 0.12]
}

pub fn composite(truth: &GrayImage, background: &str) -> RgbaImage {
    RgbaImage::from_fn(truth.width(), truth.height(), |x, y| {
        let a = f32::from(truth.get_pixel(x, y)[0]) / 255.;
        let foreground = foreground_color(x);
        let bg = match background {
            "light" => [0.88, 0.91, 0.96],
            "dark" => [0.02, 0.03, 0.06],
            "ambiguous" => [foreground[0] + 0.015 * (y as f32 * 0.12).sin(), 0.18, 0.12],
            _ => panic!("unknown corpus background"),
        };
        Rgba([
            (255. * (a * foreground[0] + (1. - a) * bg[0])).round() as u8,
            (255. * (a * foreground[1] + (1. - a) * bg[1])).round() as u8,
            (255. * (a * foreground[2] + (1. - a) * bg[2])).round() as u8,
            255,
        ])
    })
}
