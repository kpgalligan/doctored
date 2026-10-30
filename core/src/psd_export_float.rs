//! A 32-bit float document's PSD and PSB export, the way Photoshop
//! writes 32 bits a channel: header depth 32, RGB, four channels;
//! Photoshop's own 32-bit color-mode data (its `hdrt`/`hdra` blocks); the
//! LINEAR TWIN of the document's profile (1039: the same colorants with
//! gamma-1.0 curves, named "<profile> (Linear RGB Profile)",
//! `icc_builtin::linear_twin`), as Photoshop embeds at 32 bits; the
//! resolution (1005); an EMPTY
//! layer-information sub-section, the empty global layer mask, an empty
//! `Mt32` block declaring the merged alpha and the records in an `Lr32` tail
//! block with a positive count — Photoshop 26.10.0's own form, read from its
//! file in a Photoshop take (`psd_export_encode::write_psd32`); every
//! channel, mask and placeholder plane Raw; the stored composite Raw.
//!
//! Samples are the document's stored linear-light f32, straight alpha, no
//! curve applied — the `float_io` TIFF convention, bit for bit. A mask (`-2`)
//! is its u16 coverage as big-endian f32 `v / 65535`, which the reader
//! (`psd_raster_masks::MaskRecord::attach_typed`) returns exactly. The
//! composite is the host's projection matted over white,
//! `m = c·a + (1 − a)` per colour sample with alpha straight (the classic
//! writers' matte in float, candidate H1 of the take), which
//! `psd_import_float::composite32` inverts.
//!
//! The record walk is `psd_export16`'s (`Walk::float`): a group, a raster, a
//! written adjustment record and a preserved style travel exactly as at 16
//! bits. Every style and fill colour is written as Photoshop's 32-bit
//! numbers, the document's LINEAR light × 255 (`FxColours::Float`, through
//! the save's `style_float::FloatStyle`), and a solid color fill is
//! Photoshop's PIXEL-LESS 32-bit `SoCo` record (`fill_style32.psd`'s form,
//! `psd_fill_layer`'s 32 bits). What Photoshop has shown no 32-bit form for
//! — a gradient or pattern fill layer, a style needing a bake, an adjustment
//! with no Photoshop record — refuses the LAYERED mode by name
//! ([`layers_refusal`]); the flattened mode always writes. The layered TIFF
//! carries the same records big-endian; the little-endian reader refuses a
//! descriptor block (`SoCo`, `lfx2`) by name.
//!
//! **The layered TIFF** is Photoshop's own `layered32.tif` form, big-endian
//! (`tiff_layers::write_with`): the same record walk, the records in an
//! `Lr32` block in tag 37724 with a POSITIVE count and no `Mt32` (the
//! TIFF's flat image carries the alpha), every channel Raw f32 as in the PSD;
//! the flat image the projection's straight RGBA as IEEE f32 (SampleFormat
//! 3, ExtraSamples 2), not matted; tag 34675 and resource 1039 the profile's
//! linear twin. Little-endian is read (`tiff_layers_order`), not written.
//!
//! # Admission
//!
//! The 16-bit export's shape: the preflight's worst-case file bytes plus a
//! canvas term and a wide style colour's encoder. The record list and the
//! file are the two copies of the planes the save holds
//! (`psd_export_encode::write_psd32` lays the file into one exactly-sized
//! buffer and writes the composite straight into it; `tiff_layers::write_with`
//! does the same with the layer information and the flat strip).
//!
//! Measured 2026-10-20 with `core/examples/psd_float_admission.rs` (Release,
//! one case per process, `MallocLargeCache=0`, process limit 8 GiB, three runs
//! and a baseline twin; E = the `phys_footprint` interval peak −
//! the footprint before the save, the document and projection held outside
//! it), 6000×4000, one opaque layer, layered PSD: the twin reads E 0; the
//! save max E 1,152,697,184 before and 1,152,680,800 after the canvas term
//! was raised. At the 16-bit shape's 16 B a pixel the plan was 1,158,083,400
//! (1.0047 × E), a miss; 28 B a pixel is the smallest integer covering
//! ⌈1.25 × max E⌉ = 1,440,871,480: plan 1,446,083,400 (1.2545 × E). (Before
//! the single-buffer writer, E was 2,305,394,296: the layer information, the
//! section and the file were three more copies.) The layered TIFF,
//! measured 2026-10-23 the same way (`save-tiff`, `twin-tiff`): the twin
//! reads E 16,384; max E 1,152,680,800 over three runs against the plan
//! 1,446,082,660 (1.2545 × E; ⌈1.25 × max E⌉ = 1,440,851,000), so the 28 B
//! term stands unchanged.

