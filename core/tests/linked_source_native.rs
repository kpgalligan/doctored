//! Independent v16/v17 framing and portable mixed-depth source caches.
//! Original PNGs are decoded independently; dependency paths are deliberately
//! nonexistent, proving native persistence never tries to access linked files.
use doctored_core::doc::DrDocument;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::retained_source::{SourceEdit, SourceInput, SourceLink};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use std::ffi::CString;
use std::ptr;
use std::sync::Arc;
use tempfile::TempDir;

const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const PATH: &str = "/missing/doctored-source-test/原本 photograph.png";
fn byte(profile: Arc<IccProfile>) -> DrDocument {
    let mut d = DrDocument::from_pixels(RgbaImage::from_fn(4, 2, |x, y| {
        Rgba([
            31 + x as u8,
            121 + y as u8,
            211 - x as u8,
            [0, 1, 128, 255][x as usize],
        ])
    }));
    d.profile = profile;
    d
}
fn typed(profile: Arc<IccProfile>) -> PrecisionDocument {
    let image = PrecisionImage::from_rgba16(
        4,
        2,
        (0..8)
            .map(|i| {
                [
                    30001 + i,
                    12003,
                    49997 - i,
                    [0, 1, 32769, 65535][i as usize % 4],
                ]
            })
            .collect(),
        profile,
        None,
    )
    .unwrap();
    PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap()
}
fn place() -> Value {
    place_at(PATH)
}
fn place_at(path: &str) -> Value {
    json!({"op":"place","source_id":"stable-source","revision_id":"revision-a","name":"Linked original","x":0,"y":0,"depth_conversion":"convert_fallback","link":{"path":path,"sha256":HASH}})
}
fn edit8(d: &DrDocument, input: Option<SourceInput<'_>>, request: Value) -> DrDocument {
    d.source_edit_input(
        input,
        &SourceEdit::parse(&request.to_string()).unwrap(),
        REFERENCE_WORKING_BYTES,
        0,
        None,
    )
    .unwrap()
    .unwrap()
}
fn edit16(
    d: &PrecisionDocument,
    input: Option<SourceInput<'_>>,
    request: Value,
) -> PrecisionDocument {
    d.source_edit_input(
        input,
        &SourceEdit::parse(&request.to_string()).unwrap(),
        REFERENCE_WORKING_BYTES,
        0,
        None,
    )
    .unwrap()
    .unwrap()
}
fn save8(d: &DrDocument, dir: &TempDir) -> Vec<u8> {
    let path = dir.path().join("byte.rz");
    let cpath = CString::new(path.to_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    assert!(unsafe {
        doctored_core::ffi_doc::dr_doc_save_native(d, cpath.as_ptr(), std::ptr::null(), &mut err)
    });
    assert!(err.is_null());
    std::fs::read(path).unwrap()
}
fn save16(d: &PrecisionDocument, dir: &TempDir) -> Vec<u8> {
    let path = dir.path().join("typed.rz");
    let cpath = CString::new(path.to_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    assert!(unsafe {
        doctored_core::ffi_precision::dr_precision_doc_save_native(
            d,
            cpath.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    });
    assert!(err.is_null());
    std::fs::read(path).unwrap()
}
fn open8(bytes: &[u8], dir: &TempDir) -> Result<DrDocument, String> {
    let path = dir.path().join("moved-byte.rz");
    std::fs::write(&path, bytes).unwrap();
    DrDocument::open(path.to_str().unwrap())
}
fn open16(bytes: &[u8]) -> Result<PrecisionDocument, String> {
    PrecisionDocument::parse_native(bytes, REFERENCE_WORKING_BYTES, 0, None)
}
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}
struct AssetLocation {
    start: usize,
    id: usize,
    revision: usize,
    depth: usize,
    profile: usize,
    png: usize,
    png_len: usize,
}
struct LinkLocation {
    start: usize,
    flag: usize,
    path: Option<usize>,
    sha: Option<usize>,
    end: usize,
}
struct Frame {
    start: usize,
    assets: Vec<AssetLocation>,
    bindings: Vec<LinkLocation>,
    binding_count: usize,
}
fn frame(bytes: &[u8]) -> Frame {
    // Offset 8: 12E-1's identity wrapper owns the outermost word.
    assert!(matches!(u32_at(bytes, 8), 16 | 17));
    let start = bytes.windows(4).position(|v| v == b"RSRC").unwrap();
    let count = u32_at(bytes, start + 4) as usize;
    let mut at = start + 8;
    let mut assets = Vec::new();
    for _ in 0..count {
        let begin = at;
        let id = at;
        at += 4 + u32_at(bytes, at) as usize;
        let revision = at;
        at += 4 + u32_at(bytes, at) as usize;
        let depth = at;
        at += 4;
        let profile = at;
        at += 4 + u32_at(bytes, at) as usize;
        let png_len = u64_at(bytes, at + 8) as usize;
        at += 16;
        let png = at;
        at += png_len;
        assets.push(AssetLocation {
            start: begin,
            id,
            revision,
            depth,
            profile,
            png,
            png_len,
        });
    }
    let binding_count = at;
    let count = u32_at(bytes, at) as usize;
    at += 4;
    let mut bindings = Vec::new();
    for _ in 0..count {
        let start = at;
        at += 60;
        let flag = at;
        at += 1;
        let (path, sha) = if bytes[flag] == 1 {
            let path = at;
            at += 4 + u32_at(bytes, at) as usize;
            let sha = at;
            at += 64;
            (Some(path), Some(sha))
        } else {
            (None, None)
        };
        bindings.push(LinkLocation {
            start,
            flag,
            path,
            sha,
            end: at,
        });
    }
    // Since 12E-1 the source frame is no longer the file's last block: the
    // always-written `LIDS` table closes every file, and this walk checks
    // that it is exactly what follows.
    assert_eq!(&bytes[at..at + 4], b"LIDS");
    assert_eq!(u32_at(bytes, at + 4), 1, "identity schema");
    let records = u32_at(bytes, at + 8) as usize;
    assert_eq!(bytes.len(), at + 12 + 8 * records);
    Frame {
        start,
        assets,
        bindings,
        binding_count,
    }
}
fn original_png(bytes: &[u8], asset: &AssetLocation) -> image::DynamicImage {
    image::load_from_memory(&bytes[asset.png..asset.png + asset.png_len]).unwrap()
}
fn original_profile<'a>(bytes: &'a [u8], asset: &AssetLocation) -> &'a [u8] {
    let at = asset.profile;
    let n = u32_at(bytes, at) as usize;
    &bytes[at + 4..at + 4 + n]
}

