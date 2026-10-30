//! The process-wide WORKING CMYK profile: the profile an untagged CMYK
//! TIFF or JPEG is converted FROM as the flat open reads it
//! (`open_profile::decide`): the working CMYK assigns a profile to an
//! untagged CMYK file before converting it.
//!
//! The host resolves the file (U.S. Web Coated (SWOP) v2 when it is
//! installed, else the system's Generic CMYK) and hands its bytes over once
//! at launch through `dr_set_working_cmyk_profile`. The slot holds only a
//! profile the CMYK conversion can run: a CMYK device space with
//! a relative-colorimetric device-to-PCS table, the same test `decide` asks
//! of an embedded CMYK profile. With the slot empty an untagged CMYK file
//! opens as it always did (`image`'s naive numbers, labelled sRGB), under a
//! sentence saying no working CMYK profile is set.
//!
//! The slot also keeps the profile's BYTES (`working_cmyk_bytes`): a CMYK
//! document converted into the working profile, or an untagged CMYK file
//! opened as a CMYK document (#547), embeds exactly the bytes the host
//! handed over.

use std::sync::{Arc, RwLock};

use crate::icc_intent::RenderingIntent;
use crate::icc_lut::{DeviceSpace, ProofProfile};

/// The one slot. An `Arc` so an open in flight keeps the profile it decided
/// on even if the host replaces it meanwhile.
static SLOT: RwLock<Option<Working>> = RwLock::new(None);

/// The parsed profile and the verbatim bytes it was parsed from.
type Working = (Arc<ProofProfile>, Arc<[u8]>);

/// Sets (`Some`) or clears (`None`) the working CMYK profile. `Err` when the
/// bytes are not a profile `ProofProfile::parse` reads, are not a CMYK
/// profile, or have no relative-colorimetric device-to-PCS table; the slot is
/// then left as it was.
pub(crate) fn set_working_cmyk_profile(icc: Option<&[u8]>) -> Result<(), String> {
    let profile = match icc {
        None => None,
        Some(bytes) => {
            let profile = ProofProfile::parse(bytes)
                .map_err(|e| format!("cannot use the working CMYK profile: {e}"))?;
            if profile.device() != DeviceSpace::Cmyk {
                return Err(format!(
                    "cannot use the working CMYK profile: “{}” is not a CMYK profile",
                    profile.name()
                ));
            }
            if profile
                .to_pcs(RenderingIntent::RelativeColorimetric)
                .is_none()
            {
                return Err(format!(
                    "cannot use the working CMYK profile: “{}” has no device-to-PCS table",
                    profile.name()
                ));
            }
            Some((Arc::new(profile), Arc::from(bytes)))
        }
    };
    let mut slot = SLOT
        .write()
        .map_err(|_| "cannot set the working CMYK profile: its lock is poisoned".to_string())?;
    *slot = profile;
    Ok(())
}

/// The working CMYK profile, `None` when none is set. A poisoned lock reads
/// as `None`: an untagged CMYK file then opens as if none were set.
pub(crate) fn working_cmyk_profile() -> Option<Arc<ProofProfile>> {
    SLOT.read()
        .ok()
        .and_then(|slot| slot.as_ref().map(|(profile, _)| profile.clone()))
}

/// The working CMYK profile's bytes, exactly as the host handed them over;
/// `None` when none is set.
pub(crate) fn working_cmyk_bytes() -> Option<Arc<[u8]>> {
    SLOT.read()
        .ok()
        .and_then(|slot| slot.as_ref().map(|(_, bytes)| bytes.clone()))
}
