//! Crop past the canvas: the byte and typed adapters over
//! `crop_background`, and the byte `extended_crop`.
//!
//! # The Background (Default) fill
//!
//! A [`GeometryEdit::Straighten`] carrying `background` pads record 0 with
//! that colour to `crop_background::pad_rect` BEFORE the op runs, when — and
//! only when — every clause holds:
//!
//! * record 0 is D5's Background: `style_knockout::background_entry` (typed:
//!   `precision_style_knockout::background_record`) with `Cut::NONE` — a
//!   visible, Normal, 100 %, unmasked, unstyled raster at (0, 0) covering the
//!   canvas, with no clipped record above it;
//! * it is a raster Delete Cropped Pixels trims (`doc_delete_cropped::
//!   eligible`, typed `precision_geometry_canvas::trims`);
//! * its pixels are fully OPAQUE (alpha 255, 65535, float 1.0, CMYK alpha
//!   65535 everywhere): only a Background layer is padded, and a layer with
//!   a transparent pixel is not one (take1's R7);
//! * the crop deletes its cropped pixels: with Delete Cropped Pixels off,
//!   the Background becomes "Layer 0" and the new area stays transparent
//!   (take1's R2).
//!
//! Every other record is untouched, so its new area is transparent. The pad
//! moves record 0's origin, so the comps follow it through
//! `comps_after_growth`, as every origin-moving growth does. The
//! op then runs over the padded document and Delete Cropped Pixels trims
//! after it.
//!
//! The colour: byte `round(v · 255)` of `color` in `document` space only; at
//! 16 bits `precision_paint::prepare_colors` then `store`, at float
//! `float_paint_ink::linear_ink` (`linear` admitted only there, as paint
//! admits it); on CMYK `ink` as `cmyk_paint` reads it, stored as ink codes.
//!
//! Before any allocation the pad is refused by name ([`CROP_PAD_TOO_LARGE`])
//! when `pad_rect` refuses it or when the padded record would break the
//! native save cap (byte `native_layers_fit_dimensions`, typed
//! `native_fits_geometry`). The plan is the op's plan over the padded
//! document plus the pad's own bytes ([`DrDocument::background_pad_bytes`]);
//! a plan query plans over a filled stand-in of it
//! ([`DrDocument::background_plan_input`]) and copies no pixels, so only
//! the admitted edit pre-pass makes the pad.

use image::RgbaImage;

use crate::cmyk_image::{CmykImage, CmykSample};
use crate::crop_background::{pad_rect, padded, window_fits};
use crate::doc::{DrDocument, LayerKind, LayerRaster};
use crate::doc_channel::Channel;
use crate::doc_transform::Affine;
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_geometry::{
    CropBackground, GeometryEdit, CROP_BACKGROUND_SHAPE, CROP_PAD_TOO_LARGE,
};
use crate::precision_image::{PrecisionImage, PrecisionStorage};
use crate::precision_paint::PaintColorSpace;
use crate::precision_sampling::GeometryFilter;
use crate::style_composite::Cut;
use crate::tile_store::TilePlane;

/// An 8-bit background named in any space but the document's.
const BYTE_COLOR_SPACE: &str = "an 8-bit crop background takes color_space document";

type Rect = (i64, i64, u32, u32);
/// A fill request: the matrix, the window and the colour.
type Request<'a> = ([f64; 6], Rect, &'a CropBackground);

/// The straighten's matrix, window and background when it asks for a fill
/// AND deletes its cropped pixels; the window is checked first, so a window
/// out of range is refused by its own sentence and never reaches `pad_rect`.
fn request(edit: &GeometryEdit, channels: usize) -> Result<Option<Request<'_>>, String> {
    let GeometryEdit::Straighten {
        matrix,
        x,
        y,
        width,
        height,
        delete_cropped,
        background: Some(background),
        ..
    } = edit
    else {
        return Ok(None);
    };
    window_fits(*x, *y, *width, *height, channels)?;
    if !delete_cropped {
        return Ok(None);
    }
    Ok(Some((
        *matrix,
        (i64::from(*x), i64::from(*y), *width, *height),
        background,
    )))
}