#[test]
fn byte_native_keeps_a_genuine_u16_linked_master_and_byte_fallback() {
    let dir = TempDir::new().unwrap();
    let input = typed(IccProfile::display_p3());
    let base = byte(IccProfile::srgb());
    let placed = edit8(&base, Some(SourceInput::Typed(&input)), place());
    let bytes = save8(&placed, &dir);
    assert_eq!(u32_at(&bytes, 8), 16);
    let f = frame(&bytes);
    assert_eq!(f.assets.len(), 1);
    let png = original_png(&bytes, &f.assets[0]);
    assert_eq!(png.color(), image::ColorType::Rgba16);
    let expected: Vec<_> = input
        .layer_source(0)
        .unwrap()
        .to_rgba16()
        .ok()
        .unwrap()
        .iter()
        .flat_map(|p| p.iter().copied())
        .collect();
    assert_eq!(png.to_rgba16().as_raw(), &expected);
    assert_eq!(
        original_profile(&bytes, &f.assets[0]),
        input.profile().bytes().as_ref()
    );
    std::fs::remove_file(dir.path().join("byte.rz")).unwrap();
    let opened = open8(&bytes, &dir).unwrap();
    let source = opened.layers[1].effective_source(&opened.profile).unwrap();
    assert_eq!(source.asset.depth(), 16);
    assert_eq!(source.asset.id, "stable-source");
    assert_eq!(source.asset.revision_id, "revision-a");
    assert_eq!(
        source.asset.rgba16().unwrap().to_rgba16().ok(),
        input.layer_source(0).unwrap().to_rgba16().ok()
    );
    assert_eq!(source.link.as_ref().unwrap().path, PATH);
    assert_eq!(source.link.as_ref().unwrap().sha256, HASH);
    assert_eq!(
        *opened.layers[1].pixels.contiguous_image(),
        *placed.layers[1].pixels.contiguous_image()
    );
    assert_eq!(opened.profile.bytes(), base.profile.bytes());
    assert!(open16(&bytes).is_err());
}

