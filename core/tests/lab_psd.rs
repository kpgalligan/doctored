//! #1 Lab phase 3a, synthetic Lab PSD, PSB and layered TIFF files (built
//! here over `cmyk_psd_fixture`'s generic records, colour word 9 and
//! Photometric 8 patched in by `lab_psd_fixture`), opened through the FFI at
//! 8 and 16 bits and checked against the independent oracle
//! (`lab_psd_fixture::psd_units`, a thin wrapper over
//! `lab_blend_oracle::psd_unit`: 8 bits P8, 16 bits L\* = 100v/65535 and
//! a\*/b\* on Photoshop's 15-bit grid), never against the importer:
//!
//! * every plane decodes exactly to the oracle's code (a 16-bit a\* 61
//!   stored as 48383 reads as the code of 61);
//! * the records survive: masks (enabled and disabled), Normal and Pass
//!   Through groups, clipping, opacity and Fill;
//! * a Darken record opens as Normal with a loss line;
//! * an `LbCl` Color Overlay comes through as the sRGB hex of its Lab
//!   colour (the CIE oracle, within one code);
//! * Blend If: a slot-0 copy of the Lightness range is dropped, a differing
//!   Gray range refuses the style by name;
//! * a carried Levels opens as an adjustment layer; a refused Exposure and a
//!   Solid Color fill are omitted and named;
//! * mode 1 opens the stored composite, merged alpha unmatted against Lab
//!   white (the f64 oracle within one code);
//! * an embedded profile is named and unused;
//! * the PSB and layered TIFF twins import EQUAL to the PSD;
//! * the report answers `import_precision` "lab", `color_mode` "Lab",
//!   `profile` null and `layered_plan_bytes`;
//! * a 32-bit Lab file refuses, and the RGB and gray opens refuse Lab by
//!   name.

use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::doc::BlendMode;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_precision::{dr_precision_doc_descriptor, dr_precision_doc_free};
use doctored_core::ffi_psd::{
    dr_doc_open_psd, dr_precision_doc_open_gray_psd, dr_precision_doc_open_lab_psd,
    dr_precision_doc_open_psd, dr_psd_import_report,
};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use serde_json::Value;
use tempfile::TempDir;

use crate::cmyk_psd_fixture::{self as fx, Comp, PsdSpec, Rec};
use crate::common::{cpath, take_err_string};
use crate::gray_fixture::gray_icc;
use crate::lab_fixture::lab_to_srgb;
use crate::lab_psd_fixture::{
    code_of, lab_channels, lab_empty, lab_folder, lab_layered_tiff, lab_psd, psd_sample, psd_units,
};
use crate::psd_descriptor_fixture::{body, boolean, doub, enumeration, objc, rgbc, untf};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const W: u32 = 8;
const H: u32 = 6;
const FULL: [u8; 4] = [0, 0, 255, 255];
const UNDER: [u8; 4] = [0, 0, 96, 160];
/// The `LbCl` of the Color Overlay (take1's measured colour).
const LBCL: [f64; 3] = [59.45, 21.09, 44.38];
/// Lab white in the document's codes: L\* 100, a\* = b\* = 128·257.
const WHITE: [f64; 3] = [65535.0, 32896.0, 32896.0];

// ------------------------------------------------------------- the files --

fn rect(top: i32, left: i32, h: u32, w: u32) -> [i32; 4] {
    [top, left, top + h as i32, left + w as i32]
}

/// Deterministic L\*, a\*, b\* units of pixel `i` of a record seeded `seed`
/// (off the integer grid at 16 bits).
fn units(i: u32, seed: u32, bits: u16) -> [f64; 3] {
    let frac = if bits == 16 { 0.37 } else { 0.0 };
    [
        f64::from(5 + (i * 37 + seed * 11) % 90) + frac,
        f64::from((i * 53 + seed * 29) % 220) - 110.0 + frac,
        f64::from((i * 71 + seed * 17) % 230) - 115.0 - frac,
    ]
}

