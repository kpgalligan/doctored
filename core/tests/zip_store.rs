//! The zipped Action bundle container (`zip_store.rs`), black-box through the FFI
//! (`dr_zip_pack_dir`, `dr_zip_unpack`). Oracles: the system's own `/usr/bin/unzip`, `/usr/bin/zip`
//! and `/usr/bin/ditto` (each case skips when its tool is absent); a reader written here from the
//! ZIP specification (local headers walked in order, raw deflate through `miniz_oxide`, CRC-32
//! through `crc32fast`); and archives built byte by byte here, valid and broken.

use std::collections::BTreeMap;
use std::ffi::{c_char, CStr, CString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_zip::{dr_zip_pack_dir, dr_zip_unpack};
use tempfile::TempDir;

fn c(path: &Path) -> CString {
    CString::new(path.to_str().unwrap()).unwrap()
}

fn text(pointer: *mut c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let value = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(pointer) };
    value
}

fn pack(dir: &Path, zip: &Path) -> Result<(), String> {
    let mut err = ptr::null_mut();
    let ok = unsafe { dr_zip_pack_dir(c(dir).as_ptr(), c(zip).as_ptr(), &mut err) };
    if ok {
        assert!(err.is_null());
        Ok(())
    } else {
        Err(text(err))
    }
}

fn unpack(zip: &Path, dest: &Path) -> Result<(), String> {
    let mut err = ptr::null_mut();
    let ok = unsafe { dr_zip_unpack(c(zip).as_ptr(), c(dest).as_ptr(), &mut err) };
    if ok {
        assert!(err.is_null());
        Ok(())
    } else {
        Err(text(err))
    }
}

/// Every regular file under `root`, relative path → bytes.
fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, root, out);
            } else {
                let key = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_owned();
                out.insert(key, fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

/// A bundle folder: a compressible `action.json` and two assets, one incompressible.
fn bundle(parent: &Path) -> PathBuf {
    let root = parent.join("watermark.draction");
    fs::create_dir_all(root.join("assets")).unwrap();
    let json = r#"{"name":"Watermark","schema_version":2,"steps":[]}"#.repeat(40);
    fs::write(root.join("action.json"), json).unwrap();
    let mut state = 0x1234_5678u32;
    let noise: Vec<u8> = (0..5000)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect();
    fs::write(root.join("assets/logo.png"), &noise).unwrap();
    fs::write(root.join("assets/empty.txt"), b"").unwrap();
    root
}

fn tool(path: &str) -> bool {
    Path::new(path).exists()
}

// ---- a reader written from the specification ---------------------------------------------------

fn le16(b: &[u8], at: usize) -> usize {
    usize::from(u16::from_le_bytes([b[at], b[at + 1]]))
}

fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// The local entries in file order: (name, method, data inflated, crc in the header).
fn local_entries(bytes: &[u8]) -> Vec<(String, u16, Vec<u8>, u32)> {
    let mut at = 0;
    let mut out = Vec::new();
    while le32(bytes, at) == 0x0403_4b50 {
        let method = le16(bytes, at + 8) as u16;
        let crc = le32(bytes, at + 14);
        let compressed = le32(bytes, at + 18) as usize;
        let name_length = le16(bytes, at + 26);
        let extra_length = le16(bytes, at + 28);
        let name = String::from_utf8(bytes[at + 30..at + 30 + name_length].to_vec()).unwrap();
        let start = at + 30 + name_length + extra_length;
        let body = &bytes[start..start + compressed];
        let data = match method {
            0 => body.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec(body).unwrap(),
            other => panic!("unexpected method {other}"),
        };
        out.push((name, method, data, crc));
        at = start + compressed;
    }
    out
}

// ---- archives built here -----------------------------------------------------------------------

struct Built {
    name: Vec<u8>,
    data: Vec<u8>,
    deflate: bool,
    flags: u16,
    made_by: u16,
    external: u32,
    crc: Option<u32>,
    size: Option<u32>,
    extra: Vec<u8>,
    descriptor: bool,
}

fn entry(name: &str, data: &[u8]) -> Built {
    Built {
        name: name.as_bytes().to_vec(),
        data: data.to_vec(),
        deflate: false,
        flags: 0,
        made_by: 20,
        external: 0,
        crc: None,
        size: None,
        extra: Vec::new(),
        descriptor: false,
    }
}

fn build(entries: &[Built]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for e in entries {
        let body = if e.deflate {
            miniz_oxide::deflate::compress_to_vec(&e.data, 6)
        } else {
            e.data.clone()
        };
        let method: u16 = if e.deflate { 8 } else { 0 };
        let crc = e.crc.unwrap_or_else(|| crc32fast::hash(&e.data));
        let size = e.size.unwrap_or(e.data.len() as u32);
        let flags = e.flags | if e.descriptor { 1 << 3 } else { 0 };
        let offset = out.len() as u32;
        let (local_crc, local_compressed, local_size) = if e.descriptor {
            (0, 0, 0)
        } else {
            (crc, body.len() as u32, size)
        };
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&method.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0x21, 0]);
        out.extend_from_slice(&local_crc.to_le_bytes());
        out.extend_from_slice(&local_compressed.to_le_bytes());
        out.extend_from_slice(&local_size.to_le_bytes());
        out.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
        out.extend_from_slice(&(e.extra.len() as u16).to_le_bytes());
        out.extend_from_slice(&e.name);
        out.extend_from_slice(&e.extra);
        out.extend_from_slice(&body);
        if e.descriptor {
            out.extend_from_slice(&0x0807_4b50u32.to_le_bytes());
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes());
        }
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&e.made_by.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&flags.to_le_bytes());
        central.extend_from_slice(&method.to_le_bytes());
        central.extend_from_slice(&[0, 0, 0x21, 0]);
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(body.len() as u32).to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
        central.extend_from_slice(&(e.extra.len() as u16).to_le_bytes());
        central.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        central.extend_from_slice(&e.external.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(&e.name);
        central.extend_from_slice(&e.extra);
    }
    let central_offset = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&central_offset.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    out
}

