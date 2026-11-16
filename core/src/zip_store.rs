//! A plain ZIP container for a zipped Action bundle (`<slug>.draction.zip`): one writer, one
//! reader, nothing else.
//!
//! **The writer** (`pack_dir`) stores a folder's regular files under the folder's own name —
//! `<slug>.draction/action.json`, `<slug>.draction/assets/<file>` — which is the layout Finder's
//! Compress gives the folder, so either tool's archive opens in the other. It is deterministic:
//! entries sorted by name, every timestamp 1980-01-01 00:00, raw deflate at level 6 (stored when
//! deflate would not shrink an entry), so packaging the same folder twice gives the same bytes.
//! It enforces the reader's caps, so every archive it writes can be read back.
//!
//! **The reader** (`unpack`) takes untrusted archives. Everything it needs comes from the central
//! directory (sizes and CRC included), so an archive whose local headers defer them to a data
//! descriptor (general-purpose bit 3, what Archive Utility and `ditto` write) reads like any
//! other. Every entry name is checked before anything is written, and a name that is absolute,
//! holds `..`, `.`, an empty component, a backslash, a colon or a NUL is refused, so nothing is
//! ever written outside `dest`. Also refused: zip64, multi-disk and encrypted archives, a method
//! other than stored (0) or deflate (8), a symbolic link entry, a duplicate name (compared without
//! case, as the default macOS volume does), a CRC or size mismatch, trailing bytes after an
//! entry's deflate stream, more than [`MAX_ENTRIES`] entries, an entry over
//! [`MAX_ENTRY_BYTES`], more than [`MAX_TOTAL_BYTES`] in all and a name over [`MAX_NAME_BYTES`].
//! Directory entries (`name/`) are created; `__MACOSX/` entries and any entry with a component
//! starting with `.` (`.DS_Store`, AppleDouble `._*` files) are skipped, after their names are
//! checked. An entry is inflated into a buffer of exactly its declared size (`psd_zip`'s exact
//! inflate), so the declared sizes against the caps are the bomb bound.
//!
//! On `Err`, `dest` may hold the entries written before the failure; the caller unpacks into a
//! fresh folder of its own and removes it whatever the outcome.

use std::collections::HashSet;
use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::psd_zip::{deflate_raw, inflate_raw_exact};

/// The most entries an archive may hold — a bundle is an `action.json` and its assets.
pub const MAX_ENTRIES: usize = 1024;
/// The largest one entry may inflate to.
pub const MAX_ENTRY_BYTES: u64 = 512 * 1024 * 1024;
/// The most all entries together may inflate to.
pub const MAX_TOTAL_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// The longest entry name, in bytes.
pub const MAX_NAME_BYTES: usize = 255;
/// The largest archive file read: every entry stored, plus room for the headers.
const MAX_ARCHIVE_BYTES: u64 = MAX_TOTAL_BYTES + 16 * 1024 * 1024;

const LOCAL_SIGNATURE: u32 = 0x0403_4b50;
const CENTRAL_SIGNATURE: u32 = 0x0201_4b50;
const END_SIGNATURE: u32 = 0x0605_4b50;
const ZIP64_LOCATOR_SIGNATURE: u32 = 0x0706_4b50;
/// 1980-01-01 in MS-DOS date form (day 1, month 1, year 0 = 1980), with time 00:00:00.
const DOS_DATE: u16 = (1 << 5) | 1;
/// General-purpose bit 11: the name is UTF-8.
const UTF8_FLAG: u16 = 1 << 11;
/// "Made by" Unix, spec 2.0, so the external attributes carry a Unix mode.
const MADE_BY_UNIX: u16 = (3 << 8) | 20;
const VERSION_NEEDED: u16 = 20;
/// A regular file, `rw-r--r--`, in the external attributes' high half.
const REGULAR_FILE_MODE: u32 = 0o100_644 << 16;

// ---- writing ----------------------------------------------------------------------------------

