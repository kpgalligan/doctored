//! A Lab Color document's PSD, PSB and layered-TIFF export (#1 Lab phase
//! 3b): its report, its save, the three-plane composite, the CIELab strip
//! and the four-plane channel writer the shared record walk calls.
//! `psd_export_gray` is the template.
//!
//! Everything but the colour mode is the 16-bit RGB export's
//! (`psd_export16`): the same inventory (`lab`: three composite planes, no
//! profile, preflighted at the DECLARED depth), the same refusals, losses and
//! observations (`psd_export_preflight`) and the same record walk
//! (`psd_export16::records`, which reads the Lab policy off its document),
//! whose rasters and bakes are written as L, a, b and alpha at the declared
//! depth ([`channels_lab`]: ids `-1`, `0`, `1`, `2`, each code through
//! `lab_encode::psd_sample` — 8 bits P8, 16 bits L\* as the code and a\*/b\*
//! on Photoshop's 15-bit grid), whose masks and placeholders take the same
//! depth, and whose effect colours are `LbCl` ([`srgb_lbcl`], the exact
//! inverse of the read's `psd_import_lab::lbcl_srgb`). An adjustment layer is
//! written as its STAMP, a pixel layer holding the picture up to it,
//! reported as a loss: no Lab adjustment record was ever observed
//! (`psd_import_lab`'s H-L). The composite is the host's projection of this
//! revision, as an RGB document's is. The plan is `save_gray_psd`'s (the
//! 16-bit formula: the file's worst case, the canvas at 8 B a pixel, the
//! stamps' scratch and a wide style encoder).
//!
//! **Photoshop's own Lab layout.** An offline diff of the writer's output
//! against Photoshop's own Lab files (`lab-files-547/take2`'s `layers8.psd`,
//! `layers16.psd`, `layers16.psb`, `layers8.tif`, `layers16.tif` and
//! `styled8.psd`, read in place) moved it to the form Photoshop writes:
//!
//! * **L1** — a PSD or PSB holds THREE composite planes, L, a and b matted
//!   over Lab white (`lab_encode::matte_white`, then `psd_sample`:
//!   [`composite_planes_lab`]), header channels 3 and a POSITIVE record count
//!   (`psd_export_encode::write_psd`): Photoshop's Lab files carry no
//!   merged-alpha plane.
//! * **L2** — no ICC anywhere: no resource 1039 and no TIFF tag 34675,
//!   because L\*a\*b\* values are CIE D50 by definition, and Photoshop writes
//!   none.
//! * **L3** — an open or closed folder's `lsct` is 16 bytes, its sub-type (0)
//!   after the blend key ([`photoshop_form_lab`],
//!   `psd_export_encode::photoshop_folder_lsct`, Gray's M4).
//! * **L4** — a layered TIFF's strip is THREE CIELab samples (TIFF 6.0 §23,
//!   a\*/b\* signed) of the composite matted over Lab white
//!   ([`composite_strip_lab`]) under Photometric 8, big-endian,
//!   uncompressed, with no ExtraSamples and no SampleFormat, and its
//!   `Layr`/`Lr16` count is positive, as Photoshop's `layers8.tif` and
//!   `layers16.tif`.
//!
//! Recorded, NOT moved (Gray's list, forms the RGB writer shares and
//! Photoshop opens): the absent `lyid`/`shmd`/`clbl`/`infx`/`knko`/`lspf`/
//! `fxrp` blocks of a plain record, blending ranges only on a record with
//! Blend If, a 16-bit file's records in the layer-information section
//! rather than an `Lr16` block, flags 0x08 on a styled raster (Photoshop
//! 0x28), and RLE composite planes (Photoshop's 16-bit: Raw). Blend If needs
//! no move: a Lab document refuses a Gray range, so slot 0 is the default
//! and slots 1 to 3 hold Lightness, a and b, as Photoshop writes them (H-B).
//!
//! The layered mode carries the comps (`lyid`/`shmd`, 1065), raster-less
//! vector masks (`vmsk`) and the Background record as the RGB writer does
//! (`psd_typed_exchange`, stamped before L3).

use serde_json::{json, Value};

