//! Basic layered PSD export through the C API: PackBits framing, the per-mode
//! preflight report, refusals, and a complete self-reimport round trip.
//!
//! Independent oracles only. The PackBits decoder and the record/channel
//! reader below are written here, from Adobe's published framing, and never
//! call the writer they check; the `psd` crate is the second, third-party
//! reader. The round trip's SOURCE document is an independent byte
//! construction imported through the existing PSD reader, so the export's
//! record walk is compared against a stack it did not build.

use std::ffi::{c_char, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::{BlendMode, DrDocument, LayerKind};
use doctored_core::ffi_psd::{
    dr_doc_open_psd, dr_doc_psd_export_report, dr_doc_save_psd, dr_psd_import_report,
};
use doctored_core::icc::IccProfile;
use doctored_core::style::LayerStyle;
use image::{GrayImage, Rgba, RgbaImage};
use serde_json::Value;
use tempfile::TempDir;

use crate::common;
use common::{cpath, take_err_string};

const LAYERS: i32 = 0;
const FLATTENED: i32 = 1;

// ------------------------------------------------------------- C plumbing --

fn export_report(doc: &DrDocument, mode: i32) -> Value {
    let mut err: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_doc_psd_export_report(doc, 0, mode, &mut err) };
    assert!(!json.is_null(), "report: {}", take_err_string(err));
    let text = unsafe { std::ffi::CStr::from_ptr(json) }
        .to_string_lossy()
        .into_owned();
    unsafe { doctored_core::ffi::dr_string_free(json) };
    serde_json::from_str(&text).expect("report JSON")
}

fn save(doc: &DrDocument, path: &std::path::Path, mode: i32) -> Result<(), String> {
    let c = cpath(path);
    let mut err: *mut c_char = ptr::null_mut();
    if unsafe { dr_doc_save_psd(doc, ptr::null(), c.as_ptr(), 0, mode, &mut err) } {
        assert!(err.is_null(), "err_out set on success");
        Ok(())
    } else {
        Err(take_err_string(err))
    }
}

fn import_report(path: &std::path::Path) -> Value {
    let c = cpath(path);
    let mut err: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_psd_import_report(c.as_ptr(), &mut err) };
    assert!(!json.is_null(), "import report: {}", take_err_string(err));
    let text = unsafe { std::ffi::CStr::from_ptr(json) }
        .to_string_lossy()
        .into_owned();
    unsafe { doctored_core::ffi::dr_string_free(json) };
    serde_json::from_str(&text).expect("import report JSON")
}

fn open_psd(path: &std::path::Path, mode: i32, fingerprint: &str) -> Box<DrDocument> {
    let c = cpath(path);
    let f = CString::new(fingerprint).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let doc = unsafe { dr_doc_open_psd(c.as_ptr(), mode, f.as_ptr(), &mut err) };
    assert!(!doc.is_null(), "open_psd({mode}): {}", take_err_string(err));
    unsafe { Box::from_raw(doc) }
}

fn strings(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .expect("array")
        .iter()
        .map(|v| v.as_str().expect("string").to_string())
        .collect()
}

// ------------------------------------------------- independent PSD readers --

/// The PackBits decoder this file checks the writer against — Adobe's rule,
/// written out directly: a non-negative control means `n + 1` literal bytes,
/// a negative one (never -128) means one byte repeated `1 - n` times.
fn unpack(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let control = bytes[i] as i8;
        i += 1;
        assert_ne!(control, -128, "the writer must never emit the -128 control");
        if control >= 0 {
            let n = control as usize + 1;
            out.extend_from_slice(&bytes[i..i + n]);
            i += n;
        } else {
            let n = (1 - i32::from(control)) as usize;
            out.extend(std::iter::repeat_n(bytes[i], n));
            i += 1;
        }
    }
    out
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
    fn take(&mut self, n: usize) -> &'a [u8] {
        let out = &self.bytes[self.offset..self.offset + n];
        self.offset += n;
        out
    }
    fn u16(&mut self) -> u16 {
        u16::from_be_bytes(self.take(2).try_into().unwrap())
    }
    fn u32(&mut self) -> u32 {
        u32::from_be_bytes(self.take(4).try_into().unwrap())
    }
    fn section(&mut self) -> &'a [u8] {
        let n = self.u32() as usize;
        self.take(n)
    }
}

/// One record as this file reads it back out of the bytes: the fields the
/// export promises, plus every channel's payload.
struct ReadRecord {
    bounds: [i32; 4],
    channels: Vec<(i16, Vec<u8>)>,
    blend: [u8; 4],
    opacity: u8,
    clipping: u8,
    flags: u8,
    mask: Option<(Vec<u8>, u8, u8)>,
    pascal: String,
    unicode: String,
    divider: Option<u32>,
    /// The record's `iOpa` body, when it writes one.
    fill: Option<Vec<u8>>,
}

struct ReadFile {
    width: u32,
    height: u32,
    channels: u16,
    depth: u16,
    color: u16,
    icc: Vec<u8>,
    resolution: Vec<u8>,
    records: Vec<ReadRecord>,
}