#[test]
fn typed_native_keeps_byte_master_codes_and_u16_rendered_fallback() {
    let dir = TempDir::new().unwrap();
    let input = byte(IccProfile::display_p3());
    let base = typed(IccProfile::srgb());
    let placed = edit16(&base, Some(SourceInput::Byte(&input)), place());
    let bytes = save16(&placed, &dir);
    assert_eq!(u32_at(&bytes, 8), 17);
    let f = frame(&bytes);
    let png = original_png(&bytes, &f.assets[0]);
    assert_eq!(png.color(), image::ColorType::Rgba8);
    assert_eq!(
        png.to_rgba8().as_raw(),
        input.layers[0].pixels.contiguous_image().as_raw()
    );
    assert_eq!(
        original_profile(&bytes, &f.assets[0]),
        input.profile.bytes().as_ref()
    );
    let opened = open16(&bytes).unwrap();
    let source = opened.layer(1).unwrap().effective_source().unwrap();
    assert_eq!(source.asset.depth(), 8);
    assert!(source
        .asset
        .rgba8()
        .unwrap()
        .same_bytes(&input.layers[0].pixels));
    assert_eq!(source.asset.profile().bytes(), input.profile.bytes());
    assert_eq!(source.link.as_ref().unwrap().path, PATH);
    assert_eq!(
        opened.layer_source(1).unwrap().to_rgba16().ok(),
        placed.layer_source(1).unwrap().to_rgba16().ok()
    );
    // Base/fallback each8RGBA16 pixels plus8RGBA8 original pixels; no hidden
    // promoted master may inflate the actual serialized-pixel byte count.
    assert_eq!(
        placed.native_plan().unwrap().serialized_pixel_bytes,
        8 * 8 + 8 * 8 + 8 * 4
    );
    assert!(open8(&bytes, &dir).is_err());
}

#[test]
fn duplicated_bindings_share_one_revision_but_keep_independent_link_metadata() {
    let dir = TempDir::new().unwrap();
    let input = typed(IccProfile::display_p3());
    let base = byte(IccProfile::srgb());
    let placed = edit8(&base, Some(SourceInput::Typed(&input)), place());
    // The link is a field of the RECORD, not of the shared asset: the copy's
    // is rewritten directly (no source edit changes a link alone).
    let mut relinked = placed.duplicating_layer(1).unwrap();
    let mut binding = relinked.layers[2].retained_source.clone().unwrap();
    binding.link = Some(Arc::new(SourceLink {
        path: "/other/missing/source.png".into(),
        sha256: HASH.into(),
    }));
    relinked.layers[2].retained_source = Some(binding);
    let bytes = save8(&relinked, &dir);
    let f = frame(&bytes);
    assert_eq!(f.assets.len(), 1);
    assert_eq!(f.bindings.len(), 2);
    let opened = open8(&bytes, &dir).unwrap();
    let a = opened.layers[1].effective_source(&opened.profile).unwrap();
    let b = opened.layers[2].effective_source(&opened.profile).unwrap();
    assert!(Arc::ptr_eq(&a.asset, &b.asset));
    assert_ne!(a.link.as_ref().unwrap().path, b.link.as_ref().unwrap().path);
    assert_eq!(a.asset.revision_id, b.asset.revision_id);
    let mut embedded = opened.clone();
    let mut binding = embedded.layers[1].retained_source.clone().unwrap();
    binding.link = None;
    embedded.layers[1].retained_source = Some(binding);
    let bytes = save8(&embedded, &dir);
    assert_eq!(frame(&bytes).assets.len(), 1);
    let opened = open8(&bytes, &dir).unwrap();
    assert!(opened.layers[1]
        .effective_source(&opened.profile)
        .unwrap()
        .link
        .is_none());
    assert!(opened.layers[2]
        .effective_source(&opened.profile)
        .unwrap()
        .link
        .is_some());
}