use crate::icc_builtin::linear_twin;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, PrecisionImage, PrecisionStorage};
use crate::precision_style_render::StyleEnv;
use crate::psd_export16::{
    export_style16, exported_fill16, exported_fills16, record_styles, records, Walk,
};
use crate::psd_export_encode::{
    image_resources, layer_info_counted_into, layer_info_len, raw_plane, write_psd32, Record,
};
use crate::psd_export_preflight::PsdExportMode;
use crate::psd_framing::LayeredContainer;
use crate::style::GlobalLight;
use crate::tiff_layers::{write_with, ByteOrder, LayeredTiff};

/// Why ONE record has no 32-bit form in the layered mode, or `None`: a fill
/// layer Photoshop has shown no 32-bit record for (a gradient or pattern;
/// a solid is `SoCo`, module doc), a style that would have to be
/// rasterized, or an adjustment with no Photoshop record.
pub(crate) fn layer_refusal(layer: &PrecisionDocumentLayer) -> Option<String> {
    let name = layer.name();
    if let Some(fill) = exported_fill16(layer).filter(|fill| !fill.written_at_32()) {
        return Some(format!(
            "the {} fill layer “{name}” has no 32-bit PSD record in Doctored; rasterize it, or export Flattened",
            fill.kind_word()
        ));
    }
    if export_style16(layer, false).is_some_and(|style| style.bake.is_some()) {
        return Some(format!(
            "the layer style on “{name}” would have to be rasterized, which Doctored does not do at 32 bits; export Flattened"
        ));
    }
    if let Content::Adjustment(state) = &layer.content {
        if crate::psd_adjustment::encode(state.description()).is_none() {
            return Some(format!(
                "the {} adjustment layer “{name}” has no Photoshop record and is not baked at 32 bits; export Flattened",
                state.kernel.operation()
            ));
        }
    }
    None
}

/// The first record in postorder (children before their group, bottom
/// first) that [`layer_refusal`] refuses.
pub(crate) fn layers_refusal(layers: &[PrecisionDocumentLayer]) -> Option<String> {
    layers.iter().find_map(|layer| {
        layer
            .children()
            .and_then(layers_refusal)
            .or_else(|| layer_refusal(layer))
    })
}

/// An RGBA32 float image's alpha, red, green and blue channels as Raw planes
/// of big-endian f32, ids -1, 0, 1, 2; `None` for u16 storage.
pub(crate) fn channels32(image: &PrecisionImage) -> Option<Vec<(i16, Vec<u8>)>> {
    let (width, height) = image.dimensions();
    let count = width as usize * height as usize * 4;
    let mut planes: [Vec<u8>; 4] = std::array::from_fn(|_| Vec::with_capacity(count));
    image.runs_rgba32()?.flatten().for_each(|pixel| {
        let [a, r, g, b] = &mut planes;
        a.extend_from_slice(&pixel[3].to_be_bytes());
        r.extend_from_slice(&pixel[0].to_be_bytes());
        g.extend_from_slice(&pixel[1].to_be_bytes());
        b.extend_from_slice(&pixel[2].to_be_bytes());
    });
    Some(
        [-1i16, 0, 1, 2]
            .into_iter()
            .zip(planes)
            .map(|(id, bytes)| (id, raw_plane(&bytes)))
            .collect(),
    )
}

/// A u16 coverage plane as a Raw plane of big-endian f32 `v / 65535`. The
/// quotient is formed in f64 and rounded once to f32, so the reader's
/// `round(v · 65535)` returns every code exactly.
pub(crate) fn mask_plane32(samples: &[u16]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 4);
    for sample in samples {
        bytes.extend_from_slice(&((f64::from(*sample) / 65535.0) as f32).to_be_bytes());
    }
    raw_plane(&bytes)
}