/// The RGB `color` and its space, or the shape refusal.
fn rgb(background: &CropBackground) -> Result<([f64; 3], PaintColorSpace), String> {
    match (background.color, background.ink) {
        (Some(color), None) if color.iter().all(|v| v.is_finite()) => Ok((
            color,
            background.color_space.unwrap_or(PaintColorSpace::Document),
        )),
        _ => Err(CROP_BACKGROUND_SHAPE.into()),
    }
}

/// The CMYK `ink`, or the shape refusal.
fn ink(background: &CropBackground) -> Result<[f64; 4], String> {
    match (background.ink, background.color, background.color_space) {
        (Some(ink), None, None) if ink.iter().all(|v| v.is_finite()) => Ok(ink),
        _ => Err(CROP_BACKGROUND_SHAPE.into()),
    }
}

fn unit(v: f64) -> f32 {
    v.clamp(0.0, 1.0) as f32
}

fn area(rect: Rect) -> u64 {
    u64::from(rect.2) * u64::from(rect.3)
}

impl DrDocument {
    /// The pad rectangle and colour when record 0 qualifies (module doc),
    /// with every refusal named before anything is allocated.
    fn background_pad(&self, edit: &GeometryEdit) -> Result<Option<(Rect, [u8; 4])>, String> {
        let Some((matrix, window, background)) = request(edit, self.channels.len())? else {
            return Ok(None);
        };
        let (color, space) = rgb(background)?;
        if !matches!(space, PaintColorSpace::Document) {
            return Err(BYTE_COLOR_SPACE.into());
        }
        let fill = [
            (unit(color[0]) * 255.0).round() as u8,
            (unit(color[1]) * 255.0).round() as u8,
            (unit(color[2]) * 255.0).round() as u8,
            255,
        ];
        let canvas = (self.width, self.height);
        if crate::style_knockout::background_entry(&self.layers, canvas, Cut::NONE, self.background)
            != Some(0)
        {
            return Ok(None);
        }
        let layer = &self.layers[0];
        if layer.kind != LayerKind::Raster || !crate::doc_delete_cropped::eligible(layer) {
            return Ok(None);
        }
        // The tiles' own runs: a plan query binds no view (no allocation).
        if !layer
            .pixels
            .runs()
            .all(|run| run.iter().all(|p| p[3] == 255))
        {
            return Ok(None);
        }
        let (w, h) = layer.pixels.dimensions();
        let extent = (i64::from(layer.offset.0), i64::from(layer.offset.1), w, h);
        let Some(rect) = pad_rect(extent, matrix, window)? else {
            return Ok(None);
        };
        let dims = self.layers.iter().enumerate().map(|(i, l)| {
            if i == 0 {
                (rect.2, rect.3)
            } else {
                l.pixels.dimensions()
            }
        });
        if !self.native_layers_fit_dimensions(dims) {
            return Err(CROP_PAD_TOO_LARGE.into());
        }
        Ok(Some((rect, fill)))
    }

    /// The bytes the Background pad costs (0 when nothing is padded): the
    /// plan's extra term (module doc), 8 B per padded pixel — the padded
    /// buffer and the raster it is adopted into, each 4 B/px. Measured
    /// (`scripts/crop-extend-admission/measure.py`, Release, 4800×3200 noise
    /// cropped to 6000×4000 past the canvas, 3 runs plus a baseline twin):
    /// at 4 B/px `extendrot8` (angle 5) planned 853,699,632 against max E
    /// 691,225,128 (ratio 1.235, short of 1.25); `extend8` planned
    /// 730,195,922 against 424,247,632.
    pub(crate) fn background_pad_bytes(&self, edit: &GeometryEdit) -> Result<u64, String> {
        Ok(self
            .background_pad(edit)?
            .map_or(0, |(rect, _)| area(rect).saturating_mul(8)))
    }

