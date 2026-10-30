//! ICC LOOKS for Color Lookup: a device-link or abstract ICC
//! profile baked into the ordinary 3D `color_lookup` table, so the live
//! layer, the destructive filter, 8 bits and 16 bits all run the existing
//! `adjust_lut` path and nothing downstream knows a profile was involved.
//!
//! # What loads
//!
//! Two classes, which between them cover every look Adobe ships in its Color
//! Profiles folder that this build can read:
//!
//! * a DEVICE LINK (`link`) from `RGB ` to `RGB `. Its `A2B0` maps the
//!   document's own numbers straight to new numbers, so the table is the
//!   link sampled at each node — no profile is consulted.
//! * an ABSTRACT profile (`abst`) whose data colour space and PCS are each
//!   `Lab ` or `XYZ ` (all four combinations, `icc_lut::abstract_table`). Its
//!   `A2B0` edits colours in the PCS, so it is baked THROUGH THE DOCUMENT'S
//!   PROFILE, relative colorimetric: each node goes RGB → PCS XYZ (→ Lab when
//!   the input side is Lab) → abstract → (Lab → XYZ when the output side is
//!   Lab) → PCS XYZ → RGB (`icc_intent::DocumentPcs` for the two ends).
//!
//! Everything else is REFUSED BY NAME rather than half-believed: other
//! classes (a pixel profile is a colour space, not a look), a CMYK link, a
//! profile whose `A2B0` is missing (`missing_table_refusal` names the first
//! floating-point `D2B0..3` or `B2D0..3` it carries instead) or is an `mpet`
//! element, and an abstract with no readable document profile.
//!
//! # An abstract look keeps its profile, and is re-baked
//!
//! An Abstract look's profile is stored in the layer and the look is
//! evaluated through the document's CURRENT profile: an abstract
//! look's params carry [`ABSTRACT_KEY`], the standard base64 of the profile's
//! own bytes, and every profile write (Assign, Convert, a Don't-Convert
//! paste — `doc_look` at 8 bits, `PrecisionAdjustmentState::rebind` at 16)
//! runs [`rebake_meta`], which replaces the table through the new profile
//! and keeps every other key (`strength` among them). A link or a `.cube`
//! carries no such key and is never touched: its numbers are the document's
//! numbers. A re-bake that fails fails the whole profile write, naming the
//! look and the profile. The kept bytes are capped at
//! [`MAX_ABSTRACT_LOOK_BYTES`]; `.rz` word 71 (`look_native`) guards the key.
//!
//! # The table
//!
//! A device link whose `A2B0` is a PLAIN GRID (`icc_lut::LutPipeline::plain_grid`:
//! linear curves around one 3-input `n³` CLUT, no matrix) of at most 33
//! nodes is stored at its OWN grid, `n³` nodes at `k / (n − 1)`, red fastest,
//! each node the link's own entry — so the tetrahedral read of the stored
//! table is Photoshop's read of the link
//! (receipt
//! `float-adjust-473/take5`, runner `float_adjust_take5.py` sha `3d2ac9fb…`,
//! LG: `own` 0.048 tolerance units at 16 bits and 1.2e-4 at 32 on 6- and
//! 11-node links, where the 33-node bake missed by 31 and trilinear by 47).
//! Every other link (a larger grid, a non-linear curve, a matrix) and every
//! abstract is sampled at 33³ nodes at `k / 32` — `adjust_lut::MAX_STORED_3D`,
//! the largest 3D table a layer stores, so a look is sampled exactly as finely
//! as a resampled `.cube` would be. `source_size` is the larger of `size` and
//! the link's own CLUT grid (an abstract's is 33). `adjust_lut::lookup_params_json`
//! writes the params through the `.cube` writer, so the stored shape has one author.
//! The file is size-checked from its metadata before it is read, against
//! [`MAX_LOOKUP_PROFILE_BYTES`], and the table's own CLUT is charged against
//! `icc_lut::MAX_CLUT_BYTES`.
//!
//! The `A2B0` is parsed by `icc_lut::transform_table` (a link) or `icc_lut::abstract_table` (an
//! abstract, Lab or XYZ on either side); a link is sampled as it stands, an abstract is baked
//! through the DOCUMENT profile, relative colorimetric (`icc_intent::DocumentPcs` shares
//! `General::color`'s decode/encode), keeps its bytes under `abstract_profile` and is re-baked by
//! [`rebake_meta`] on every profile write.

use serde_json::Value;

use crate::adjust_lut::{base64_decode, base64_encode, lookup_params_json, MAX_STORED_3D};
use crate::icc::{IccBytes, IccProfile};
use crate::icc_encode_refusal::{own_profile_error, own_profile_sentence};
use crate::icc_intent::DocumentPcs;
use crate::icc_lut::{
    abstract_table, lab_to_xyz, missing_table_refusal, transform_table, xyz_to_lab, LutPipeline,
    PcsEncoding, PcsSpace,
};