/// Writes `dir`'s regular files into a new ZIP at `zip_path`, each named `<dir name>/<path>`.
/// The archive is written beside `zip_path` and renamed into place, so an existing file there is
/// replaced only by a complete archive. A symbolic link or any other non-regular file, or a folder
/// past the reader's caps, refuses with nothing written.
pub fn pack_dir(dir: &Path, zip_path: &Path) -> Result<(), String> {
    let shown = dir.display();
    let root = dir
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("{shown}: the folder has no usable name"))?;
    let mut files = Vec::new();
    collect(dir, root, &mut files)?;
    files.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    if files.len() > MAX_ENTRIES {
        return Err(format!(
            "{shown}: a bundle holds at most {MAX_ENTRIES} files"
        ));
    }
    let mut total = 0u64;
    for (name, path) in &files {
        check_name(name).map_err(|reason| format!("{shown}: {reason}"))?;
        let size = fs::metadata(path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?
            .len();
        if size > MAX_ENTRY_BYTES {
            return Err(format!(
                "{}: a bundle file may be at most 512 MiB",
                path.display()
            ));
        }
        total += size;
    }
    if total > MAX_TOTAL_BYTES {
        return Err(format!("{shown}: a bundle may hold at most 2 GiB"));
    }
    let partial = partial_path(zip_path)?;
    let result = write_archive(&files, &partial).and_then(|()| {
        fs::rename(&partial, zip_path)
            .map_err(|e| format!("failed to write {}: {e}", zip_path.display()))
    });
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result
}

/// Every regular file under `dir`, as (`prefix/relative/name`, path). A symbolic link or a
/// special file refuses.
fn collect(dir: &Path, prefix: &str, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
    let entries =
        fs::read_dir(dir).map_err(|e| format!("failed to read {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("failed to read {}: {e}", dir.display()))?;
        let path = entry.path();
        let kind = fs::symlink_metadata(&path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?
            .file_type();
        let name = entry
            .file_name()
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| format!("{}: the name is not UTF-8", path.display()))?;
        let joined = format!("{prefix}/{name}");
        if kind.is_dir() {
            collect(&path, &joined, out)?;
        } else if kind.is_file() {
            out.push((joined, path));
        } else {
            return Err(format!(
                "{}: a bundle holds regular files only",
                path.display()
            ));
        }
        if out.len() > MAX_ENTRIES {
            return Err(format!(
                "{}: a bundle holds at most {MAX_ENTRIES} files",
                dir.display()
            ));
        }
    }
    Ok(())
}

/// `.<name>.partial` beside `zip_path`, so the rename that publishes it stays on one volume.
fn partial_path(zip_path: &Path) -> Result<PathBuf, String> {
    let name = zip_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("{}: the archive has no usable name", zip_path.display()))?;
    Ok(zip_path.with_file_name(format!(".{name}.partial")))
}