use crate::lab_document::LabDepth;
use crate::lab_encode::{matte_white, psd_sample, tiff_samples16, tiff_samples8};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, PrecisionImage, PrecisionStorage};
use crate::precision_style_render::StyleEnv;
use crate::psd_export16::{exported_fills16, knockout_written16, record_styles, records, Walk};
use crate::psd_export_cmyk::Depth;
use crate::psd_export_encode::{
    encode_plane, image_resources, layer_info_counted, photoshop_folder_lsct, write_psd, Record,
};
use crate::psd_export_preflight::PsdExportMode;
use crate::psd_framing::LayeredContainer;
use crate::psd_layer_pixels::import_envelope_bytes;

/// The colour planes of a Lab file.
const LAB_PLANES: usize = 3;

/// The first general limitation of a Lab export, in place of the RGB one.
fn first_limitation(container: LayeredContainer) -> &'static str {
    match container {
        LayeredContainer::Psd => "A Lab Color document is written as a Lab Color classic PSD at its declared depth (8 or 16 bits), its L*a*b* values stored as Photoshop stores them and no profile embedded, with PackBits (RLE) channel compression; ZIP is not written, and a Lab Color document is never written as RGB.",
        LayeredContainer::Psb => "A Lab Color document is written as a Lab Color large-document PSB at its declared depth (8 or 16 bits), its L*a*b* values stored as Photoshop stores them and no profile embedded, with PackBits (RLE) channel compression; ZIP is not written, and a Lab Color document is never written as RGB.",
        LayeredContainer::Tiff => "A Lab Color document is written as a Lab Color TIFF at its declared depth (8 or 16 bits): an uncompressed flat CIELab image of the composite over Lab white with Photoshop's layer block beside it, its channels PackBits (RLE) compressed, and no profile embedded.",
    }
}

/// The second general limitation of a Lab export, in place of the RGB one,
/// whose adjustment records a Lab file does not carry (module doc).
const STYLES_AND_ADJUSTMENTS: &str = "Layer styles are rasterized into their layer's pixels only when Photoshop's own dialog or a PSD mask cannot hold them, and are otherwise written as Photoshop layer effects with their colours in L*a*b* and blending options (lfx2, lfxs on a group, iOpa, blending ranges on Lightness, a and b) with the document's global light; either way they are reported per layer, and the effect setting Doctored does not model (the glows' technique) is not written. Every adjustment layer is written as a pixel layer holding the picture up to that layer, reported per layer: no Photoshop Lab adjustment record is written. No Photoshop type descriptor is written. Knockout and Blend Interior Effects as Group are written as Photoshop's own knko and infx records whenever the style itself is written, and only when they are not at their defaults.";

/// The ONE refusal sentence of a projection this export cannot read.
const PROJECTION: &str = "a Lab Color PSD export requires an RGBA16 UNORM projection";

/// One sample at the file's depth: a byte at 8 bits (already
/// `psd_sample`'s byte), else big-endian.
fn push_sample(depth: Depth, out: &mut Vec<u8>, sample: u16) {
    if depth.eight {
        out.push(sample as u8);
    } else {
        out.extend_from_slice(&sample.to_be_bytes());
    }
}

/// The file's bits per sample.
fn bits(depth: Depth) -> u16 {
    (depth.bytes() * 8) as u16
}

/// An RGBA16 Lab image's alpha, L, a and b channels at `depth`, ids `-1`,
/// `0`, `1`, `2` in that order, from ONE walk of its samples: alpha at the
/// depth (`Depth::push`), each code through `lab_encode::psd_sample`.
/// `None` for f32 storage.
pub(crate) fn channels_lab(
    image: &PrecisionImage,
    depth: Depth,
    large: bool,
) -> Option<Vec<(i16, Vec<u8>)>> {
    let (width, height) = image.dimensions();
    let count = width as usize * height as usize * depth.bytes() as usize;
    let mut planes: [Vec<u8>; 4] = std::array::from_fn(|_| Vec::with_capacity(count));
    let bits = bits(depth);
    for pixel in image.runs_rgba16()?.flatten() {
        let [alpha, colour @ ..] = &mut planes;
        depth.push(alpha, pixel[3]);
        for (channel, plane) in colour.iter_mut().enumerate() {
            push_sample(depth, plane, psd_sample(bits, channel, pixel[channel]));
        }
    }
    Some(
        [-1i16, 0, 1, 2]
            .into_iter()
            .zip(planes)
            .map(|(id, bytes)| {
                (
                    id,
                    encode_plane(&bytes, width * depth.bytes(), height, large),
                )
            })
            .collect(),
    )
}

