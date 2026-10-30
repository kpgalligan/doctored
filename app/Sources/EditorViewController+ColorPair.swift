import AppKit

// The rail's glyphs call `setPaintColor`/`setBackgroundColor` too. A rail swatch
// click is `presentColorPicker(_:)` (the Color Picker sheet, committing through the same two
// writers); System Colors… is `openSystemColorPanel(_:)` in the frozen controller. An
// options-bar swatch in a 32-bit float document opens the same sheet (`presentSwatchPicker`).

/// The canvas's colour-pair keys: bare X and D. The canvas only
/// reports the key; a key repeat is consumed and ignored there.
enum ColorPairKey: Equatable {
    /// X: swap the foreground and background colours.
    case swap
    /// D: reset them to the defaults.
    case reset
}

/// X / D and the rail's swap and default-colours glyphs. The pair is the
/// app-wide `ToolOptionsStore.sharedState`, so this is no document edit: no
/// undo step, no Action step and no MCP twin. Both write through
/// `setPaintColor` / `setBackgroundColor`, the pair's only writers, and
/// neither touches `NSColorPanel`, which an options-bar swatch may have
/// rebound.
extension EditorViewController {
    /// Canvas → controller. Refused (beep) while a mouse button is down, the
    /// screen keys' rule: a colour change mid-stroke would change the ink.
    func colorPairKeyPressed(_ key: ColorPairKey) {
        guard NSEvent.pressedMouseButtons == 0 else {
            NSSound.beep()
            return
        }
        switch key {
        case .swap:
            swapColors()
        case .reset:
            resetColors()
        }
    }

    /// Exchanges the foreground and background colours, alpha included, and
    /// their stored CMYK inks (#547) and Lab inks (#547 Lab phase 2e) with them.
    func swapColors() {
        let foreground = paintColor
        let background = backgroundColor
        let shared = ToolOptionsStore.shared.sharedState
        setPaintInk(shared.backgroundInk, color: background)
        setBackgroundInk(shared.foregroundInk, color: foreground)
        var swapped = ToolOptionsStore.shared.sharedState
        swapped.foregroundLab = shared.backgroundLab
        swapped.backgroundLab = shared.foregroundLab
        ToolOptionsStore.shared.sharedState = swapped
        optionsBar.refreshValues()
    }

    /// Black foreground over white background — or white over black while a
    /// layer mask is the edit target (the mask default). Planes,
    /// channels and Quick Mask keep black/white. The stored CMYK inks are
    /// cleared, so a CMYK document's black converts through its profile
    /// into the profile's rich black.
    func resetColors() {
        let maskDefault = paintTarget == .mask && !canvas.quickMaskActive
        setPaintColor(maskDefault ? .white : .black)
        setBackgroundColor(maskDefault ? .black : .white)
        optionsBar.refreshValues()
    }

    // MARK: - Color Picker (the rail swatches)

    /// The rail's foreground or background swatch click: the Color Picker
    /// sheet on that swatch's colour. OK writes through the pair's two
    /// writers; System Colors… hands the same swatch to `NSColorPanel`.
    /// At 32 bits the sheet opens on the colour's SDR base and its
    /// intensity in stops (`ExtendedRangeColor.split`), so an
    /// above-white colour can be seen and authored.
    func presentColorPicker(_ slot: ColorPairSlot) {
        let current = slot == .foreground ? paintColor : backgroundColor
        let split = document?.doc?.isFloat == true ? ExtendedRangeColor.split(current) : nil
        let shared = ToolOptionsStore.shared.sharedState
        // #547: the C/M/Y/K fields — a CMYK document's own ink, or the working
        // CMYK equivalent beside an RGB document's colour (not at 32 bits).
        let cmyk = split == nil ? PickerCmykContext(
            document: document?.doc?.precision, isCmyk: document?.doc?.isCmyk == true,
            ink: slot == .foreground ? shared.foregroundInk : shared.backgroundInk) : nil
        let sheet = ColorPickerSheetController(
            title: slot == .foreground
                ? "Color Picker (Foreground Color)" : "Color Picker (Background Color)",
            color: split?.base ?? current, intensity: split?.intensity, cmyk: cmyk,
            onCommit: { [weak self] in self?.commitPickedColor($0, ink: $1, to: slot) },
            onSystemColors: { [weak self] in self?.openSystemColorPanel(slot) })
        presentAsSheet(sheet)
    }

    /// The foreground's write path with a CMYK ink (#547): `color` is the ink's display,
    /// written through `setPaintColor`, and `ink` (nil clears it) is kept exactly in the
    /// shared state, so a CMYK document paints with the ink picked rather than a conversion.
    func setPaintInk(_ ink: CmykInk?, color: NSColor) {
        setPaintColor(color)
        var shared = ToolOptionsStore.shared.sharedState
        shared.foregroundInk = ink
        ToolOptionsStore.shared.sharedState = shared
    }

    /// The background's twin of `setPaintInk`.
    func setBackgroundInk(_ ink: CmykInk?, color: NSColor) {
        setBackgroundColor(color)
        var shared = ToolOptionsStore.shared.sharedState
        shared.backgroundInk = ink
        ToolOptionsStore.shared.sharedState = shared
    }

