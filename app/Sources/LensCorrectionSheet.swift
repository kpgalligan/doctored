import AppKit

/// Filters ▸ Lens Correction…: distortion, chromatic aberration, vignette and a
/// perspective / angle / scale transform on the active layer or plane, through one core kernel
/// (`dr_image_filter_op` "lens_correction" at 8 bits, the selected-filter request of the same name
/// at 16), so the sheet, its recorded Action step and `apply_filter` agree, as
/// `MotionLensBlurSheets.swift`'s three do. Destructive and cancellable; it takes no frame, so a
/// selection confines the result by coverage while the kernel resamples the whole target about
/// its centre. The radial model is RAW development's (`lens_model.rs`).
///
/// Every value is scale-free — the map works in units of half the longer side — so the preview's
/// proxy takes the values unchanged. The Show Grid accessory lays an alignment grid over the
/// canvas while the sheet is up (`LensGridOverlay.swift`) and adds no values.
///
/// The Lens Profile popup (v1-lensfun, `LensProfileAccessory`) picks a bundled Lensfun profile:
/// Auto (the match for the document's EXIF camera and lens and the target's pixel size), None, or
/// one lens. The params carry the resolved `profile` object; the recorded step carries `profile`
/// "auto" (when Auto matched) or the lens id at every depth, which `apply_filter` resolves again on
/// replay. At 16 and 32 bits the frozen slider sheet runs its edit from that step, and
/// `SelectedAdjustment.edit` resolves the string for this document (`lensResolvedParameters`).
extension SliderSheetController {
    static func lensCorrection(document: ImageDocument, canvas: ImageCanvasView) -> SliderSheetController {
        let target = document.planeEditTarget.layerEditAgentName(in: document.doc)
        let keys = [
            "distortion", "chromatic_red_cyan", "chromatic_blue_yellow", "vignette_amount",
            "vignette_midpoint", "vertical_perspective", "horizontal_perspective", "angle", "scale",
        ]
        let lens = LensProfileAccessory(document: document)
        // The sliders' values, then the popup's index (the grid accessory adds none).
        let profileIndex: ([Double]) -> Int = { $0.count > keys.count ? Int($0[keys.count]) : -1 }
        let sliders: ([Double]) -> [String: Any] = { values in
            Dictionary(uniqueKeysWithValues: zip(keys, values.map { Int($0.rounded()) }))
        }
        let params: ([Double]) -> [String: Any] = { values in
            var result = sliders(values)
            if let profile = lens.profile(at: profileIndex(values)) { result["profile"] = profile }
            return result
        }
        let recorded: ([Double]) -> [String: Any] = { values in
            var result = sliders(values)
            if let argument = lens.recordedArgument(at: profileIndex(values)) { result["profile"] = argument }
            return result
        }
        let amount: (Double) -> String = { String(format: "%+.0f", $0.rounded()) }
        let slider: (String, Double, Double, Double, @escaping (Double) -> String) -> FilterSliderDescriptor = {
            FilterSliderDescriptor(label: $0, min: $1, max: $2, initial: $3, isInteger: true, format: $4)
        }
        return SliderSheetController(
            document: document, canvas: canvas,
            title: "Lens Correction", actionName: "Lens Correction",
            sliders: [
                // Positive corrects barrel distortion (pulls the edges out); negative pincushion.
                slider("Distortion:", -100, 100, 0, amount),
                // Chromatic aberration: the red and blue channels' size against green's.
                slider("Red/Cyan:", -100, 100, 0, amount),
                slider("Blue/Yellow:", -100, 100, 0, amount),
                // Vignette: positive brightens the corners, the midpoint how far in it reaches.
                slider("Vignette:", -100, 100, 0, amount),
                slider("Midpoint:", 0, 100, 50, { String(format: "%.0f", $0.rounded()) }),
                // Transform: negative Vertical widens the top, negative Horizontal the left.
                slider("Vertical:", -100, 100, 0, amount),
                slider("Horizontal:", -100, 100, 0, amount),
                // Positive turns the picture counterclockwise.
                slider("Angle:", -180, 180, 0, { String(format: "%.0f°", $0.rounded()) }),
                slider("Scale:", 50, 150, 100, { String(format: "%.0f%%", $0.rounded()) }),
            ],
            accessories: [lens, LensGridAccessory(canvas: canvas)],
            step: { values in .filter("lens_correction", recorded(values), target: target) },
            proxy: { values, _ in values },
            request: { ByteFilterRequest(op: "lens_correction", params: params($0)) },
            compute: { image, values, control in
                do { return try image.filterOp("lens_correction", params: params(values), control: control) ?? image }
                catch { return nil }
            })
    }
}

