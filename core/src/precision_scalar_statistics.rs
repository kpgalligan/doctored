//! Raw scalar histograms use the shared exact coverage-weighted statistics.
//! No RGBA image or byte preview is materialized: a scalar is virtual opaque
//! gray for the common counter. Selection is mapped from canvas to raw storage;
//! absent selection measures the complete raw extent, including off-canvas data.
//! A 32-bit float document's masks and channels are u16 coverage and measure
//! exactly as at 16 bits; its red/green/blue/alpha are f32 samples, not u16
//! codes, so they refuse at plan, before any admission.
//!
//! Raw scalar statistics share the scoped counter without an RGBA allocation, map native canvas
//! weights to raw storage, and retain complete exact endpoints/code sums independently of display
//! stride; `precision_scalar_statistics.rs` FFI tests cover every scalar target, placement,
//! cancellation, invalid lengths and resource refusal. At 32-bit float, masks and channels (u16
//! coverage) measure exactly as at 16 bits, while red/green/blue/alpha refuse at plan by name,
//! before any admission — `ScalarStatistics::plan` first, then `PlaneRead::plan` for every
//! `read_plane` caller.
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, reserved_vec};
use crate::precision_plane_read::PlaneRead;
use crate::scoped_statistics::{AnalysisRequest, STATISTICS_SCRATCH};
use crate::selected_adjustment::coverage_at;
use serde::Deserialize;
use serde_json::{json, Value};

const FLOAT_COMPONENT_REFUSAL: &str = "scalar statistics of red, green, blue or alpha are unavailable in a 32-bit float document: its raster components are float samples, not u16 codes; its masks and channels are u16 coverage and are measured as at 16 bits";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScalarStatistics {
    op: String,
    target: String,
    index: usize,
    #[serde(default)]
    selection: bool,
    #[serde(default = "one")]
    stride: u32,
}
fn one() -> u32 {
    1
}

impl ScalarStatistics {
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > 4096 {
            return Err("scalar statistics request exceeds4096 bytes".into());
        }
        let value: Self =
            serde_json::from_str(text).map_err(|e| format!("invalid scalar statistics: {e}"))?;
        if value.op != "scalar_statistics"
            || value.stride == 0
            || !matches!(
                value.target.as_str(),
                "mask"
                    | "channel"
                    | "red"
                    | "green"
                    | "blue"
                    | "alpha"
                    | "cyan"
                    | "magenta"
                    | "yellow"
                    | "black"
            )
        {
            return Err("scalar statistics needs a raw scalar target and positive stride".into());
        }
        Ok(value)
    }
    fn read(&self) -> Result<PlaneRead, String> {
        PlaneRead::parse(
            &json!({"op":"read_plane","target":self.target,"index":self.index}).to_string(),
        )
    }
    pub fn plan(&self, doc: &PrecisionDocument) -> Result<Value, String> {
        if doc.is_float() && matches!(self.target.as_str(), "red" | "green" | "blue" | "alpha") {
            return Err(FLOAT_COMPONENT_REFUSAL.into());
        }
        let read = self.read()?;
        let mut plan = read.plan(doc)?;
        let (w, h) = read.dimensions(doc)?;
        let (x, y) = read.offset(doc)?;
        let canvas = u64::from(doc.width) * u64::from(doc.height);
        let input = if self.selection { canvas } else { 0 };
        let mapped = if self.selection {
            u64::from(w) * u64::from(h) * 2
        } else {
            0
        };
        plan["scratch_bytes"] = json!(
            plan["scratch_bytes"]
                .as_u64()
                .ok_or("invalid scalar read plan")?
                + STATISTICS_SCRATCH
                + mapped
                + input * 2
        );
        plan["input_samples"] = json!(input);
        plan["input_width"] = json!(doc.width);
        plan["input_height"] = json!(doc.height);
        plan["input_planes"] = json!(u8::from(self.selection));
        plan["x"] = json!(x);
        plan["y"] = json!(y);
        plan.as_object_mut()
            .ok_or("invalid scalar plan")?
            .remove("output_samples");
        Ok(plan)
    }
    pub(crate) fn evaluate(
        &self,
        doc: &PrecisionDocument,
        input: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Value, String> {
        job.check()?;
        let plan = self.plan(doc)?;
        if input.len() as u64
            != plan["input_samples"]
                .as_u64()
                .ok_or("invalid scalar input plan")?
        {
            return Err("scalar statistics selection differs from canvas dimensions".into());
        }
        let scratch = plan["scratch_bytes"]
            .as_u64()
            .ok_or("invalid scalar scratch plan")?;
        admit(doc.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let job = job.with_reservation_floor(scratch);
        let read = self.read()?;
        let dimensions = read.dimensions(doc)?;
        let offset = read.offset(doc)?;
        let read_scratch = read.plan(doc)?["scratch_bytes"]
            .as_u64()
            .ok_or("invalid scalar read plan")?;
        let values = read.read_with_context(
            doc,
            limit,
            other + scratch - read_scratch,
            job.range(0., 0.4),
        )?;
        let mut coverage = reserved_vec(if self.selection { values.len() } else { 0 })?;
        if self.selection {
            for i in 0..values.len() {
                job.every(i)?;
                coverage.push(coverage_at(input, doc.dimensions(), dimensions, offset, i));
            }
        }
        let request = AnalysisRequest::parse(
            &json!({"operation":"histogram","source":"composite","stride":self.stride}).to_string(),
        )?;
        let mut result = crate::scoped_statistics::evaluate(
            &request,
            dimensions,
            u16::MAX,
            &coverage,
            |i| [values[i], values[i], values[i], u16::MAX],
            job.range(0.4, 1.),
        )?;
        result["source"] = json!("scalar");
        result["storage"] = json!("u16_scalar");
        result["target"] = json!(self.target);
        result["index"] = json!(self.index);
        result["scope"] = json!({"source":"scalar","target":self.target,"index":self.index,
            "width":dimensions.0,"height":dimensions.1,"x":offset.0,"y":offset.1,
            "semantics":"complete raw scalar storage; canvas coverage weights; shared RGB rows repeat scalar codes with virtual opaque alpha"});
        job.report(1., 5)?;
        Ok(result)
    }
}
