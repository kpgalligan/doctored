//! Contact sheets: a grid of thumbnails composed onto a flat background,
//! entirely in the core and entirely out of the shipped kernels.
//!
//! # What this module owns and what it does not
//!
//! It owns the GEOMETRY — how many rows a count needs, where cell `k` sits,
//! how a source fits into a cell — and the composition order. It owns no
//! pixel math: the thumbnail conversion is
//! `retained_source_render::color8_image`, the reduction `ops::resize`, the
//! placement `blend::source_over_rgba8`.
//!
//! It also rasterizes no text. A caption is a BITMAP the host renders and
//! hands in, because type is a platform concern (fonts, shaping, hinting) and
//! a core that grew a glyph rasterizer for a file-name strip would be a much
//! larger thing than a contact sheet.
//!
//! # The sheet's colour
//!
//! Cells and sheet are 8-bit sRGB. A contact sheet is a proof — something to
//! look at, print small, and throw away — so there is nothing for the extra
//! depth to protect, and one destination space means a sheet built from a
//! Display P3 document and an sRGB one shows them side by side honestly. A
//! source already describing sRGB is passed through unconverted, so the
//! common case costs nothing.
//!
//! # Geometry
//!
//! The gutter is drawn OUTSIDE the grid as well as between cells, so
//!
//! ```text
//! width  = gutter + columns * (cell_width + gutter)
//! height = gutter + rows    * (cell_height + caption_height + gutter)
//! ```
//!
//! and the caption strip belongs to its cell rather than to the gutter — a
//! caption pushed into the gutter would sit as close to the picture below it
//! as to its own.
//!
//! Geometry only, no pixel math: the fit is `ops::resize`, the conversion
//! `retained_source_render::color8_image` (the owned-image form of `color8`, sharing its one
//! per-pixel loop `convert8_runs`), the placement `blend::source_over_rgba8`. `width = gutter +
//! columns·(cell_w + gutter)`, `height = gutter + rows·(cell_h + caption_h + gutter)` with `rows =
//! ceil(n / columns)`; caps are 1…256 cells, side ≤ 30000, pixels ≤ `MAX_PIXELS`. `fitted` keeps
//! the aspect and NEVER upscales; a cell is centred, so a transparent source shows the background.
//! Cells and sheet are 8-bit sRGB (a proof has nothing for the extra depth to protect) and a source
//! that `describes_srgb` is passed through unconverted.

use image::imageops::FilterType;
use image::RgbaImage;
use serde_json::Value;

use crate::doc::{DrDocument, MAX_PIXELS};
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::DrImage;

/// The most cells one sheet may hold. A contact sheet is read at a glance;
/// past a few hundred thumbnails it is a mosaic, and the cost is real.
pub const MAX_CELLS: usize = 256;

/// The largest side a sheet may have — the same ceiling `doc_export` puts on
/// a deliverable, for the same reason.
const MAX_SHEET_SIDE: u32 = 30_000;

/// The parsed layout request.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContactSheetRequest {
    pub columns: u32,
    pub cell: (u32, u32),
    pub gutter: u32,
    pub caption_height: u32,
    /// Straight RGBA8; `#rrggbb` means fully opaque.
    pub background: [u8; 4],
    pub filter: FilterType,
}

impl Default for ContactSheetRequest {
    fn default() -> Self {
        ContactSheetRequest {
            columns: 4,
            cell: (256, 256),
            gutter: 16,
            caption_height: 0,
            background: [255, 255, 255, 255],
            filter: FilterType::Lanczos3,
        }
    }
}

fn integer(
    object: &serde_json::Map<String, Value>,
    key: &str,
    range: std::ops::RangeInclusive<u32>,
    default: u32,
) -> Result<u32, String> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(value) => {
            let number = value
                .as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .filter(|v| range.contains(v))
                .ok_or_else(|| {
                    format!(
                        "contact sheet {key} must be an integer in [{},{}]",
                        range.start(),
                        range.end()
                    )
                })?;
            Ok(number)
        }
    }
}

