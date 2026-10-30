//! DRDC26: an outer u32 version before the prior typed version/body, followed
//! after all prior tails by LBLD, u32 schema=1, u32 count, then strictly ascending
//! (u32 postorder index, u32 BlendMode) pairs. Only Multiply=1 and Screen=2 are
//! stored. DRDC27 uses schema2, adding exactly Overlay=3, SoftLight=4,
//! HardLight=5, Darken=6, Lighten=7, Difference=8 and Exclusion=9. Its writer
//! is conditional on an added mode; old-only records still write26/schema1.
//! DRDC30/schema3 adds only ColorDodge=10, ColorBurn=11, Addition=12 and
//! LinearBurn=15. Earlier schemas have frozen whitelists, independent of
//! current edit eligibility. DRDC31/schema4 adds only Subtract=13, VividLight=18,
//! LinearLight=19, PinLight=20 and Divide=22. DRDC33/schema5 adds only the
//! nonseparable Hue=23, Saturation=24, Color=25 and Luminosity=26 through the
//! unchanged f32 `blend.rs` NonSeparable kernel. DRDC34/schema6 adds only the
//! threshold modes Dissolve=14, DarkerColor=16, LighterColor=17 and HardMix=21
//! through the unchanged f32 kernels under the v34 strong/weak band policy
//! (designs/phase-10d-threshold-modes-status.md).
//! DRDC37/schema7 adds NO mode: its whitelist is exactly schema6's twenty-six.
//! What it adds is a TARGET — an adjustment record (kind 3) may carry a mode,
//! evaluated by `precision_render.rs` as the byte
//! `doc.rs::composite_adjustment_into` rule
//! (designs/phase-10d-adjustment-blending-status.md). 37 is required only when
//! some nondefault record is an adjustment; a raster/group-only document keeps
//! the oldest sufficient of 26/27/30/31/33/34. Schemas 1-6 keep refusing a
//! kind-3 target, so every prior file's meaning is unchanged.
//! The writer uses the oldest applicable wrapper.
//! Normal and group-only Pass Through retain the prior record meaning.
//! All schemas use encoded document RGB, existing W3C f32 source-over, native mask
//! coverage and opacity once at each isolated boundary, and final u16 storage.
//! No transfer curve or intermediate group quantization is introduced. Unknown
//! schema/mode, duplicate/unordered/out-of-range references and incompatible
//! hidden records refuse during borrowed preflight, before PNG decoding.
//!
//! Conditional native v26/schema1 retains Multiply/Screen-only meaning; v27/schema2 adds exactly
//! Overlay/SoftLight/HardLight/Darken/Lighten/Difference/Exclusion. Conditional v30/schema3 adds
//! only ColorDodge10/ColorBurn11/Addition12/LinearBurn15; v31/schema4 adds exactly
//! Subtract13/VividLight18/LinearLight19/PinLight20/Divide22; v33/schema5 adds exactly
//! Hue23/Saturation24/Color25/Luminosity26; v34/schema6 adds exactly
//! Dissolve14/DarkerColor16/LighterColor17/HardMix21; all six schema whitelists are explicitly
//! frozen. The threshold modes are admitted under the v34 strong/weak band policy
//! (`designs/phase-10d-threshold-modes-status.md`: the oracle's branch is guaranteed only for
//! direct inputs; composited inputs are bound to one branch, never a mixture) — still never by
//! widening an RGB tolerance. They wrap compatible older typed bodies and v25 channel properties
//! with strict ordered `LBLD` tables. Mask29 is outermost when outside255 is present, with MOUT
//! after LBLD.
use crate::blend::BlendMode;
use crate::drdc::Reader;
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_layer_blend::{is_nondefault, BlendTarget};
use crate::precision_native::append;