#[test]
fn replacement_keeps_stable_identity_and_serializes_both_live_content_revisions() {
    let dir = TempDir::new().unwrap();
    let input = typed(IccProfile::display_p3());
    let base = byte(IccProfile::srgb());
    let placed = edit8(&base, Some(SourceInput::Typed(&input)), place());
    let duplicate = placed.duplicating_layer(1).unwrap();
    let new_input = byte(IccProfile::srgb());
    let changed = edit8(
        &duplicate,
        Some(SourceInput::Byte(&new_input)),
        json!({"op":"replace","layer":1,"revision_id":"revision-b","depth_conversion":"convert_fallback"}),
    );
    let bytes = save8(&changed, &dir);
    let f = frame(&bytes);
    assert_eq!(f.assets.len(), 2);
    let opened = open8(&bytes, &dir).unwrap();
    let a = opened.layers[1].effective_source(&opened.profile).unwrap();
    let b = opened.layers[2].effective_source(&opened.profile).unwrap();
    assert_eq!(a.asset.id, b.asset.id);
    assert_eq!(a.asset.revision_id, "revision-b");
    assert_eq!(b.asset.revision_id, "revision-a");
    assert_eq!(a.asset.depth(), 8);
    assert_eq!(b.asset.depth(), 16);
    assert!(!Arc::ptr_eq(&a.asset, &b.asset));
    assert!(a
        .asset
        .rgba8()
        .unwrap()
        .same_bytes(&new_input.layers[0].pixels));
    assert_eq!(
        b.asset.rgba16().unwrap().to_rgba16().ok(),
        input.layer_source(0).unwrap().to_rgba16().ok()
    );
    assert_eq!(placed.source_info(1).unwrap()["revision_id"], "revision-a");
    // Two separately allocated records cannot claim one identical identity /
    // revision pair, even when another caller manufactured them directly.
    let mut invalid = opened.clone();
    let mut binding = invalid.layers[1].retained_source.clone().unwrap();
    let mut asset = (*binding.asset).clone();
    asset.revision_id = b.asset.revision_id.clone();
    binding.asset = Arc::new(asset);
    invalid.layers[1].retained_source = Some(binding);
    assert!(invalid
        .save_native(dir.path().join("bad-identity.rz").to_str().unwrap())
        .unwrap_err()
        .contains("identity and revision"));
}

fn malformed(bytes: &[u8]) -> Vec<Vec<u8>> {
    let f = frame(bytes);
    let a = &f.assets[0];
    let b = &f.bindings[0];
    let path = b.path.unwrap();
    let sha = b.sha.unwrap();
    let mut out = Vec::new();
    for (at, value) in [
        (a.id + 4, vec![b'!']),
        (b.start, 1000_u32.to_le_bytes().to_vec()),
        (a.revision, 0_u32.to_le_bytes().to_vec()),
        (a.revision, 129_u32.to_le_bytes().to_vec()),
        (a.revision + 4, vec![b'!']),
        (a.depth, vec![32]),
        (a.depth + 1, vec![1]),
        (a.depth, vec![if bytes[a.depth] == 8 { 16 } else { 8 }]),
        (b.flag, vec![2]),
        (path, (16_u32 * 1024 + 1).to_le_bytes().to_vec()),
        (path + 4, vec![b'.']),
        (path + 5, vec![0]),
        (path + 4, vec![0xff]),
        (sha, vec![b'Z']),
        (sha, vec![b'A']),
        (sha, vec![0xff]),
    ] {
        let mut bad = bytes.to_vec();
        bad[at..at + value.len()].copy_from_slice(&value);
        out.push(bad);
    }
    for at in [a.revision + 1, a.depth + 1, b.flag, path + 1, b.end - 1] {
        out.push(bytes[..at].to_vec());
    }
    let mut trailing = bytes.to_vec();
    trailing.push(0);
    out.push(trailing);
    // Duplicate a fully framed asset with the same identity/revision pair.
    let mut duplicate = bytes[..f.start + 4].to_vec();
    duplicate.extend(2_u32.to_le_bytes());
    duplicate.extend(&bytes[a.start..f.binding_count]);
    duplicate.extend(&bytes[a.start..f.binding_count]);
    duplicate.extend(&bytes[f.binding_count..]);
    out.push(duplicate);
    out
}