    /// The plan query's stand-in for [`Self::background_padded`]: record 0
    /// at the padded rect as one FILLED raster (`LayerRaster::filled`, at
    /// most four tiles), so planning copies no pixels; only the admitted
    /// edit pre-pass makes the pad. The byte planner reads record 0's
    /// dimensions and offset alone, except the inventory, which sees the
    /// shared tiles; the second value is the padded raster's own 4 B/px the
    /// caller adds to the plan's retained bytes (over by the filled tiles,
    /// never under). `None` when nothing is padded.
    pub(crate) fn background_plan_input(
        &self,
        edit: &GeometryEdit,
    ) -> Result<Option<(Self, u64)>, String> {
        let Some((rect, fill)) = self.background_pad(edit)? else {
            return Ok(None);
        };
        let origin = (rect.0 as i32, rect.1 as i32);
        let comps = self.comps_after_growth(0, origin);
        let mut out = self.clone();
        out.layers[0].pixels = LayerRaster::filled(rect.2, rect.3, fill);
        out.layers[0].offset = origin;
        if let Some(comps) = comps {
            out.comps = comps;
        }
        Ok(Some((out, area(rect).saturating_mul(4))))
    }

    /// The document with record 0 padded for `edit` (module doc), `None`
    /// when nothing is padded.
    pub(crate) fn background_padded(&self, edit: &GeometryEdit) -> Result<Option<Self>, String> {
        let Some((rect, fill)) = self.background_pad(edit)? else {
            return Ok(None);
        };
        let layer = &self.layers[0];
        let (w, h) = layer.pixels.dimensions();
        let extent = (i64::from(layer.offset.0), i64::from(layer.offset.1), w, h);
        let image = layer.pixels.contiguous_image();
        let (samples, _) = image.as_raw().as_chunks::<4>();
        let grown = padded(samples, extent, rect, fill)?.into_flattened();
        let pixels = RgbaImage::from_raw(rect.2, rect.3, grown)
            .ok_or("the background pad has the wrong length")?;
        let origin = (rect.0 as i32, rect.1 as i32);
        let comps = self.comps_after_growth(0, origin);
        let mut out = self.clone();
        out.layers[0].pixels = LayerRaster::unseeded(pixels);
        out.layers[0].offset = origin;
        if let Some(comps) = comps {
            out.comps = comps;
        }
        Ok(Some(out))
    }

    /// A crop to `(x, y, w, h)` that may lie past any canvas edge: `None`
    /// channels is exactly `canvas_resize(w, h, (−x, −y))`; `Some` are the
    /// `w × h` planes already in the window (the turned arm's
    /// `doc_rotate::channels_into`), kept as given and never padded again.
    /// `None` where `canvas_resize` refuses, for planes of the wrong count or
    /// size, and for an `i32::MIN` corner.
    pub(crate) fn extended_crop(
        &self,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        channels: Option<Vec<Channel>>,
    ) -> Option<Self> {
        let origin = (x.checked_neg()?, y.checked_neg()?);
        let Some(planes) = channels else {
            return self.canvas_resize(w, h, origin);
        };
        if planes.len() != self.channels.len()
            || planes.iter().any(|c| c.data.dimensions() != (w, h))
            || !crate::doc_channel::channels_fit_at(w, h, planes.len())
        {
            return None;
        }
        let bare = Self {
            channels: Vec::new(),
            ..self.clone()
        };
        let mut out = bare.canvas_resize(w, h, origin)?;
        out.channels = planes;
        Some(out)
    }