pub const BASIC_VERSION: u32 = 26;
pub const SEPARABLE_VERSION: u32 = 27;
pub const FOUR_MODE_VERSION: u32 = 30;
pub const FIVE_MODE_VERSION: u32 = 31;
pub const HSL_VERSION: u32 = 33;
pub const THRESHOLD_VERSION: u32 = 34;
/// Schema 7: schema6's twenty-six modes, now also on adjustment records.
pub const ADJUSTMENT_VERSION: u32 = 37;
// Persisted meanings are immutable: never derive an older schema's whitelist
// from the current editable mode set when admitting a new feature.
const SCHEMA1_MODES: [BlendMode; 2] = [BlendMode::Multiply, BlendMode::Screen];
const SCHEMA2_MODES: [BlendMode; 9] = [
    BlendMode::Multiply,
    BlendMode::Screen,
    BlendMode::Overlay,
    BlendMode::SoftLight,
    BlendMode::HardLight,
    BlendMode::Darken,
    BlendMode::Lighten,
    BlendMode::Difference,
    BlendMode::Exclusion,
];
const SCHEMA3_ADDITIONS: [BlendMode; 4] = [
    BlendMode::ColorDodge,
    BlendMode::ColorBurn,
    BlendMode::Addition,
    BlendMode::LinearBurn,
];
const SCHEMA4_ADDITIONS: [BlendMode; 5] = [
    BlendMode::Subtract,
    BlendMode::VividLight,
    BlendMode::LinearLight,
    BlendMode::PinLight,
    BlendMode::Divide,
];
const SCHEMA5_ADDITIONS: [BlendMode; 4] = [
    BlendMode::Hue,
    BlendMode::Saturation,
    BlendMode::Color,
    BlendMode::Luminosity,
];
const SCHEMA6_ADDITIONS: [BlendMode; 4] = [
    BlendMode::Dissolve,
    BlendMode::DarkerColor,
    BlendMode::LighterColor,
    BlendMode::HardMix,
];
fn schema_allows(version: u32, mode: BlendMode) -> bool {
    match version {
        BASIC_VERSION => SCHEMA1_MODES.contains(&mode),
        SEPARABLE_VERSION => SCHEMA2_MODES.contains(&mode),
        FOUR_MODE_VERSION => SCHEMA2_MODES.contains(&mode) || SCHEMA3_ADDITIONS.contains(&mode),
        FIVE_MODE_VERSION => {
            SCHEMA2_MODES.contains(&mode)
                || SCHEMA3_ADDITIONS.contains(&mode)
                || SCHEMA4_ADDITIONS.contains(&mode)
        }
        HSL_VERSION => {
            SCHEMA2_MODES.contains(&mode)
                || SCHEMA3_ADDITIONS.contains(&mode)
                || SCHEMA4_ADDITIONS.contains(&mode)
                || SCHEMA5_ADDITIONS.contains(&mode)
        }
        THRESHOLD_VERSION | ADJUSTMENT_VERSION => {
            SCHEMA2_MODES.contains(&mode)
                || SCHEMA3_ADDITIONS.contains(&mode)
                || SCHEMA4_ADDITIONS.contains(&mode)
                || SCHEMA5_ADDITIONS.contains(&mode)
                || SCHEMA6_ADDITIONS.contains(&mode)
        }
        _ => false,
    }
}
pub(crate) fn version(document: &PrecisionDocument) -> Option<u32> {
    let mut version = None;
    for layer in (0..document.layer_count()).filter_map(|i| document.layer(i)) {
        if is_nondefault(layer.blend_mode()) {
            // An adjustment TARGET needs schema7 whatever its mode is; every
            // other record keeps the oldest schema its mode first appeared in.
            let required = if layer.blend_target() == BlendTarget::Adjustment {
                ADJUSTMENT_VERSION
            } else if SCHEMA1_MODES.contains(&layer.blend_mode()) {
                BASIC_VERSION
            } else if SCHEMA2_MODES.contains(&layer.blend_mode()) {
                SEPARABLE_VERSION
            } else if SCHEMA3_ADDITIONS.contains(&layer.blend_mode()) {
                FOUR_MODE_VERSION
            } else if SCHEMA4_ADDITIONS.contains(&layer.blend_mode()) {
                FIVE_MODE_VERSION
            } else if SCHEMA5_ADDITIONS.contains(&layer.blend_mode()) {
                HSL_VERSION
            } else {
                THRESHOLD_VERSION
            };
            version = Some(version.unwrap_or(0).max(required));
        }
    }
    version
}
fn schema(version: u32) -> Result<u32, String> {
    match version {
        BASIC_VERSION => Ok(1),
        SEPARABLE_VERSION => Ok(2),
        FOUR_MODE_VERSION => Ok(3),
        FIVE_MODE_VERSION => Ok(4),
        HSL_VERSION => Ok(5),
        THRESHOLD_VERSION => Ok(6),
        ADJUSTMENT_VERSION => Ok(7),
        _ => Err("unsupported native layer blend version".into()),
    }
}
fn count(document: &PrecisionDocument) -> usize {
    (0..document.layer_count())
        .filter_map(|i| document.layer(i))
        .filter(|l| is_nondefault(l.blend_mode()))
        .count()
}
pub(crate) fn header_bytes(document: &PrecisionDocument) -> u64 {
    let n = count(document);
    if n == 0 {
        0
    } else {
        16 + n as u64 * 8
    }
}
pub(crate) fn encode(
    document: &PrecisionDocument,
    out: &mut Vec<u8>,
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    let n = count(document);
    if n == 0 {
        return Ok(());
    }
    append(out, b"LBLD", bound, job)?;
    for value in [
        schema(version(document).ok_or("missing native blend version")?)?,
        n as u32,
    ] {
        append(out, &value.to_le_bytes(), bound, job)?;
    }
    for index in 0..document.layer_count() {
        job.check()?;
        let mode = document
            .layer(index)
            .ok_or("invalid native blend index")?
            .blend_mode();
        if is_nondefault(mode) {
            for value in [index as u32, mode.to_c() as u32] {
                append(out, &value.to_le_bytes(), bound, job)?;
            }
        }
    }
    Ok(())
}
pub(crate) fn parse(
    reader: &mut Reader<'_>,
    records: usize,
    version: u32,
    job: JobContext<'_>,
) -> Result<Vec<(usize, BlendMode)>, String> {
    if reader.take(4)? != b"LBLD" || reader.u32()? != schema(version)? {
        return Err("unsupported native layer blend schema".into());
    }
    let count = reader.u32()? as usize;
    if count == 0 || count > records {
        return Err("invalid native layer blend count".into());
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        job.check()?;
        let index = reader.u32()? as usize;
        let mode = i32::try_from(reader.u32()?)
            .ok()
            .and_then(BlendMode::from_c)
            .filter(|&mode| schema_allows(version, mode))
            .ok_or("unsupported native layer blend mode")?;
        if index >= records || out.last().is_some_and(|&(prior, _)| index <= prior) {
            return Err("invalid native layer blend reference order or index".into());
        }
        out.push((index, mode));
    }
    Ok(out)
}