/// The four composite planes of a float canvas appended to `out`, row-major
/// big-endian f32 in R, G, B, A order: each colour sample matted over white,
/// `c·a + (1 − a)`, alpha straight (module doc). Exact at alpha 1 (the colour
/// itself) and 0 (1.0). Written straight into the file's buffer
/// (`psd_export_encode::write_psd32`), one plane per walk of the runs.
pub(crate) fn append_composite32(
    composite: &PrecisionImage,
    width: u32,
    height: u32,
    out: &mut Vec<u8>,
) -> Result<(), String> {
    if composite.dimensions() != (width, height) {
        return Err("PSD composite preview does not match the canvas".into());
    }
    let matte = |c: f32, a: f32| (f64::from(c) * f64::from(a) + (1.0 - f64::from(a))) as f32;
    for component in 0..4 {
        let runs = composite
            .runs_rgba32()
            .ok_or("32-bit PSD export requires an RGBA32 float projection")?;
        for pixel in runs.flatten() {
            let value = if component == 3 {
                pixel[3]
            } else {
                matte(pixel[component], pixel[3])
            };
            out.extend_from_slice(&value.to_be_bytes());
        }
    }
    Ok(())
}

/// A 32-bit layered TIFF's flat strip appended to `out`: the projection's
/// straight RGBA, interleaved, row 0 first, each sample a big-endian f32 as
/// stored (module doc), no matte.
pub(crate) fn append_flat32(
    projection: &PrecisionImage,
    width: u32,
    height: u32,
    out: &mut Vec<u8>,
) -> Result<(), String> {
    if projection.dimensions() != (width, height) {
        return Err("layered TIFF flat image does not match the canvas".into());
    }
    let runs = projection
        .runs_rgba32()
        .ok_or("32-bit PSD export requires an RGBA32 float projection")?;
    for pixel in runs.flatten() {
        for sample in pixel {
            out.extend_from_slice(&sample.to_be_bytes());
        }
    }
    Ok(())
}