/// `#rrggbb` or `#rrggbbaa`, the one colour spelling this build's JSON uses.
fn colour(text: &str) -> Result<[u8; 4], String> {
    let body = text
        .strip_prefix('#')
        .ok_or("contact sheet background must be #rrggbb or #rrggbbaa")?;
    if body.len() != 6 && body.len() != 8 || !body.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("contact sheet background must be #rrggbb or #rrggbbaa".into());
    }
    let byte = |at: usize| u8::from_str_radix(&body[at..at + 2], 16).unwrap_or(0);
    Ok([
        byte(0),
        byte(2),
        byte(4),
        if body.len() == 8 { byte(6) } else { 255 },
    ])
}

impl ContactSheetRequest {
    /// The ONE parser. Every field is optional; the defaults are a readable
    /// four-column sheet of 256 px cells on white.
    pub fn parse(json: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(json)
            .map_err(|e| format!("contact sheet request is not valid JSON: {e}"))?;
        let object = value
            .as_object()
            .ok_or("contact sheet request must be a JSON object")?;
        for key in object.keys() {
            if ![
                "columns",
                "cell_width",
                "cell_height",
                "gutter",
                "caption_height",
                "background",
                "filter",
            ]
            .contains(&key.as_str())
            {
                return Err(format!(
                    "contact sheet request has an unknown key \"{key}\""
                ));
            }
        }
        let default = ContactSheetRequest::default();
        let background = match object.get("background") {
            None | Some(Value::Null) => default.background,
            Some(Value::String(text)) => colour(text)?,
            Some(_) => return Err("contact sheet background must be a string".into()),
        };
        let filter = match object.get("filter") {
            None | Some(Value::Null) => default.filter,
            Some(Value::String(text)) => match text.as_str() {
                "lanczos3" => FilterType::Lanczos3,
                "nearest" => FilterType::Nearest,
                "bilinear" => FilterType::Triangle,
                "catmull-rom" => FilterType::CatmullRom,
                other => {
                    return Err(format!(
                        "contact sheet filter \"{other}\" is not one of nearest, bilinear, catmull-rom or lanczos3"
                    ))
                }
            },
            Some(_) => return Err("contact sheet filter must be a string".into()),
        };
        Ok(ContactSheetRequest {
            columns: integer(object, "columns", 1..=32, default.columns)?,
            cell: (
                integer(object, "cell_width", 16..=2048, default.cell.0)?,
                integer(object, "cell_height", 16..=2048, default.cell.1)?,
            ),
            gutter: integer(object, "gutter", 0..=256, default.gutter)?,
            caption_height: integer(object, "caption_height", 0..=128, default.caption_height)?,
            background,
            filter,
        })
    }

    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "columns": self.columns,
            "cell_width": self.cell.0,
            "cell_height": self.cell.1,
            "gutter": self.gutter,
            "caption_height": self.caption_height,
        })
    }

    /// The sheet's `(width, height, rows)` for `count` cells.
    pub fn dimensions(&self, count: usize) -> Result<(u32, u32, u32), String> {
        if count == 0 {
            return Err("a contact sheet needs at least one cell".into());
        }
        if count > MAX_CELLS {
            return Err(format!("a contact sheet holds at most {MAX_CELLS} cells"));
        }
        let columns = u64::from(self.columns.max(1));
        let rows = (count as u64).div_ceil(columns);
        let width =
            u64::from(self.gutter) + columns * (u64::from(self.cell.0) + u64::from(self.gutter));
        let height = u64::from(self.gutter)
            + rows
                * (u64::from(self.cell.1)
                    + u64::from(self.caption_height)
                    + u64::from(self.gutter));
        if width > u64::from(MAX_SHEET_SIDE) || height > u64::from(MAX_SHEET_SIDE) {
            return Err(format!(
                "a contact sheet must be at most {MAX_SHEET_SIDE} pixels on a side"
            ));
        }
        if width * height > MAX_PIXELS {
            return Err("contact sheet dimensions exceed the supported pixel limit".into());
        }
        Ok((width as u32, height as u32, rows as u32))
    }

    /// The top-left corner of cell `k`'s PICTURE area (the caption strip
    /// starts `cell_height` below it).
    pub fn cell_origin(&self, k: usize) -> (u32, u32) {
        let columns = self.columns.max(1) as usize;
        let (column, row) = ((k % columns) as u32, (k / columns) as u32);
        (
            self.gutter + column * (self.cell.0 + self.gutter),
            self.gutter + row * (self.cell.1 + self.caption_height + self.gutter),
        )
    }

    /// `source` scaled to fit the cell with its aspect ratio kept, and NEVER
    /// upscaled: a contact sheet of 64 px icons must show 64 px icons rather
    /// than a wall of blur.
    pub fn fitted(&self, source: (u32, u32)) -> (u32, u32) {
        let (sw, sh) = source;
        if sw == 0 || sh == 0 {
            return (1, 1);
        }
        let scale = (f64::from(self.cell.0) / f64::from(sw))
            .min(f64::from(self.cell.1) / f64::from(sh))
            .min(1.0);
        let side = |v: u32| {
            (f64::from(v) * scale)
                .round()
                .clamp(1.0, f64::from(u32::MAX)) as u32
        };
        (side(sw).min(self.cell.0), side(sh).min(self.cell.1))
    }
}