fn read_psd(bytes: &[u8]) -> ReadFile {
    let mut r = Reader::new(bytes);
    assert_eq!(r.take(4), b"8BPS");
    assert_eq!(r.u16(), 1);
    assert_eq!(r.take(6), [0; 6]);
    let channels = r.u16();
    let height = r.u32();
    let width = r.u32();
    let depth = r.u16();
    let color = r.u16();
    assert!(r.section().is_empty(), "RGB has no colour-mode data");
    let resources = r.section();
    let (mut icc, mut resolution) = (Vec::new(), Vec::new());
    let mut rr = Reader::new(resources);
    while rr.offset < resources.len() {
        assert_eq!(rr.take(4), b"8BIM");
        let id = rr.u16();
        assert_eq!(rr.take(2), [0, 0], "empty Pascal resource name");
        let body = rr.section();
        if !body.len().is_multiple_of(2) {
            rr.take(1);
        }
        match id {
            1039 => icc = body.to_vec(),
            1005 => resolution = body.to_vec(),
            other => panic!("unexpected image resource {other}"),
        }
    }
    let section = r.section();
    let mut sr = Reader::new(section);
    let info = sr.section();
    assert_eq!(sr.u32(), 0, "no global layer mask");
    assert_eq!(sr.offset, section.len());
    let mut ir = Reader::new(info);
    let declared = ir.u16() as i16;
    assert!(declared < 0, "merged alpha must be declared");
    let count = declared.unsigned_abs() as usize;
    let mut tables = Vec::with_capacity(count);
    let mut records = Vec::with_capacity(count);
    for _ in 0..count {
        let mut bounds = [0i32; 4];
        for edge in &mut bounds {
            *edge = ir.u32() as i32;
        }
        let n = ir.u16() as usize;
        let mut table = Vec::with_capacity(n);
        for _ in 0..n {
            let id = ir.u16() as i16;
            table.push((id, ir.u32() as usize));
        }
        assert_eq!(ir.take(4), b"8BIM");
        let blend: [u8; 4] = ir.take(4).try_into().unwrap();
        let fields = ir.take(4).to_vec();
        let extra = ir.section();
        let mut er = Reader::new(extra);
        let body = er.section();
        let mask = match body.len() {
            0 => None,
            20 => Some((body[..16].to_vec(), body[16], body[17])),
            other => panic!("unexpected mask body length {other}"),
        };
        assert!(er.section().is_empty(), "no Blend If ranges are written");
        let name_len = er.take(1)[0] as usize;
        let pascal = String::from_utf8(er.take(name_len).to_vec()).expect("utf-8 Pascal name");
        er.take(((name_len + 4) & !3) - 1 - name_len);
        let (mut unicode, mut divider, mut fill) = (String::new(), None, None);
        while er.offset < extra.len() {
            assert_eq!(er.take(4), b"8BIM");
            let key: [u8; 4] = er.take(4).try_into().unwrap();
            let block = er.section();
            assert!(block.len().is_multiple_of(2), "even block body");
            match &key {
                b"luni" => {
                    let units = u32::from_be_bytes(block[..4].try_into().unwrap()) as usize;
                    assert_eq!(block.len(), 4 + units * 2);
                    let utf16: Vec<u16> = block[4..]
                        .chunks_exact(2)
                        .map(|p| u16::from_be_bytes(p.try_into().unwrap()))
                        .collect();
                    unicode = String::from_utf16(&utf16).expect("utf-16 name");
                }
                b"lsct" => {
                    divider = Some(u32::from_be_bytes(block[..4].try_into().unwrap()));
                    if block.len() >= 12 {
                        assert_eq!(&block[4..8], b"8BIM");
                        assert_eq!(&block[8..12], &blend);
                    }
                }
                b"iOpa" => fill = Some(block.to_vec()),
                other => panic!("unexpected additional layer block {other:?}"),
            }
        }
        tables.push(table);
        records.push(ReadRecord {
            bounds,
            channels: Vec::new(),
            blend,
            opacity: fields[0],
            clipping: fields[1],
            flags: fields[2],
            mask,
            pascal,
            unicode,
            divider,
            fill,
        });
    }
    for (record, table) in records.iter_mut().zip(&tables) {
        for &(id, len) in table {
            record.channels.push((id, ir.take(len).to_vec()));
        }
    }
    assert!(info.len() - ir.offset <= 1, "layer info is fully claimed");
    ReadFile {
        width,
        height,
        channels,
        depth,
        color,
        icc,
        resolution,
        records,
    }
}

/// Decodes one RLE channel plane, checking each row's declared length, exact
/// decoded width and the writer's promised size bound.
fn decode_plane(bytes: &[u8], width: usize, height: usize) -> Vec<u8> {
    assert_eq!(&bytes[..2], &[0, 1], "planes are written PackBits");
    let mut offset = 2 + height * 2;
    let mut out = Vec::with_capacity(width * height);
    for y in 0..height {
        let len = u16::from_be_bytes(bytes[2 + y * 2..4 + y * 2].try_into().unwrap()) as usize;
        let row = &bytes[offset..offset + len];
        assert!(
            len <= width + width.div_ceil(128),
            "row {y} is {len} bytes, past the {} byte bound",
            width + width.div_ceil(128)
        );
        let decoded = unpack(row);
        assert_eq!(
            decoded.len(),
            width,
            "row {y} decodes to the declared width"
        );
        out.extend_from_slice(&decoded);
        offset += len;
    }
    assert_eq!(offset, bytes.len(), "no trailing plane bytes");
    out
}

// -------------------------------------------------- independent PSD source --