    /// The byte Straighten arm's turned window past an edge: the layers
    /// straightened as the inside turned arm straightens them, each saved
    /// channel resampled once straight into the window
    /// (`doc_rotate::channels_into`), then [`Self::extended_crop`]. `scratch`
    /// is the plan's scratch, the layers' floor.
    pub(crate) fn extended_straighten(
        &self,
        matrix: [f64; 6],
        filter: GeometryFilter,
        window: (i32, i32, u32, u32),
        job: JobContext<'_>,
        scratch: u64,
    ) -> Result<Option<Self>, String> {
        let (x, y, width, height) = window;
        let matrix = Affine::from_array(matrix);
        let windowed = Affine {
            tx: matrix.tx - f64::from(x),
            ty: matrix.ty - f64::from(y),
            ..matrix
        };
        let inverse = windowed
            .inverse()
            .ok_or("typed affine matrix is singular")?;
        let members = self.layers.len() + self.channels.len();
        let s = if members == 0 {
            1.0
        } else {
            self.layers.len() as f64 / members as f64
        };
        let straightened = match self.straighten_layers_with_job(
            matrix,
            crate::doc_geometry_set::filter_type(filter),
            job.range(0.0, s),
            scratch,
        )? {
            Some(doc) => doc,
            None if self.straighten_writes_nothing() => self.clone(),
            None => return Err("byte straighten could not transform all layers".into()),
        };
        let n = self.channels.len() as f64;
        let planes = crate::doc_rotate::channels_into(
            &self.channels,
            &inverse,
            (width, height),
            filter,
            job,
            |c: f64| s + (1.0 - s) * c / n,
        )?;
        Ok(straightened.extended_crop(x, y, width, height, Some(planes)))
    }
}

/// The typed pad's fill, by the record's storage.
#[derive(Clone, Copy)]
enum Fill {
    Rgba16([u16; 4]),
    Rgba32([f32; 4]),
    Cmyk(CmykSample),
}

impl PrecisionDocument {
    /// The typed twin of the byte `background_pad`.
    fn background_pad(&self, edit: &GeometryEdit) -> Result<Option<(Rect, Fill)>, String> {
        let Some((matrix, window, background)) = request(edit, self.channels.len())? else {
            return Ok(None);
        };
        let fill = if self.is_cmyk() {
            let ink = ink(background)?;
            let code = |v: f64| crate::cmyk_composite::code(unit(v));
            Fill::Cmyk([
                code(ink[0]),
                code(ink[1]),
                code(ink[2]),
                code(ink[3]),
                65535,
            ])
        } else {
            let (color, space) = rgb(background)?;
            let color = [color[0] as f32, color[1] as f32, color[2] as f32, 1.0];
            if self.is_float() {
                Fill::Rgba32(crate::float_paint_ink::linear_ink(color, space, self)?)
            } else {
                if matches!(space, PaintColorSpace::Linear) {
                    return Err(crate::float_paint::LINEAR_ONLY.into());
                }
                let color = color.map(|v| v.clamp(0.0, 1.0));
                let prepared = crate::precision_paint::prepare_colors(&[color], space, self)?;
                Fill::Rgba16(crate::precision_paint::store(prepared[0]))
            }
        };
        if !crate::precision_style_knockout::background_record(
            &self.layers,
            self.dimensions(),
            Cut::NONE,
            self.float.is_some(),
            self.background,
        ) {
            return Ok(None);
        }
        let Some(record) = self.layers.first() else {
            return Ok(None);
        };
        if !crate::precision_geometry::trims(record) {
            return Ok(None);
        }
        let (dimensions, offset, opaque, fill) = match (&record.content, fill) {
            (Content::Raster { image, offset }, Fill::Rgba16(_) | Fill::Rgba32(_)) => {
                let fill = match (image.storage(), fill) {
                    (PrecisionStorage::Rgba16Unorm, Fill::Rgba16(_))
                    | (PrecisionStorage::Rgba32Float, Fill::Rgba32(_)) => fill,
                    // A raster whose storage is not the document's depth is
                    // not a Background this pad can grow.
                    _ => return Ok(None),
                };
                (image.dimensions(), *offset, opaque(image), fill)
            }
            (Content::CmykRaster { image, offset }, Fill::Cmyk(_)) => {
                (image.dimensions(), *offset, opaque_cmyk(image), fill)
            }
            _ => return Ok(None),
        };
        if !opaque {
            return Ok(None);
        }
        let extent = (
            i64::from(offset.0),
            i64::from(offset.1),
            dimensions.0,
            dimensions.1,
        );
        let Some(rect) = pad_rect(extent, matrix, window)? else {
            return Ok(None);
        };
        let (mut rasters, masks) = self.native_dimensions();
        if let Some(first) = rasters.first_mut() {
            *first = (rect.2, rect.3);
        }
        self.native_fits_geometry(self.dimensions(), &rasters, &masks, self.channels.len())
            .map_err(|_| CROP_PAD_TOO_LARGE.to_string())?;
        Ok(Some((rect, fill)))
    }