/// The composite's straight codes matted over Lab white (module doc, L1 and
/// L4), opaque.
fn matted(composite: &PrecisionImage, width: u32, height: u32) -> Result<Vec<[u16; 4]>, String> {
    if composite.dimensions() != (width, height) {
        return Err("PSD composite preview does not match the canvas".into());
    }
    let runs = composite.runs_rgba16().ok_or(PROJECTION)?;
    Ok(runs
        .flatten()
        .map(|p| {
            [
                matte_white(0, p[0], p[3]),
                matte_white(1, p[1], p[3]),
                matte_white(2, p[2], p[3]),
                65535,
            ]
        })
        .collect())
}

/// A Lab document's THREE composite planes (L1): L, a and b matted over Lab
/// white, then stored as `lab_encode::psd_sample` at the file's `depth`. The
/// alpha is not written: Photoshop's own Lab files carry no merged-alpha
/// plane.
pub(crate) fn composite_planes_lab(
    composite: &PrecisionImage,
    width: u32,
    height: u32,
    depth: Depth,
) -> Result<Vec<Vec<u8>>, String> {
    let pixels = matted(composite, width, height)?;
    let bits = bits(depth);
    let mut planes: Vec<Vec<u8>> = (0..LAB_PLANES)
        .map(|_| Vec::with_capacity(pixels.len() * depth.bytes() as usize))
        .collect();
    for pixel in &pixels {
        for (channel, plane) in planes.iter_mut().enumerate() {
            push_sample(depth, plane, psd_sample(bits, channel, pixel[channel]));
        }
    }
    Ok(planes)
}

/// A Lab document's layered-TIFF strip (L4): the composite matted over Lab
/// white as three CIELab samples a pixel (`lab_encode::tiff_samples8` /
/// `tiff_samples16`, alpha dropped), big-endian at 16 bits.
pub(crate) fn composite_strip_lab(
    composite: &PrecisionImage,
    width: u32,
    height: u32,
    depth: Depth,
) -> Result<Vec<u8>, String> {
    let pixels = matted(composite, width, height)?;
    if depth.eight {
        let mut out = Vec::with_capacity(pixels.len() * LAB_PLANES);
        tiff_samples8(&pixels, false, &mut out);
        return Ok(out);
    }
    let mut samples = Vec::with_capacity(pixels.len() * LAB_PLANES);
    tiff_samples16(&pixels, false, &mut samples);
    Ok(samples.iter().flat_map(|v| v.to_be_bytes()).collect())
}

/// One authored sRGB colour as the L\*, a\*, b\* (D50) an `LbCl` stores,
/// UNQUANTISED: sRGB into the Lab twin, relative colorimetric
/// (`authored_color::authored_color`, Mode ▸ Lab Color's rule) — the exact
/// inverse of `psd_import_lab::lbcl_srgb` — scaled to L\* 0…100 and
/// a\*/b\* −128…127; `[0, 0, 0]` when the pipeline cannot build. Never the
/// 8-bit `AuthoredColors::color`, whose byte grid would quantise the Lab.
pub(crate) fn srgb_lbcl(rgb: [u8; 3]) -> [f64; 3] {
    let unit = rgb.map(|v| f32::from(v) / 255.0);
    crate::authored_color::authored_color(&crate::lab_document::lab_twin(), true, unit).map_or(
        [0.0; 3],
        |u| {
            [
                100.0 * f64::from(u[0]),
                255.0 * f64::from(u[1]) - 128.0,
                255.0 * f64::from(u[2]) - 128.0,
            ]
        },
    )
}