fn write_archive(files: &[(String, PathBuf)], target: &Path) -> Result<(), String> {
    let failed = |e: std::io::Error| format!("failed to write {}: {e}", target.display());
    let file = fs::File::create(target).map_err(failed)?;
    let mut out = BufWriter::new(file);
    let mut central = Vec::new();
    let mut offset = 0u64;
    for (name, path) in files {
        let data = fs::read(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        let crc = crc32fast::hash(&data);
        let deflated = deflate_raw(&data, 6);
        let (method, body) = if deflated.len() < data.len() {
            (8u16, deflated.as_slice())
        } else {
            (0u16, data.as_slice())
        };
        let (compressed, size, at) = (
            u32_of(body.len())?,
            u32_of(data.len())?,
            u32_of_u64(offset)?,
        );
        let name_bytes = name.as_bytes();
        let mut local = Vec::with_capacity(30 + name_bytes.len());
        push32(&mut local, LOCAL_SIGNATURE);
        push16(&mut local, VERSION_NEEDED);
        push16(&mut local, UTF8_FLAG);
        push16(&mut local, method);
        push16(&mut local, 0);
        push16(&mut local, DOS_DATE);
        push32(&mut local, crc);
        push32(&mut local, compressed);
        push32(&mut local, size);
        push16(&mut local, u16_of(name_bytes.len())?);
        push16(&mut local, 0);
        local.extend_from_slice(name_bytes);
        out.write_all(&local).map_err(failed)?;
        out.write_all(body).map_err(failed)?;
        offset += (local.len() + body.len()) as u64;

        push32(&mut central, CENTRAL_SIGNATURE);
        push16(&mut central, MADE_BY_UNIX);
        push16(&mut central, VERSION_NEEDED);
        push16(&mut central, UTF8_FLAG);
        push16(&mut central, method);
        push16(&mut central, 0);
        push16(&mut central, DOS_DATE);
        push32(&mut central, crc);
        push32(&mut central, compressed);
        push32(&mut central, size);
        push16(&mut central, u16_of(name_bytes.len())?);
        push16(&mut central, 0); // extra
        push16(&mut central, 0); // comment
        push16(&mut central, 0); // disk
        push16(&mut central, 0); // internal attributes
        push32(&mut central, REGULAR_FILE_MODE);
        push32(&mut central, at);
        central.extend_from_slice(name_bytes);
    }
    let count = u16_of(files.len())?;
    let mut end = Vec::with_capacity(22);
    push32(&mut end, END_SIGNATURE);
    push16(&mut end, 0);
    push16(&mut end, 0);
    push16(&mut end, count);
    push16(&mut end, count);
    push32(&mut end, u32_of(central.len())?);
    push32(&mut end, u32_of_u64(offset)?);
    push16(&mut end, 0);
    out.write_all(&central).map_err(failed)?;
    out.write_all(&end).map_err(failed)?;
    let file = out.into_inner().map_err(|e| failed(e.into_error()))?;
    file.sync_all().map_err(failed)
}

fn push16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn u16_of(value: usize) -> Result<u16, String> {
    u16::try_from(value).map_err(|_| "a zip field is out of range".to_string())
}

fn u32_of(value: usize) -> Result<u32, String> {
    u32::try_from(value).map_err(|_| "a zip field is out of range".to_string())
}

fn u32_of_u64(value: u64) -> Result<u32, String> {
    u32::try_from(value).map_err(|_| "a zip field is out of range".to_string())
}

// ---- reading ----------------------------------------------------------------------------------

/// One central-directory entry, as the reader uses it.
struct Entry {
    name: String,
    method: u16,
    crc: u32,
    compressed: u64,
    size: u64,
    offset: u64,
    directory: bool,
}

/// Extracts the archive at `zip_path` into the existing folder `dest` (module doc: what it
/// refuses, and what `dest` may hold after an `Err`).
pub fn unpack(zip_path: &Path, dest: &Path) -> Result<(), String> {
    let shown = zip_path.display();
    if !dest.is_dir() {
        return Err(format!(
            "{}: the destination is not a folder",
            dest.display()
        ));
    }
    let length = fs::metadata(zip_path)
        .map_err(|e| format!("failed to read {shown}: {e}"))?
        .len();
    if length > MAX_ARCHIVE_BYTES {
        return Err(format!(
            "{shown}: the archive is larger than a bundle may be"
        ));
    }
    let bytes = fs::read(zip_path).map_err(|e| format!("failed to read {shown}: {e}"))?;
    let entries = central_directory(&bytes).map_err(|reason| format!("{shown}: {reason}"))?;
    // Every name and cap is checked before the first byte is written.
    for entry in &entries {
        let target = dest.join(&entry.name);
        if skipped(&entry.name) {
            continue;
        }
        if entry.directory {
            fs::create_dir_all(&target)
                .map_err(|e| format!("failed to write {}: {e}", target.display()))?;
            continue;
        }
        let data = entry_data(&bytes, entry).map_err(|reason| format!("{shown}: {reason}"))?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to write {}: {e}", parent.display()))?;
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|e| format!("failed to write {}: {e}", target.display()))?;
        file.write_all(&data)
            .map_err(|e| format!("failed to write {}: {e}", target.display()))?;
    }
    Ok(())
}