    /// The foreground's write path with a Lab ink (#547 Lab phase 2e), `setPaintInk`'s shape:
    /// `color` is the ink's display, written first through `setPaintColor` (which clears the
    /// stored inks), then `ink` (nil clears it) is kept exactly, so a Lab document paints the
    /// L*a*b* picked rather than its clipped sRGB swatch (`LabPaint.color`).
    func setPaintLab(_ ink: LabInk?, color: NSColor) {
        setPaintColor(color)
        var shared = ToolOptionsStore.shared.sharedState
        shared.foregroundLab = ink
        ToolOptionsStore.shared.sharedState = shared
    }

    /// The background's twin of `setPaintLab`.
    func setBackgroundLab(_ ink: LabInk?, color: NSColor) {
        setBackgroundColor(color)
        var shared = ToolOptionsStore.shared.sharedState
        shared.backgroundLab = ink
        ToolOptionsStore.shared.sharedState = shared
    }

    /// An options-bar swatch click (Text colour, Shape fill and stroke) in a
    /// 32-bit float document: the Color Picker sheet with its
    /// Intensity row on `color`'s SDR base and stops, as `presentColorPicker`
    /// opens a rail swatch, so the swatch can author an above-white colour.
    /// OK writes through the swatch's own `commit`; System Colors… is its
    /// own panel binding. A swatch showing no colour (alpha 0, a shape's
    /// "none") commits the picked colour opaque: the sheet has no alpha
    /// control. Answers false, presenting nothing, at 8 and 16 bits, where
    /// the swatch keeps the system panel.
    ///
    /// In a CMYK document (#547) the swatch opens the sheet with its
    /// C/M/Y/K fields on the document's ink (`PickerCmykContext`), so a type
    /// or shape colour can be picked as exact ink: OK hands `commitInk` the
    /// colour and the ink (nil when an RGB-side field was edited), else
    /// `commit` the colour.
    func presentSwatchPicker(
        title: String, color: NSColor, commit: @escaping (NSColor) -> Void,
        systemColors: @escaping () -> Void, commitInk: ((NSColor, CmykInk?) -> Void)? = nil,
        ink: CmykInk? = nil
    ) -> Bool {
        if let doc = document?.doc, doc.isCmyk {
            let empty = (color.usingColorSpace(.extendedSRGB)?.alphaComponent ?? 1) == 0
            let sheet = ColorPickerSheetController(
                title: title, color: color, intensity: nil,
                cmyk: PickerCmykContext(document: doc.precision, isCmyk: true, ink: ink),
                onCommit: { picked, ink in
                    let picked = empty ? picked.withAlphaComponent(1) : picked
                    if let commitInk { commitInk(picked, ink) } else { commit(picked) }
                },
                onSystemColors: systemColors)
            presentAsSheet(sheet)
            return true
        }
        guard document?.doc?.isFloat == true else { return false }
        let split = ExtendedRangeColor.split(color)
        let empty = (color.usingColorSpace(.extendedSRGB)?.alphaComponent ?? 1) == 0
        let sheet = ColorPickerSheetController(
            title: title, color: split.base, intensity: split.intensity, cmyk: nil,
            onCommit: { color, _ in commit(empty ? color.withAlphaComponent(1) : color) },
            onSystemColors: systemColors)
        presentAsSheet(sheet)
        return true
    }

    private func commitPickedColor(_ color: NSColor, ink: CmykInk?, to slot: ColorPairSlot) {
        switch slot {
        case .foreground:
            setPaintInk(ink, color: color)
        case .background:
            setBackgroundInk(ink, color: color)
        }
        optionsBar.refreshValues()
    }

    // MARK: - Fill with the pair (⌥⌫ / ⌘⌫ / ⇧⌥⌫ / ⇧⌘⌫)

    /// Edit ▸ Fill with Foreground / Background Color, the second pair
    /// keeping transparency: Normal at 100% through `applyFill`, whatever
    /// the Fill… sheet last saved — or, on a text or shape layer, the
    /// artwork recoloured and kept editable (`recolorArtworkForFill`).
    /// Validation is `validateFillItem`'s.
    @objc func fillWithForeground(_ sender: Any?) {
        fillWithPair(.foreground, preserve: false, note: "Edit ▸ Fill with Foreground Color")
    }

    @objc func fillWithBackground(_ sender: Any?) {
        fillWithPair(.background, preserve: false, note: "Edit ▸ Fill with Background Color")
    }

    @objc func fillWithForegroundPreserving(_ sender: Any?) {
        fillWithPair(.foreground, preserve: true,
                     note: "Edit ▸ Fill with Foreground Color, Preserve Transparency")
    }

    @objc func fillWithBackgroundPreserving(_ sender: Any?) {
        fillWithPair(.background, preserve: true,
                     note: "Edit ▸ Fill with Background Color, Preserve Transparency")
    }

    /// The four chords' one body: the artwork recolour first, then the
    /// pixel fill.
    private func fillWithPair(_ contents: FillContents, preserve: Bool, note: String) {
        if recolorArtworkForFill(contents) { return }
        applyFill(FillSettings(contents: contents, mode: DR_BLEND_NORMAL, opacity: 100,
                               preserveTransparency: preserve),
                  note: note)
    }
}