/// The record walk's output in Photoshop's own Lab form (module doc, L3),
/// applied after `psd_export16::records`.
pub(crate) fn photoshop_form_lab(records: &mut [Record]) {
    for record in records.iter_mut() {
        photoshop_folder_lsct(record);
    }
}

impl PrecisionDocument {
    /// TEST SEAM: [`srgb_lbcl`], the `LbCl` an authored sRGB colour is
    /// written as, which a black-box test cannot otherwise read for a whole
    /// colour grid.
    #[doc(hidden)]
    pub fn lbcl_of_srgb(rgb: [u8; 3]) -> [f64; 3] {
        srgb_lbcl(rgb)
    }

    /// TEST SEAM: `psd_import_lab::lbcl_srgb`, the sRGB an `LbCl` is read
    /// as ([`Self::lbcl_of_srgb`]'s inverse).
    #[doc(hidden)]
    pub fn srgb_of_lbcl(lab: [f64; 3]) -> [u8; 3] {
        crate::psd_import_lab::lbcl_srgb(lab)
    }

    /// The Lab export report (`dr_precision_doc_psd_export_report`'s Lab
    /// arm): the shared preflight's report at the declared depth with
    /// `"color_mode":"lab"` and the Lab first and second limitations.
    pub(crate) fn lab_psd_report_json(
        &self,
        container: LayeredContainer,
        mode: PsdExportMode,
        file: u64,
        reopen: u64,
    ) -> Value {
        let depth = self.lab_policy().map_or(LabDepth::Sixteen, |p| p.depth());
        let report = self.limited_psd_report(container, mode, file, reopen);
        let mut out = report.json(container, mode, depth.bits() as u16);
        out["color_mode"] = json!("lab");
        if let Some(first) = out["general_limitations"].get_mut(0) {
            *first = json!(first_limitation(container));
        }
        if let Some(second) = out["general_limitations"].get_mut(1) {
            *second = json!(STYLES_AND_ADJUSTMENTS);
        }
        out
    }