#[test]
fn both_source_depths_refuse_malformed_revision_depth_link_and_fingerprint_records() {
    let dir = TempDir::new().unwrap();
    let b = byte(IccProfile::srgb());
    let t = typed(IccProfile::display_p3());
    let bytes8 = save8(&edit8(&b, Some(SourceInput::Typed(&t)), place()), &dir);
    let bytes16 = save16(&edit16(&t, Some(SourceInput::Byte(&b)), place()), &dir);
    for (index, bad) in malformed(&bytes8).iter().enumerate() {
        assert!(
            open8(bad, &dir).is_err(),
            "byte corruption {index} accepted"
        );
    }
    for (index, bad) in malformed(&bytes16).iter().enumerate() {
        assert!(open16(bad).is_err(), "typed corruption {index} accepted");
    }
}

#[test]
fn invalid_later_dependencies_refuse_before_any_earlier_fallback_pixels_decode() {
    let dir = TempDir::new().unwrap();
    let b = byte(IccProfile::srgb());
    let t = typed(IccProfile::display_p3());
    for (mut bytes, is_typed) in [
        (
            save8(&edit8(&b, Some(SourceInput::Typed(&t)), place()), &dir),
            false,
        ),
        (
            save16(&edit16(&t, Some(SourceInput::Byte(&b)), place()), &dir),
            true,
        ),
    ] {
        let f = frame(&bytes);
        let sha = f.bindings[0].sha.unwrap();
        bytes[sha] = b'z';
        let idat = bytes.windows(4).position(|v| v == b"IDAT").unwrap();
        bytes[idat + 4] ^= 0xff;
        let failure = if is_typed {
            open16(&bytes).err().unwrap()
        } else {
            open8(&bytes, &dir).err().unwrap()
        };
        assert!(failure.contains("fingerprint"), "{failure}");
    }
}

#[test]
fn typed_mixed_source_admission_counts_path_metadata_and_every_actual_plane() {
    let dir = TempDir::new().unwrap();
    let input = byte(IccProfile::display_p3());
    let base = typed(IccProfile::srgb());
    let path = format!("/{}", "x".repeat(16 * 1024 - 1));
    let placed = edit16(&base, Some(SourceInput::Byte(&input)), place_at(&path));
    let linked = placed.duplicate_layer(1).unwrap().unwrap();
    let plan = linked.native_plan().unwrap();
    assert_eq!(plan.serialized_pixel_bytes, 8 * 8 * 3 + 8 * 4);
    let bytes = save16(&linked, &dir);
    assert!(bytes.len() as u64 <= plan.encoded_bound);
    let opened = open16(&bytes).unwrap();
    assert!(opened.retained_bytes() <= plan.reopen_retained_bytes);
    let a = opened.layer(1).unwrap().effective_source().unwrap();
    let b = opened.layer(2).unwrap().effective_source().unwrap();
    assert!(Arc::ptr_eq(&a.asset, &b.asset));
    assert_eq!(b.link.as_ref().unwrap().path.len(), 16 * 1024);
    let limit = linked.retained_bytes() + plan.write_scratch_bytes;
    assert!(linked.encode_native(limit - 1, 0, None).is_err());
    assert!(linked.encode_native(limit, 0, None).is_ok());
    assert!(
        PrecisionDocument::parse_native(&bytes, bytes.len() as u64 + 1024 * 1024, 0, None).is_err()
    );
}

#[test]
fn byte_native_capacity_charges_genuine_u16_originals_for_both_component_bytes() {
    let dir = TempDir::new().unwrap();
    let input_image = PrecisionImage::from_rgba16(
        625,
        625,
        vec![[1, 2, 3, 65535]; 625 * 625],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let input =
        PrecisionDocument::from_image(input_image, Metadata::default(), Resolution::default())
            .unwrap();
    let base = byte(IccProfile::srgb());
    let mut placed = edit8(&base, Some(SourceInput::Typed(&input)), place());
    placed.layers = vec![placed.layers[1].clone(); 1023];
    // 1023 byte fallbacks plus a u16 original cost1025 RGBA8-equivalent planes.
    // Counting the original as8-bit would wrongly admit exactly1024 planes.
    let failure = placed
        .save_native(dir.path().join("over-budget.rz").to_str().unwrap())
        .unwrap_err();
    assert!(failure.contains("native pixel limit"), "{failure}");
}
