//! Independent uncompressed PNG/classic-TIFF fixtures and libpng/libtiff
//! interoperability, with exact unsigned16 samples and metadata/refusal checks.
use std::io::Cursor;
use std::process::Command;
use std::sync::Arc;

use crate::common;
use common::probe::{cancelled_parked_at, entry_then_cancel, parked_line};

use doctored_core::doc_color::{
    CARRIES_EXIF, CARRIES_IPTC, CARRIES_PROFILE, CARRIES_RESOLUTION, CARRIES_XMP,
};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_codec::{self, PrecisionFormat};
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};

fn crc(bytes: &[u8]) -> u32 {
    let mut value = !0_u32;
    for byte in bytes {
        value ^= u32::from(*byte);
        for _ in 0..8 {
            value = (value >> 1) ^ if value & 1 != 0 { 0xedb88320 } else { 0 };
        }
    }
    !value
}
fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut result = (data.len() as u32).to_be_bytes().to_vec();
    result.extend(kind);
    result.extend(data);
    result.extend(crc(&result[4..]).to_be_bytes());
    result
}
fn stored_zlib(raw: &[u8]) -> Vec<u8> {
    let mut result = vec![0x78, 0x01];
    let mut blocks = raw.chunks(65535).peekable();
    while let Some(block) = blocks.next() {
        result.push(u8::from(blocks.peek().is_none()));
        let len = block.len() as u16;
        result.extend(len.to_le_bytes());
        result.extend((!len).to_le_bytes());
        result.extend(block);
    }
    let (mut a, mut b) = (1_u32, 0_u32);
    for &v in raw {
        a = (a + u32::from(v)) % 65521;
        b = (b + a) % 65521;
    }
    result.extend(((b << 16) | a).to_be_bytes());
    result
}
fn png(width: u32, height: u32, pixels: &[[u16; 4]], channels: u8, extra: Vec<Vec<u8>>) -> Vec<u8> {
    let mut header = width.to_be_bytes().to_vec();
    header.extend(height.to_be_bytes());
    header.extend([16, if channels == 4 { 6 } else { 2 }, 0, 0, 0]);
    let mut raw = Vec::new();
    for row in pixels.chunks(width as usize) {
        raw.push(0);
        for pixel in row {
            for sample in &pixel[..channels as usize] {
                raw.extend(sample.to_be_bytes());
            }
        }
    }
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend(chunk(b"IHDR", &header));
    for item in extra {
        bytes.extend(item);
    }
    bytes.extend(chunk(b"IDAT", &stored_zlib(&raw)));
    bytes.extend(chunk(b"IEND", &[]));
    bytes
}