/// Largest profile file this reader opens, checked BEFORE reading it. The
/// same 64 MiB bound as the `.cube` reader and as `MAX_CLUT_BYTES`: a look
/// larger than its own decoded table cap cannot be one this build keeps.
const MAX_LOOKUP_PROFILE_BYTES: u64 = 64 << 20;

/// The params key an abstract look keeps its profile under: standard base64
/// of the profile's own bytes. Only abstracts write it.
pub(crate) const ABSTRACT_KEY: &str = "abstract_profile";

/// Largest abstract profile a layer KEEPS. A layer's meta is capped at
/// 16 MiB (`drdc::MAX_DRDC_META_LEN`) and must also hold the ~575 KB baked
/// table, so a profile anywhere near the 64 MiB load cap could never be
/// stored; 4 MiB of profile is about 5.6 MiB of base64, which leaves the
/// meta well inside its cap. Every abstract Adobe ships is under 100 KB.
pub(crate) const MAX_ABSTRACT_LOOK_BYTES: usize = 4 << 20;

/// The params keys a bake writes, which a re-bake replaces as one set.
const BAKED_KEYS: [&str; 8] = [
    "kind",
    "size",
    "source_size",
    "domain_min",
    "domain_max",
    "table",
    "title",
    ABSTRACT_KEY,
];

/// The two classes of look this reader accepts; an abstract names the PCS
/// of each side of its table.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    Link,
    Abstract(PcsSpace, PcsSpace),
}