/// One record of the hand-built source PSD, written from Adobe's framing.
struct Src {
    bounds: [i32; 4],
    rgba: Vec<u8>,
    name: String,
    divider: u32,
    opacity: u8,
    flags: u8,
    blend: [u8; 4],
    mask: Option<([i32; 4], Vec<u8>, u8, bool)>,
}

impl Src {
    fn raster(name: &str, bounds: [i32; 4], rgba: Vec<u8>) -> Self {
        Self {
            bounds,
            rgba,
            name: name.to_string(),
            divider: 0,
            opacity: 255,
            flags: 8,
            blend: *b"norm",
            mask: None,
        }
    }
    fn folder(name: &str, divider: u32, blend: &[u8; 4]) -> Self {
        Self {
            bounds: [0; 4],
            rgba: Vec::new(),
            name: name.to_string(),
            divider,
            opacity: 255,
            flags: 24,
            blend: *blend,
            mask: None,
        }
    }
    fn channels(&self) -> Vec<(i16, Vec<u8>)> {
        let mut out = Vec::new();
        for (id, component) in [(-1i16, 3usize), (0, 0), (1, 1), (2, 2)] {
            let plane: Vec<u8> = self.rgba.chunks_exact(4).map(|p| p[component]).collect();
            out.push((id, [vec![0u8, 0], plane].concat()));
        }
        if let Some((_, coverage, _, _)) = &self.mask {
            out.push((-2, [vec![0u8, 0], coverage.clone()].concat()));
        }
        out
    }
    fn record(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for edge in self.bounds {
            out.extend_from_slice(&edge.to_be_bytes());
        }
        let channels = self.channels();
        out.extend_from_slice(&(channels.len() as u16).to_be_bytes());
        for (id, data) in &channels {
            out.extend_from_slice(&id.to_be_bytes());
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        }
        out.extend_from_slice(b"8BIM");
        out.extend_from_slice(&self.blend);
        out.extend_from_slice(&[self.opacity, 0, self.flags, 0]);
        let mut extra = Vec::new();
        match &self.mask {
            Some((bounds, _, outside, enabled)) => {
                extra.extend_from_slice(&20u32.to_be_bytes());
                for edge in bounds {
                    extra.extend_from_slice(&edge.to_be_bytes());
                }
                extra.extend_from_slice(&[*outside, u8::from(!enabled) * 2, 0, 0]);
            }
            None => extra.extend_from_slice(&0u32.to_be_bytes()),
        }
        extra.extend_from_slice(&0u32.to_be_bytes());
        extra.push(self.name.len() as u8);
        extra.extend_from_slice(self.name.as_bytes());
        while !extra.len().is_multiple_of(4) {
            extra.push(0);
        }
        if self.divider != 0 {
            let mut body = self.divider.to_be_bytes().to_vec();
            if matches!(self.divider, 1 | 2) {
                body.extend_from_slice(b"8BIM");
                body.extend_from_slice(&self.blend);
            }
            extra.extend_from_slice(b"8BIM");
            extra.extend_from_slice(b"lsct");
            extra.extend_from_slice(&(body.len() as u32).to_be_bytes());
            extra.extend_from_slice(&body);
        }
        out.extend_from_slice(&(extra.len() as u32).to_be_bytes());
        out.extend_from_slice(&extra);
        out
    }
}

fn source_psd(records: &[Src], width: u32, height: u32) -> Vec<u8> {
    let mut info = (-(records.len() as i32) as i16).to_be_bytes().to_vec();
    for record in records {
        info.extend_from_slice(&record.record());
    }
    for record in records {
        for (_, data) in record.channels() {
            info.extend_from_slice(&data);
        }
    }
    if !info.len().is_multiple_of(2) {
        info.push(0);
    }
    let mut section = (info.len() as u32).to_be_bytes().to_vec();
    section.extend_from_slice(&info);
    section.extend_from_slice(&0u32.to_be_bytes());
    let mut bytes = b"8BPS\0\x01\0\0\0\0\0\0".to_vec();
    bytes.extend_from_slice(&4u16.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&8u16.to_be_bytes());
    bytes.extend_from_slice(&3u16.to_be_bytes());
    bytes.extend_from_slice(&[0; 8]); // no colour-mode data, no image resources
    bytes.extend_from_slice(&(section.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&section);
    // Raw composite: four fully opaque planes, never read by layered import.
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&vec![200u8; width as usize * height as usize * 4]);
    bytes
}

fn export_fixture(name: &str, bytes: &[u8]) {
    if let Ok(directory) = std::env::var("DR_PSD_EXPORT_FIXTURES") {
        let directory = std::path::Path::new(&directory);
        std::fs::create_dir_all(directory).unwrap();
        std::fs::write(directory.join(format!("{name}.psd")), bytes).unwrap();
    }
}

// --------------------------------------------------------------- fixtures --

fn ramp(w: u32, h: u32, seed: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let i = x * 7 + y * 13 + seed;
            out.extend_from_slice(&[(i % 251) as u8, (i % 241) as u8, (i % 239) as u8, 255]);
        }
    }
    out
}

/// Partly transparent pixels that KEEP their colour under alpha 0 — the case
/// straight-alpha layer planes preserve and a matted composite cannot.
fn hidden_rgb(w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let a = [0u8, 1, 64, 127, 128, 200, 255][((x + y) % 7) as usize];
            out.extend_from_slice(&[17 + x as u8, 200 - y as u8, 99, a]);
        }
    }
    out
}

