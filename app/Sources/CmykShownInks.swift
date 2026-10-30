import AppKit

/// A CMYK document's inks as the colours a sheet SHOWS (#1): the Gradient
/// Map sheet's stops and the Layer Style sheet's colour slots.
///
/// A sheet edits `#rrggbb` colours in `space`; the document stores ink. Each
/// ink the sheet opens on is shown as its colour under the document's profile
/// and remembered by that colour, so a slot the user leaves alone gives back
/// its ink EXACTLY; any other colour is separated through the profile
/// (`CmykPaint.ink`) when the sheet builds what it sends.
struct CmykShownInks {
    let document: RasterPrecisionDocument
    let space: NSColorSpace
    /// The inks shown so far, by the colour shown for them.
    private var known: [String: [Double]] = [:]

    init(document: RasterPrecisionDocument, space: NSColorSpace) {
        self.document = document
        self.space = space
    }

    /// The colour shown for `ink` (four fractions 0…1), remembered so the
    /// same colour gives the same ink back.
    mutating func shownHex(ofInk ink: [Double]) -> String {
        guard let value = CmykInk(percent: ink.map { $0 * 100 }),
              let rgb = try? RasterPrecisionDocument.srgb(fromInk: value, document: document),
              rgb.count == 3
        else { return "#000000" }
        let hex = AdjustmentColor.hex(
            NSColor(srgbRed: rgb[0], green: rgb[1], blue: rgb[2], alpha: 1), in: space)
        known[hex] = ink
        return hex
    }

    /// Remembers `ink` as the ink of the shown colour `hex` (a swatch whose
    /// stored ink the sheet keeps).
    mutating func remember(_ ink: [Double], shownAs hex: String) {
        known[hex] = ink
    }

    /// The ink of a shown colour: a known one, else separated.
    func ink(ofShown hex: String) -> [Double] {
        if let ink = known[hex] { return ink }
        guard let color = AdjustmentColor.color(fromHex: hex, in: space),
              let separated = try? CmykPaint.ink(color, stored: nil, in: document)
        else { return [0, 0, 0, 1] }
        return Array(separated.prefix(4))
    }
}
