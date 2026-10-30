//! The flattened composite's grey in a Gray working space: what an 8-bit RGB
//! document's Load Selection from its luminosity reads (`gray_working`'s
//! model and its Photoshop record). The relative PCS Y of each pixel's
//! encoded RGB comes from `cmyk_color::document_xyz`, the one RGB → PCS
//! decode (the matrix/TRC model, else the LUT's relative table), and goes
//! through [`GrayWorkingSpace::gray`]; alpha is not read, as the Rec. 709
//! luma plane does not read it.
//!
//! Apply Image, Calculations, the luminosity masks, the histogram and
//! `composite_plane(Plane::Luma)` keep Rec. 709: only Load Selection takes
//! the Gray working space.

use crate::cmyk_color::document_xyz;
use crate::doc::DrDocument;
use crate::gray_working::GrayWorkingSpace;

impl DrDocument {
    /// The flattened composite's grey in `space`, canvas-sized, row 0 top:
    /// `round(255 · gray(Y))`, Y the document profile's relative PCS Y of the
    /// straight RGB codes over 255. `None` when the profile has neither a
    /// matrix/TRC model nor a LUT (no Y to read), or the composite fails.
    pub(crate) fn composite_gray_plane(&self, space: GrayWorkingSpace) -> Option<Vec<u8>> {
        let profile = self.profile.as_ref();
        if profile.model().is_none() && profile.lut().is_none() {
            return None;
        }
        let image = self.flattened().ok()?;
        let unit = |v: u8| f64::from(v) / 255.0;
        let code = |y: f64| (255.0 * space.gray(y)).round() as u8;
        if profile.model().is_some() {
            // A matrix/TRC model's Y is a sum of one term per channel (its Y
            // row times that channel's curve, `MatrixTrc::to_pcs_xyz64`), so
            // three 256-entry tables of `document_xyz` itself reproduce every
            // pixel's Y: each entry is the channel's term over the black
            // point's, and black's Y is added back once.
            let black = document_xyz(profile, [0.0; 3])[1];
            let tables: [Vec<f64>; 3] = std::array::from_fn(|c| {
                (0..=255u8)
                    .map(|v| {
                        let mut rgb = [0.0; 3];
                        rgb[c] = unit(v);
                        document_xyz(profile, rgb)[1] - black
                    })
                    .collect()
            });
            return Some(
                image
                    .pixels()
                    .map(|p| {
                        let y = black
                            + tables[0][usize::from(p[0])]
                            + tables[1][usize::from(p[1])]
                            + tables[2][usize::from(p[2])];
                        code(y)
                    })
                    .collect(),
            );
        }
        Some(
            image
                .pixels()
                .map(|p| code(document_xyz(profile, [unit(p[0]), unit(p[1]), unit(p[2])])[1]))
                .collect(),
        )
    }
}