const LONG_NAME: &str = "レイヤー名前テスト・";

/// The round-trip source: nested open/closed groups with Pass Through and
/// Multiply, an empty group, a clipped group and a clipped layer, a hidden
/// layer, a negative offset, hidden RGB under partial alpha, a long Unicode
/// name, absolute raster masks with both outside defaults and a disabled one,
/// a group mask, Display P3 and 300 x 240 PPI.
fn round_trip_source() -> DrDocument {
    let (w, h) = (8u32, 6u32);
    let mut ghost = Src::raster("Ghost", [-2, -1, 1, 2], hidden_rgb(3, 3));
    ghost.flags = 8 | 2;
    ghost.mask = Some((
        [1, 2, 4, 5],
        (0..9).map(|i| (i * 28) as u8).collect(),
        0,
        false,
    ));
    let mut deep = Src::raster("Deep", [4, 5, 6, 7], ramp(2, 2, 5));
    deep.blend = *b"over";
    let mut inner = Src::folder("Inner", 2, b"mul ");
    inner.opacity = 128;
    let mut outer = Src::folder("Outer", 1, b"pass");
    outer.mask = Some((
        [0, 0, h as i32, w as i32],
        (0..(w * h)).map(|i| (i * 5 % 256) as u8).collect(),
        255,
        true,
    ));
    let mut top = Src::raster("Top", [0, 0, h as i32, w as i32], ramp(w, h, 3));
    top.mask = Some((
        [0, 0, h as i32, w as i32],
        (0..(w * h)).map(|i| (255 - i * 3 % 256) as u8).collect(),
        255,
        true,
    ));
    let records = vec![
        Src::raster("Base", [0, 0, h as i32, w as i32], ramp(w, h, 0)),
        Src::raster("Clip", [1, 2, 4, 6], ramp(4, 3, 9)),
        ghost,
        Src::folder("</Layer group>", 3, b"norm"),
        Src::folder("</Layer group>", 3, b"norm"),
        deep,
        inner,
        Src::folder("</Layer group>", 3, b"norm"),
        Src::folder("Empty", 1, b"norm"),
        outer,
        top,
    ];
    let bytes = source_psd(&records, w, h);
    let directory = TempDir::new().expect("tempdir");
    let path = directory.path().join("source.psd");
    std::fs::write(&path, &bytes).expect("write source");
    let report = import_report(&path);
    assert_eq!(report["layered_available"], Value::Bool(true));
    let fingerprint = report["source_fingerprint"].as_str().expect("fingerprint");
    let mut doc = *open_psd(&path, LAYERS, fingerprint);
    assert_eq!(doc.layers.len(), 8, "the source stack the export walks");
    let names: Vec<&str> = doc.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(
        names,
        ["Base", "Clip", "Ghost", "Deep", "Inner", "Empty", "Outer", "Top"]
    );
    assert_eq!(
        doc.layers.iter().map(|l| l.depth).collect::<Vec<_>>(),
        [0, 0, 0, 2, 1, 1, 0, 0]
    );
    // Import now retains group clipping; it never sets a long name. Both are
    // set here: they are exactly what the export must carry and disclose.
    doc.layers[1].clipped = true;
    doc.layers[6].clipped = true;
    doc.layers[7].name = LONG_NAME.repeat(20);
    assert!(doc.layers[7].name.len() > 255, "exercises name truncation");
    doc.profile = IccProfile::display_p3();
    doc.resolution = doctored_core::metadata::Resolution { x: 300.0, y: 240.0 };
    doc
}

// ------------------------------------------------------------------ tests --

#[test]
fn packbits_rows_decode_exactly_and_stay_inside_the_declared_bound() {
    let directory = TempDir::new().expect("tempdir");
    for width in [1u32, 128, 129, 257] {
        let height = 4u32;
        let mut pixels = RgbaImage::new(width, height);
        for x in 0..width {
            let x8 = (x % 256) as u8;
            // Row 0 pseudo-random, row 1 all equal, row 2 alternating, row 3
            // long runs broken by single samples — the four shapes the
            // encoder's run/literal decision has to get right.
            pixels.put_pixel(x, 0, Rgba([((x * 37 + 11) % 251) as u8, x8, 3, 255]));
            pixels.put_pixel(x, 1, Rgba([77, 77, 77, 255]));
            pixels.put_pixel(x, 2, Rgba([if x % 2 == 0 { 0 } else { 255 }, x8, 3, 255]));
            pixels.put_pixel(x, 3, Rgba([if x % 40 == 7 { 200 } else { 5 }, x8, 3, 255]));
        }
        let doc = DrDocument::from_pixels(pixels.clone());
        let path = directory.path().join(format!("packbits-{width}.psd"));
        save(&doc, &path, LAYERS).expect("save");
        let bytes = std::fs::read(&path).expect("read back");
        let file = read_psd(&bytes);
        assert_eq!(file.records.len(), 1);
        for (id, data) in &file.records[0].channels {
            let component = if *id == -1 { 3 } else { *id as usize };
            let decoded = decode_plane(data, width as usize, height as usize);
            let expected: Vec<u8> = pixels
                .as_raw()
                .chunks_exact(4)
                .map(|p| p[component])
                .collect();
            assert_eq!(decoded, expected, "plane {id} at width {width}");
        }
    }
}