/// `__MACOSX/` and dot-file entries: Finder's metadata, never a bundle's.
fn skipped(name: &str) -> bool {
    name.split('/')
        .any(|component| component == "__MACOSX" || component.starts_with('.'))
}

/// The refusal for a name that could reach outside the destination, or nil.
fn check_name(name: &str) -> Result<(), String> {
    let refuse = || {
        Err(format!(
            "an entry name is not a relative path inside the archive (\"{}\")",
            name.escape_default()
        ))
    };
    if name.is_empty() || name.len() > MAX_NAME_BYTES {
        return Err(format!(
            "an entry name is empty or longer than {MAX_NAME_BYTES} bytes"
        ));
    }
    if name.starts_with('/') || name.contains(['\\', ':', '\0']) {
        return refuse();
    }
    let body = name.strip_suffix('/').unwrap_or(name);
    if body
        .split('/')
        .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return refuse();
    }
    Ok(())
}

fn le16(bytes: &[u8], at: usize) -> Option<u16> {
    bytes
        .get(at..at.checked_add(2)?)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
}

fn le32(bytes: &[u8], at: usize) -> Option<u32> {
    bytes
        .get(at..at.checked_add(4)?)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// The end record's position: the LAST signature within the 64 KiB comment window.
fn end_record(bytes: &[u8]) -> Option<usize> {
    let last = bytes.len().checked_sub(22)?;
    let first = last.saturating_sub(0xFFFF);
    (first..=last)
        .rev()
        .find(|&at| le32(bytes, at) == Some(END_SIGNATURE))
}

fn central_directory(bytes: &[u8]) -> Result<Vec<Entry>, String> {
    let truncated = || "the archive is truncated or not a zip".to_string();
    let end = end_record(bytes).ok_or_else(truncated)?;
    if end >= 20 && le32(bytes, end - 20) == Some(ZIP64_LOCATOR_SIGNATURE) {
        return Err("zip64 archives are not supported".into());
    }
    let disk = le16(bytes, end + 4).ok_or_else(truncated)?;
    let directory_disk = le16(bytes, end + 6).ok_or_else(truncated)?;
    let here = le16(bytes, end + 8).ok_or_else(truncated)?;
    let count = le16(bytes, end + 10).ok_or_else(truncated)?;
    let size = le32(bytes, end + 12).ok_or_else(truncated)?;
    let offset = le32(bytes, end + 16).ok_or_else(truncated)?;
    if count == 0xFFFF || size == 0xFFFF_FFFF || offset == 0xFFFF_FFFF {
        return Err("zip64 archives are not supported".into());
    }
    if disk != 0 || directory_disk != 0 || here != count {
        return Err("multi-disk archives are not supported".into());
    }
    if usize::from(count) > MAX_ENTRIES {
        return Err(format!("an archive holds at most {MAX_ENTRIES} entries"));
    }
    let start = offset as usize;
    let stop = start.checked_add(size as usize).ok_or_else(truncated)?;
    if stop > end {
        return Err(truncated());
    }
    let mut entries = Vec::with_capacity(usize::from(count));
    let mut names = HashSet::new();
    let mut total = 0u64;
    let mut at = start;
    for _ in 0..count {
        if le32(bytes, at) != Some(CENTRAL_SIGNATURE) {
            return Err("the central directory is damaged".into());
        }
        let field16 = |delta: usize| le16(bytes, at + delta).ok_or_else(truncated);
        let field32 = |delta: usize| le32(bytes, at + delta).ok_or_else(truncated);
        let made_by = field16(4)?;
        let flags = field16(8)?;
        let method = field16(10)?;
        let crc = field32(16)?;
        let compressed = field32(20)?;
        let size = field32(24)?;
        let name_length = usize::from(field16(28)?);
        let extra_length = usize::from(field16(30)?);
        let comment_length = usize::from(field16(32)?);
        let external = field32(38)?;
        let local = field32(42)?;
        let name_start = at + 46;
        let extra_start = name_start + name_length;
        let next = extra_start + extra_length + comment_length;
        if next > stop {
            return Err(truncated());
        }
        if flags & 1 != 0 || flags & (1 << 6) != 0 {
            return Err("encrypted archives are not supported".into());
        }
        if method != 0 && method != 8 {
            return Err(format!(
                "entry compression method {method} is not supported (stored or deflate only)"
            ));
        }
        if compressed == 0xFFFF_FFFF || size == 0xFFFF_FFFF || local == 0xFFFF_FFFF {
            return Err("zip64 archives are not supported".into());
        }
        let mut extra = extra_start;
        while extra + 4 <= extra_start + extra_length {
            let id = le16(bytes, extra).ok_or_else(truncated)?;
            let length = usize::from(le16(bytes, extra + 2).ok_or_else(truncated)?);
            if id == 0x0001 {
                return Err("zip64 archives are not supported".into());
            }
            extra += 4 + length;
        }
        let name = std::str::from_utf8(&bytes[name_start..extra_start])
            .map_err(|_| "an entry name is not UTF-8".to_string())?
            .to_owned();
        check_name(&name)?;
        if made_by >> 8 == 3 && (external >> 16) & 0o170_000 == 0o120_000 {
            return Err(format!(
                "\"{}\" is a symbolic link; a bundle holds regular files only",
                name.escape_default()
            ));
        }
        if !names.insert(name.trim_end_matches('/').to_lowercase()) {
            return Err(format!(
                "\"{}\" appears twice in the archive",
                name.escape_default()
            ));
        }
        let directory = name.ends_with('/');
        if directory && size != 0 {
            return Err(format!(
                "the folder entry \"{}\" holds data",
                name.escape_default()
            ));
        }
        if u64::from(size) > MAX_ENTRY_BYTES {
            return Err(format!(
                "\"{}\" is larger than 512 MiB",
                name.escape_default()
            ));
        }
        total += u64::from(size);
        if total > MAX_TOTAL_BYTES {
            return Err("the archive inflates to more than 2 GiB".into());
        }
        entries.push(Entry {
            name,
            method,
            crc,
            compressed: u64::from(compressed),
            size: u64::from(size),
            offset: u64::from(local),
            directory,
        });
        at = next;
    }
    Ok(entries)
}

/// One entry's bytes: located through its local header, inflated to exactly its declared size,
/// its CRC checked.
fn entry_data(bytes: &[u8], entry: &Entry) -> Result<Vec<u8>, String> {
    let shown = entry.name.escape_default();
    let damaged = || format!("\"{shown}\" is damaged or truncated");
    let at = usize::try_from(entry.offset).map_err(|_| damaged())?;
    if le32(bytes, at) != Some(LOCAL_SIGNATURE) {
        return Err(damaged());
    }
    let name_length = usize::from(le16(bytes, at + 26).ok_or_else(damaged)?);
    let extra_length = usize::from(le16(bytes, at + 28).ok_or_else(damaged)?);
    let name_start = at + 30;
    if bytes.get(name_start..name_start + name_length) != Some(entry.name.as_bytes()) {
        return Err(format!("\"{shown}\": the local header names another entry"));
    }
    let start = name_start + name_length + extra_length;
    let compressed = usize::try_from(entry.compressed).map_err(|_| damaged())?;
    let size = usize::try_from(entry.size).map_err(|_| damaged())?;
    let body = bytes
        .get(start..start.checked_add(compressed).ok_or_else(damaged)?)
        .ok_or_else(damaged)?;
    let data = if entry.method == 0 {
        if compressed != size {
            return Err(format!("\"{shown}\": the stored size does not match"));
        }
        body.to_vec()
    } else {
        inflate_raw_exact(body, size).map_err(|reason| format!("\"{shown}\": {reason}"))?
    };
    if crc32fast::hash(&data) != entry.crc {
        return Err(format!("\"{shown}\": the CRC does not match its data"));
    }
    Ok(data)
}