type Field = (u16, u16, u32, Vec<u8>);
struct Tiff {
    bytes: Vec<u8>,
    be: bool,
}
impl Tiff {
    fn new(be: bool) -> Self {
        Self {
            bytes: if be {
                b"MM\0*\0\0\0\0".to_vec()
            } else {
                b"II*\0\0\0\0\0".to_vec()
            },
            be,
        }
    }
    fn u16(&self, v: u16) -> Vec<u8> {
        if self.be {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
        .to_vec()
    }
    fn u32(&self, v: u32) -> Vec<u8> {
        if self.be {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
        .to_vec()
    }
    fn root(mut self, mut fields: Vec<Field>) -> Vec<u8> {
        fields.sort_by_key(|f| f.0);
        if !self.bytes.len().is_multiple_of(2) {
            self.bytes.push(0);
        }
        let at = self.bytes.len();
        let offset = self.u32(at as u32);
        self.bytes[4..8].copy_from_slice(&offset);
        self.bytes.extend(self.u16(fields.len() as u16));
        self.bytes.resize(at + 2 + fields.len() * 12 + 4, 0);
        for (i, (tag, kind, count, data)) in fields.into_iter().enumerate() {
            let p = at + 2 + 12 * i;
            let (tag, kind, count) = (self.u16(tag), self.u16(kind), self.u32(count));
            self.bytes[p..p + 2].copy_from_slice(&tag);
            self.bytes[p + 2..p + 4].copy_from_slice(&kind);
            self.bytes[p + 4..p + 8].copy_from_slice(&count);
            if data.len() <= 4 {
                self.bytes[p + 8..p + 8 + data.len()].copy_from_slice(&data);
            } else {
                if !self.bytes.len().is_multiple_of(2) {
                    self.bytes.push(0);
                }
                let address = self.u32(self.bytes.len() as u32);
                self.bytes[p + 8..p + 12].copy_from_slice(&address);
                self.bytes.extend(data);
            }
        }
        self.bytes
    }
}
fn density_fields(t: &Tiff, orientation: u16) -> Vec<Field> {
    vec![
        (274, 3, 1, t.u16(orientation)),
        (282, 5, 1, [t.u32(300), t.u32(1)].concat()),
        (283, 5, 1, [t.u32(150), t.u32(1)].concat()),
        (296, 3, 1, t.u16(2)),
    ]
}
fn exif(orientation: u16) -> Vec<u8> {
    let t = Tiff::new(false);
    let fields = density_fields(&t, orientation);
    t.root(fields)
}
fn tiff_fixture(
    pixels: &[[u16; 4]],
    channels: u8,
    orientation: u16,
    be: bool,
    extra_alpha: u16,
) -> Vec<u8> {
    let mut t = Tiff::new(be);
    for pixel in pixels {
        for &v in &pixel[..channels as usize] {
            t.bytes.extend(t.u16(v));
        }
    }
    let mut fields = density_fields(&t, orientation);
    fields.extend([
        (256, 4, 1, t.u32(3)),
        (257, 4, 1, t.u32(2)),
        (
            258,
            3,
            u32::from(channels),
            t.u16(16).repeat(channels as usize),
        ),
        (259, 3, 1, t.u16(1)),
        (262, 3, 1, t.u16(2)),
        (273, 4, 1, t.u32(8)),
        (277, 3, 1, t.u16(u16::from(channels))),
        (278, 4, 1, t.u32(2)),
        (
            279,
            4,
            1,
            t.u32((pixels.len() * usize::from(channels) * 2) as u32),
        ),
        (284, 3, 1, t.u16(1)),
        (339, 3, 1, t.u16(1)),
    ]);
    if channels == 4 {
        fields.push((338, 3, 1, t.u16(extra_alpha)));
    }
    t.root(fields)
}

fn source(pixels: Vec<[u16; 4]>, width: u32) -> PrecisionImage {
    PrecisionImage::from_rgba16(
        width,
        pixels.len() as u32 / width,
        pixels,
        IccProfile::display_p3(),
        None,
    )
    .unwrap()
}
fn decode(bytes: &[u8]) -> precision_codec::DecodedPrecision {
    precision_codec::decode(
        bytes,
        "independent fixture",
        REFERENCE_WORKING_BYTES,
        0,
        None,
    )
    .unwrap()
}

#[test]
fn independent_rgb_rgba_and_both_tiff_endian_orders_preserve_all_orientation_permutations() {
    let pixels: Vec<_> = (0..6)
        .map(|i| {
            [
                30000 + i,
                1000 + i,
                62000 - i,
                if i == 0 { 0 } else { 17 + i },
            ]
        })
        .collect();
    let orders = [
        vec![0, 1, 2, 3, 4, 5],
        vec![2, 1, 0, 5, 4, 3],
        vec![5, 4, 3, 2, 1, 0],
        vec![3, 4, 5, 0, 1, 2],
        vec![0, 3, 1, 4, 2, 5],
        vec![3, 0, 4, 1, 5, 2],
        vec![5, 2, 4, 1, 3, 0],
        vec![2, 5, 1, 4, 0, 3],
    ];
    for channels in [3, 4] {
        for orientation in 1..=8 {
            let bytes = png(
                3,
                2,
                &pixels,
                channels,
                vec![chunk(b"eXIf", &exif(orientation))],
            );
            let fixtures = [
                bytes,
                tiff_fixture(&pixels, channels, orientation, false, 2),
                tiff_fixture(&pixels, channels, orientation, true, 2),
            ];
            for fixture in fixtures {
                let result = decode(&fixture);
                let expected: Vec<_> = orders[orientation as usize - 1]
                    .iter()
                    .map(|&i| {
                        let mut p = pixels[i];
                        if channels == 3 {
                            p[3] = 65535;
                        }
                        p
                    })
                    .collect();
                assert_eq!(result.image.to_rgba16().ok().unwrap(), expected);
                let dims = if orientation >= 5 { (2, 3) } else { (3, 2) };
                assert_eq!(result.image.dimensions(), dims);
                assert_eq!(
                    result.resolution,
                    if orientation >= 5 {
                        Resolution { x: 150., y: 300. }
                    } else {
                        Resolution { x: 300., y: 150. }
                    }
                );
            }
        }
    }
}

#[test]
fn true_sixteen_bit_exports_reopen_through_independent_imagemagick_and_retain_profile() {
    let directory = tempfile::tempdir().unwrap();
    let source = source(
        (0..=u16::MAX)
            .map(|i| [i, 65535 - i, i.wrapping_add(1), 65535])
            .collect(),
        256,
    );
    let has_magick = Command::new("magick").arg("-version").output().is_ok();
    if !has_magick {
        eprintln!("ImageMagick unavailable; external-reader assertions skipped");
    }
    for (format, name) in [
        (PrecisionFormat::Png, "codes.png"),
        (PrecisionFormat::Tiff, "codes.tiff"),
    ] {
        let path = directory.path().join(name);
        let carried = precision_codec::save(
            &source,
            &Metadata::default(),
            Resolution { x: 300., y: 150. },
            path.to_str().unwrap(),
            format,
            true,
            false,
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap();
        assert_eq!(carried, CARRIES_PROFILE | CARRIES_RESOLUTION);
        let result = decode(&std::fs::read(&path).unwrap());
        assert_eq!(result.image.to_rgba16().ok(), source.to_rgba16().ok());
        assert_eq!(result.image.profile().bytes(), source.profile().bytes());
        assert!((result.resolution.x - 300.).abs() < 0.02);
        assert!((result.resolution.y - 150.).abs() < 0.02);
        if has_magick {
            let read = Command::new("magick")
                .arg(&path)
                .args(["-depth", "16", "-endian", "LSB", "rgba:-"])
                .output()
                .unwrap();
            assert!(
                read.status.success(),
                "{}",
                String::from_utf8_lossy(&read.stderr)
            );
            let samples: Vec<_> = read
                .stdout
                .chunks_exact(2)
                .map(|p| u16::from_le_bytes([p[0], p[1]]))
                .collect();
            let expected: Vec<_> = source
                .to_rgba16()
                .ok()
                .unwrap()
                .iter()
                .flatten()
                .copied()
                .collect();
            assert_eq!(samples, expected, "independent reader of {name}");
        }
        if format == PrecisionFormat::Tiff {
            let mut reader =
                tiff::decoder::Decoder::new(FileReader::new(std::fs::read(&path).unwrap()))
                    .unwrap();
            assert_eq!(reader.colortype().unwrap(), tiff::ColorType::RGBA(16));
            assert_eq!(
                reader
                    .get_tag_u16_vec(tiff::tags::Tag::ExtraSamples)
                    .unwrap(),
                vec![2]
            );
        }
    }
}
type FileReader = Cursor<Vec<u8>>;

#[test]
fn shared_metadata_policy_reports_per_packet_losses_and_normalizes_orientation() {
    let directory = tempfile::tempdir().unwrap();
    let image = source(vec![[30001, 40001, 20001, 1]; 6], 3);
    let xmp:Arc<[u8]>=Arc::from(&b"<x:xmpmeta xmlns:x='adobe:ns:meta/' xmlns:tiff='http://ns.adobe.com/tiff/1.0/'><tiff:Orientation>6</tiff:Orientation></x:xmpmeta>"[..]);
    let mut iptc = b"8BIM\x04\x04\0\0".to_vec();
    iptc.extend(6_u32.to_be_bytes());
    iptc.extend(b"\x1c\x02\x19\0\x01X");
    let metadata = Metadata {
        exif: Some(Arc::from(exif(6))),
        xmp: Some(xmp.clone()),
        iptc: Some(Arc::from(iptc)),
    };
    for (format, name) in [
        (PrecisionFormat::Png, "metadata.png"),
        (PrecisionFormat::Tiff, "metadata.tiff"),
    ] {
        let path = directory.path().join(name);
        let carried = precision_codec::save(
            &image,
            &metadata,
            Resolution { x: 240., y: 120. },
            path.to_str().unwrap(),
            format,
            true,
            false,
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap();
        let expected = CARRIES_PROFILE
            | CARRIES_EXIF
            | CARRIES_XMP
            | CARRIES_RESOLUTION
            | if format == PrecisionFormat::Tiff {
                CARRIES_IPTC
            } else {
                0
            };
        assert_eq!(carried, expected);
        let result = decode(&std::fs::read(&path).unwrap());
        assert_eq!(result.image.to_rgba16().ok(), image.to_rgba16().ok());
        assert!(
            String::from_utf8_lossy(result.metadata.xmp.as_deref().unwrap())
                .contains(">1</tiff:Orientation>")
        );
        assert_eq!(result.image.dimensions(), (3, 2));
        assert!((result.resolution.x - 240.).abs() < 0.02);
        let stripped = precision_codec::save(
            &image,
            &metadata,
            Resolution::default(),
            path.to_str().unwrap(),
            format,
            false,
            true,
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap();
        assert_eq!(stripped, CARRIES_RESOLUTION);
        let stripped = decode(&std::fs::read(&path).unwrap());
        assert!(stripped.metadata.xmp.is_none() && stripped.metadata.iptc.is_none());
        if format == PrecisionFormat::Png {
            assert!(stripped.metadata.exif.is_none());
        }
        // TIFF's own orientation/PPI tags are rebuilt as an EXIF packet on
        // import even when no inherited packet was exported.
        assert!((stripped.resolution.x - 72.0).abs() < 0.02);
    }
    let bad = Metadata {
        exif: Some(Arc::from(&b"malformed"[..])),
        xmp: None,
        iptc: None,
    };
    let path = directory.path().join("bad-exif.tiff");
    let carried = precision_codec::save(
        &image,
        &bad,
        Resolution::default(),
        path.to_str().unwrap(),
        PrecisionFormat::Tiff,
        true,
        false,
        REFERENCE_WORKING_BYTES,
        0,
        None,
    )
    .unwrap();
    assert_eq!(carried, CARRIES_PROFILE | CARRIES_RESOLUTION);
    assert_eq!(metadata.xmp.as_deref(), Some(&*xmp));
}

#[test]
fn typed_depth_alpha_profile_and_container_refusals_never_promote_or_convert() {
    let pixels = vec![[30001, 20001, 10001, 65535]; 6];
    let rgba = png(3, 2, &pixels, 4, vec![]);
    let mut byte_header = rgba.clone();
    byte_header[24] = 8;
    let check = crc(&byte_header[12..29]);
    byte_header[29..33].copy_from_slice(&check.to_be_bytes());
    assert!(
        precision_codec::decode(&byte_header, "8-bit", REFERENCE_WORKING_BYTES, 0, None)
            .err()
            .unwrap()
            .contains("depth")
    );
    // A usable bare gAMA is the profile it describes (`png_declared`); a
    // zero one declares nothing and refuses.
    let gamma = png(3, 2, &pixels, 4, vec![chunk(b"gAMA", &0_u32.to_be_bytes())]);
    assert!(
        precision_codec::decode(&gamma, "gamma", REFERENCE_WORKING_BYTES, 0, None)
            .err()
            .unwrap()
            .contains("profile")
    );
    let mut icc = b"invalid\0\0".to_vec();
    icc.extend(stored_zlib(b"not an ICC profile"));
    let invalid = png(3, 2, &pixels, 4, vec![chunk(b"iCCP", &icc)]);
    assert!(
        precision_codec::decode(&invalid, "invalid ICC", REFERENCE_WORKING_BYTES, 0, None).is_err()
    );
    // A BigTIFF header now reaches the TIFF decoder; a truncated one
    // still refuses, through the decoder rather than as an unknown format.
    assert!(precision_codec::decode(
        b"II+\0\x08\0\0\0",
        "other",
        REFERENCE_WORKING_BYTES,
        0,
        None
    )
    .is_err());
    for format in [b"JPEG".as_slice(), b"RZDC", b"DRDC"] {
        assert!(
            precision_codec::decode(format, "other", REFERENCE_WORKING_BYTES, 0, None)
                .err()
                .unwrap()
                .contains("format")
        );
    }
    let associated = tiff_fixture(&pixels, 4, 1, false, 1);
    assert!(
        precision_codec::decode(&associated, "associated", REFERENCE_WORKING_BYTES, 0, None)
            .err()
            .unwrap()
            .contains("alpha")
    );
    let float = source(pixels, 3)
        .convert(
            PrecisionStorage::Rgba32Float,
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap();
    assert!(precision_codec::encode_scratch_bytes(&float)
        .unwrap_err()
        .contains("explicit RGBA16"));
}

#[test]
fn admission_precedes_pixel_decode_and_save_never_truncates_a_previous_destination() {
    let pixels = vec![[30001, 40001, 20001, 0]; 6];
    let bytes = png(3, 2, &pixels, 4, vec![]);
    let plan = precision_codec::inspect(&bytes, "valid").unwrap();
    assert_eq!((plan.width, plan.height, plan.source_channels), (3, 2, 4));
    let total = bytes.len() as u64 + plan.scratch_bytes;
    assert!(
        precision_codec::decode(&bytes, "bounded", total - 1, 0, None)
            .err()
            .unwrap()
            .contains("capacity")
    );
    assert!(precision_codec::decode(&bytes, "bounded", total, 0, None).is_ok());
    assert!(
        precision_codec::decode(&bytes, "bounded", REFERENCE_WORKING_BYTES, u64::MAX, None)
            .is_err()
    );
    let mut huge = bytes.clone();
    huge[16..20].copy_from_slice(&10001_u32.to_be_bytes());
    huge[20..24].copy_from_slice(&10000_u32.to_be_bytes());
    let check = crc(&huge[12..29]);
    huge[29..33].copy_from_slice(&check.to_be_bytes());
    assert!(precision_codec::inspect(&huge, "huge").is_err());
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("original.png");
    std::fs::write(&path, b"previous complete file").unwrap();
    let image = source(pixels, 3);
    let control = DrJobControl::default();
    assert!(precision_codec::save(
        &image,
        &Metadata::default(),
        Resolution::default(),
        path.to_str().unwrap(),
        PrecisionFormat::Png,
        true,
        false,
        1,
        0,
        Some(&control)
    )
    .is_err());
    assert_eq!(control.scratch_bytes(), 0);
    assert_eq!(std::fs::read(&path).unwrap(), b"previous complete file");
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn cancellation_at_decode_stage_discards_output_and_clears_reservation() {
    let pixels = vec![[30001, 10001, 60001, 1]; 1024 * 4096];
    let bytes = png(1024, 4096, &pixels, 4, vec![]);
    drop(pixels);
    let render = |token: usize| {
        let control = unsafe { &*(token as *const DrJobControl) };
        match precision_codec::decode(
            &bytes,
            "cancellable",
            REFERENCE_WORKING_BYTES,
            0,
            Some(control),
        ) {
            Ok(_) => (true, String::new()),
            Err(error) => (false, error),
        }
    };
    let (entry, next) = entry_then_cancel(&|progress, _, _| progress >= 0.75, render);
    let n0 = entry.checkpoints;
    let k = 1;
    assert!(
        next.expect("the entry's kernel never reported again")
            .checkpoints
            > n0 + 1 + k,
        "the entry's kernel polls fewer than {} checkpoints before its next report (entry {n0}, next {next:?})",
        k + 1
    );
    let parked = cancelled_parked_at(
        n0 + 1 + k,
        &|progress, stage| stage == entry.stage && progress == entry.progress,
        render,
    );
    println!("decode cancellation: {}", parked_line(&parked));
    assert!(parked.progress >= 0.75 && parked.progress < 1.0);
    let cancelled = DrJobControl::default();
    cancelled.cancel();
    assert!(precision_codec::decode(
        &bytes,
        "already cancelled",
        REFERENCE_WORKING_BYTES,
        0,
        Some(&cancelled)
    )
    .is_err());
    assert_eq!(cancelled.scratch_bytes(), 0);
}