#[test]
fn layered_export_round_trips_through_this_application() {
    let source = round_trip_source();
    let directory = TempDir::new().expect("tempdir");
    let path = directory.path().join("nested.psd");

    let report = export_report(&source, LAYERS);
    assert_eq!(report["available"], Value::Bool(true));
    assert_eq!(report["mode"], Value::String("layers".into()));
    assert_eq!(report["layer_count"], Value::from(5));
    assert_eq!(report["group_count"], Value::from(3));
    assert!(
        strings(&report, "losses").is_empty(),
        "{:?}",
        strings(&report, "losses")
    );
    assert!(strings(&report, "observations")
        .iter()
        .any(|o| o.starts_with("Group “Outer” is clipped;")));
    assert!(strings(&report, "observations")
        .iter()
        .any(|o| o.contains("Display P3") && o.contains("300")));

    save(&source, &path, LAYERS).expect("save layered");
    let bytes = std::fs::read(&path).expect("read back");
    export_fixture("nested", &bytes);
    assert!(
        (bytes.len() as u64) <= report["worst_case_bytes"].as_u64().expect("worst case"),
        "the preflight's worst case must bound the file it wrote"
    );

    // 1. The bytes themselves, read by this file's independent reader.
    let file = read_psd(&bytes);
    assert_eq!((file.width, file.height), (8, 6));
    assert_eq!((file.channels, file.depth, file.color), (4, 8, 3));
    assert_eq!(file.icc, source.profile.bytes().to_vec());
    assert_eq!(
        file.resolution,
        [
            &(300u32 * 65536).to_be_bytes()[..],
            &[0, 1, 0, 1],
            &(240u32 * 65536).to_be_bytes()[..],
            &[0, 1, 0, 1],
        ]
        .concat()
    );
    assert_eq!(file.records.len(), 11, "eight entries and three closers");
    let dividers: Vec<Option<u32>> = file.records.iter().map(|r| r.divider).collect();
    assert_eq!(
        dividers,
        [
            None,
            None,
            None,
            Some(3),
            Some(3),
            None,
            Some(2),
            Some(3),
            Some(1),
            Some(1),
            None
        ]
    );
    assert_eq!(file.records[10].unicode, source.layers[7].name);
    assert!(file.records[10].pascal.len() <= 255);
    assert!(source.layers[7].name.starts_with(&file.records[10].pascal));
    assert_eq!(file.records[9].blend, *b"pass");
    assert_eq!(file.records[9].clipping, 1, "the clipped group's flag");
    assert_eq!(file.records[6].opacity, 128);
    assert_eq!(file.records[6].blend, *b"mul ");
    assert_eq!(file.records[2].flags, 8 | 2, "the hidden layer");
    assert_eq!(file.records[2].bounds, [-2, -1, 1, 2]);
    for record in &file.records {
        let ids: Vec<i16> = record.channels.iter().map(|(id, _)| *id).collect();
        assert_eq!(&ids[..4], &[-1, 0, 1, 2]);
        assert_eq!(record.mask.is_some(), ids.contains(&-2));
    }

    // 2. The third-party reader.
    let parsed = psd::Psd::from_bytes(&bytes).expect("psd crate parses the export");
    assert_eq!(parsed.layers().len(), 5);
    assert_eq!(parsed.groups().len(), 3);
    let mut parsed_names: Vec<String> = parsed
        .layers()
        .iter()
        .map(|l| l.name().to_string())
        .chain(parsed.groups().values().map(|g| g.name().to_string()))
        .collect();
    parsed_names.sort();
    let mut expected: Vec<String> = source.layers.iter().map(|l| l.name.clone()).collect();
    expected.sort();
    assert_eq!(parsed_names, expected);

    // 3. This application's own importer.
    let back = import_report(&path);
    assert_eq!(back["layered_available"], Value::Bool(true));
    assert!(
        strings(&back, "verified_losses").is_empty(),
        "{:?}",
        strings(&back, "verified_losses")
    );
    let fingerprint = back["source_fingerprint"].as_str().expect("fingerprint");
    let reopened = open_psd(&path, LAYERS, fingerprint);
    assert!(reopened.layers[6].clipped, "the clipped group round-trips");
    assert_eq!(reopened.layers.len(), source.layers.len());
    assert_eq!(reopened.width, source.width);
    assert_eq!(reopened.height, source.height);
    assert_eq!(reopened.profile.bytes(), source.profile.bytes());
    assert_eq!(reopened.resolution, source.resolution);
    for (index, (was, now)) in source.layers.iter().zip(&reopened.layers).enumerate() {
        let at = format!("layer {index} (“{}”)", was.name);
        assert_eq!(now.name, was.name, "name of {at}");
        assert_eq!(now.kind, was.kind, "kind of {at}");
        assert_eq!(now.depth, was.depth, "depth of {at}");
        assert_eq!(now.blend, was.blend, "blend of {at}");
        assert_eq!(now.visible, was.visible, "visible of {at}");
        assert_eq!(now.open, was.open, "open of {at}");
        assert_eq!(
            (now.opacity * 255.0).round(),
            (was.opacity * 255.0).round(),
            "opacity byte of {at}"
        );
        assert_eq!(now.offset, was.offset, "offset of {at}");
        if was.kind == LayerKind::Raster {
            assert_eq!(
                now.pixels.dimensions(),
                was.pixels.dimensions(),
                "size {at}"
            );
            assert_eq!(
                now.pixels.contiguous_image().as_raw(),
                was.pixels.contiguous_image().as_raw(),
                "pixels of {at}"
            );
        }
        assert_eq!(
            now.mask.as_ref().map(|m| m.as_raw().clone()),
            was.mask.as_ref().map(|m| m.as_raw().clone()),
            "mask samples of {at}"
        );
        assert_eq!(
            now.mask.as_ref().map(|m| m.dimensions()),
            was.mask.as_ref().map(|m| m.dimensions()),
            "mask size of {at}"
        );
        assert_eq!(now.mask_enabled, was.mask_enabled, "mask enabled of {at}");
        assert_eq!(
            now.mask_properties.offset, was.mask_properties.offset,
            "mask offset of {at}"
        );
        assert_eq!(
            now.mask_properties.outside_coverage, was.mask_properties.outside_coverage,
            "mask outside of {at}"
        );
        assert_eq!(now.clipped, was.clipped, "clipped of {at}");
    }
}

