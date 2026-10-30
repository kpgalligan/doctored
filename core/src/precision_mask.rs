//! Immutable native u16 coverage for typed masks and channels. Raw coverage is
//! independent of raster dimensions. Feather shares the legacy blur schedule;
//! effective density/feather is f32 and never overwrites the raw source.
//!
//! Effective mask inspection ignores the enabled flag; rendering bypasses disabled masks. Float
//! feather shares `plane_blur.rs` with legacy byte masks/styles; no retained typed blur cache.
use crate::doc_mask::{density_coverage, MaskProperties};
use crate::drdc::MAX_DRDC_NAME_LEN;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_image::{
    admit, pixel_count, process_working_limit, reserved_vec, BOOKKEEPING_BYTES,
};
use crate::resources::Inventory;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

#[derive(Clone)]
pub struct PrecisionPlane {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Arc<Vec<u16>>,
}
impl PrecisionPlane {
    /// Adopt an owned native-endian plane. No byte promotion or allocation copy.
    pub fn from_u16(
        width: u32,
        height: u32,
        pixels: Vec<u16>,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let out = Self::adopt(width, height, pixels)?;
        admit(
            out.storage_bytes(),
            BOOKKEEPING_BYTES,
            0,
            process_working_limit(),
        )?;
        job.reserve(BOOKKEEPING_BYTES)?;
        job.report(1.0, 5)?;
        Ok(out)
    }
    pub(crate) fn adopt(width: u32, height: u32, pixels: Vec<u16>) -> Result<Self, String> {
        if pixels.len() != pixel_count(width, height)? {
            return Err("precision plane length differs from dimensions".into());
        }
        Ok(Self {
            width,
            height,
            pixels: Arc::new(pixels),
        })
    }
    pub(crate) fn copy(
        width: u32,
        height: u32,
        pixels: &[u16],
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        if pixels.len() != pixel_count(width, height)? {
            return Err("precision plane length differs from dimensions".into());
        }
        let mut out = reserved_vec(pixels.len())?;
        for chunk in pixels.chunks(4096) {
            job.check()?;
            out.extend_from_slice(chunk);
        }
        job.check()?;
        Self::adopt(width, height, out)
    }
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    pub fn samples(&self) -> &[u16] {
        &self.pixels
    }
    pub fn storage_bytes(&self) -> u64 {
        self.pixels.capacity() as u64 * 2
    }
    pub fn retained_bytes(&self) -> u64 {
        let mut inventory = Inventory::default();
        self.record_resources(&mut inventory);
        inventory.unique_bytes()
    }
    pub(crate) fn record_resources(&self, i: &mut Inventory) {
        i.object(&*self.pixels, "precision_plane", false);
        i.vec(&self.pixels, "precision_plane", false);
    }
    pub(crate) fn equals(&self, other: &Self, job: JobContext<'_>) -> Result<bool, String> {
        if self.dimensions() != other.dimensions() {
            return Ok(false);
        }
        if Arc::ptr_eq(&self.pixels, &other.pixels) {
            return Ok(true);
        }
        for (a, b) in self
            .samples()
            .chunks(4096)
            .zip(other.samples().chunks(4096))
        {
            job.check()?;
            if a != b {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
#[derive(Clone)]
pub struct PrecisionMask {
    pub(crate) plane: PrecisionPlane,
    pub(crate) properties: MaskProperties,
    pub(crate) enabled: bool,
}
impl PrecisionMask {
    pub fn new(
        plane: PrecisionPlane,
        properties: MaskProperties,
        enabled: bool,
    ) -> Result<Self, String> {
        if !properties.valid(plane.dimensions()) {
            return Err("invalid typed mask properties or padded extent".into());
        }
        // A supplied legacy property value may own byte caches. Typed masks
        // copy only its semantic fields, so no byte-derived cache crosses over.
        let mut clean = MaskProperties::at(properties.offset);
        clean.linked = properties.linked;
        clean.density = properties.density;
        clean.feather = properties.feather;
        clean.outside_coverage = properties.outside_coverage;
        Ok(Self {
            plane,
            properties: clean,
            enabled,
        })
    }
    pub fn plane(&self) -> &PrecisionPlane {
        &self.plane
    }
    pub fn properties(&self) -> &MaskProperties {
        &self.properties
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn with_properties(
        &self,
        properties: MaskProperties,
        enabled: bool,
    ) -> Result<Self, String> {
        Self::new(self.plane.clone(), properties, enabled)
    }
    pub(crate) fn record_resources(&self, i: &mut Inventory) {
        self.plane.record_resources(i);
    }
    pub(crate) fn plan(&self) -> Result<(u64, u64), String> {
        if !self.enabled || self.properties.feather == 0.0 {
            return Ok((0, 0));
        }
        let (w, h, _) = self
            .properties
            .padded_dimensions(self.plane.dimensions())
            .ok_or("invalid typed mask feather extent")?;
        let (scratch, mut work) = crate::plane_blur::float_plan(w, h, self.properties.sigma())?;
        if self.properties.outside_coverage == 255 {
            work += u64::from(w) * u64::from(h) * 2;
        }
        Ok((scratch, work))
    }
    pub(crate) fn prepare(&self, job: JobContext<'_>) -> Result<PreparedMask<'_>, String> {
        job.check()?;
        let mut out = PreparedMask {
            mask: self,
            blurred: None,
        };
        if !self.enabled || self.properties.feather == 0.0 {
            return Ok(out);
        }
        let (w, h, pad) = self
            .properties
            .padded_dimensions(self.plane.dimensions())
            .ok_or("invalid typed mask feather extent")?;
        self.plan()?;
        let mut values = reserved_vec(pixel_count(w, h)?)?;
        while values.len() < w as usize * h as usize {
            job.check()?;
            values.extend(std::iter::repeat_n(
                f32::from(self.properties.outside_coverage) / 255.0,
                (w as usize * h as usize - values.len()).min(4096),
            ));
        }
        for y in 0..self.plane.height {
            for x in 0..self.plane.width {
                let i = y as usize * self.plane.width as usize + x as usize;
                job.every(i)?;
                values[(y + pad) as usize * w as usize + (x + pad) as usize] =
                    f32::from(self.plane.pixels[i]) / 65535.0;
            }
        }
        // Blur the finite complement, because the shared reduced-resolution
        // workspace deliberately zero-extends; preserve a constant white far field.
        if self.properties.outside_coverage == 255 {
            for chunk in values.chunks_mut(4096) {
                job.check()?;
                for v in chunk {
                    *v = 1.0 - *v;
                }
            }
        }
        job.report(0.2, 6)?;
        crate::plane_blur::floats(
            &mut values,
            w,
            h,
            self.properties.sigma(),
            job.range(0.2, 0.95),
        )?;
        if self.properties.outside_coverage == 255 {
            for chunk in values.chunks_mut(4096) {
                job.check()?;
                for v in chunk {
                    *v = 1.0 - *v;
                }
            }
        }
        out.blurred = Some((values, w, h, pad));
        job.check()?;
        Ok(out)
    }
}
pub(crate) struct PreparedMask<'a> {
    mask: &'a PrecisionMask,
    blurred: Option<(Vec<f32>, u32, u32, u32)>,
}
impl PreparedMask<'_> {
    pub(crate) fn at(&self, x: i64, y: i64) -> f32 {
        if !self.mask.enabled {
            return 1.0;
        }
        let properties = &self.mask.properties;
        let px = x - i64::from(properties.offset.0);
        let py = y - i64::from(properties.offset.1);
        let outside = f32::from(properties.outside_coverage) / 255.0;
        let coverage = if let Some((values, w, h, pad)) = &self.blurred {
            let px = px + i64::from(*pad);
            let py = py + i64::from(*pad);
            if px >= 0 && py >= 0 && px < i64::from(*w) && py < i64::from(*h) {
                values[py as usize * *w as usize + px as usize]
            } else {
                outside
            }
        } else if px >= 0
            && py >= 0
            && px < i64::from(self.mask.plane.width)
            && py < i64::from(self.mask.plane.height)
        {
            f32::from(
                self.mask.plane.pixels[py as usize * self.mask.plane.width as usize + px as usize],
            ) / 65535.0
        } else {
            outside
        };
        density_coverage(coverage, properties.density, 1.0)
    }
}
static NEXT_CHANNEL_ID: AtomicU64 = AtomicU64::new(1);
#[derive(Clone)]
pub struct PrecisionChannel {
    pub(crate) id: u64,
    pub(crate) name: Arc<String>,
    pub(crate) plane: PrecisionPlane,
    pub(crate) metadata: crate::precision_channel_metadata::ChannelMetadata,
}
impl PrecisionChannel {
    pub fn new(name: &str, plane: PrecisionPlane) -> Result<Self, String> {
        if name.len() > MAX_DRDC_NAME_LEN as usize {
            return Err("typed channel name exceeds native limit".into());
        }
        Ok(Self {
            id: NEXT_CHANNEL_ID.fetch_add(1, Ordering::Relaxed),
            name: Arc::new(name.into()),
            plane,
            metadata: Default::default(),
        })
    }
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn plane(&self) -> &PrecisionPlane {
        &self.plane
    }
    pub fn metadata(&self) -> &crate::precision_channel_metadata::ChannelMetadata {
        &self.metadata
    }
    pub(crate) fn duplicate(&self) -> Self {
        let mut next = self.clone();
        next.id = NEXT_CHANNEL_ID.fetch_add(1, Ordering::Relaxed);
        next
    }
    pub(crate) fn record_resources(&self, i: &mut Inventory) {
        i.object(&*self.name, "precision_channel", false);
        i.string(&self.name, "precision_channel", false);
        if let Some(text) = &self.metadata.provenance {
            i.object(&**text, "precision_channel_provenance", false);
            i.string(text, "precision_channel_provenance", false);
        }
        self.plane.record_resources(i);
    }
}
