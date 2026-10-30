import AppKit

/// The Black & White dialog: the six hue-weight sliders plus a Tint
/// checkbox and swatch.
///
/// Every parameter, range and default comes from `AdjustmentSchema`; the
/// lifecycle — the captured handles, the debounced in-context preview, one
/// undo step on Apply and nothing on Cancel — is `AdjustmentSheet`'s, in all
/// three modes (Image ▸ Adjustments, Layer ▸ New Adjustment Layer, and
/// Adjustment Options… on an existing layer).
///
/// Two conventions this sheet carries, both from the core's schema row for
/// `black_and_white`:
///
/// - **The sliders are percentages; the meta stores fractions.**
///   A weight runs −200…300 % on screen and −2…3 in the params, quantized to
///   whole percent — what makes an unchanged re-commit byte-stable: 40/100 is
///   exactly the 0.4 the schema defaults to, so a weight left alone
///   re-encodes to the identical number.
/// - **The tint colour is the DOCUMENT's numbers** (`AdjustmentColor`). A
///   colour the layer already stores is used verbatim; the factory `#998a66`
///   is an AUTHORED sRGB preset, so it converts once — here, at the moment
///   the sheet authors it. All three numbers reach the op: the colour's
///   luminosity is set to the grey the weights produced (SetLum with
///   ClipColor, measured against Photoshop), so a darker or
///   lighter swatch of one hue tints differently. No weight can tint a
///   neutral pixel.
///
/// There is no Auto button. Setting the weights from the picture needs a
/// canvas-sized hue histogram, and neither the core nor this side has one;
/// a button that guessed would be worse than no button.
final class BlackAndWhiteSheet: AdjustmentSheet {
    /// The six weights in `AdjustmentSchema.hueBands` order, as the core's
    /// fractions; all of this sheet's state is read in `loadState()`.
    private var weights: [Double] = AdjustmentSchema.blackAndWhiteDefaults

    private var tint = false

    /// The swatch's colour, in the document's numbers.
    private var tintHex = AdjustmentSchema.blackAndWhiteTint

    /// The keys the params the sheet opened on spelled out — see
    /// `currentParams()` for why that is worth remembering.
    private var explicitKeys: Set<String> = []

    override func loadState() {
        weights = zip(AdjustmentSchema.hueBands, AdjustmentSchema.blackAndWhiteDefaults)
            .map { band, factory in Self.clamped(initial.number(band, default: factory), -2, 3) }
        tint = initial.bool("tint", default: false)
        explicitKeys = Set(initial.params.keys)
        let factory = AdjustmentSchema.blackAndWhiteTint
        // In `.edit` the params are the LAYER's own, so a colour it stores is
        // already the document's numbers and is shown verbatim — and so is a
        // Photoshop preset's, read by PSD import's `blwh` rule, and a saved
        // preset's whenever it is not the factory spelling. Everywhere
        // else — and for a layer that stores none — the value is the factory
        // tint, an authored sRGB preset, converted once here so the swatch
        // and what gets stored are the document's own spelling of it.
        let stored = initial.params["tint_color"] != nil
            ? initial.colorHex("tint_color", default: factory) : nil
        if let stored, case .edit = mode {
            tintHex = stored
        } else if let stored, preset != nil || stored != factory {
            tintHex = stored
        } else {
            tintHex = AdjustmentColor.documentHex(forSRGBHex: factory, in: documentColorSpace)
        }
        tintHexEdited = false
    }

    /// Whether the user picked a tint colour in this sitting. A pick made
    /// while Tint is off still has to survive Apply — the colour is the
    /// user's, not the checkbox's.
    private var tintHexEdited = false

    private var tintWell: NSColorWell?

    // MARK: - Content

    override func makeContent() -> NSView {
        var rows: [[NSView]] = []
        for (index, band) in AdjustmentSchema.hueBands.enumerated() {
            // The row label IS the schema key capitalized, so the dialog and
            // the params can never name different things.
            rows.append(
                sliderRow(
                    "\(band.capitalized):", range: -200...300, value: weights[index] * 100,
                    format: Self.percent
                ) { [weak self] percent in
                    self?.weights[index] = percent.rounded() / 100
                })
        }
        rows.append(
            checkboxRow("Tint", value: tint) { [weak self] on in
                self?.tint = on
                self?.syncTintWell()
            })
        let colorViews = colorRow("Tint color:", hex: tintHex) { [weak self] hex in
            self?.tintHex = hex
            self?.tintHexEdited = true
        }
        tintWell = colorViews.compactMap { $0 as? NSColorWell }.first
        rows.append(colorViews)

        let note = NSTextField(
            wrappingLabelWithString:
                "Each weight sets how much of that hue reaches the grey. A neutral pixel "
                + "has no hue, so no weight can tint one — that is what Tint is for: the "
                + "swatch's colour is brought to the grey's brightness, so its own lightness "
                + "counts too.")
        note.font = DS.sans(12)
        note.textColor = DS.textMuted
        note.preferredMaxLayoutWidth = sheetWidth - sheetInset * 2

        let stack = NSStackView(views: [AdjustmentSheet.grid(rows), note])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 12
        syncTintWell()
        return stack
    }

    /// The swatch only means anything while Tint is on, so it greys out —
    /// and a disabled well also refuses to open the colour panel behind the
    /// sheet.
    private func syncTintWell() {
        tintWell?.isEnabled = tint
    }

    // MARK: - Params

    /// One rule for every key here: **write it when its value differs from
    /// the core's default for it, or when the params the sheet opened on
    /// already carried it.** The core reads an absent key as exactly that
    /// default, so the pixels are the same either way; what the rule buys is
    /// a small meta (it travels in `.rz` and in every `get_document` reply)
    /// and a byte-identical re-commit when nothing was touched.
    ///
    /// `tint_color` takes one clause more: it is also written whenever Tint
    /// is ON, because an absent colour means the sRGB spelling of the factory
    /// tint — a different colour on a wide-gamut document — and the swatch
    /// and the render must not disagree about what the user is looking at.
    override func currentParams() -> [String: Any] {
        var params: [String: Any] = [:]
        for (index, band) in AdjustmentSchema.hueBands.enumerated() {
            let weight = weights[index]
            let factory = AdjustmentSchema.blackAndWhiteDefaults[index]
            if weight != factory || explicitKeys.contains(band) { params[band] = weight }
        }
        if tint || explicitKeys.contains("tint") { params["tint"] = tint }
        if tint || tintHexEdited || explicitKeys.contains("tint_color") {
            params["tint_color"] = tintHex
        }
        return params
    }

    override var footnote: String { "greys stay grey · one undo step" }

    // MARK: - Helpers

    /// The readout: whole percent, the step the sliders quantize to.
    private static func percent(_ value: Double) -> String {
        String(format: "%.0f %%", value.rounded())
    }

    private static func clamped(_ value: Double, _ low: Double, _ high: Double) -> Double {
        min(max(value, low), high)
    }
}