/// A record's stored samples `[L, a, b, alpha]` at the file's depth; the
/// Base's first pixel holds a\* exactly 61.
fn file_pixels(w: u32, h: u32, seed: u32, bits: u16, alpha: bool) -> Vec<[u16; 4]> {
    let top = if bits == 8 { 255u32 } else { 65535 };
    (0..w * h)
        .map(|i| {
            let mut u = units(i, seed, bits);
            if seed == 1 && i == 0 {
                u[1] = 61.0;
            }
            let a = if alpha {
                ((i * 41 + seed * 7) % 200 + 40) * top / 255
            } else {
                top
            };
            [
                psd_sample(bits, 0, u[0]),
                psd_sample(bits, 1, u[1]),
                psd_sample(bits, 2, u[2]),
                a as u16,
            ]
        })
        .collect()
}

/// The oracle's document codes of stored samples.
fn expected(bits: u16, stored: &[[u16; 4]]) -> Vec<[u16; 4]> {
    stored
        .iter()
        .map(|s| {
            [
                code_of(0, psd_units(bits, 0, s[0])),
                code_of(1, psd_units(bits, 1, s[1])),
                code_of(2, psd_units(bits, 2, s[2])),
                if bits == 8 { s[3] * 257 } else { s[3] },
            ]
        })
        .collect()
}

fn comp(bits: u16) -> Comp {
    if bits == 8 {
        Comp::Rle
    } else {
        Comp::ZipPredicted
    }
}

struct Build {
    bits: u16,
    large: bool,
}

impl Build {
    fn raster(&self, name: &str, bounds: [i32; 4], seed: u32, alpha: bool) -> Rec {
        let (w, h) = (
            (bounds[3] - bounds[1]) as u32,
            (bounds[2] - bounds[0]) as u32,
        );
        let samples = file_pixels(w, h, seed, self.bits, alpha);
        Rec::raster(
            name,
            bounds,
            lab_channels(
                &samples,
                w as usize,
                self.bits,
                alpha,
                comp(self.bits),
                self.large,
            ),
        )
    }

    fn mask_plane(&self, w: u32, h: u32) -> Vec<u8> {
        let top = if self.bits == 8 { 1 } else { 257 };
        let values: Vec<u16> = (0..w * h)
            .map(|i| ((i * 41 + 7) % 256) as u16 * top)
            .collect();
        fx::plane(
            &values,
            w as usize,
            h as usize,
            self.bits,
            comp(self.bits),
            self.large,
        )
    }

    /// The structured scene, file order (bottom first).
    fn records(&self) -> Vec<Rec> {
        let ramp = self.raster("Ramp", rect(0, 0, 3, 8), 2, true).mask(
            rect(0, 2, 3, 4),
            0,
            true,
            self.mask_plane(4, 3),
        );
        let mut off = self.raster("Off", rect(3, 0, 3, 8), 3, true).mask(
            rect(3, 0, 2, 2),
            255,
            false,
            self.mask_plane(2, 2),
        );
        off.opacity = 128;
        let mut child_a = self.raster("Child A", rect(0, 0, 4, 6), 4, true);
        child_a.blend = *b"mul ";
        child_a.opacity = 128;
        let mut child_b = self
            .raster("Child B", rect(2, 2, 4, 6), 5, true)
            .with_block(b"iOpa", &[102, 0, 0, 0]);
        child_b.blend = *b"over";
        child_b.clipping = 1;
        let mut group = lab_folder("Group N", 1, *b"norm");
        group.opacity = 200;
        let mut pass_child = self.raster("Pass child", rect(4, 0, 2, 8), 6, true);
        pass_child.blend = *b"scrn";
        let mut dark = self.raster("Dark", rect(0, 6, 6, 2), 7, false);
        dark.blend = *b"dark";
        let mut overlay = self.raster("Overlay", rect(0, 0, 2, 2), 8, false);
        overlay.blocks = lbcl_overlay(LBCL);
        let mut copy = self.raster("Copy", rect(2, 2, 2, 2), 9, false);
        copy.ranges = ranges(&[(FULL, UNDER), (FULL, UNDER)]);
        let mut differ = self.raster("Differ", rect(4, 4, 2, 2), 10, false);
        differ.ranges = ranges(&[(FULL, [0, 0, 50, 200]), (FULL, UNDER)]);
        let levels = lab_empty("Levels").with_block(b"levl", &levels_body([10, 240, 0, 255, 120]));
        let exposure = lab_empty("Exposure").with_block(b"expA", &exposure_body());
        let fill = lab_empty("Fill").with_block(
            b"SoCo",
            &body(16, b"null", &[rgbc(b"Clr ", 200.0, 40.0, 90.0)]),
        );
        vec![
            self.raster("Base", rect(0, 0, H, W), 1, false),
            ramp,
            off,
            lab_folder("</Layer group>", 3, *b"norm"),
            child_a,
            child_b,
            group,
            lab_folder("</Layer group>", 3, *b"norm"),
            pass_child,
            lab_folder("Pass", 1, *b"pass"),
            dark,
            overlay,
            copy,
            differ,
            levels,
            exposure,
            fill,
        ]
    }
}