/// Unpacks `bytes` into a fresh folder inside a fresh parent, asserting nothing appears beside
/// the folder whatever the outcome.
fn unpack_bytes(bytes: &[u8]) -> (Result<(), String>, BTreeMap<String, Vec<u8>>) {
    let scratch = TempDir::new().unwrap();
    let zip = scratch.path().join("in.zip");
    fs::write(&zip, bytes).unwrap();
    let dest = scratch.path().join("dest");
    fs::create_dir(&dest).unwrap();
    let result = unpack(&zip, &dest);
    let mut beside: Vec<String> = fs::read_dir(scratch.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_owned())
        .collect();
    beside.sort();
    assert_eq!(
        beside,
        ["dest", "in.zip"],
        "an entry escaped the destination"
    );
    (result, tree(&dest))
}

fn refused(bytes: &[u8], sentence: &str) {
    let (result, _) = unpack_bytes(bytes);
    let message = result.expect_err("the archive was accepted");
    assert!(
        message.contains(sentence),
        "expected “{sentence}”, got “{message}”"
    );
}

// ---- the writer ----------------------------------------------------------------------------------

#[test]
fn a_packed_bundle_reads_back_through_an_independent_reader() {
    let scratch = TempDir::new().unwrap();
    let root = bundle(scratch.path());
    let zip = scratch.path().join("watermark.draction.zip");
    pack(&root, &zip).unwrap();
    let bytes = fs::read(&zip).unwrap();
    let entries = local_entries(&bytes);
    let names: Vec<&str> = entries.iter().map(|e| e.0.as_str()).collect();
    assert_eq!(
        names,
        [
            "watermark.draction/action.json",
            "watermark.draction/assets/empty.txt",
            "watermark.draction/assets/logo.png",
        ]
    );
    let source = tree(&root);
    for (name, method, data, crc) in &entries {
        let key = name.strip_prefix("watermark.draction/").unwrap();
        assert_eq!(data, &source[key], "{name}");
        assert_eq!(*crc, crc32fast::hash(data), "{name}");
        // The JSON shrinks, so it is deflated; the noise and the empty file are stored.
        let expected: u16 = if key == "action.json" { 8 } else { 0 };
        assert_eq!(*method, expected, "{name}");
    }
}