extension EditorViewController {
    @objc func showLensCorrection(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document else { NSSound.beep(); return }
        presentAsSheet(SliderSheetController.lensCorrection(document: document, canvas: canvas))
    }
}

/// Lens Correction's Lens Profile row (v1-lensfun): a popup of Auto (the bundled Lensfun
/// database's match for the document's EXIF camera and lens, refused for a target whose aspect is
/// not the camera frame's), None, the candidate lenses and, after a separator, every other lens on
/// the camera's mount family; and under it the match's one-sentence summary. Its value is the
/// selected item's index; `profile(at:)` resolves the item to the `profile` object.
final class LensProfileAccessory: NSObject, FilterSheetAccessory {
    let view: NSView
    var onChange: (() -> Void)?
    private let popup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let status = NSTextField(wrappingLabelWithString: "")
    private let identity: LensIdentity
    private let dimensions: (width: Int, height: Int)?
    /// The popup's items in order; nil for the separator.
    private var choices: [LensProfileChoice?] = []
    private var matches: [Int: LensProfileMatch] = [:]

    init(document: ImageDocument) {
        let doc = document.doc
        identity = doc?.lensIdentity ?? LensIdentity()
        let layer = doc?.layerInfo(document.activeLayerIndex)
        if let layer, layer.width > 0, layer.height > 0 {
            dimensions = (layer.width, layer.height)
        } else if let doc {
            dimensions = (doc.width, doc.height)
        } else {
            dimensions = nil
        }
        let auto = try? LensProfileMatch.resolve(identity, .auto, all: true, dimensions: dimensions)
        let autoTitle = auto?.profile != nil ? auto?.selected.map { "Auto \u{2014} \($0.model)" } : nil
        popup.addItem(withTitle: autoTitle ?? "Auto \u{2014} no match")
        choices.append(.auto)
        popup.addItem(withTitle: "None")
        choices.append(LensProfileChoice.none)
        let candidates = auto?.candidates ?? []
        for lens in candidates {
            popup.addItem(withTitle: lens.model)
            popup.lastItem?.representedObject = lens.id
            choices.append(.lens(lens.id))
        }
        let others = (auto?.mountLenses ?? []).filter { lens in !candidates.contains { $0.id == lens.id } }
        if !others.isEmpty {
            popup.menu?.addItem(.separator())
            choices.append(nil)
            for lens in others {
                popup.addItem(withTitle: lens.id)
                choices.append(.lens(lens.id))
            }
        }
        if let auto { matches[0] = auto }
        popup.selectItem(at: auto?.profile != nil ? 0 : 1)
        popup.setAccessibilityIdentifier("lens-correction.profile")
        status.font = DS.sans(11)
        status.textColor = DS.textMuted
        status.preferredMaxLayoutWidth = 300
        status.setAccessibilityIdentifier("lens-correction.profile-status")
        let grid = NSGridView(views: [[fieldLabel("Lens Profile:"), popup], [NSView(), status]])
        grid.rowSpacing = 6
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106
        view = grid
        super.init()
        popup.target = self
        popup.action = #selector(changed(_:))
        refreshStatus()
    }

    var values: [Double] { [Double(max(0, popup.indexOfSelectedItem))] }

    /// The `profile` object item `index` applies, nil for None, an Auto with no match or a lens
    /// that resolves to no profile.
    func profile(at index: Int) -> [String: Any]? { match(at: index)?.profile }

    /// What a recorded step carries for item `index`: "auto" when Auto matched, the lens id, or
    /// nil (nothing applied).
    func recordedArgument(at index: Int) -> String? {
        guard let match = match(at: index), match.profile != nil,
              let choice = choices.indices.contains(index) ? choices[index] : nil else { return nil }
        return choice.argument
    }

    private func match(at index: Int) -> LensProfileMatch? {
        guard choices.indices.contains(index), let choice = choices[index], choice != .none else { return nil }
        if let cached = matches[index] { return cached }
        let resolved = try? LensProfileMatch.resolve(identity, choice, dimensions: dimensions)
        matches[index] = resolved
        return resolved
    }

    private func refreshStatus() {
        let index = popup.indexOfSelectedItem
        if choices.indices.contains(index), choices[index] == LensProfileChoice.none {
            status.stringValue = "No lens profile."
        } else {
            status.stringValue = match(at: index)?.summary ?? "The lens database could not be read."
        }
    }

    @objc private func changed(_ sender: Any?) {
        refreshStatus()
        onChange?()
    }
}