/// Composes one sheet. `captions` is positional with `sources`; an entry may
/// be `None` (no caption for that cell) and the slice may be shorter.
///
/// A transparent cell shows the background, which is what makes an empty
/// layer a legible cell rather than a hole.
pub(crate) fn compose(
    request: &ContactSheetRequest,
    sources: &[&DrDocument],
    captions: &[Option<&DrImage>],
    job: JobContext<'_>,
) -> Result<DrImage, String> {
    let (width, height, _) = request.dimensions(sources.len())?;
    job.check()?;
    let mut sheet = RgbaImage::from_pixel(width, height, image::Rgba(request.background));
    let srgb = IccProfile::srgb();

    for (k, document) in sources.iter().enumerate() {
        job.check()?;
        let flat = std::sync::Arc::new(document.flattened()?);
        let converted = if document.profile.describes_srgb() {
            flat
        } else {
            crate::retained_source_render::color8_image(&flat, &document.profile, &srgb, job)?
        };
        let fitted = request.fitted(converted.dimensions());
        let thumbnail = if converted.dimensions() == fitted {
            (*converted).clone()
        } else {
            crate::ops::resize(&converted, fitted.0, fitted.1, request.filter)
                .ok_or("contact sheet thumbnail resampling produced invalid dimensions")?
        };
        let (cx, cy) = request.cell_origin(k);
        // Centred in the cell: a portrait and a landscape thumbnail in
        // neighbouring cells line up on their centres rather than their
        // corners, which is what makes a mixed sheet readable.
        let x = cx + (request.cell.0 - fitted.0) / 2;
        let y = cy + (request.cell.1 - fitted.1) / 2;
        draw(&mut sheet, &thumbnail, x, y);

        if request.caption_height > 0 {
            if let Some(Some(caption)) = captions.get(k) {
                let (caw, cah) = caption.pixels.dimensions();
                let cxx = cx + request.cell.0.saturating_sub(caw) / 2;
                let cyy = cy + request.cell.1 + request.caption_height.saturating_sub(cah) / 2;
                draw(&mut sheet, &caption.pixels, cxx, cyy);
            }
        }
    }
    job.report(1.0, 1)?;
    Ok(DrImage { pixels: sheet })
}

/// Source-over placement of `patch` at `(x, y)`, clipped to the sheet — the
/// shipped straight-alpha pixel primitive, one pixel at a time.
fn draw(sheet: &mut RgbaImage, patch: &RgbaImage, x: u32, y: u32) {
    let (sw, sh) = sheet.dimensions();
    let (pw, ph) = patch.dimensions();
    for row in 0..ph {
        let dy = y + row;
        if dy >= sh {
            break;
        }
        for column in 0..pw {
            let dx = x + column;
            if dx >= sw {
                break;
            }
            let source = patch.get_pixel(column, row).0;
            let sa = f32::from(source[3]) / 255.0;
            if sa == 0.0 {
                continue;
            }
            let premultiplied = [
                f32::from(source[0]) / 255.0 * sa,
                f32::from(source[1]) / 255.0 * sa,
                f32::from(source[2]) / 255.0 * sa,
            ];
            crate::blend::source_over_rgba8(&mut sheet.get_pixel_mut(dx, dy).0, premultiplied, sa);
        }
    }
}
