import AppKit

// The Convert Precision sheet (no `NSAlert` on the path) has a float-only mode popup, a
// Flatten/Preserve Layers radio (Preserve Layers default, falling back to Flatten with
// the core's sentence when the plan blocks — `initialStructure`; Preserve enabled at every depth
// since 12H-2), the
// 8-bit-only dither box, the plan text read straight from `promoteStructurePlan()` on a byte source
// or `narrowStructurePlan(targetDepth:)` for the current mode on a typed one — exactly
// `PrecisionModeConversion.init`'s routing, with id-0 rows printed as `document: <note>` (never
// through the agent seam, so opening the sheet records nothing), Prepare Preview (`compute` +
// `previewDelta`, before/converted thumbnails, "max Δ / mean Δ codes of 65535") and Apply
// Conversion, enabled only while a prepared result matches the controls.
// An 8- or 16-bit source's Mode popup offers its one integer target and "To 32-bit float"
// (Image ▸ Mode ▸ 32 Bits/Channel… preselects it through `target`); at 32 Preserve Layers is forced
// on, Flatten and dither are disabled, and the plan text is the float plan.

/// Image ▸ Mode ▸ Convert Precision… — the one conversion dialog, replacing
/// the two alerts the flattened path used to open (`PrecisionModeConversion`).
///
/// Preserve Layers is the default: the structure-preserving
/// conversion in every direction (12H-1 8→16, 12H-2 16→8 and float→16/8).
/// When the core's plan blocks it (`initialStructure`), the sheet opens on
/// Flatten instead and says why. The plan text is read from the core directly
/// (`promoteStructurePlan` / `narrowStructurePlan`), never through the agent
/// seam, so opening the sheet records nothing. Prepare Preview runs the same
/// `compute` + `previewDelta` the MCP handler runs; Apply publishes that
/// prepared result as one undo step with the recorded step
/// `PrecisionModeConversion.step` builds. Any control change discards the
/// prepared result, so Apply is only ever the preview the user saw.
final class ConvertPrecisionSheetController: NSViewController {
    private weak var editor: EditorViewController?
    private let document: ImageDocument
    private let source: RasterDocument?
    private let before: CGImage?

    private let modePopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private var flattenRadio: NSButton!
    private var preserveRadio: NSButton!
    private var ditherCheck: NSButton!
    private let planView = NSTextView(frame: NSRect(x: 0, y: 0, width: 376, height: 150))
    private let beforeView = NSImageView(frame: .zero)
    private let convertedView = NSImageView(frame: .zero)
    private let deltaLabel = NSTextField(wrappingLabelWithString: "")
    private let statusLabel = NSTextField(wrappingLabelWithString: "")
    /// The mode the status line was last derived for, so every mode change
    /// re-derives it, and leaving 32-bit float (whose Preserve is forced)
    /// re-derives the radios too.
    private var shownMode: PrecisionMode?
    private var prepareButton: NSButton!
    private var applyButton: NSButton!

    private var capture: PrecisionModeConversion?
    private var prepared: PreparedDocumentEdit?
    /// The mode the sheet opens on, when the caller names one (Image ▸ Mode
    /// ▸ 32 Bits/Channel… passes `.rgba32`).
    private let target: PrecisionMode?

    init(editor: EditorViewController, document: ImageDocument, target: PrecisionMode? = nil) {
        self.editor = editor
        self.document = document
        self.target = target
        source = document.doc
        before = document.previewImage
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("ConvertPrecisionSheetController does not support NSCoder")
    }

    // MARK: - Choice

    private var isFloat: Bool { source?.isFloat == true }

    private var mode: PrecisionMode {
        if isFloat {
            return modePopup.indexOfSelectedItem == 1 ? .rgba8 : .rgba16
        }
        if modePopup.indexOfSelectedItem == 1 { return .rgba32 }
        return source?.isHighDepth == true ? .rgba8 : .rgba16
    }

    private var structure: ConversionStructure {
        preserveRadio.state == .on ? .preserve : .flatten
    }

    private var dither: PrecisionDither {
        mode == .rgba8 && ditherCheck.state == .on ? .ordered : .none
    }

    // MARK: - View