#[test]
fn flattened_export_writes_one_raster_and_a_recoverable_preview() {
    let (w, h) = (7u32, 5u32);
    let pixels = RgbaImage::from_raw(w, h, hidden_rgb(w, h)).expect("fixture");
    let source = DrDocument::from_pixels(pixels);
    let directory = TempDir::new().expect("tempdir");
    let path = directory.path().join("flat.psd");

    let report = export_report(&source, FLATTENED);
    assert_eq!(report["available"], Value::Bool(true));
    assert_eq!(report["layer_count"], Value::from(1));
    assert_eq!(report["group_count"], Value::from(0));
    save(&source, &path, FLATTENED).expect("save flattened");
    let bytes = std::fs::read(&path).expect("read back");
    export_fixture("flat", &bytes);
    let file = read_psd(&bytes);
    assert_eq!(file.records.len(), 1);
    assert_eq!(file.records[0].pascal, "Composite");
    assert_eq!(file.records[0].divider, None);

    let composite = source.flattened().expect("flatten");
    let back = import_report(&path);
    let fingerprint = back["source_fingerprint"].as_str().expect("fingerprint");
    let layered = open_psd(&path, LAYERS, fingerprint);
    assert_eq!(layered.layers.len(), 1);
    assert_eq!(
        layered.layers[0].pixels.contiguous_image().as_raw(),
        composite.as_raw(),
        "the layer plane is raw, not matted"
    );
    let stored = open_psd(&path, FLATTENED, fingerprint);
    let recovered = &stored.layers[0].pixels;
    assert!(
        composite
            .as_raw()
            .chunks_exact(4)
            .any(|p| (1..128).contains(&p[3])),
        "the fixture must contain low alphas"
    );
    for (was, now) in composite
        .as_raw()
        .chunks_exact(4)
        .zip(recovered.contiguous_image().as_raw().chunks_exact(4))
    {
        assert_eq!(now[3], was[3], "alpha survives the matte exactly");
        let tolerance = match was[3] {
            0 | 255 => 0,
            a => 255u32.div_ceil(2 * u32::from(a)),
        };
        for channel in 0..3 {
            if was[3] == 0 {
                continue; // alpha 0 has no recoverable colour; both canonicalize
            }
            let delta = u32::from(was[channel]).abs_diff(u32::from(now[channel]));
            assert!(
                delta <= tolerance,
                "channel {channel} at alpha {}: {} vs {} (tolerance {tolerance})",
                was[3],
                was[channel],
                now[channel]
            );
        }
    }
}

#[test]
fn flattening_a_layered_document_is_reported_as_a_loss() {
    let source = round_trip_source();
    let report = export_report(&source, FLATTENED);
    assert_eq!(report["available"], Value::Bool(true));
    assert!(strings(&report, "losses")
        .iter()
        .any(|l| l.contains("one composited raster layer")));
    let plain = DrDocument::from_pixels(RgbaImage::new(4, 4));
    assert!(strings(&export_report(&plain, FLATTENED), "losses").is_empty());
}