/// Blending ranges, slot 0 (the composite, read as Gray) then Lightness, a
/// and b; slots not given are the identity.
fn ranges(slots: &[([u8; 4], [u8; 4])]) -> Vec<u8> {
    let mut out = Vec::new();
    for k in 0..4 {
        let (this, under) = slots.get(k).copied().unwrap_or((FULL, FULL));
        out.extend(this);
        out.extend(under);
    }
    out
}

/// An `lfx2` block of one Color Overlay whose colour is `LbCl` {L\*, a\*, b\*}.
fn lbcl_overlay(lab: [f64; 3]) -> Vec<u8> {
    let mut out = 0u32.to_be_bytes().to_vec();
    out.extend(body(
        16,
        b"null",
        &[
            untf(b"Scl ", b"#Prc", 100.0),
            boolean(b"masterFXSwitch", 1),
            objc(
                b"SoFi",
                b"SoFi",
                &[
                    boolean(b"enab", 1),
                    enumeration(b"Md  ", b"BlnM", b"Nrml"),
                    objc(
                        b"Clr ",
                        b"LbCl",
                        &[
                            doub(b"Lmnc", lab[0]),
                            doub(b"A   ", lab[1]),
                            doub(b"B   ", lab[2]),
                        ],
                    ),
                    untf(b"Opct", b"#Prc", 100.0),
                ],
            ),
        ],
    ));
    fx::block(b"lfx2", &out)
}

/// A `levl` body: version 2, record 0 `master`, 28 identity records.
fn levels_body(master: [u16; 5]) -> Vec<u8> {
    let mut out = 2u16.to_be_bytes().to_vec();
    for i in 0..29 {
        let record = if i == 0 {
            master
        } else {
            [0, 255, 0, 255, 100]
        };
        for v in record {
            out.extend(v.to_be_bytes());
        }
    }
    out
}

/// An `expA` body: version 1, exposure +1, offset 0, gamma 1.
fn exposure_body() -> Vec<u8> {
    let mut out = 1u16.to_be_bytes().to_vec();
    for v in [1.0f32, 0.0, 1.0] {
        out.extend(v.to_be_bytes());
    }
    out
}

/// The stored composite planes (L, a, b and, when `alpha`, merged alpha)
/// at the file's depth.
fn composite_planes(bits: u16, alpha: bool) -> Vec<Vec<u16>> {
    let pixels = file_pixels(W, H, 11, bits, true);
    let top = if bits == 8 { 255 } else { 65535 };
    (0..3 + usize::from(alpha))
        .map(|c| {
            pixels
                .iter()
                .enumerate()
                .map(|(i, p)| match (c, i) {
                    (3, 0) => 0,
                    (3, 1) => top,
                    _ => p[c],
                })
                .collect()
        })
        .collect()
}

