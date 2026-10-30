//! The typed document's GLOBAL LIGHT — the twin of the byte
//! `DrDocument::set_global_light` (`style.rs`), and nothing more.
//!
//! The light is document state: one angle/altitude pair that every style
//! effect with `use_global_light` reads. It reaches the typed render through
//! the scene (`PrecisionScene::with_light`, chained by
//! `PrecisionDocument::scene_for`) and the style extents through
//! `precision_style_render`'s `light` parameter, so a promoted document
//! renders bit-identically to its byte source. It persists in the native
//! `LGLT` table (`precision_light_native`), rides every rebuild, profile edit
//! and byte↔typed conversion, and round-trips through PSD16's 1037/1049
//! resources.
//!
//! **One sanitizer.** `GlobalLight::sane` (`style_model.rs`) — the byte
//! setter's, the DRDC reader's and the PSD reader's — so the two depths
//! cannot store one light two ways, and an echoed four-decimal value is
//! "no change" here exactly as it is there.
//!
//! **Float keeps it**. A 32-bit float document holds layer
//! styles, so it holds the light too (measured against Photoshop 26.10.0,
//! which sets a 32-bit document's light and reads it back under a
//! `use_global_light` drop shadow: `scripts/photoshop-probe/float_styles.py`),
//! and the v40 file carries its `LGLT` table like any other.
//!
//! The request is `{"op":"global_light","angle":<number>,"altitude":<number>}`
//! through `dr_precision_doc_edit` — the `layer_open` precedent: no new
//! export. Both numbers are required and an unknown field refuses.
//!
//! `PrecisionDocument.global_light` (the byte `DrDocument.global_light` twin, default 120°/30°),
//! `global_light()` and the pure `with_global_light` (one sanitizer, `GlobalLight::sane`; a
//! non-finite component or an unchanged light is `Ok(None)`, at every depth),
//! reached as the strict `{op:"global_light",angle,altitude}` request through
//! `dr_precision_doc_edit` (family `global_light`, target `document`, in the float table) and
//! read as `dr_precision_doc_info`'s `global_light` object. The scene carries it
//! (`PrecisionScene::with_light`, chained in `scene_for`) and the style extents take it
//! (`style_extent16_of`, `bake_style16_scratch`), so a promoted document renders bit-identically to
//! its byte source. Carried by every rebuild, profile edit, 8↔16 conversion (`global_light` is
//! PRESERVED; flatten too), `float_to_u16` and 16-bit PSD import/export (1037/1049).

use serde::Deserialize;

use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, BOOKKEEPING_BYTES};
use crate::style::GlobalLight;

impl PrecisionDocument {
    /// The document's global light; the default 120°/30° unless set.
    pub fn global_light(&self) -> GlobalLight {
        self.global_light
    }

    /// Pure setter, at every depth (a 32-bit float document keeps the light
    /// too, measured against Photoshop 26.10.0, which sets
    /// and reads it back at 32 bits under a `use_global_light` drop shadow).
    /// `Ok(None)` for a non-finite component or a light that is unchanged after
    /// `GlobalLight::sane` — never an identical copy.
    pub fn with_global_light(&self, light: GlobalLight) -> Result<Option<Self>, String> {
        if !light.angle.is_finite() || !light.altitude.is_finite() {
            return Ok(None);
        }
        let light = light.sane();
        if light == self.global_light {
            return Ok(None);
        }
        let mut next = self.clone();
        next.global_light = light;
        Ok(Some(next))
    }
}

/// `{op:"global_light",angle,altitude}`, parsed strictly.
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum GlobalLightEdit {
    GlobalLight { angle: f32, altitude: f32 },
}

impl GlobalLightEdit {
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|e| format!("invalid native global light request: {e}"))
    }

    /// Bookkeeping only: the light holds no pixels and the result shares
    /// every allocation with its input.
    pub(crate) fn scratch_bytes(&self, _document: &PrecisionDocument) -> Result<u64, String> {
        Ok(4 * BOOKKEEPING_BYTES)
    }

    pub(crate) fn apply(
        &self,
        document: &PrecisionDocument,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<PrecisionDocument>, String> {
        let scratch = self.scratch_bytes(document)?;
        admit(document.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        job.check()?;
        let Self::GlobalLight { angle, altitude } = *self;
        document.with_global_light(GlobalLight { angle, altitude })
    }
}