    /// The typed twin of [`DrDocument::background_pad_bytes`]: 4¼ copies of
    /// the padded record's storage — 34 B/px at 16 bits, 68 at float, 43 on
    /// CMYK (42½ rounded up): the source copy, the padded buffer, the tiles
    /// it is adopted into and the new canvas's display preparation, live
    /// together, plus a quarter for what stays resident beside them. An
    /// identity typed crop's own plan is near zero (its planes are `Keep`), so
    /// this term carries the whole edit. Measured as the byte term's: at one
    /// copy (8 B/px) `extend16` planned 197,245,620 against max E 576,684,512
    /// (ratio 0.342). Four copies (32 B/px) passed at max E 576,684,512 only
    /// while the plan query built the real pad, which happened to evict
    /// ~71 MB resident before the edit. With the filled stand-in (review
    /// finding 2) max E is 647,938,504, and four copies planned 773,245,620
    /// (ratio 1.193). At 4¼ copies the plan is 821,245,620 against max E
    /// 648,298,952 (ratio 1.267). Float and CMYK take the same 4¼ copies,
    /// unmeasured.
    pub(crate) fn background_pad_bytes(&self, edit: &GeometryEdit) -> Result<u64, String> {
        Ok(self.background_pad(edit)?.map_or(0, |(rect, fill)| {
            let per = match fill {
                Fill::Rgba16(_) => 34,
                Fill::Rgba32(_) => 68,
                Fill::Cmyk(_) => 43,
            };
            area(rect).saturating_mul(per)
        }))
    }

    /// The typed twin of [`DrDocument::background_plan_input`]: record 0 as
    /// one FILLED plane at the padded rect, its raster's own bytes (8, 16 or
    /// 10 B/px) the second value. The typed planner reads one thing past
    /// dimensions and offsets, a u16 raster's uniformity
    /// (`precision_sampling::uniform_source`): the stand-in is uniform only
    /// when the padded plane would be — the record already uniform in the
    /// fill's own value — and otherwise holds one other sample in one tile.
    pub(crate) fn background_plan_input(
        &self,
        edit: &GeometryEdit,
    ) -> Result<Option<(Self, u64)>, String> {
        let Some((rect, fill)) = self.background_pad(edit)? else {
            return Ok(None);
        };
        let Some(record) = self.layers.first() else {
            return Ok(None);
        };
        let origin = (rect.0 as i32, rect.1 as i32);
        let (content, per) = match (&record.content, fill) {
            (Content::Raster { image, .. }, Fill::Rgba16(fill)) => {
                let mut plane = TilePlane::filled(rect.2, rect.3, fill)?;
                if image.uniform_rgba16() != Some(fill) {
                    let other = [fill[0] ^ 1, fill[1], fill[2], fill[3]];
                    if let Some(marked) = plane.with_rect(0, 0, 1, 1, &[other], None)? {
                        plane = marked;
                    }
                }
                let image =
                    PrecisionImage::from_rgba16_tiles_unseeded(plane, image.profile().clone())?;
                (
                    Content::Raster {
                        image,
                        offset: origin,
                    },
                    8,
                )
            }
            (Content::Raster { image, .. }, Fill::Rgba32(fill)) => (
                Content::Raster {
                    image: PrecisionImage::filled_rgba32_extended(
                        rect.2,
                        rect.3,
                        fill,
                        image.profile().clone(),
                    )?,
                    offset: origin,
                },
                16,
            ),
            (Content::CmykRaster { .. }, Fill::Cmyk(fill)) => (
                Content::CmykRaster {
                    image: CmykImage::filled(rect.2, rect.3, fill)?,
                    offset: origin,
                },
                10,
            ),
            _ => return Ok(None),
        };
        let mut layer = record.clone();
        layer.content = content;
        let comps = self.comps_after_growth(0, origin);
        let mut out = self.replacing_layer_for_plan(0, layer)?;
        if let Some(comps) = comps {
            out.comps = comps;
        }
        Ok(Some((out, area(rect).saturating_mul(per))))
    }

