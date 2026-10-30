//! Image ▸ Mode ▸ Color Table (#1 Indexed phase 1c): an Indexed
//! document's palette and transparent index read, planned and replaced, and
//! the `.act` file read and written (`designs/indexed-mode.md`).
//!
//! Replacing the table keeps the index plane: each pixel keeps its index and
//! takes the new entry's colour, so the one "Index" layer is re-rendered
//! from the new policy ([`IndexedPolicy::with_table`]) and its record keeps
//! everything else (name, id, locks). A table shorter than the plane's
//! largest index plus one refuses, naming both. The `indexed_act` kernel
//! owns the file layout and the JSON. The re-render is
//! [`PrecisionDocument::retabled`], which Convert to Profile shares (#1
//! Indexed phase 3a, `indexed_profile`).

use std::sync::Arc;

use serde_json::{json, Value};

use crate::indexed_act::{encode_act, parse_act, table_from_json, table_json, ColorTable};
use crate::indexed_convert::TO_INDEXED_BYTES_PER_PIXEL;
use crate::indexed_document::IndexedPolicy;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{admit, BOOKKEEPING_BYTES};

/// The refusal on any document but an Indexed one.
const NOT_INDEXED: &str = "the document is not Indexed Color";

impl PrecisionDocument {
    /// The palette and transparent index,
    /// `{"colors":[[r,g,b],…],"transparent_index":n|null}`.
    pub fn indexed_color_table(&self) -> Result<Value, String> {
        let policy = self.indexed_policy().ok_or(NOT_INDEXED)?;
        Ok(table_json(&ColorTable {
            colors: policy.palette().to_vec(),
            transparent: policy.transparent(),
        }))
    }

    /// The table edit's working memory: the re-rendered layer's share of the
    /// conversion's per-pixel shape and bookkeeping (also Convert to
    /// Profile's re-render, `indexed_profile`).
    pub(crate) fn set_color_table_scratch(&self) -> u64 {
        let canvas = u64::from(self.width) * u64::from(self.height);
        canvas.saturating_mul(TO_INDEXED_BYTES_PER_PIXEL) + BOOKKEEPING_BYTES
    }

    /// The new policy `table` makes, or `None` when it changes nothing.
    fn table_policy(&self, table: &str) -> Result<Option<IndexedPolicy>, String> {
        let policy = self.indexed_policy().ok_or(NOT_INDEXED)?;
        let table = table_from_json(table)?;
        if table.colors == policy.palette() && table.transparent == policy.transparent() {
            return Ok(None);
        }
        policy.with_table(table.colors, table.transparent).map(Some)
    }

    /// `{"working_memory_plan_bytes","refusal","changes"}` for replacing the
    /// table with `table`; a table the plane cannot take (too short) or a
    /// non-Indexed document is the `refusal`, a malformed table the `Err`.
    pub fn set_color_table_plan(&self, table: &str) -> Result<Value, String> {
        table_from_json(table)?;
        let (refusal, changes) = match self.table_policy(table) {
            Ok(next) => (None, next.is_some()),
            Err(reason) => (Some(reason), false),
        };
        Ok(json!({
            "working_memory_plan_bytes": self.set_color_table_scratch(),
            "refusal": refusal,
            "changes": changes,
        }))
    }

    /// Image ▸ Mode ▸ Color Table (module doc); `Ok(None)` when `table` and
    /// its transparent index equal the document's.
    pub fn set_color_table(
        &self,
        table: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let Some(policy) = self.table_policy(table)? else {
            return Ok(None);
        };
        let scratch = self.set_color_table_scratch();
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        self.retabled(policy, job).map(Some)
    }

    /// This document's one layer re-rendered from `policy` in the document's
    /// profile, its record kept, and `policy` attached (module doc): the
    /// Color Table edit's rebuild, shared by Convert to Profile
    /// (`indexed_profile::convert`). The caller admits and reserves.
    pub(crate) fn retabled(
        &self,
        policy: IndexedPolicy,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let [layer] = self.root_layers() else {
            return Err("an Indexed Color document holds one layer".into());
        };
        let image = policy.render(self.profile(), job.range(0.0, 0.95))?;
        let mut layer = layer.clone();
        layer.content = Content::Raster {
            image,
            offset: (0, 0),
        };
        let out = self
            .without_indexed()
            .rebuilt(
                self.width,
                self.height,
                vec![layer],
                self.channels().to_vec(),
            )?
            .with_indexed(Arc::new(policy))?;
        job.report(1.0, 5)?;
        Ok(out)
    }
}

/// The table in the `.act` file at `path`, as [`PrecisionDocument::indexed_color_table`]
/// answers it.
pub fn read_act(path: &str) -> Result<Value, String> {
    let length = std::fs::metadata(path)
        .map_err(|e| format!("failed to read {path}: {e}"))?
        .len();
    if length > crate::indexed_act::ACT_FILE_BYTES as u64 {
        return Err(format!(
            "failed to read {path}: a colour table file is 768 or 772 bytes, not {length}"
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("failed to read {path}: {e}"))?;
    Ok(table_json(&parse_act(&bytes)?))
}

/// `table` (the JSON [`read_act`] answers) written to `path` as a 772-byte
/// `.act`.
pub fn write_act(path: &str, table: &str) -> Result<(), String> {
    let table = table_from_json(table)?;
    std::fs::write(path, encode_act(&table)).map_err(|e| format!("failed to write {path}: {e}"))
}