fn spec<'a>(bits: u16, large: bool, icc: Option<&'a [u8]>, alpha: bool) -> PsdSpec<'a> {
    PsdSpec {
        canvas: (W, H),
        depth: bits,
        icc,
        composite: composite_planes(bits, alpha),
        merged_alpha: alpha,
        large,
        lr16: bits == 16,
        resources: Vec::new(),
    }
}

fn written(dir: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

// ------------------------------------------------------------- the C API --

fn report(path: &Path) -> Value {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn fingerprint(path: &Path) -> CString {
    CString::new(report(path)["source_fingerprint"].as_str().unwrap()).unwrap()
}

fn open(path: &Path, mode: i32) -> Result<*mut PrecisionDocument, String> {
    let print = fingerprint(path);
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_lab_psd(
            cpath(path).as_ptr(),
            mode,
            print.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if doc.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(doc)
    }
}

fn free(doc: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(doc) };
}

fn descriptor(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_precision_doc_descriptor(doc, &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn find<'a>(
    layers: &'a [PrecisionDocumentLayer],
    name: &str,
) -> Option<&'a PrecisionDocumentLayer> {
    for layer in layers {
        if layer.name() == name {
            return Some(layer);
        }
        if let Some(found) = layer.children().and_then(|c| find(c, name)) {
            return Some(found);
        }
    }
    None
}

fn samples(layer: &PrecisionDocumentLayer) -> Vec<[u16; 4]> {
    layer.source().unwrap().to_rgba16().unwrap()
}

fn byte(v: f32) -> u8 {
    (f64::from(v) * 255.0).round() as u8
}

/// Every layer's record and codes, top-first, for an equality check.
fn snapshot(layers: &[PrecisionDocumentLayer], out: &mut Vec<String>) {
    for layer in layers.iter().rev() {
        let pixels = layer
            .source()
            .map(|image| (image.dimensions(), image.to_rgba16().unwrap()));
        let mask = layer.mask().map(|m| {
            let p = m.properties();
            (
                m.enabled(),
                m.plane().dimensions(),
                m.plane().samples().to_vec(),
                p.offset,
                p.outside_coverage,
            )
        });
        out.push(format!(
            "{} {:?} {} {} {} {:?} {:?} {:?} {:?} {:?} {:?}",
            layer.name(),
            layer.blend_mode(),
            layer.opacity(),
            layer.visible(),
            layer.clipped(),
            layer.offset(),
            mask,
            layer.blend_if(),
            layer.reported_style(),
            layer.is_pass_through(),
            pixels
        ));
        if let Some(children) = layer.children() {
            snapshot(children, out);
        }
    }
}

// ----------------------------------------------------------------- tests --

#[test]
fn lab_psd_records_planes_and_report_open_as_a_lab_document() {
    let dir = TempDir::new().unwrap();
    let icc = gray_icc("Test Gray", 2.2);
    for bits in [8u16, 16] {
        let build = Build { bits, large: false };
        let path = written(
            &dir,
            &format!("scene{bits}.psd"),
            &lab_psd(&spec(bits, false, Some(&icc), false), &build.records()),
        );
        let r = report(&path);
        assert_eq!(r["import_precision"], "lab", "{r}");
        assert_eq!(r["color_mode"], "Lab");
        assert_eq!(r["depth"], bits);
        assert!(r["profile"].is_null(), "{r}");
        assert_eq!(r["layered_available"], true, "{r}");
        assert_eq!(r["stored_composite_available"], true, "{r}");
        assert!(r["layered_plan_bytes"].as_u64().unwrap() > 0, "{r}");
        let observations = r["observations"].to_string();
        let losses = r["verified_losses"].to_string();
        assert!(
            observations.contains(
                "The file\u{2019}s embedded profile \u{201c}Test Gray\u{201d} is not used"
            ) || observations
                .contains("The file's embedded profile \u{201c}Test Gray\u{201d} is not used"),
            "{observations}"
        );
        assert!(
            losses.contains("(\u{201c}Dark\u{201d}) has the blend key Darken")
                && losses.contains("it is imported as Normal."),
            "{losses}"
        );
        assert!(
            observations.contains("(\u{201c}Overlay\u{201d}) has Photoshop layer effects (")
                && observations.contains("color_overlay"),
            "{observations}"
        );
        assert!(
            losses.contains("(\u{201c}Differ\u{201d}) has Photoshop layer effects that a Lab Color document cannot carry (Blend If on the"),
            "{losses}"
        );
        assert!(
            losses.contains("(\u{201c}Exposure\u{201d}) is a Photoshop Exposure adjustment layer (expA) that a Lab Color document cannot hold ("),
            "{losses}"
        );
        assert!(
            losses.contains("(\u{201c}Fill\u{201d}) is a Photoshop ")
                && losses.contains("a Lab Color document's layered import omits fill layers"),
            "{losses}"
        );

        let doc = open(&path, 0).unwrap();
        let d = unsafe { &*doc };
        assert!(d.is_lab());
        assert_eq!(descriptor(doc)["lab"]["depth"], bits);
        let warnings = d.info().unwrap()["import_warnings"].to_string();
        assert!(warnings.contains("Test Gray"), "{warnings}");
        let names: Vec<&str> = d.root_layers().iter().map(|l| l.name()).collect();
        assert_eq!(
            names,
            [
                "Base", "Ramp", "Off", "Group N", "Pass", "Dark", "Overlay", "Copy", "Differ",
                "Levels"
            ],
            "Exposure and Fill are omitted"
        );
        // Planes: exact against the oracle's code of each stored sample.
        for (name, bounds, seed, alpha) in [
            ("Base", rect(0, 0, H, W), 1, false),
            ("Ramp", rect(0, 0, 3, 8), 2, true),
            ("Child B", rect(2, 2, 4, 6), 5, true),
        ] {
            let (w, h) = (
                (bounds[3] - bounds[1]) as u32,
                (bounds[2] - bounds[0]) as u32,
            );
            let want = expected(bits, &file_pixels(w, h, seed, bits, alpha));
            let layer = find(d.root_layers(), name).unwrap();
            assert_eq!(layer.offset(), (bounds[1], bounds[0]), "{name}");
            assert_eq!(samples(layer), want, "{name} at {bits} bits");
        }
        let base = samples(&d.root_layers()[0]);
        assert_eq!(base[0][1], 189 * 257, "a* 61 reads as the code of 61");
        if bits == 16 {
            assert_eq!(psd_sample(16, 1, 61.0), 48383, "stored on the 15-bit grid");
        }
        // Records.
        let ramp = find(d.root_layers(), "Ramp").unwrap();
        let mask = ramp.mask().unwrap();
        assert!(mask.enabled());
        assert_eq!(mask.plane().dimensions(), (4, 3));
        let off = find(d.root_layers(), "Off").unwrap();
        assert!(!off.mask().unwrap().enabled());
        assert_eq!(byte(off.opacity()), 128);
        let group = find(d.root_layers(), "Group N").unwrap();
        assert!(!group.is_pass_through());
        assert_eq!(byte(group.opacity()), 200);
        let children: Vec<&str> = group.children().unwrap().iter().map(|l| l.name()).collect();
        assert_eq!(children, ["Child A", "Child B"]);
        let child_a = find(d.root_layers(), "Child A").unwrap();
        assert_eq!(child_a.blend_mode(), BlendMode::Multiply);
        assert_eq!(byte(child_a.opacity()), 128);
        let child_b = find(d.root_layers(), "Child B").unwrap();
        assert!(child_b.clipped());
        assert_eq!(child_b.blend_mode(), BlendMode::Overlay);
        assert_eq!(byte(child_b.style().unwrap().fill_opacity), 102);
        let pass = find(d.root_layers(), "Pass").unwrap();
        assert!(pass.is_pass_through());
        assert_eq!(
            find(d.root_layers(), "Pass child").unwrap().blend_mode(),
            BlendMode::Screen
        );
        assert_eq!(
            find(d.root_layers(), "Dark").unwrap().blend_mode(),
            BlendMode::Normal,
            "Darken opens as Normal"
        );
        // H-C's rule: the `LbCl` as the sRGB hex of its Lab colour.
        let overlay = find(d.root_layers(), "Overlay").unwrap();
        let style: Value = serde_json::from_str(&overlay.reported_style().unwrap()).unwrap();
        let effect = style["effects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["type"] == "color_overlay")
            .expect("the Color Overlay is carried")
            .clone();
        let hex = effect["color"].as_str().unwrap();
        let oracle = lab_to_srgb(LBCL);
        for (c, unit) in oracle.iter().enumerate() {
            let v = u8::from_str_radix(&hex[1 + 2 * c..3 + 2 * c], 16).unwrap();
            let want = (unit.clamp(0.0, 1.0) * 255.0).round() as u8;
            assert!(v.abs_diff(want) <= 1, "{hex} channel {c}: oracle {want}");
        }
        // Blend If: the slot-0 copy dropped, one Lightness range kept.
        let copy = find(d.root_layers(), "Copy").unwrap();
        let blend_if = copy.blend_if().expect("the Blend If is carried");
        let kept: Vec<_> = blend_if.ranges().collect();
        assert_eq!(kept.len(), 1, "{blend_if:?}");
        assert_eq!(format!("{:?}", kept[0].channel), "Red");
        assert_eq!(kept[0].underlying, UNDER);
        let differ = find(d.root_layers(), "Differ").unwrap();
        assert!(differ.style().is_none() && differ.blend_if().is_none());
        // The carried Levels: an adjustment layer (no pixels, no children).
        let levels = find(d.root_layers(), "Levels").unwrap();
        assert!(levels.source().is_none() && levels.children().is_none());
        free(doc);
    }
}

#[test]
fn the_psb_and_the_layered_tiff_are_the_psds_twins() {
    let dir = TempDir::new().unwrap();
    for bits in [8u16, 16] {
        let psd = written(
            &dir,
            &format!("twin{bits}.psd"),
            &lab_psd(
                &spec(bits, false, None, false),
                &Build { bits, large: false }.records(),
            ),
        );
        let psb = written(
            &dir,
            &format!("twin{bits}.psb"),
            &lab_psd(
                &spec(bits, true, None, false),
                &Build { bits, large: true }.records(),
            ),
        );
        let strip: Vec<[u16; 5]> = (0..W * H)
            .map(|_| {
                let top = if bits == 8 { 255 } else { 65535 };
                [top, 0, 0, top, 0]
            })
            .collect();
        let tiff = written(
            &dir,
            &format!("twin{bits}.tif"),
            &lab_layered_tiff(
                (W, H),
                bits,
                &strip,
                &Build { bits, large: false }.records(),
            ),
        );
        let reference = open(&psd, 0).unwrap();
        let mut right = Vec::new();
        snapshot(unsafe { &*reference }.root_layers(), &mut right);
        for twin in [&psb, &tiff] {
            let r = report(twin);
            assert_eq!(r["import_precision"], "lab", "{}: {r}", twin.display());
            let doc = open(twin, 0).unwrap();
            assert!(unsafe { &*doc }.is_lab());
            let mut left = Vec::new();
            snapshot(unsafe { &*doc }.root_layers(), &mut left);
            assert_eq!(left.len(), right.len(), "{}", twin.display());
            for (l, r) in left.iter().zip(&right) {
                assert!(l == r, "{}: a record differs:\n{l}\n{r}", twin.display());
            }
            free(doc);
        }
        // A TIFF has no stored composite here.
        assert!(open(&tiff, 1).is_err());
        free(reference);
    }
}

#[test]
fn mode_one_opens_the_stored_composite_unmatted_against_lab_white() {
    let dir = TempDir::new().unwrap();
    for bits in [8u16, 16] {
        let build = Build { bits, large: false };
        let path = written(
            &dir,
            &format!("composite{bits}.psd"),
            &lab_psd(&spec(bits, false, None, true), &build.records()[..1]),
        );
        let doc = open(&path, 1).unwrap();
        let d = unsafe { &*doc };
        assert!(d.is_lab());
        assert_eq!(descriptor(doc)["lab"]["depth"], bits);
        assert_eq!(d.root_layers().len(), 1);
        let got = samples(&d.root_layers()[0]);
        let planes = composite_planes(bits, true);
        let top = if bits == 8 { 255.0 } else { 65535.0 };
        for (i, pixel) in got.iter().enumerate() {
            let a = f64::from(planes[3][i]) / top;
            let alpha = if bits == 8 {
                planes[3][i] * 257
            } else {
                planes[3][i]
            };
            assert_eq!(pixel[3], alpha, "pixel {i} alpha");
            if a == 0.0 {
                assert_eq!(pixel[..3], [0, 32896, 32896], "pixel {i}: canonical");
                continue;
            }
            for c in 0..3 {
                // Stored against white, linearly in the codes: the stored
                // sample's code (the oracle's decode), then unmatted.
                let stored = f64::from(code_of(c, psd_units(bits, c, planes[c][i])));
                let straight = ((stored - WHITE[c] * (1.0 - a)) / a).clamp(0.0, 65535.0);
                let diff = (f64::from(pixel[c]) - straight).abs();
                assert!(
                    diff <= 1.0,
                    "{bits} bits pixel {i} channel {c}: {} against {straight}",
                    pixel[c]
                );
            }
        }
        free(doc);
    }
}

#[test]
fn a_32_bit_lab_file_refuses_and_the_rgb_and_gray_opens_refuse_lab_by_name() {
    let dir = TempDir::new().unwrap();
    let build = Build {
        bits: 8,
        large: false,
    };
    let mut bytes = lab_psd(&spec(8, false, None, false), &build.records()[..1]);
    let lab = written(&dir, "lab8.psd", &bytes);
    let print = fingerprint(&lab);
    let refusal = "a Lab Color file opens only as a Lab Color document";
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_psd(
            cpath(&lab).as_ptr(),
            0,
            print.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(doc.is_null());
    let error = take_err_string(err);
    assert!(error.contains(refusal), "{error}");
    let mut err = ptr::null_mut();
    let byte_doc = unsafe { dr_doc_open_psd(cpath(&lab).as_ptr(), 0, print.as_ptr(), &mut err) };
    if !byte_doc.is_null() {
        unsafe { dr_doc_free(byte_doc) };
        panic!("the byte RGB open admitted a Lab file");
    }
    let error = take_err_string(err);
    assert!(error.contains(refusal), "{error}");
    let working = CString::new("dot_gain_20").unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_gray_psd(
            cpath(&lab).as_ptr(),
            0,
            print.as_ptr(),
            working.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(doc.is_null());
    let error = take_err_string(err);
    assert!(error.contains("a Lab file is not Grayscale"), "{error}");
    // The header's depth word at 32: the Lab open refuses by name.
    bytes[22..24].copy_from_slice(&32u16.to_be_bytes());
    let deep = written(&dir, "lab32.psd", &bytes);
    let r = report(&deep);
    assert_eq!(r["layered_available"], false, "{r}");
    assert_eq!(r["stored_composite_available"], false, "{r}");
    assert!(
        r["layered_refusal"].to_string().contains("32-bit Lab PSD"),
        "{r}"
    );
    let error = open(&deep, 0).unwrap_err();
    assert!(
        error.contains(
            "32-bit Lab file cannot be opened: a Lab Color document is 8 or 16 bits per channel"
        ),
        "{error}"
    );
}