    /// The typed twin of [`DrDocument::background_padded`].
    pub(crate) fn background_padded(&self, edit: &GeometryEdit) -> Result<Option<Self>, String> {
        let Some((rect, fill)) = self.background_pad(edit)? else {
            return Ok(None);
        };
        let Some(record) = self.layers.first() else {
            return Ok(None);
        };
        let origin = (rect.0 as i32, rect.1 as i32);
        let content = match (&record.content, fill) {
            (Content::Raster { image, offset }, Fill::Rgba16(fill)) => {
                let extent = extent_of(image.dimensions(), *offset);
                let grown = padded(&image.to_rgba16()?, extent, rect, fill)?;
                Content::Raster {
                    image: PrecisionImage::from_rgba16_unseeded(
                        rect.2,
                        rect.3,
                        grown,
                        image.profile().clone(),
                        None,
                    )?,
                    offset: origin,
                }
            }
            (Content::Raster { image, offset }, Fill::Rgba32(fill)) => {
                let extent = extent_of(image.dimensions(), *offset);
                let grown = padded(&image.to_rgba32()?, extent, rect, fill)?;
                Content::Raster {
                    image: PrecisionImage::from_rgba32_extended(
                        rect.2,
                        rect.3,
                        grown,
                        image.profile().clone(),
                        None,
                    )?,
                    offset: origin,
                }
            }
            (Content::CmykRaster { image, offset }, Fill::Cmyk(fill)) => {
                let extent = extent_of(image.dimensions(), *offset);
                let grown = padded(&image.to_contiguous()?, extent, rect, fill)?;
                Content::CmykRaster {
                    image: CmykImage::from_contiguous(rect.2, rect.3, &grown, JobContext::NONE)?,
                    offset: origin,
                }
            }
            _ => return Ok(None),
        };
        let mut layer = record.clone();
        layer.content = content;
        let comps = self.comps_after_growth(0, origin);
        let mut out = self.replacing_layer_for_plan(0, layer)?;
        if let Some(comps) = comps {
            out.comps = comps;
        }
        Ok(Some(out))
    }
}

fn extent_of(dimensions: (u32, u32), offset: (i32, i32)) -> Rect {
    (
        i64::from(offset.0),
        i64::from(offset.1),
        dimensions.0,
        dimensions.1,
    )
}

/// Every alpha sample at its maximum (65535, float 1.0). Also the PSD
/// writer's "can this bottom be a Background record" scan (`psd_background`).
pub(crate) fn opaque(image: &PrecisionImage) -> bool {
    if let Some(mut runs) = image.runs_rgba16() {
        return runs.all(|run| run.iter().all(|p| p[3] == 65535));
    }
    if let Some(mut runs) = image.runs_rgba32() {
        return runs.all(|run| run.iter().all(|p| p[3] == 1.0));
    }
    false
}

/// Every CMYK alpha code at 65535. Also the CMYK twin of [`opaque`]'s
/// Background-record scan (`psd_background`).
pub(crate) fn opaque_cmyk(image: &CmykImage) -> bool {
    let (w, h) = image.dimensions();
    (0..h).all(|y| {
        image
            .row_segments(y, 0, w)
            .all(|(_, run)| run.iter().all(|p| p[4] == 65535))
    })
}