#[test]
fn every_refusal_is_reported_and_leaves_an_existing_file_untouched() {
    let directory = TempDir::new().expect("tempdir");
    let sentinel = b"not a psd".to_vec();

    // An adjustment layer is no longer among the refusals: one without a
    // Photoshop record is baked (`psd_adjustment_bake.rs`).
    // A style is WRITTEN as Photoshop layer effects; a styled RASTER whose
    // style Photoshop cannot hold is rasterized instead, and a styled GROUP —
    // whose effects cannot be rasterized without flattening it — is the one
    // style the layered mode refuses.
    let (mut styled, group, _, _) = DrDocument::from_pixels(RgbaImage::new(4, 4))
        .group_layers(&[0], "Set")
        .expect("group");
    styled.layers[group].style = Some(Arc::new(
        LayerStyle::from_json(r#"{"effects":[{"type":"stroke","size":0.5}]}"#)
            .expect("a style below Photoshop's stroke minimum"),
    ));

    let mut feathered = DrDocument::from_pixels(RgbaImage::new(4, 4));
    feathered.layers[0].mask = Some(Arc::new(GrayImage::new(4, 4)));
    feathered.layers[0].mask_properties.feather = 2.0;

    let wide = DrDocument::from_pixels(RgbaImage::new(30_001, 1));

    for (name, doc, fragment) in [
        ("style", &styled, "has a layer style"),
        ("feather", &feathered, "density or feather"),
        ("wide", &wide, "30000 pixels per side"),
    ] {
        let report = export_report(doc, LAYERS);
        assert_eq!(report["available"], Value::Bool(false), "{name}");
        let refusal = report["refusal"].as_str().expect("refusal");
        assert!(refusal.contains(fragment), "{name}: {refusal}");
        let path = directory.path().join(format!("{name}.psd"));
        std::fs::write(&path, &sentinel).expect("sentinel");
        let error = save(doc, &path, LAYERS).expect_err("refused");
        assert_eq!(error, refusal, "{name} saves with its own refusal");
        assert_eq!(std::fs::read(&path).expect("still there"), sentinel);
    }

    // A file-size limit refuses both modes without reading any pixel.
    let doc = DrDocument::from_pixels(RgbaImage::new(64, 64));
    for mode in [LAYERS, FLATTENED] {
        let tiny: Value =
            serde_json::from_str(&doc.psd_export_report_with_limits(mode, 1024, u64::MAX))
                .expect("json");
        assert_eq!(tiny["available"], Value::Bool(false));
        assert!(tiny["refusal"]
            .as_str()
            .expect("refusal")
            .contains("past the format's"));
    }
}

#[test]
fn a_file_past_the_reopen_envelope_is_a_loss_and_still_exports() {
    let doc = round_trip_source();
    let report: Value =
        serde_json::from_str(&doc.psd_export_report_with_limits(LAYERS, u64::MAX, 1))
            .expect("json");
    assert_eq!(report["available"], Value::Bool(true));
    assert!(strings(&report, "losses")
        .iter()
        .any(|l| l.contains("2 GiB layered import envelope")));
    let directory = TempDir::new().expect("tempdir");
    let path = directory.path().join("envelope.psd");
    save(&doc, &path, LAYERS).expect("the envelope is a report entry, never a refusal");
    // The production report does NOT carry it: this document is tiny.
    assert!(!strings(&export_report(&doc, LAYERS), "losses")
        .iter()
        .any(|l| l.contains("import envelope")));
}

#[test]
fn null_and_out_of_range_arguments_are_refused() {
    let doc = DrDocument::from_pixels(RgbaImage::new(2, 2));
    let directory = TempDir::new().expect("tempdir");
    let path = cpath(&directory.path().join("never.psd"));

    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe { dr_doc_psd_export_report(ptr::null(), 0, LAYERS, &mut err) }.is_null());
    assert!(take_err_string(err).contains("NULL"));

    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe { dr_doc_psd_export_report(&doc, 0, 7, &mut err) }.is_null());
    assert!(take_err_string(err).contains("unknown PSD export mode"));

    assert!(unsafe { dr_doc_psd_export_report(ptr::null(), 0, LAYERS, ptr::null_mut()) }.is_null());

    let mut err: *mut c_char = ptr::null_mut();
    assert!(!unsafe {
        dr_doc_save_psd(ptr::null(), ptr::null(), path.as_ptr(), 0, LAYERS, &mut err)
    });
    assert!(take_err_string(err).contains("NULL"));

    let mut err: *mut c_char = ptr::null_mut();
    assert!(!unsafe { dr_doc_save_psd(&doc, ptr::null(), ptr::null(), 0, LAYERS, &mut err) });
    assert!(take_err_string(err).contains("path is NULL"));

    let mut err: *mut c_char = ptr::null_mut();
    assert!(!unsafe { dr_doc_save_psd(&doc, ptr::null(), path.as_ptr(), 0, -1, &mut err) });
    assert!(take_err_string(err).contains("unknown PSD export mode"));

    assert!(!unsafe {
        dr_doc_save_psd(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            LAYERS,
            ptr::null_mut(),
        )
    });
    assert!(!std::path::Path::new(directory.path().join("never.psd").as_path()).exists());

    // Dissolve exports too, under Photoshop's own `diss` key.
    let dissolve = doc
        .with_layer_blend_mode(0, BlendMode::Dissolve)
        .expect("blend");
    assert_eq!(
        export_report(&dissolve, LAYERS)["available"],
        Value::Bool(true)
    );
}

