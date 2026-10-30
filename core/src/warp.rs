//! Persisted displacement lattice shared by byte and native u16 documents.
//! Nodes lie on the original plane's pixel-edge rectangle, in row-major order.
//!
//! The shared 2…16 lattice validates finite ±30000-pixel nodes and a per-cell Jacobian row-sum
//! contraction ≤0.5 before allocation. It maps original plane pixel-edge rectangles, inverts
//! destination centres in binary64, and uses `sampling::Kernel::weights_f64` through
//! `sampling_walk::warp_into`; existing affine/projective arithmetic is untouched. Identity is a
//! no-op, common integral translation shares original allocations, and filtered re-edits always
//! sample the retained originals. Pixel-target linked masks ride; unlinked masks stay; mask-only
//! works on raster/group/adjustment masks. Flat/PSD preflight discloses loss of editability.
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::precision_image::PrecisionImage;
use crate::precision_mask::PrecisionPlane;
use crate::sampling::Kernel;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WarpTarget {
    Pixels,
    Mask,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WarpFilter {
    Bilinear,
    Bicubic,
}

impl WarpFilter {
    pub(crate) fn kernel(self) -> Kernel {
        match self {
            Self::Bilinear => Kernel::Bilinear,
            Self::Bicubic => Kernel::Bicubic,
        }
    }
}

#[derive(Clone, Debug)]
pub struct WarpField {
    pub columns: u32,
    pub rows: u32,
    pub displacements: Arc<Vec<[f64; 2]>>,
    pub filter: WarpFilter,
}

#[derive(Clone)]
pub struct WarpBinding<P, M> {
    pub field: Arc<WarpField>,
    pub target: WarpTarget,
    pub source_pixels: Option<P>,
    pub source_offset: (i32, i32),
    pub source_mask: Option<M>,
    pub source_mask_offset: (i32, i32),
    pub rendered_pixels: Option<P>,
    pub rendered_offset: (i32, i32),
    pub rendered_mask: Option<M>,
    pub rendered_mask_offset: (i32, i32),
    pub profile: Arc<IccProfile>,
    pub linked_mask: bool,
    pub mask_outside: u8,
}

pub type WarpBinding8 = WarpBinding<crate::layer_raster::LayerRaster, Arc<image::GrayImage>>;
pub type WarpBinding16 = WarpBinding<PrecisionImage, PrecisionPlane>;

impl WarpBinding8 {
    pub(crate) fn record_resources(&self, inventory: &mut crate::resources::Inventory) {
        inventory.object(&*self.field, "warp_lattice", false);
        inventory.object(&*self.field.displacements, "warp_lattice", false);
        inventory.vec(&self.field.displacements, "warp_lattice", false);
        for p in [&self.source_pixels, &self.rendered_pixels]
            .into_iter()
            .flatten()
        {
            p.record_resources(inventory, "warp_pixels", false);
        }
        for m in [&self.source_mask, &self.rendered_mask]
            .into_iter()
            .flatten()
        {
            inventory.object(&**m, "shared_buffer_structure", false);
            inventory.vec(m.as_raw(), "warp_masks", false);
        }
    }
}
impl WarpBinding16 {
    pub(crate) fn record_resources(&self, inventory: &mut crate::resources::Inventory) {
        inventory.object(&*self.field, "warp_lattice", false);
        inventory.object(&*self.field.displacements, "warp_lattice", false);
        inventory.vec(&self.field.displacements, "warp_lattice", false);
        for p in [&self.source_pixels, &self.rendered_pixels]
            .into_iter()
            .flatten()
        {
            p.record_resources(inventory);
        }
        for m in [&self.source_mask, &self.rendered_mask]
            .into_iter()
            .flatten()
        {
            m.record_resources(inventory);
        }
    }
}

#[derive(Clone, Debug)]
pub enum WarpEdit {
    Set {
        layer: usize,
        target: WarpTarget,
        field: WarpField,
        /// The background colour a clear pixel's entry is nearest on an
        /// Indexed document with no transparent index (#1 Indexed phase
        /// 3b, `indexed_paint::BACKGROUND_KEY`); read only there, ignored
        /// on every other document.
        indexed_background: Option<crate::precision_geometry::CropBackground>,
    },
    Clear {
        layer: usize,
    },
    Bake {
        layer: usize,
    },
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum WireEdit {
    Set {
        layer: usize,
        target: WarpTarget,
        columns: u32,
        rows: u32,
        displacements: Vec<[f64; 2]>,
        filter: WarpFilter,
        #[serde(default)]
        indexed_background: Option<crate::precision_geometry::CropBackground>,
    },
    Clear {
        layer: usize,
    },
    Bake {
        layer: usize,
    },
}

impl WarpEdit {
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > 64 * 1024 {
            return Err("warp request exceeds 64 KiB".into());
        }
        let wire: WireEdit =
            serde_json::from_str(text).map_err(|e| format!("invalid warp request: {e}"))?;
        Ok(match wire {
            WireEdit::Set {
                layer,
                target,
                columns,
                rows,
                displacements,
                filter,
                indexed_background,
            } => {
                let field = WarpField {
                    columns,
                    rows,
                    displacements: Arc::new(displacements),
                    filter,
                };
                field.validate()?;
                Self::Set {
                    layer,
                    target,
                    field,
                    indexed_background,
                }
            }
            WireEdit::Clear { layer } => Self::Clear { layer },
            WireEdit::Bake { layer } => Self::Bake { layer },
        })
    }

    pub fn layer(&self) -> usize {
        match self {
            Self::Set { layer, .. } | Self::Clear { layer } | Self::Bake { layer } => *layer,
        }
    }
}

impl WarpField {
    pub fn validate(&self) -> Result<(), String> {
        if !(2..=16).contains(&self.columns) || !(2..=16).contains(&self.rows) {
            return Err("warp lattice dimensions must each be in 2...16".into());
        }
        if self.displacements.len() != (self.columns * self.rows) as usize {
            return Err("warp displacement count differs from lattice dimensions".into());
        }
        if self
            .displacements
            .iter()
            .flat_map(|v| v.iter())
            .any(|v| !v.is_finite() || v.abs() > 30_000.0)
        {
            return Err("warp displacement must be finite and within 30000 pixels".into());
        }
        Ok(())
    }

    pub(crate) fn normalize(&mut self) {
        Arc::make_mut(&mut self.displacements)
            .iter_mut()
            .for_each(|pair| {
                pair.iter_mut().for_each(|x| {
                    if *x == 0.0 {
                        *x = 0.0
                    }
                })
            });
    }

    pub(crate) fn identity(&self) -> bool {
        self.displacements
            .iter()
            .all(|v| v[0] == 0.0 && v[1] == 0.0)
    }

    pub(crate) fn translation(&self) -> Option<(i32, i32)> {
        let first = self.displacements.first()?;
        if first[0].fract() != 0.0 || first[1].fract() != 0.0 {
            return None;
        }
        if self.displacements.iter().all(|v| *v == *first) {
            Some((first[0] as i32, first[1] as i32))
        } else {
            None
        }
    }

    pub(crate) fn equivalent(&self, other: &Self) -> bool {
        self.columns == other.columns
            && self.rows == other.rows
            && self.filter == other.filter
            && self.displacements == other.displacements
    }

    pub(crate) fn node(&self, x: usize, y: usize) -> [f64; 2] {
        self.displacements[y * self.columns as usize + x]
    }

    /// The row-sum bound holds everywhere in each bilinear cell because each
    /// partial derivative is affine along its perpendicular coordinate.
    pub(crate) fn validate_for_rect(&self, rect: (i32, i32, u32, u32)) -> Result<(), String> {
        self.validate()?;
        let sx = (self.columns - 1) as f64 / f64::from(rect.2);
        let sy = (self.rows - 1) as f64 / f64::from(rect.3);
        for y in 0..self.rows as usize - 1 {
            for x in 0..self.columns as usize - 1 {
                let a = self.node(x, y);
                let b = self.node(x + 1, y);
                let c = self.node(x, y + 1);
                let d = self.node(x + 1, y + 1);
                for component in 0..2 {
                    let dx = (b[component] - a[component])
                        .abs()
                        .max((d[component] - c[component]).abs())
                        * sx;
                    let dy = (c[component] - a[component])
                        .abs()
                        .max((d[component] - b[component]).abs())
                        * sy;
                    if dx + dy > 0.5 {
                        return Err("warp field exceeds the non-folding contraction bound".into());
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn displacement(&self, rect: (i32, i32, u32, u32), p: [f64; 2]) -> [f64; 2] {
        let x = ((p[0] - f64::from(rect.0)) / f64::from(rect.2) * f64::from(self.columns - 1))
            .clamp(0.0, f64::from(self.columns - 1));
        let y = ((p[1] - f64::from(rect.1)) / f64::from(rect.3) * f64::from(self.rows - 1))
            .clamp(0.0, f64::from(self.rows - 1));
        let ix = (x.floor() as usize).min(self.columns as usize - 2);
        let iy = (y.floor() as usize).min(self.rows as usize - 2);
        let tx = x - ix as f64;
        let ty = y - iy as f64;
        let a = self.node(ix, iy);
        let b = self.node(ix + 1, iy);
        let c = self.node(ix, iy + 1);
        let d = self.node(ix + 1, iy + 1);
        [
            (a[0] * (1.0 - tx) + b[0] * tx) * (1.0 - ty) + (c[0] * (1.0 - tx) + d[0] * tx) * ty,
            (a[1] * (1.0 - tx) + b[1] * tx) * (1.0 - ty) + (c[1] * (1.0 - tx) + d[1] * tx) * ty,
        ]
    }

    pub(crate) fn inverse(
        &self,
        rect: (i32, i32, u32, u32),
        destination: [f64; 2],
        job: JobContext<'_>,
    ) -> Result<[f64; 2], String> {
        let mut p = destination;
        for i in 0..64 {
            job.every(i)?;
            let d = self.displacement(rect, p);
            let next = [destination[0] - d[0], destination[1] - d[1]];
            let residual = [
                (next[0] + self.displacement(rect, next)[0] - destination[0]).abs(),
                (next[1] + self.displacement(rect, next)[1] - destination[1]).abs(),
            ];
            if residual[0].max(residual[1]) <= 1e-10 {
                return Ok(next);
            }
            p = next;
        }
        Err("warp inverse did not converge within 64 iterations".into())
    }

    pub(crate) fn bounds(
        &self,
        rect: (i32, i32, u32, u32),
    ) -> Result<(i32, i32, u32, u32), String> {
        self.validate_for_rect(rect)?;
        let mut min = [f64::INFINITY; 2];
        let mut max = [f64::NEG_INFINITY; 2];
        for y in 0..self.rows {
            for x in 0..self.columns {
                let p = [
                    f64::from(rect.0)
                        + f64::from(rect.2) * f64::from(x) / f64::from(self.columns - 1),
                    f64::from(rect.1) + f64::from(rect.3) * f64::from(y) / f64::from(self.rows - 1),
                ];
                let d = self.node(x as usize, y as usize);
                for c in 0..2 {
                    min[c] = min[c].min(p[c] + d[c]);
                    max[c] = max[c].max(p[c] + d[c]);
                }
            }
        }
        let low = [min[0].floor(), min[1].floor()];
        let high = [max[0].ceil(), max[1].ceil()];
        if low
            .iter()
            .chain(high.iter())
            .any(|v| !v.is_finite() || *v < i32::MIN as f64 || *v > i32::MAX as f64)
        {
            return Err("warp destination exceeds signed coordinate bounds".into());
        }
        let w = high[0] - low[0];
        let h = high[1] - low[1];
        if w < 1.0 || h < 1.0 || w * h > crate::doc::MAX_PIXELS as f64 {
            return Err("warp destination exceeds the native pixel limit".into());
        }
        Ok((low[0] as i32, low[1] as i32, w as u32, h as u32))
    }

    pub(crate) fn json(
        &self,
        target: WarpTarget,
        rect: (i32, i32, u32, u32),
        linked: bool,
    ) -> Value {
        serde_json::json!({"target":target,"columns":self.columns,"rows":self.rows,
            "displacements":&*self.displacements,"filter":self.filter,
            "source_rect":[rect.0,rect.1,rect.2 as i64,rect.3 as i64],"linked_mask":linked})
    }
}