    /// Writes this Lab document as a colour-mode-9 PSD, PSB or layered TIFF
    /// at its declared depth (module doc). `projection` is the host's RGBA16
    /// rendering of THIS revision in the document's Lab twin. Pure, and
    /// atomic through the shared temp-file rename; the preflight's refusal is
    /// this call's error.
    pub(crate) fn save_lab_psd(
        &self,
        projection: Option<&PrecisionImage>,
        path: &str,
        container: LayeredContainer,
        mode: PsdExportMode,
        (limit, other): (u64, u64),
        job: crate::job_control::JobContext<'_>,
    ) -> Result<(), String> {
        let policy = self
            .lab_policy()
            .ok_or("a Lab Color PSD export needs a Lab Color document")?
            .clone();
        let report = self.limited_psd_report(
            container,
            mode,
            container.max_file_bytes(),
            import_envelope_bytes(),
        );
        if let Some(refusal) = report.refusal {
            return Err(refusal);
        }
        let projection = projection.ok_or(
            "a Lab Color document's PSD export requires its projection (required handle is NULL)",
        )?;
        let large = container.large();
        let (width, height) = self.dimensions();
        if projection.dimensions() != (width, height) {
            return Err("the supplied projection does not match this document's canvas".into());
        }
        if projection.storage() != PrecisionStorage::Rgba16Unorm {
            return Err(PROJECTION.into());
        }
        if projection.profile().bytes() != self.profile().bytes() {
            return Err("the supplied projection does not carry this document's ICC bytes".into());
        }
        // `save_gray_psd`'s formula: the file's worst case, the canvas at 8 B
        // a pixel, the stamps' scratch and a wide style encoder.
        let canvas_bytes = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|pixels| pixels.checked_mul(8))
            .ok_or("16-bit PSD export size calculation overflow")?;
        let wide_bytes = if crate::wide_color_native::styles_wide16(self) {
            crate::authored_color::wide_scratch_bytes()
        } else {
            0
        };
        admit(
            self.retained_bytes(),
            report
                .worst_case_bytes
                .saturating_add(canvas_bytes)
                .saturating_add(report.bake_scratch_bytes)
                .saturating_add(wide_bytes),
            other,
            limit,
        )?;
        let depth = Depth {
            eight: policy.depth() == LabDepth::Eight,
        };
        let colors = StyleEnv::colors_for(self.profile());
        let env = StyleEnv {
            light: self.global_light(),
            canvas: (width, height),
            colors: colors.as_ref(),
            transfer: None,
            float: false,
            float_style: None,
            knockout: crate::style_knockout::KnockoutBackdrop::NONE,
            capture: None,
            // A bake composites by the Lab arms.
            lab: self.lab_grid(),
        };
        let mut out = Vec::new();
        match mode {
            PsdExportMode::Layers => {
                records(
                    self.root_layers(),
                    self.profile(),
                    &env,
                    &Walk {
                        document: self,
                        limit,
                        other,
                        large,
                        float: false,
                        gray: None,
                    },
                    &mut 0,
                    &mut out,
                )?;
                // The comps, the raster-less vector masks and the Background
                // record (`psd_typed_exchange`), before Photoshop's own form
                // (`psd_export_gray`'s order).
                crate::psd_typed_exchange::stamp(self, &mut out, &knockout_written16, &|style| {
                    crate::psd_comps::effect_style_items(
                        style,
                        self.global_light(),
                        crate::psd_layer_effects::FxColours::Lab,
                    )
                });
                photoshop_form_lab(&mut out);
            }
            PsdExportMode::Flattened => out.push(Record {
                bounds: [0, 0, height as i32, width as i32],
                channels: channels_lab(projection, depth, large).ok_or(PROJECTION)?,
                blend: *b"norm",
                opacity: 255,
                clipping: 0,
                flags: 0x08,
                mask: None,
                name: "Composite".to_string(),
                divider: None,
                adjustment: None,
                blocks: Vec::new(),
                ranges: Vec::new(),
            }),
        }
        // The document's light, declared only when some record writes
        // effects (`psd_export16`'s rule).
        let light = out
            .iter()
            .any(|record| {
                record
                    .blocks
                    .iter()
                    .any(|(key, _)| matches!(key, b"lfx2" | b"lfxs" | b"lmfx"))
            })
            .then_some(self.global_light());
        // The pattern table (`psd_patterns`: `Patt` at 8 bits, `Pat2` at 16),
        // its tiles as stored (sRGB, Doctored's rule).
        let tail = match mode {
            PsdExportMode::Layers => {
                // The comps' recorded styles after the layers' own: their patterns
                // ride in the same table (`psd_comp_limits::recorded_styles`).
                let comp_styles = crate::psd_typed_exchange::comp_styles(self, &knockout_written16);
                let mut styles = Vec::new();
                record_styles(self.root_layers(), &mut styles);
                styles.extend(comp_styles.iter());
                let mut fills = Vec::new();
                exported_fills16(self.root_layers(), &mut fills);
                let fills: Vec<_> = fills
                    .iter()
                    .filter_map(crate::psd_fill_layer::Exported::pattern)
                    .collect();
                crate::psd_patterns::tail_blocks_with(styles, &fills, !depth.eight)
            }
            PsdExportMode::Flattened => Vec::new(),
        };
        // The paths' resources, and the layered mode's 1065
        // (`psd_typed_exchange`).
        let extra =
            crate::psd_typed_exchange::resources(self, matches!(mode, PsdExportMode::Layers));
        // L2: no profile anywhere.
        let bytes = if container == LayeredContainer::Tiff {
            crate::tiff_layers::write(
                width,
                height,
                bits(depth),
                9,
                &[],
                self.resolution(),
                &image_resources(&[], self.resolution(), light, self.color_samplers(), &extra),
                &layer_info_counted(&out, false, false)?,
                &composite_strip_lab(projection, width, height, depth)?,
                &tail,
            )?
        } else {
            write_psd(
                width,
                height,
                bits(depth),
                9,
                &[],
                self.resolution(),
                light,
                self.color_samplers(),
                &out,
                &composite_planes_lab(projection, width, height, depth)?,
                large,
                &tail,
                &crate::psd_spot::SpotPlanes::default(),
                &extra,
            )?
        };
        job.check()?;
        crate::dr_image::save_bytes_atomically(path, &bytes, job)
    }
}