#[test]
fn a_packed_bundle_reads_back_through_unzip() {
    if !tool("/usr/bin/unzip") {
        eprintln!("SKIP: /usr/bin/unzip is absent");
        return;
    }
    let scratch = TempDir::new().unwrap();
    let root = bundle(scratch.path());
    let zip = scratch.path().join("watermark.draction.zip");
    pack(&root, &zip).unwrap();
    let out = scratch.path().join("unzipped");
    let status = Command::new("/usr/bin/unzip")
        .args(["-q", zip.to_str().unwrap(), "-d", out.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(tree(&out.join("watermark.draction")), tree(&root));
}

#[test]
fn packing_twice_gives_the_same_bytes_and_replaces_the_archive() {
    let scratch = TempDir::new().unwrap();
    let root = bundle(scratch.path());
    let first = scratch.path().join("a.zip");
    let second = scratch.path().join("b.zip");
    pack(&root, &first).unwrap();
    pack(&root, &second).unwrap();
    assert_eq!(fs::read(&first).unwrap(), fs::read(&second).unwrap());
    fs::write(&second, b"stale").unwrap();
    pack(&root, &second).unwrap();
    assert_eq!(fs::read(&first).unwrap(), fs::read(&second).unwrap());
    // No partial file is left beside the archive.
    let mut names: Vec<String> = fs::read_dir(scratch.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_str().unwrap().to_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["a.zip", "b.zip", "watermark.draction"]);
}

#[test]
fn packing_refuses_a_symbolic_link() {
    let scratch = TempDir::new().unwrap();
    let root = bundle(scratch.path());
    std::os::unix::fs::symlink("/etc/hosts", root.join("assets/link.png")).unwrap();
    let zip = scratch.path().join("out.zip");
    let message = pack(&root, &zip).unwrap_err();
    assert!(message.contains("regular files only"), "{message}");
    assert!(!zip.exists());
}

#[test]
fn its_own_archive_unpacks_to_the_same_tree() {
    let scratch = TempDir::new().unwrap();
    let root = bundle(scratch.path());
    let zip = scratch.path().join("watermark.draction.zip");
    pack(&root, &zip).unwrap();
    let (result, unpacked) = unpack_bytes(&fs::read(&zip).unwrap());
    result.unwrap();
    let expected: BTreeMap<String, Vec<u8>> = tree(&root)
        .into_iter()
        .map(|(k, v)| (format!("watermark.draction/{k}"), v))
        .collect();
    assert_eq!(unpacked, expected);
}

// ---- the reader on other writers' archives -----------------------------------------------------

#[test]
fn an_archive_made_by_zip_unpacks_and_skips_dot_files() {
    if !tool("/usr/bin/zip") {
        eprintln!("SKIP: /usr/bin/zip is absent");
        return;
    }
    let scratch = TempDir::new().unwrap();
    let root = bundle(scratch.path());
    fs::write(root.join(".DS_Store"), b"finder").unwrap();
    let zip = scratch.path().join("made.zip");
    let status = Command::new("/usr/bin/zip")
        .current_dir(scratch.path())
        .args(["-q", "-r", zip.to_str().unwrap(), "watermark.draction"])
        .status()
        .unwrap();
    assert!(status.success());
    let (result, unpacked) = unpack_bytes(&fs::read(&zip).unwrap());
    result.unwrap();
    let mut expected: BTreeMap<String, Vec<u8>> = tree(&root)
        .into_iter()
        .map(|(k, v)| (format!("watermark.draction/{k}"), v))
        .collect();
    expected.remove("watermark.draction/.DS_Store");
    assert_eq!(unpacked, expected);
}

#[test]
fn an_archive_made_by_ditto_unpacks() {
    // `ditto -c -k --keepParent` is Finder's Compress: data descriptors (bit 3) and folder
    // entries.
    if !tool("/usr/bin/ditto") {
        eprintln!("SKIP: /usr/bin/ditto is absent");
        return;
    }
    let scratch = TempDir::new().unwrap();
    let root = bundle(scratch.path());
    let zip = scratch.path().join("finder.zip");
    let status = Command::new("/usr/bin/ditto")
        .args(["-c", "-k", "--keepParent"])
        .arg(&root)
        .arg(&zip)
        .status()
        .unwrap();
    assert!(status.success());
    let bytes = fs::read(&zip).unwrap();
    // The central directory's flags: at least one entry defers its sizes to a data descriptor.
    let end = bytes.len() - 22;
    let (count, mut at) = (le16(&bytes, end + 10), le32(&bytes, end + 16) as usize);
    let mut descriptors = 0;
    for _ in 0..count {
        descriptors += usize::from(le16(&bytes, at + 8) & (1 << 3) != 0);
        at += 46 + le16(&bytes, at + 28) + le16(&bytes, at + 30) + le16(&bytes, at + 32);
    }
    assert!(
        descriptors > 0,
        "no ditto entry carries a data descriptor; the case no longer covers bit 3"
    );
    let (result, unpacked) = unpack_bytes(&bytes);
    result.unwrap();
    let expected: BTreeMap<String, Vec<u8>> = tree(&root)
        .into_iter()
        .map(|(k, v)| (format!("watermark.draction/{k}"), v))
        .collect();
    assert_eq!(unpacked, expected);
}

#[test]
fn a_data_descriptor_archive_built_here_unpacks() {
    let mut deflated = entry("b.draction/action.json", &b"{\"steps\":[]}".repeat(30));
    deflated.deflate = true;
    deflated.descriptor = true;
    let mut folder = entry("b.draction/assets/", b"");
    folder.descriptor = true;
    let mut junk = entry("__MACOSX/b.draction/._action.json", b"apple double");
    junk.descriptor = true;
    let (result, unpacked) = unpack_bytes(&build(&[folder, deflated, junk]));
    result.unwrap();
    assert_eq!(
        unpacked.keys().collect::<Vec<_>>(),
        ["b.draction/action.json"]
    );
    assert_eq!(
        unpacked["b.draction/action.json"],
        b"{\"steps\":[]}".repeat(30)
    );
}

// ---- refusals ----------------------------------------------------------------------------------

#[test]
fn a_name_that_leaves_the_destination_is_refused() {
    for name in [
        "b/../x.png",
        "../x.png",
        "/tmp/x.png",
        "b\\x.png",
        "b//x.png",
        "./x.png",
        "c:x.png",
    ] {
        refused(
            &build(&[entry("b/ok.json", b"{}"), entry(name, b"evil")]),
            "not a relative path inside the archive",
        );
    }
    refused(
        &build(&[entry(&"n".repeat(256), b"")]),
        "longer than 255 bytes",
    );
}

#[test]
fn a_crc_or_size_mismatch_is_refused() {
    let mut wrong_crc = entry("b/a.json", b"{\"a\":1}");
    wrong_crc.crc = Some(0xDEAD_BEEF);
    refused(&build(&[wrong_crc]), "CRC does not match");
    let mut stored_size = entry("b/a.json", b"{\"a\":1}");
    stored_size.size = Some(3);
    refused(&build(&[stored_size]), "stored size does not match");
    // A deflate stream that inflates short of its declared size, and one that would inflate past
    // it (the output buffer is the declared size, so the inflater stops with output pending).
    let mut short = entry("b/a.json", &b"abc".repeat(100));
    short.deflate = true;
    short.size = Some(301);
    refused(&build(&[short]), "another size than declared");
    let mut long = entry("b/a.json", &b"abc".repeat(100));
    long.deflate = true;
    long.size = Some(299);
    refused(&build(&[long]), "could not be decompressed");
}

#[test]
fn trailing_bytes_after_an_entrys_stream_are_refused() {
    let data = b"hello hello hello hello".repeat(10);
    let mut bytes = build(&[{
        let mut e = entry("b/a.txt", &data);
        e.deflate = true;
        e
    }]);
    // Grow the entry's compressed size by one byte, inserted right after its stream.
    let stream = miniz_oxide::deflate::compress_to_vec(&data, 6).len();
    let insert_at = 30 + "b/a.txt".len() + stream;
    bytes.insert(insert_at, 0);
    let grow = |bytes: &mut Vec<u8>, at: usize| {
        let value = le32(bytes, at) + 1;
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    };
    grow(&mut bytes, 18);
    let end = bytes.len() - 22;
    let central = le32(&bytes, end + 16) as usize + 1;
    bytes[end + 16..end + 20].copy_from_slice(&(central as u32).to_le_bytes());
    grow(&mut bytes, central + 20);
    refused(&bytes, "trailing data");
}

#[test]
fn unsupported_archives_are_refused() {
    let mut encrypted = entry("b/a.json", b"{}");
    encrypted.flags = 1;
    refused(&build(&[encrypted]), "encrypted");
    let mut zip64 = entry("b/a.json", b"{}");
    zip64.extra = vec![1, 0, 0, 0];
    refused(&build(&[zip64]), "zip64");
    let mut link = entry("b/link.png", b"/etc/hosts");
    link.made_by = (3 << 8) | 20;
    link.external = 0o120_777 << 16;
    refused(&build(&[link]), "symbolic link");
    // Method 12 (bzip2), written over a stored entry's method fields.
    let mut bytes = build(&[entry("b/a.json", b"{}")]);
    bytes[8] = 12;
    let central = le32(&bytes, bytes.len() - 22 + 16) as usize;
    bytes[central + 10] = 12;
    refused(&bytes, "compression method 12");
    // An end record that defers to zip64.
    let mut bytes = build(&[entry("b/a.json", b"{}")]);
    let end = bytes.len() - 22;
    bytes[end + 10..end + 12].copy_from_slice(&[0xFF, 0xFF]);
    refused(&bytes, "zip64");
}

#[test]
fn duplicates_and_caps_are_refused() {
    refused(
        &build(&[entry("b/Logo.png", b"1"), entry("b/logo.png", b"2")]),
        "appears twice",
    );
    let many: Vec<Built> = (0..1025).map(|i| entry(&format!("b/{i}"), b"")).collect();
    refused(&build(&many), "at most 1024 entries");
    let mut huge = entry("b/huge.bin", &[0u8; 64]);
    huge.deflate = true;
    huge.size = Some(600 * 1024 * 1024);
    refused(&build(&[huge]), "larger than 512 MiB");
    let mut folder = entry("b/assets/", b"x");
    folder.descriptor = false;
    refused(&build(&[folder]), "holds data");
}

#[test]
fn every_truncation_is_refused() {
    let mut deflated = entry("b/action.json", &b"{\"steps\":[1,2,3]}".repeat(8));
    deflated.deflate = true;
    let bytes = build(&[deflated, entry("b/assets/logo.png", b"PNGDATA")]);
    for length in 0..bytes.len() {
        let (result, _) = unpack_bytes(&bytes[..length]);
        assert!(result.is_err(), "a {length}-byte prefix was accepted");
    }
    let (result, _) = unpack_bytes(&bytes);
    result.unwrap();
}

#[test]
fn null_inputs_refuse() {
    let scratch = TempDir::new().unwrap();
    let path = c(scratch.path());
    let mut err = ptr::null_mut();
    assert!(!unsafe { dr_zip_pack_dir(ptr::null(), path.as_ptr(), &mut err) });
    assert_eq!(text(err), "dir is NULL");
    let mut err = ptr::null_mut();
    assert!(!unsafe { dr_zip_pack_dir(path.as_ptr(), ptr::null(), &mut err) });
    assert_eq!(text(err), "zip_path is NULL");
    let mut err = ptr::null_mut();
    assert!(!unsafe { dr_zip_unpack(ptr::null(), path.as_ptr(), &mut err) });
    assert_eq!(text(err), "zip_path is NULL");
    let mut err = ptr::null_mut();
    assert!(!unsafe { dr_zip_unpack(path.as_ptr(), ptr::null(), &mut err) });
    assert_eq!(text(err), "dest_dir is NULL");
    assert!(!unsafe { dr_zip_unpack(ptr::null(), ptr::null(), ptr::null_mut()) });
    assert!(!unsafe { dr_zip_pack_dir(ptr::null(), ptr::null(), ptr::null_mut()) });
}