    override func loadView() {
        if isFloat {
            modePopup.addItems(withTitles: ["To 16-bit", "To 8-bit"])
        } else {
            modePopup.addItems(withTitles: [source?.isHighDepth == true ? "To 8-bit" : "To 16-bit",
                                            "To 32-bit float"])
            if target == .rgba32 { modePopup.selectItem(at: 1) }
        }
        modePopup.font = DS.sans(13)
        modePopup.target = self
        modePopup.action = #selector(choiceChanged(_:))

        flattenRadio = NSButton(radioButtonWithTitle: "Flatten", target: self,
                                action: #selector(structureChanged(_:)))
        preserveRadio = NSButton(radioButtonWithTitle: "Preserve Layers", target: self,
                                 action: #selector(structureChanged(_:)))
        let initial = source.map { Self.initialStructure(source: $0, mode: mode) }
        let preserve = initial?.structure == .preserve
        flattenRadio.state = preserve ? .off : .on
        preserveRadio.state = preserve ? .on : .off
        statusLabel.stringValue = initial?.refusal ?? ""
        shownMode = mode
        preserveRadio.isEnabled = source != nil
        ditherCheck = NSButton(checkboxWithTitle: "Ordered RGB dithering", target: self,
                               action: #selector(choiceChanged(_:)))

        let radios = NSStackView(views: [flattenRadio, preserveRadio])
        radios.orientation = .horizontal
        radios.spacing = 16
        var rows: [[NSView]] = []
        if source != nil { rows.append([fieldLabel("Mode:"), modePopup]) }
        rows.append([fieldLabel("Structure:"), radios])
        rows.append([fieldLabel("Dither:"), ditherCheck])
        let grid = NSGridView(views: rows)
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 84

        planView.isEditable = false
        planView.isSelectable = true
        planView.drawsBackground = false
        planView.font = DS.sans(12)
        planView.textColor = DS.textStrong
        let planScroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 376, height: 150))
        planScroll.documentView = planView
        planScroll.hasVerticalScroller = true
        planScroll.drawsBackground = false
        planScroll.widthAnchor.constraint(equalToConstant: 376).isActive = true
        planScroll.heightAnchor.constraint(equalToConstant: 150).isActive = true

        let images = NSStackView()
        images.orientation = .horizontal
        images.spacing = 12
        for (label, view) in [("Before", beforeView), ("Converted", convertedView)] {
            view.imageScaling = .scaleProportionallyUpOrDown
            view.widthAnchor.constraint(equalToConstant: 180).isActive = true
            view.heightAnchor.constraint(equalToConstant: 120).isActive = true
            let column = NSStackView(views: [fieldLabel(label), view])
            column.orientation = .vertical
            column.alignment = .leading
            images.addArrangedSubview(column)
        }
        if let before { beforeView.image = NSImage(cgImage: before, size: NSSize(width: before.width, height: before.height)) }

        for label in [deltaLabel, statusLabel] {
            label.font = DS.sans(12)
            label.textColor = DS.textMuted
            label.preferredMaxLayoutWidth = sheetWidth - sheetInset * 2
        }

        let content = NSStackView(views: [grid, planScroll, images, deltaLabel, statusLabel])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 12

        prepareButton = StickerButton(title: "Prepare Preview", style: .secondary, target: self,
                                      action: #selector(prepareClicked(_:)))
        applyButton = StickerButton(title: "Apply Conversion", style: .primary, target: self,
                                    action: #selector(applyClicked(_:)))
        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        view = makeSheetView(
            title: "Convert Precision",
            hint: "Flatten renders the canvas into one raster. Preserve Layers carries every "
                + "layer to the target precision. The previews are displayed at 8 bits.",
            content: content,
            buttonRow: makeButtonRow(cancel: cancelButton, apply: applyButton, leading: [prepareButton]))
        syncControls()
    }

    // MARK: - State

    private func invalidate() {
        capture = nil
        prepared = nil
        convertedView.image = nil
        deltaLabel.stringValue = ""
    }

    private func syncControls() {
        ditherCheck.isEnabled = mode == .rgba8
        if mode != .rgba8 { ditherCheck.state = .off }
        // The conversion into float keeps every layer: Preserve is forced.
        let float = mode == .rgba32
        if float {
            preserveRadio.state = .on
            flattenRadio.state = .off
        }
        flattenRadio.isEnabled = !float
        applyButton.isEnabled = prepared != nil
        planView.string = planText()
    }

    /// The core's preserve plan for `mode`, routed exactly as
    /// `PrecisionModeConversion.init` routes it: the float plan at 32, a
    /// byte source's promote plan, a typed source's narrowing plan.
    private static func preservePlan(source: RasterDocument, mode: PrecisionMode) throws -> [String: Any] {
        if mode == .rgba32 {
            return try source.precision?.promoteToFloatPlan() ?? source.promoteToFloatPlan()
        }
        return try source.precision.map { try $0.narrowStructurePlan(targetDepth: mode == .rgba8 ? 8 : 16) }
            ?? source.promoteStructurePlan()
    }

    /// The disclosure for the current choice. Preserve reads the core plan
    /// directly (no agent call, so nothing is recorded), routed exactly as
    /// `PrecisionModeConversion.init` routes it: a byte source's promote
    /// plan, a typed source's narrowing plan for the current `mode`, the
    /// float plan at 32.
    private func planText() -> String {
        guard structure == .preserve, let source else {
            return PrecisionModeConversion.losses + (mode == .rgba8
                ? "\n\n8-bit output has 256 values per channel. Ordered dithering spreads RGB rounding error; alpha is rounded without dithering."
                : "\n\nPromoting the legacy rendered pixels by 257 preserves their 256 original values; it does not recover missing detail.")
        }
        do {
            return Self.summary(try Self.preservePlan(source: source, mode: mode))
        } catch {
            return (error as? DoctoredCoreError)?.message ?? error.localizedDescription
        }
    }

    /// The structure the sheet opens on: Preserve Layers, unless the
    /// core's preserve plan — routed as `planText` routes it — blocks, when
    /// it is Flatten with `PrecisionModeConversion.refusal(plan:)`'s sentence
    /// (or the plan's own error) for the status line.
    static func initialStructure(source: RasterDocument, mode: PrecisionMode)
        -> (structure: ConversionStructure, refusal: String?)
    {
        do {
            let plan = try preservePlan(source: source, mode: mode)
            if let refusal = PrecisionModeConversion.refusal(plan: plan) {
                return (.flatten, refusal)
            }
            return (.preserve, nil)
        } catch {
            return (.flatten, (error as? DoctoredCoreError)?.message ?? error.localizedDescription)
        }
    }

    static func summary(_ plan: [String: Any]) -> String {
        func words(_ key: String) -> String {
            let items = (plan[key] as? [String]) ?? []
            return items.isEmpty ? "nothing"
                : items.map { $0.replacingOccurrences(of: "_", with: " ") }.joined(separator: ", ")
        }
        func rows(_ key: String, _ field: String) -> [String] {
            ((plan[key] as? [[String: Any]]) ?? []).map { row in
                // A document-level row (id 0) names no layer — the
                // `PrecisionModeConversion.refusal(plan:)` rule.
                (row["id"] as? Int) == 0 ? "  • document: \(row[field] ?? "")"
                    : "  • layer \(row["index"] ?? "") “\(row["name"] ?? "")”: \(row[field] ?? "")"
            }
        }
        let intoFloat = (plan["direction"] as? String)?.hasSuffix("_to_rgba32_float") == true
        var lines = [intoFloat ? PrecisionModeConversion.floatLosses : PrecisionModeConversion.preserveLosses, "",
                     "Preserved: " + words("preserved") + ".", "Not carried: " + words("discarded") + "."]
        let differences = rows("rendering_differences", "note")
        lines.append("Renders differently: " + (differences.isEmpty ? "nothing." : ""))
        lines += differences
        let blocked = rows("blocked", "reason")
        lines.append("Blocked: " + (blocked.isEmpty ? "nothing." : ""))
        lines += blocked
        return lines.joined(separator: "\n")
    }

    // MARK: - Actions

    @objc private func choiceChanged(_ sender: Any?) {
        // Every mode change re-derives the status line from the new mode's
        // own plan, so entering 32-bit float shows the float plan's refusal
        // and no refusal outlives its mode. Only leaving 32-bit float, whose
        // Preserve was forced, re-derives the radios too; entering it
        // forces Preserve (`syncControls`), and a 16↔8 change on a float
        // source keeps the user's radio.
        if shownMode != mode, let source {
            let initial = Self.initialStructure(source: source, mode: mode)
            if shownMode == .rgba32 {
                let preserve = initial.structure == .preserve
                flattenRadio.state = preserve ? .off : .on
                preserveRadio.state = preserve ? .on : .off
            }
            statusLabel.stringValue = initial.refusal ?? ""
        }
        shownMode = mode
        invalidate()
        syncControls()
    }

    @objc private func structureChanged(_ sender: Any?) {
        let preserve = (sender as? NSButton) === preserveRadio
        preserveRadio.state = preserve ? .on : .off
        flattenRadio.state = preserve ? .off : .on
        invalidate()
        syncControls()
    }

    @objc private func prepareClicked(_ sender: Any?) {
        invalidate()
        statusLabel.stringValue = ""
        // The plan text and Before thumbnail describe the handle captured at
        // open; an edit that landed behind the sheet would be converted under
        // them, so refuse rather than prepare the current handle.
        guard document.doc === source else {
            NSSound.beep()
            statusLabel.stringValue = "The document changed after this dialog opened. "
                + "Cancel and reopen Convert Precision to convert the current document."
            syncControls()
            return
        }
        do {
            let capture = try PrecisionModeConversion(document, mode: mode, dither: dither,
                                                      structure: structure)
            let prepared = try capture.compute()
            let delta = try capture.previewDelta(prepared)
            self.capture = capture
            self.prepared = prepared
            if let cg = prepared.preview {
                convertedView.image = NSImage(cgImage: cg, size: NSSize(width: cg.width, height: cg.height))
            }
            if let delta {
                deltaLabel.stringValue = "max Δ \(delta["max_delta"] ?? 0) / mean Δ "
                    + String(format: "%.2f", (delta["mean_delta"] as? Double) ?? 0) + " codes of 65535"
            } else {
                deltaLabel.stringValue = "The converted preview is displayed at 8 bits."
            }
        } catch {
            statusLabel.stringValue = (error as? DoctoredCoreError)?.message ?? error.localizedDescription
        }
        syncControls()
    }

    @objc private func applyClicked(_ sender: Any?) {
        guard let capture, let prepared else {
            NSSound.beep()
            return
        }
        do {
            try capture.publish(prepared, to: document, record: [PrecisionModeConversion.step(
                mode: capture.mode, structure: capture.structure, dither: capture.dither)])
            dismiss(self)
        } catch {
            invalidate()
            statusLabel.stringValue = (error as? DoctoredCoreError)?.message ?? error.localizedDescription
            syncControls()
        }
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}