/// Reads the ICC profile at `path` and returns the `color_lookup` params
/// object for it as a JSON string: `{"kind":"3d","size":n,"source_size":m,
/// "domain_min":[0,0,0],"domain_max":[1,1,1],"table":…,"title":<desc>}`,
/// plus `abstract_profile` for an abstract. `n` is a plain-grid link's own
/// grid up to 33, else 33; `m` is the larger of `n` and the link's own CLUT
/// grid (module doc, "The table"). `document` is the document's
/// profile, which an abstract profile is baked through; a device link
/// ignores it. Every failure is an `Err` naming the path and the reason;
/// nothing panics.
pub(crate) fn parse_profile_file(
    path: &str,
    document: Option<&IccProfile>,
) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("failed to read {path}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("failed to read {path}: not a regular file"));
    }
    if meta.len() > MAX_LOOKUP_PROFILE_BYTES {
        return Err(format!(
            "failed to read {path}: the profile is too large to load as a look ({} bytes, max {MAX_LOOKUP_PROFILE_BYTES})",
            meta.len()
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("failed to read {path}: {e}"))?;
    bake_profile_bytes(&bytes, document).map_err(|reason| {
        // An abstract look encodes into the document's own profile; when that
        // profile fails the precision check the look file is not at
        // fault, so its sentence stands alone rather than as a read failure.
        if document.is_some_and(|p| reason == own_profile_sentence(p.name())) {
            reason
        } else {
            format!("failed to read {path}: {reason}")
        }
    })
}

/// [`parse_profile_file`]'s params from the profile's BYTES, with no path in
/// any error. An abstract over [`MAX_ABSTRACT_LOOK_BYTES`] refuses, because
/// its bytes are kept in the layer.
pub(crate) fn bake_profile_bytes(
    bytes: &[u8],
    document: Option<&IccProfile>,
) -> Result<String, String> {
    let icc = IccBytes::new(bytes).ok_or("the file is not an ICC profile")?;
    let class = icc.class_name();
    let pcs_of = |space: &[u8]| match space {
        b"Lab " => Some(PcsSpace::Lab),
        b"XYZ " => Some(PcsSpace::Xyz),
        _ => None,
    };
    let look = match (class.as_str(), icc.data_space(), icc.pcs_space()) {
        ("link", b"RGB ", b"RGB ") => Some(Look::Link),
        ("abst", input, output) => pcs_of(input)
            .zip(pcs_of(output))
            .map(|(i, o)| Look::Abstract(i, o)),
        _ => None,
    };
    let Some(look) = look else {
        return Err(format!(
            "a \"{class}\" profile from {} to {} is not a look Color Lookup can load; \
             it loads RGB-to-RGB device links (link) and abstract profiles (abst) from Lab or XYZ \
             to Lab or XYZ",
            icc.space_name(),
            String::from_utf8_lossy(icc.pcs_space()).trim_end()
        ));
    };
    if look != Look::Link && bytes.len() > MAX_ABSTRACT_LOOK_BYTES {
        return Err(format!(
            "the abstract profile is too large to keep with its layer ({} bytes, max {MAX_ABSTRACT_LOOK_BYTES})",
            bytes.len()
        ));
    }
    let data = icc
        .tag(b"A2B0")
        .ok_or_else(|| missing_table_refusal(&icc))?;
    let (pipeline, bridge) = match look {
        // A link's output side is device RGB, never PCS, so its encoding is
        // never read; Lab is named only because the parser asks for one.
        Look::Link => (transform_table(data, PcsSpace::Lab)?, None),
        Look::Abstract(input, output) => {
            let (pipeline, input_encoding) = abstract_table(data, input, output)?;
            let profile = document.ok_or(
                "an abstract profile is applied through the document's profile, \
                 and the document has none this build can read",
            )?;
            let pcs = DocumentPcs::new(profile).map_err(|e| own_profile_error(profile, e))?;
            (pipeline, Some((pcs, input_encoding)))
        }
    };
    // A link whose A2B0 is a plain grid of at most 33 nodes is stored at its
    // own grid, the nodes read exactly; anything else is sampled at 33³
    // (module doc, "The table").
    let cap = MAX_STORED_3D as usize;
    let size = match look {
        Look::Link => pipeline.plain_grid().filter(|&n| n <= cap).unwrap_or(cap),
        Look::Abstract(..) => cap,
    };
    let source_size = match look {
        Look::Link => size.max(pipeline.clut_grid().unwrap_or(0)),
        Look::Abstract(..) => size,
    };
    let mut table = Vec::new();
    table
        .try_reserve_exact(size * size * size * 3)
        .map_err(|_| "could not allocate a colour table")?;
    let step = (size - 1) as f64;
    for b in 0..size {
        for g in 0..size {
            for r in 0..size {
                let rgb = [r as f64 / step, g as f64 / step, b as f64 / step];
                let out = match &bridge {
                    None => {
                        let raw = pipeline.eval(&rgb);
                        [raw[0] as f32, raw[1] as f32, raw[2] as f32]
                    }
                    Some((pcs, input)) => through_abstract(&pipeline, pcs, *input, rgb),
                };
                table.extend_from_slice(&out);
            }
        }
    }
    let params = lookup_params_json(&icc.name(), size, source_size, &table)?;
    if look == Look::Link {
        return Ok(params);
    }
    let Ok(Value::Object(mut object)) = serde_json::from_str::<Value>(&params) else {
        return Err("a baked look's params are not a JSON object".into());
    };
    object.insert(ABSTRACT_KEY.into(), Value::String(base64_encode(bytes)));
    Ok(Value::Object(object).to_string())
}

/// One document RGB node through an abstract profile's table, relative
/// colorimetric at both ends: each side's PCS is Lab or XYZ, read from the
/// two encodings (`input` before the table, the pipeline's own after it).
fn through_abstract(
    pipeline: &LutPipeline,
    bridge: &DocumentPcs,
    input: PcsEncoding,
    rgb: [f64; 3],
) -> [f32; 3] {
    let xyz = bridge.to_xyz(rgb);
    let pcs_in = if input == PcsEncoding::Xyz {
        xyz
    } else {
        xyz_to_lab(xyz)
    };
    let raw = pipeline.eval(&input.encode(pcs_in));
    let output = pipeline.pcs();
    let edited = output.decode([raw[0], raw[1], raw[2]]);
    let xyz = if output == PcsEncoding::Xyz {
        edited
    } else {
        lab_to_xyz(edited)
    };
    bridge.from_xyz(xyz)
}

/// True when `meta` is a `color_lookup` adjustment carrying
/// [`ABSTRACT_KEY`] — an abstract look a profile write re-bakes, and the
/// predicate `.rz` word 71 is written by.
pub(crate) fn is_abstract_look(meta: &str) -> bool {
    crate::levels_native::params_carry(meta, "color_lookup", &[ABSTRACT_KEY])
}

/// `meta` with its abstract look re-baked through `document`: `Ok(None)`
/// when the meta is not an abstract look ([`is_abstract_look`]). Otherwise
/// the params keys a bake writes (`kind`, `size`, `source_size`,
/// `domain_min`, `domain_max`, `table`, `title`, `abstract_profile`) are
/// replaced by [`bake_profile_bytes`]' and every other key, at either level,
/// is kept. `Err` names the look and the profile and fails the whole
/// profile write.
pub(crate) fn rebake_meta(meta: &str, document: &IccProfile) -> Result<Option<String>, String> {
    if !is_abstract_look(meta) {
        return Ok(None);
    }
    let Ok(Value::Object(mut root)) = serde_json::from_str::<Value>(meta) else {
        return Ok(None);
    };
    let Some(Value::Object(params)) = root.get_mut("params") else {
        return Ok(None);
    };
    let title = params
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("untitled")
        .to_string();
    let failed = |reason: String| {
        format!(
            "the Color Lookup abstract look \"{title}\" cannot be re-baked through \"{}\": {reason}",
            document.name()
        )
    };
    let bytes = params
        .get(ABSTRACT_KEY)
        .and_then(Value::as_str)
        .and_then(base64_decode)
        .ok_or_else(|| failed("its kept profile is not valid base64".into()))?;
    let baked = bake_profile_bytes(&bytes, Some(document)).map_err(failed)?;
    let Ok(Value::Object(baked)) = serde_json::from_str::<Value>(&baked) else {
        return Err(failed("the baked params are not a JSON object".into()));
    };
    for key in BAKED_KEYS {
        params.remove(key);
    }
    params.extend(baked);
    Ok(Some(Value::Object(root).to_string()))
}