impl PrecisionDocument {
    /// The 32-bit PSD save's projection checks and admission (module doc's
    /// measurement), the one home both 32-bit writers call (this module's
    /// and `psd_export_gray32`'s): `projection` canvas-sized, RGBA32 float,
    /// in this document's exact ICC bytes; then the preflight's
    /// `worst_case_bytes`, the measured 28 B a canvas pixel and a wide style
    /// or fill colour's encoder, beside the retained document.
    pub(crate) fn admit_psd32(
        &self,
        projection: &PrecisionImage,
        worst_case_bytes: u64,
        limit: u64,
        other: u64,
    ) -> Result<(), String> {
        let (width, height) = self.dimensions();
        if projection.dimensions() != (width, height) {
            return Err("the supplied projection does not match this document's canvas".into());
        }
        if projection.storage() != PrecisionStorage::Rgba32Float {
            return Err("32-bit PSD export requires an RGBA32 float projection".into());
        }
        if projection.profile().bytes() != self.profile().bytes() {
            return Err("the supplied projection does not carry this document's ICC bytes".into());
        }
        // 28 B a canvas pixel beside the preflight's worst case: MEASURED
        // (module doc), the 16-bit shape's 16 having missed the 1.25 margin.
        let canvas_bytes = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|pixels| pixels.checked_mul(28))
            .ok_or("32-bit PSD export size calculation overflow")?;
        let mut fills = Vec::new();
        exported_fills16(self.root_layers(), &mut fills);
        let wide_fill = fills.iter().any(crate::psd_fill_layer::Exported::is_wide);
        let wide_bytes = if crate::wide_color_native::styles_wide16(self) || wide_fill {
            crate::authored_color::wide_scratch_bytes()
        } else {
            0
        };
        admit(
            self.retained_bytes(),
            worst_case_bytes
                .saturating_add(canvas_bytes)
                .saturating_add(wide_bytes),
            other,
            limit,
        )
    }

    /// Writes this float document as a 32-bit RGB PSD, PSB or layered TIFF
    /// (module doc).
    /// Pure, and a refused export never touches an existing destination: the
    /// preflight runs first, the whole file is built in memory and lands
    /// through the shared atomic rename. `projection` is the host's own
    /// RGBA32 float rendering of THIS revision, in the document's exact ICC
    /// bytes; it is the flattened mode's one record and the stored composite.
    pub(crate) fn save_psd_float(
        &self,
        projection: Option<&PrecisionImage>,
        path: &str,
        container: LayeredContainer,
        mode: PsdExportMode,
        limit: u64,
        other: u64,
    ) -> Result<(), String> {
        let report = self.psd_export_report(container, mode);
        if let Some(refusal) = report.refusal {
            return Err(refusal);
        }
        let projection = projection.ok_or(
            "an RGB document's PSD export requires its projection (required handle is NULL)",
        )?;
        let large = container.large();
        let (width, height) = self.dimensions();
        self.admit_psd32(projection, report.worst_case_bytes, limit, other)?;
        let colors = StyleEnv::colors_for(self.profile());
        // Every style and fill colour's 32-bit numbers (module doc).
        let float_style = crate::style_float::FloatStyle::for_profile(self.profile())?;
        let env = StyleEnv {
            light: self.global_light(),
            canvas: (width, height),
            colors: colors.as_ref(),
            // The walk writes stored planes and encodes preserved styles;
            // nothing is rendered or baked at 32 bits.
            transfer: None,
            float: false,
            float_style: Some(&float_style),
            knockout: crate::style_knockout::KnockoutBackdrop::NONE,
            capture: None,
            lab: None,
        };
        let mut out = Vec::new();
        match mode {
            PsdExportMode::Layers => records(
                self.root_layers(),
                self.profile(),
                &env,
                &Walk {
                    document: self,
                    limit,
                    other,
                    large,
                    float: true,
                    gray: None,
                },
                &mut 0,
                &mut out,
            )?,
            PsdExportMode::Flattened => out.push(Record {
                bounds: [0, 0, height as i32, width as i32],
                channels: channels32(projection)
                    .ok_or("32-bit PSD export requires an RGBA32 float projection")?,
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
        // effects — the 8- and 16-bit rule.
        let light: Option<GlobalLight> = out
            .iter()
            .any(|record| {
                record
                    .blocks
                    .iter()
                    .any(|(key, _)| matches!(key, b"lfx2" | b"lfxs" | b"lmfx"))
            })
            .then_some(self.global_light());
        let tail = match mode {
            PsdExportMode::Layers => {
                let mut styles = Vec::new();
                record_styles(self.root_layers(), &mut styles);
                crate::psd_patterns::tail_blocks_with(styles, &[], true)
            }
            PsdExportMode::Flattened => Vec::new(),
        };
        // Photoshop's 32-bit convention: the profile's linear-light twin.
        let twin = linear_twin(self.profile());
        let bytes = if container == LayeredContainer::Tiff {
            // Photoshop's `layered32.tif`: a positive count, no
            // `Mt32`, the records beside an IEEE-float flat image.
            let resources = image_resources(&twin, self.resolution(), light, self.color_samplers());
            let flat = width as usize * height as usize * 16;
            write_with(
                &LayeredTiff {
                    width,
                    height,
                    depth: 32,
                    color: 3,
                    icc: &twin,
                    resolution: self.resolution(),
                    resources: &resources,
                    tail: &tail,
                    order: ByteOrder::Big,
                    spots: 0,
                },
                layer_info_len(&out, false)?,
                &|file| layer_info_counted_into(&out, false, false, file).map(|_| ()),
                Some(flat),
                &|file| append_flat32(projection, width, height, file),
            )?
        } else {
            write_psd32(
                width,
                height,
                &twin,
                self.resolution(),
                light,
                self.color_samplers(),
                &out,
                large,
                &tail,
                false,
                &|file| append_composite32(projection, width, height, file),
            )?
        };
        drop(out);
        crate::dr_image::save_atomically(path, |tmp| {
            std::fs::write(tmp, &bytes).map_err(|e| format!("failed to write {path}: {e}"))
        })
    }
}