/// Photoshop reads a Color Burn, Color Dodge or Difference PIXEL record
/// without `iOpa` as a pre-Fill legacy layer (#578 take1: opacity 153 came
/// back as opacity 255, Fill 153), so those three always write it, at 255
/// too; every other mode, and a group section in any mode, writes it only
/// below 100 % Fill, as before.
#[test]
fn legacy_fill_modes_always_write_iopa_on_pixel_records() {
    let mut doc = round_trip_source();
    let index = |doc: &DrDocument, name: &str| {
        doc.layers
            .iter()
            .position(|l| l.name == name)
            .expect("layer")
    };
    let cases = [
        ("Base", BlendMode::ColorBurn, 1.0, Some(255u8)),
        ("Clip", BlendMode::ColorDodge, 1.0, Some(255)),
        ("Deep", BlendMode::Difference, 0.6, Some(153)),
        ("Ghost", BlendMode::Multiply, 1.0, None),
    ];
    for (name, mode, fill, _) in cases {
        let at = index(&doc, name);
        doc.layers[at].blend = mode;
        if fill < 1.0 {
            doc.layers[at].style = Some(Arc::new(
                LayerStyle::from_json(&format!(r#"{{"fill_opacity":{fill}}}"#)).expect("style"),
            ));
        }
    }
    let inner = index(&doc, "Inner");
    doc.layers[inner].blend = BlendMode::Difference;
    let directory = TempDir::new().expect("tempdir");
    let path = directory.path().join("legacy.psd");
    save(&doc, &path, LAYERS).expect("save");
    let file = read_psd(&std::fs::read(&path).expect("read"));
    for (name, _, _, byte) in cases {
        let record = file
            .records
            .iter()
            .find(|r| r.unicode == name)
            .expect("record");
        assert_eq!(record.fill, byte.map(|b| vec![b, 0, 0, 0]), "{name}'s iOpa");
    }
    let group = file
        .records
        .iter()
        .find(|r| r.unicode == "Inner")
        .expect("group record");
    assert_eq!(&group.blend, b"diff");
    assert_eq!(group.fill, None, "a group section keeps today's blocks");
}

/// #578's per-mode table as the layered report states it (take1-3,
/// `scripts/photoshop-probe/psd_appearance_578_take.py --rejudge`): a
/// QUALIFIED mode (Overlay) and a mode qualified at 100 % Fill used at full
/// Fill (Difference) are named nowhere; an OUT mode (Divide, Dissolve) is
/// named as unqualified in Photoshop; a full-Fill mode below 100 % Fill
/// (Hard Mix) is named; Dissolve's pattern, a clipping base below 100 % Fill
/// and a clipped group each get their own sentence.
#[test]
fn blend_observations_follow_the_qualified_tables() {
    let mut doc = round_trip_source();
    let index = |doc: &DrDocument, name: &str| {
        doc.layers
            .iter()
            .position(|l| l.name == name)
            .expect("layer")
    };
    let fill = |f: f32| {
        Some(Arc::new(
            LayerStyle::from_json(&format!(r#"{{"fill_opacity":{f}}}"#)).expect("style"),
        ))
    };
    let base = index(&doc, "Base");
    doc.layers[base].blend = BlendMode::Dissolve;
    doc.layers[base].style = fill(0.5);
    let ghost = index(&doc, "Ghost");
    doc.layers[ghost].blend = BlendMode::Divide;
    let deep = index(&doc, "Deep");
    doc.layers[deep].blend = BlendMode::HardMix;
    doc.layers[deep].style = fill(0.6);
    let inner = index(&doc, "Inner");
    doc.layers[inner].blend = BlendMode::Overlay;
    let top = doc.layers.len() - 1;
    doc.layers[top].blend = BlendMode::Difference;

    let observations = strings(&export_report(&doc, LAYERS), "observations");
    for sentence in [
        "Blend modes outside the qualified set are used (Dissolve, Divide); they are written with Photoshop keys; their appearance in Photoshop is not qualified.",
        "Blend modes qualified in Photoshop only at 100 % Fill are used below it (HardMix); their appearance in Photoshop is not qualified there.",
        "Dissolve's dither is Doctored's own; Photoshop draws its own pattern at the same density.",
        "Layer “Base” is a clipping base below 100 % Fill; the layers clipped to it are not qualified in Photoshop.",
        "Group “Outer” is clipped; it reopens here clipped, but Photoshop reads a clipped group as unclipped and composites it over every layer beneath it.",
    ] {
        assert!(
            observations.iter().any(|o| o == sentence),
            "{sentence}\n{observations:#?}"
        );
    }
    for qualified in ["Overlay", "Difference"] {
        assert!(
            !observations.iter().any(|o| o.contains(qualified)),
            "{qualified} is qualified: {observations:#?}"
        );
    }
}

/// The 8-bit writer's exact bytes, guarded against every later change to the
/// shared encoder. The goldens are regenerated ONLY with an intentional 8-bit
/// writer change, by the same command that produced them:
/// `DR_PSD_EXPORT_FIXTURES=tests/fixtures/psd_10c make t F=psd_export`.
#[test]
fn eight_bit_writer_bytes_match_the_committed_goldens() {
    let directory = TempDir::new().expect("tempdir");

    let layered = directory.path().join("nested.psd");
    save(&round_trip_source(), &layered, LAYERS).expect("save layered");
    compare_golden(
        "nested",
        &std::fs::read(&layered).expect("read layered"),
        include_bytes!("fixtures/psd_10c/nested.psd"),
    );

    let flat = directory.path().join("flat.psd");
    let pixels = RgbaImage::from_raw(7, 5, hidden_rgb(7, 5)).expect("fixture");
    save(&DrDocument::from_pixels(pixels), &flat, FLATTENED).expect("save flattened");
    compare_golden(
        "flat",
        &std::fs::read(&flat).expect("read flattened"),
        include_bytes!("fixtures/psd_10c/flat.psd"),
    );
}

fn compare_golden(name: &str, wrote: &[u8], golden: &[u8]) {
    if wrote == golden {
        return;
    }
    let offset = wrote
        .iter()
        .zip(golden)
        .position(|(a, b)| a != b)
        .unwrap_or(wrote.len().min(golden.len()));
    panic!(
        "{name}.psd changed: wrote {} bytes, golden has {} bytes; first mismatch at offset {offset}",
        wrote.len(),
        golden.len()
    );
}
