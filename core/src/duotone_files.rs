//! A Duotone document's files (#547 Duotone phase 1b,
//! `designs/duotone-mode.md`): the PSD/PSB carry and the user's own
//! Duotone Options (`.ado`) file. The adapter over `duotone_ado`'s codec.
//!
//! * **Read.** A colour-mode-8 PSD or PSB holds its description in the
//!   colour-mode data ([`container_inks`]: `duotone_ado::read_spec`), and the
//!   file block — that data and every `DUOTONE_RESOURCE_IDS` resource it
//!   holds, verbatim — rides on the inks (`DuotoneInks::carried`). The
//!   pixels are the Gray file's (`Container::duotone_gray`). A TIFF refuses:
//!   Duotone files are PSD or PSB (and EPS and DCS 2.0, which are not read).
//! * **Write.** The Gray writer builds the file unchanged and
//!   [`into_duotone_file`] post-processes it: header colour mode 8, the
//!   colour-mode data after the header (the carried block's when the inks
//!   are the ones it was read with, else `write_spec`'s), and the carried
//!   resources appended to the resource section, its length corrected. So
//!   inks unchanged since the read keep the colour-mode data and the duotone
//!   resources byte for byte; any ink change parses fresh inks with no block
//!   (`set_duotone`). The block lives in memory only: `.rz` keeps the inks,
//!   not the bytes.
//! * **`.ado`.** The same specification as a file of its own ([`read_ado`],
//!   [`write_ado`]); an unchanged read writes its own bytes back.

use crate::duotone_ado::{read_spec, write_spec, DuotoneFileBlock, DUOTONE_RESOURCE_IDS};
use crate::duotone_ink::DuotoneInks;
use crate::psd_container::Container;
use crate::psd_framing::LayeredContainer;

/// The largest `.ado` file read: the specification is 524 bytes.
const ADO_MAX_BYTES: u64 = 65_536;

/// The description a Duotone container holds, carrying its file block
/// (module doc).
pub(crate) fn container_inks(container: &Container<'_>) -> Result<DuotoneInks, String> {
    if container.kind == LayeredContainer::Tiff {
        return Err("a TIFF holds no duotone specification; a Duotone file is a PSD or PSB".into());
    }
    if container.color != 8 {
        return Err(format!("a {} file is not Duotone", container.color_name()));
    }
    let inks = read_spec(container.color_mode_data)?;
    let mut resources = Vec::new();
    for id in DUOTONE_RESOURCE_IDS {
        if let Some(body) = container.resource(id)? {
            resources.push((id, body.to_vec()));
        }
    }
    Ok(inks.with_carried(DuotoneFileBlock {
        mode_data: container.color_mode_data.to_vec(),
        resources,
    }))
}

fn u32_at(bytes: &[u8], at: usize) -> Result<usize, String> {
    let b = bytes
        .get(at..at + 4)
        .ok_or("the Grayscale file is truncated")?;
    Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
}

/// One image resource as the PSD resource section frames it: `8BIM`, the
/// id, an empty even-padded name, the length, the body, an even pad.
fn resource_block(id: u16, body: &[u8]) -> Result<Vec<u8>, String> {
    let length = u32::try_from(body.len()).map_err(|_| "a duotone resource is too large")?;
    let mut out = Vec::with_capacity(12 + body.len() + 1);
    out.extend_from_slice(b"8BIM");
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
    Ok(out)
}

/// `gray_file` (a colour-mode-1 PSD or PSB the Gray writer built) as the
/// Duotone file of `inks` (module doc). One more copy of the file is built.
pub(crate) fn into_duotone_file(gray_file: Vec<u8>, inks: &DuotoneInks) -> Result<Vec<u8>, String> {
    if gray_file.get(..4) != Some(b"8BPS") || gray_file.len() < 30 {
        return Err("the Grayscale file to make Duotone is not a PSD".into());
    }
    let mode_length = u32_at(&gray_file, 26)?;
    let resources_at = 30usize
        .checked_add(mode_length)
        .ok_or("the Grayscale file's colour-mode data overflows")?;
    let resources_length = u32_at(&gray_file, resources_at)?;
    let rest_at = resources_at
        .checked_add(4)
        .and_then(|v| v.checked_add(resources_length))
        .filter(|&end| end <= gray_file.len())
        .ok_or("the Grayscale file's image resources are truncated")?;
    let (mode_data, carried) = match inks.carried() {
        Some(block) => (block.mode_data.clone(), block.resources.as_slice()),
        None => (write_spec(inks), &[][..]),
    };
    let mut appended = Vec::new();
    for (id, body) in carried {
        appended.extend_from_slice(&resource_block(*id, body)?);
    }
    let new_resources = resources_length
        .checked_add(appended.len())
        .and_then(|v| u32::try_from(v).ok())
        .ok_or("the Duotone file's image resources are too large")?;
    let mode_bytes =
        u32::try_from(mode_data.len()).map_err(|_| "the duotone specification is too large")?;
    let mut out = Vec::with_capacity(gray_file.len() + mode_data.len() + appended.len());
    out.extend_from_slice(&gray_file[..24]);
    out.extend_from_slice(&8u16.to_be_bytes());
    out.extend_from_slice(&mode_bytes.to_be_bytes());
    out.extend_from_slice(&mode_data);
    out.extend_from_slice(&new_resources.to_be_bytes());
    out.extend_from_slice(&gray_file[resources_at + 4..rest_at]);
    out.extend_from_slice(&appended);
    out.extend_from_slice(&gray_file[rest_at..]);
    Ok(out)
}

/// The description a Duotone Options file holds, carrying its bytes.
pub(crate) fn read_ado(path: &str) -> Result<DuotoneInks, String> {
    let length = std::fs::metadata(path)
        .map_err(|e| format!("failed to read {path}: {e}"))?
        .len();
    if length > ADO_MAX_BYTES {
        return Err(format!(
            "failed to read {path}: a Duotone Options file is 524 bytes, not {length}"
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("failed to read {path}: {e}"))?;
    let inks = read_spec(&bytes).map_err(|e| format!("failed to read {path}: {e}"))?;
    Ok(inks.with_carried(DuotoneFileBlock {
        mode_data: bytes,
        resources: Vec::new(),
    }))
}

/// `inks` written to `path` as a Duotone Options file: the carried bytes
/// when the inks are the ones they were read with, else `write_spec`'s;
/// atomic through the shared temp-file rename.
pub(crate) fn write_ado(path: &str, inks: &DuotoneInks) -> Result<(), String> {
    let bytes = match inks.carried() {
        Some(block) => block.mode_data.clone(),
        None => write_spec(inks),
    };
    crate::dr_image::save_atomically(path, |tmp| {
        std::fs::write(tmp, &bytes).map_err(|e| format!("failed to write {path}: {e}"))
    })
}
