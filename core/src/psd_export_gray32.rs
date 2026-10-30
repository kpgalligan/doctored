//! A 32-bit Grayscale document's PSD and PSB export (#547 phase 3c): the
//! 32-bit RGB writer's float half (`psd_export_float`) in the 8/16-bit gray
//! writer's colour mode (`psd_export_gray`), laid out as Photoshop's own
//! `layers32.psd` (`gray-files-547/take2`).
//!
//! * **Header and blocks** — depth 32, colour mode 1, ONE channel;
//!   Photoshop's 32-bit color-mode data; NO `Mt32` block (a gray file carries
//!   no merged alpha, phase 3b's M1) and the records in an `Lr32` block with a
//!   positive count (`psd_export_encode::write_psd32` with `gray`).
//! * **Profile** — resource 1039 is the LINEAR TWIN of the document's Gray
//!   profile (`icc_builtin::gray_linear_twin`: the same `desc` plus
//!   " (Linear Grayscale Profile)", a gamma-1.0 `kTRC`), as Photoshop embeds
//!   a gamma-1.0 Gray profile at 32 bits; Photoshop's own linear profile,
//!   reopened, is written back verbatim. `psd_import_gray::gray_policy`
//!   relabels our twin to its built-in on the way back in.
//! * **Records** — `psd_export16::records` with `Walk { float: true, gray }`:
//!   every raster's R (R = G = B) and alpha as Raw big-endian f32, ids `-1`
//!   and `0` ([`channels_gray32`]), masks as the float walk's f32 coverage,
//!   two placeholder planes on a pixel-less record; then
//!   `psd_export_gray::photoshop_form` (M3–M5).
//! * **Composite** — ONE Raw plane, the projection's gray matted over white,
//!   `c·a + (1 − a)` in f64 rounded once to f32 ([`append_composite_gray32`],
//!   `append_composite32`'s rule on one plane); the alpha is not written.
//!
//! The report is the float preflight (`limited_psd_report` at 4 B a sample;
//! a layered TIFF refuses with [`TIFF_REFUSAL`]: the 32-bit layered TIFF
//! is RGB only), and the admission is the 32-bit RGB save's measured
//! one (`admit_psd32`), which charges four planes where this writes two: no
//! new plan.

use crate::gray_document::GrayDepth;
use crate::icc_builtin::gray_linear_twin;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::PrecisionImage;
use crate::precision_style_render::StyleEnv;
use crate::psd_export16::{record_styles, records, Walk};
use crate::psd_export_cmyk::Depth;
use crate::psd_export_encode::{raw_plane, write_psd32, Record};
use crate::psd_export_gray::photoshop_form;
use crate::psd_export_preflight::PsdExportMode;
use crate::psd_framing::LayeredContainer;
use crate::psd_layer_pixels::import_envelope_bytes;
use crate::style::GlobalLight;

/// Why a 32-bit Grayscale document's layered TIFF is refused, naming the
/// routes that work.
pub(crate) const TIFF_REFUSAL: &str = "A TIFF with layers is not written for a 32-bit Grayscale document; export a PSD or PSB, or a flat 32-bit TIFF (File ▸ Export…).";

/// An RGBA32 float image's alpha and gray (its R) channels as Raw planes of
/// big-endian f32, ids `-1` and `0`, from ONE walk of its samples; `None` for
/// u16 storage.
pub(crate) fn channels_gray32(image: &PrecisionImage) -> Option<Vec<(i16, Vec<u8>)>> {
    let (width, height) = image.dimensions();
    let count = width as usize * height as usize * 4;
    let mut planes: [Vec<u8>; 2] = std::array::from_fn(|_| Vec::with_capacity(count));
    image.runs_rgba32()?.flatten().for_each(|pixel| {
        let [a, gray] = &mut planes;
        a.extend_from_slice(&pixel[3].to_be_bytes());
        gray.extend_from_slice(&pixel[0].to_be_bytes());
    });
    Some(
        [-1i16, 0]
            .into_iter()
            .zip(planes)
            .map(|(id, bytes)| (id, raw_plane(&bytes)))
            .collect(),
    )
}

/// The ONE composite plane of a 32-bit Gray canvas appended to `out`,
/// row-major big-endian f32: the gray (R) matted over white, `c·a + (1 − a)`
/// formed in f64 and rounded once to f32 (module doc). Exact at alpha 1 (the
/// gray itself) and 0 (1.0).
pub(crate) fn append_composite_gray32(
    composite: &PrecisionImage,
    width: u32,
    height: u32,
    out: &mut Vec<u8>,
) -> Result<(), String> {
    if composite.dimensions() != (width, height) {
        return Err("PSD composite preview does not match the canvas".into());
    }
    let runs = composite
        .runs_rgba32()
        .ok_or("32-bit PSD export requires an RGBA32 float projection")?;
    for pixel in runs.flatten() {
        let (c, a) = (f64::from(pixel[0]), f64::from(pixel[3]));
        out.extend_from_slice(&((c * a + (1.0 - a)) as f32).to_be_bytes());
    }
    Ok(())
}

impl PrecisionDocument {
    /// Writes this 32-bit Gray document as a 32-bit Grayscale PSD or PSB
    /// (module doc). Pure, and a refused export never touches an existing
    /// destination: the preflight runs first, the whole file is built in
    /// memory and lands through the shared atomic rename. `projection` is the
    /// host's RGBA32 float rendering of THIS revision in the document's twin.
    pub(crate) fn save_gray_psd32(
        &self,
        projection: Option<&PrecisionImage>,
        path: &str,
        container: LayeredContainer,
        mode: PsdExportMode,
        limit: u64,
        other: u64,
    ) -> Result<(), String> {
        let policy = self
            .gray_policy()
            .filter(|policy| policy.depth() == GrayDepth::ThirtyTwo)
            .ok_or("a 32-bit Grayscale PSD export needs a 32-bit Grayscale document")?
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
            "a Grayscale document's PSD export requires its projection (required handle is NULL)",
        )?;
        self.admit_psd32(projection, report.worst_case_bytes, limit, other)?;
        let large = container.large();
        let (width, height) = self.dimensions();
        let colors = StyleEnv::colors_for(self.profile());
        let env = StyleEnv {
            light: self.global_light(),
            canvas: (width, height),
            colors: colors.as_ref(),
            // The walk writes stored planes and encodes preserved styles;
            // nothing is rendered or baked at 32 bits.
            transfer: None,
            float: false,
            float_style: None,
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
                    gray: Some((Depth { eight: false }, &policy)),
                },
                &mut 0,
                &mut out,
            )
            .map(|()| photoshop_form(&mut out))?,
            PsdExportMode::Flattened => out.push(Record {
                bounds: [0, 0, height as i32, width as i32],
                channels: channels_gray32(projection)
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
        // effects — the 8-, 16- and 32-bit rule.
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
        let twin = gray_linear_twin(policy.bytes());
        let bytes = write_psd32(
            width,
            height,
            &twin,
            self.resolution(),
            light,
            self.color_samplers(),
            &out,
            large,
            &tail,
            true,
            &|file| append_composite_gray32(projection, width, height, file),
        )?;
        drop(out);
        crate::dr_image::save_atomically(path, |tmp| {
            std::fs::write(tmp, &bytes).map_err(|e| format!("failed to write {path}: {e}"))
        })
    }
}
