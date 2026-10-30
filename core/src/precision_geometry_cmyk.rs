//! The CMYK arm of typed geometry (#1 phase 4b), a `#[path]` child of
//! `precision_geometry`: one planned raster plane of a CMYK document's
//! straight CMYKA16 ink, executed through `precision_sampling`'s CMYK twins
//! (`resize_cmyk`, `transform_cmyk`) of the RGBA16 `PlaneWork::image`, its
//! master. The plan (`precision_geometry_plan::work` over five planes,
//! 10 B/px output, the CMYK source and adoption copies) is the same plan.
use super::plan::{PlaneWork, WorkKind};
use crate::cmyk_image::CmykImage;
use crate::job_control::JobContext;
use crate::precision_sampling;

impl PlaneWork {
    /// `PlaneWork::image`'s CMYK twin: `Keep` shares the input allocation.
    pub(super) fn cmyk_image(
        &self,
        source: &CmykImage,
        job: JobContext<'_>,
    ) -> Result<CmykImage, String> {
        match self.kind {
            WorkKind::Keep => Ok(source.clone()),
            WorkKind::Resize(filter) => {
                precision_sampling::resize_cmyk(source, self.dest.2, self.dest.3, filter, job)
            }
            WorkKind::Affine { filter, .. } | WorkKind::Perspective { filter, .. } => {
                let map = self.map().ok_or("typed geometry plan has no source map")?;
                precision_sampling::transform_cmyk(
                    source,
                    (self.dest.2, self.dest.3),
                    &map,
                    filter,
                    job,
                )
            }
        }
    }
}
