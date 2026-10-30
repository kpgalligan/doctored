import AppKit

// The Artwork Colors sheet captures full editing state at creation.
//
// On a text or shape layer the ⌥⌫ / ⌘⌫ fill chords recolour the artwork instead of filling
// (`recolorArtworkForFill` in this file: `LayerDescription.recoloredForFill` through
// `rerenderingArtworkLayer`, one "Fill" step recorded as `edit_text_layer` / `edit_shape_layer`).

/// Exact numeric colors complement the color well and selected-text styling.
/// The sheet produces the same retained payload consumed by MCP and Actions.
final class ArtworkColorsSheetController: NSViewController {
    private weak var document: ImageDocument?
    private let source: RasterDocument
    private let state: EditingState?
    private let revision: UInt64
    private let index: Int
    private let original: LayerDescription
    private let anchor: CGPoint
    private let target = NSPopUpButton(frame: .zero, pullsDown: false)
    private let fields = (0..<4).map { _ in NSTextField(string: "0") }
    /// At 32 bits, the stops that multiply red, green and blue.
    private let intensity = NSTextField(string: "0")
    /// At 32 bits, the strings the five fields last showed and the numbers
    /// they came from: Apply with all five unchanged applies these numbers.
    private var shown: (strings: [String], rgba: [Double])?
    private let start = NSTextField(string: "1")
    private let length = NSTextField(string: "1")
    private let enabled = NSButton(checkboxWithTitle: "Paint enabled", target: nil, action: nil)
    private let notice = NSTextField(wrappingLabelWithString: "")

    init(document: ImageDocument) throws {
        // A CMYK layer's colours show as their sRGB appearance (#547, `displayedDescription`).
        guard let source = document.doc, let original = source.displayedDescription(document.activeLayerIndex),
              original.kind == .text || original.kind == .shape,
              let anchor = source.describedAnchor(document.activeLayerIndex) else {
            throw NativeArtworkEvaluation.failure("Select an editable text or shape layer.")
        }
        self.document = document; self.source = source; self.revision = document.revision
        self.state = document.captureEditingState()
        self.index = document.activeLayerIndex; self.original = original; self.anchor = anchor
        super.init(nibName: nil, bundle: nil)
    }
    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("ArtworkColorsSheetController does not support NSCoder") }

    override func loadView() {
        switch original {
        case .text(let payload): target.addItems(withTitles: ["Default Text Color", "Character Range"]); length.stringValue = String(payload.string.count)
        case .shape(let payload):
            target.addItems(withTitles: ["Fill", "Stroke"])
            if payload.kind == "line" { target.item(at: 0)?.isEnabled = false; target.selectItem(at: 1) }
        case .fill: break
        }
        target.target = self; target.action = #selector(changeTarget(_:))
        for (index, field) in (fields + [start, length]).enumerated() {
            DSField.style(field); field.widthAnchor.constraint(equalToConstant: 145).isActive = true
            field.identifier = NSUserInterfaceItemIdentifier("artworkColor.value\(index)")
        }
        DSField.style(intensity); intensity.widthAnchor.constraint(equalToConstant: 145).isActive = true
        intensity.identifier = NSUserInterfaceItemIdentifier("artworkColor.intensity")
        let grid = NSGridView(views: zip(["Red", "Green", "Blue", "Alpha"], fields).map { [fieldLabel($0.0), $0.1] }
            + [[fieldLabel("Intensity (stops)"), intensity]]
            + [[fieldLabel("First character (1-based)"), start], [fieldLabel("Character count"), length]])
        grid.rowSpacing = 8; grid.columnSpacing = 10
        grid.row(at: 4).isHidden = !source.isFloat
        notice.font = DS.sans(11); notice.preferredMaxLayoutWidth = 376
        notice.widthAnchor.constraint(equalToConstant: 376).isActive = true
        let stack = NSStackView(views: [target, grid, enabled, notice])
        stack.orientation = .vertical; stack.alignment = .leading; stack.spacing = 10
        // At 32 bits the fields are the document's own linear-light numbers.
        let components = source.isFloat
            ? "Linear-light sRGB components; Intensity multiplies red, green and blue by 2^stops (above 1 is above white)."
            : "Continuous sRGB components from 0 to 1."
        view = makeSheetView(title: "Artwork Colors", hint: components + " Character ranges preserve complete characters, including emoji.",
            content: stack, buttonRow: makeButtonRow(cancel: sheetCancelButton(target: self, action: #selector(cancel(_:))),
                                                   apply: sheetApplyButton(target: self, action: #selector(apply(_:)))))
        changeTarget(nil)
    }

    @objc private func changeTarget(_ sender: Any?) {
        let rgba: [Double]?
        switch original {
        case .text(let payload): rgba = ArtworkColor.rgba(payload.nsColor, in: payload.ink); enabled.isHidden = true
        case .shape(let payload):
            rgba = (target.indexOfSelectedItem == 0 ? payload.fillColor : payload.strokeColor)
                .flatMap { ArtworkColor.rgba($0, in: payload.ink) }
            enabled.isHidden = false
        case .fill: rgba = nil
        }
        let numbers = rgba ?? [0, 0, 0, 1]
        if source.isFloat {
            // An above-white colour shows as its base and the stops above it.
            let split = ExtendedRangeColor.split(numbers)
            for (field, value) in zip(fields, split.base) { field.stringValue = String(value) }
            intensity.stringValue = String(split.intensity)
            shown = ((fields + [intensity]).map(\.stringValue), numbers)
        } else {
            for (field, value) in zip(fields, numbers) { field.stringValue = String(value) }
        }
        enabled.state = rgba == nil ? .off : .on
        let range = original.kind == .text && target.indexOfSelectedItem == 1
        start.isEnabled = range; length.isEnabled = range
        notice.stringValue = "Values stay precise in the editable layer. Color glyphs keep their original precision; rendered precision follows the document depth."
    }

    @objc private func apply(_ sender: Any?) {
        do {
            guard let document, document.doc === source, document.revision == revision,
                  document.activeLayerIndex == index, document.isCurrentEditingState(state) else { throw NativeArtworkEvaluation.failure("The artwork changed while this sheet was open.") }
            let rgba: [Double]
            if source.isFloat {
                let strings = (fields + [intensity]).map(\.stringValue)
                if let shown, shown.strings == strings {
                    rgba = shown.rgba
                } else {
                    let base = fields.compactMap { Double($0.stringValue) }
                    rgba = Double(intensity.stringValue).flatMap {
                        ExtendedRangeColor.intensityRange.contains($0)
                            ? ExtendedRangeColor.combined(base, intensity: $0) : nil
                    } ?? []
                }
            } else {
                rgba = fields.compactMap { Double($0.stringValue) }
            }
            guard ArtworkColor.valid(rgba, in: source.authoringInk) else {
                throw NativeArtworkEvaluation.failure(source.isFloat
                    ? "Enter four finite linear-light sRGB components and an Intensity from -20 to 16 stops: red, green and blue 0 to 65504 after Intensity, alpha 0 to 1."
                    : "Enter four finite sRGB components from 0 to 1.")
            }
            var updated = original
            var record: [ActionStep]
            switch original {
            case var .text(payload):
                var style = payload.artwork
                    ?? ArtworkTextStyle(rgba: ArtworkColor.authored(payload.nsColor, in: payload.ink), in: payload.ink)
                if target.indexOfSelectedItem == 0 { style.rgba = rgba }
                else {
                    guard let first = Int(start.stringValue), let count = Int(length.stringValue), first >= 1, count > 0,
                          first - 1 <= payload.string.count, count <= payload.string.count - first + 1 else {
                        throw NativeArtworkEvaluation.failure("Choose a character range inside the text.")
                    }
                    let characters = Array(payload.string)
                    let location = String(characters.prefix(first - 1)).utf16.count
                    let size = String(characters[(first - 1)..<(first - 1 + count)]).utf16.count
                    style = style.replacingColor(NSRange(location: location, length: size), rgba: rgba)
                }
                guard style.isValid(for: payload.string) else { throw NativeArtworkEvaluation.failure("The text color range limit is 64.") }
                payload.artwork = style; payload.color = TextLayer.hex(numbers: style.rgba); updated = .text(payload)
                record = .editTextLayer(payload, layer: source.layerRef(index))
            case var .shape(payload):
                if payload.kind == "line", target.indexOfSelectedItem == 0 {
                    throw NativeArtworkEvaluation.failure("Lines have stroke color only.")
                }
                payload.setArtworkColor(enabled.state == .on ? rgba : nil,
                                        stroke: target.indexOfSelectedItem != 0)
                updated = .shape(payload)
                record = .editShapeLayer(payload, at: anchor, layer: source.layerRef(index))
            case .fill: return
            }
            // A CMYK document records and renders the colours in ink (#547): the layer's own
            // unchanged inks are kept exactly (`inkedArtwork`).
            if source.isCmyk {
                updated = try source.inkedArtwork(updated, replacing: index, swatches: [])
                switch updated {
                case let .text(payload): record = .editTextLayer(payload, layer: source.layerRef(index))
                case let .shape(payload):
                    record = .editShapeLayer(payload, at: anchor, layer: source.layerRef(index))
                case .fill: return
                }
            }
            let name = source.layerInfo(index)?.name ?? "Artwork"
            // The existing publication boundary captures target/selection and
            // rechecks it after the responsive worker before one Undo edit.
            var failure: Error?
            document.applyEdit("Edit Artwork Colors", record: record) { input in
                do { return try input.nativeArtwork(updated, anchor: anchor, layer: index, replacing: true, name: name) }
                catch { failure = error; return nil }
            }
            if let failure { throw failure }
            dismiss(self)
        } catch { notice.stringValue = error.localizedDescription }
    }
    @objc private func cancel(_ sender: Any?) { dismiss(self) }
}

extension ArtworkTextStyle {
    func replacingColor(_ range: NSRange, rgba: [Double]) -> Self {
        var result = self
        let end = NSMaxRange(range)
        result.runs = runs.flatMap { run -> [ArtworkColorRun] in
            let runEnd = run.location + run.length
            if runEnd <= range.location || run.location >= end { return [run] }
            var pieces: [ArtworkColorRun] = []
            if run.location < range.location { pieces.append(ArtworkColorRun(location: run.location, length: range.location - run.location, rgba: run.rgba)) }
            if runEnd > end { pieces.append(ArtworkColorRun(location: end, length: runEnd - end, rgba: run.rgba)) }
            return pieces
        }
        if rgba != self.rgba { result.runs.append(ArtworkColorRun(location: range.location, length: range.length, rgba: rgba)) }
        result.runs.sort { $0.location < $1.location }
        return result
    }
}

extension ShapeLayerPayload {
    /// The sheet's shape colour writer, shared with the ⌥⌫ / ⌘⌫ recolour:
    /// the fill (or, with `stroke`, the stroke) becomes `rgba` — nil
    /// turns it off — and the legacy hex fields follow the artwork style.
    mutating func setArtworkColor(_ rgba: [Double]?, stroke: Bool) {
        let ink = self.ink
        var style = artwork ?? ArtworkShapeStyle(
            fill: fillColor.flatMap { ArtworkColor.rgba($0, in: ink) },
            stroke: strokeColor.flatMap { ArtworkColor.rgba($0, in: ink) }, in: ink)
        if stroke { style.stroke = rgba } else { style.fill = rgba }
        artwork = style
        // The legacy hex holds the numbers in the payload's ink.
        let hex: ([Double]) -> String? = { ArtworkColor.valid($0, in: ink) ? TextLayer.hex(numbers: $0) : nil }
        fill = style.fill.flatMap(hex) ?? ""
        self.stroke = style.stroke.flatMap(hex) ?? ""
    }
}

extension LayerDescription {
    /// The ⌥⌫ / ⌘⌫ recolour: every character of a text layer takes
    /// `rgba` (the default colour set, colour runs cleared); a shape's fill,
    /// or a line's stroke, becomes `rgba`. `self` when that is already so;
    /// nil for a fill layer, whose colour is its own dialog's.
    func recoloredForFill(_ rgba: [Double]) -> LayerDescription? {
        switch self {
        case var .text(payload):
            let current = payload.artwork?.rgba ?? ArtworkColor.authored(payload.nsColor, in: payload.ink)
            if current == rgba, payload.artwork?.runs.isEmpty ?? true { return self }
            var style = payload.artwork ?? ArtworkTextStyle(rgba: current, in: payload.ink)
            style.rgba = rgba
            style.runs = []
            payload.artwork = style
            payload.color = TextLayer.hex(numbers: rgba)
            return .text(payload)
        case var .shape(payload):
            let stroke = payload.kind == "line"
            let current = (stroke ? payload.strokeColor : payload.fillColor)
                .flatMap { ArtworkColor.rgba($0, in: payload.ink) }
            if current == rgba { return self }
            payload.setArtworkColor(rgba, stroke: stroke)
            return .shape(payload)
        case .fill:
            return nil
        }
    }
}

extension EditorViewController {
    @objc func editArtworkColors(_ sender: Any?) {
        guard let document else { return }
        // A fill layer's colours are its own dialog's.
        if document.doc?.fillPayload(document.activeLayerIndex) != nil {
            editFillLayer(document.activeLayerIndex)
            return
        }
        do { presentAsSheet(try ArtworkColorsSheetController(document: document)) }
        catch { document.presentError(error) }
    }

    /// Whether ⌥⌫ / ⌘⌫ recolour the active layer's artwork instead of
    /// filling pixels: outside Quick Mask, on the layer's own pixels, of an
    /// editable text or shape layer.
    var fillChordRecolorsArtwork: Bool {
        guard let document, let doc = document.doc else { return false }
        switch doc.layerDescription(document.activeLayerIndex) {
        // A fill layer's chord fills its pixels, behind the rasterize prompt.
        case .fill: return false
        case .text, .shape, nil: break
        }
        return !document.quickMaskActive && document.planeEditTarget == .layer
            && doc.hasArtworkDescription(document.activeLayerIndex)
    }

    /// ⌥⌫ / ⌘⌫ (and ⇧ with them) on a text or shape layer: the
    /// artwork takes the swatch colour and stays editable, one "Fill" undo
    /// step recorded as `edit_text_layer` / `edit_shape_layer`. The
    /// selection is ignored. False when the chord should fill pixels instead
    /// (not artwork, or its description does not decode); true once handled,
    /// refused included.
    func recolorArtworkForFill(_ contents: FillContents) -> Bool {
        guard fillChordRecolorsArtwork, let document, let doc = document.doc else { return false }
        let index = document.activeLayerIndex
        // A CMYK layer recolours on its sRGB appearance and commits in ink, the swatch's stored
        // ink kept exactly (#547); a Lab layer's colours are authored sRGB (Lab phase 6a-i), so
        // the swatch is read in sRGB, not the Lab view's space.
        guard let description = doc.displayedDescription(index),
              let anchor = doc.describedAnchor(index) else { return false }
        guard !refuseLockedEdit(layer: index, kind: DR_EDIT_PIXELS) else { return true }
        guard let color = contents.color(
            foreground: paintColor, background: backgroundColor, custom: nil,
            in: doc.isCmyk || doc.isLab ? .sRGB : document.nsColorSpace),
            let rgba = ArtworkColor.rgba(color, in: doc.authoringInk),
            let recolored = description.recoloredForFill(rgba)
        else {
            NSSound.beep()
            return true
        }
        guard recolored != description else { return true }
        let updated: LayerDescription
        do {
            updated = try doc.inkedArtwork(recolored, replacing: index,
                swatches: CmykInkMatch.swatches(paint: paintColor, background: backgroundColor))
        } catch {
            document.presentError(error)
            return true
        }
        let record: [ActionStep]
        switch updated {
        case .text(let payload): record = .editTextLayer(payload, layer: doc.layerRef(index))
        case .shape(let payload): record = .editShapeLayer(payload, at: anchor, layer: doc.layerRef(index))
        case .fill: return true
        }
        let beforeEdit = document.doc
        // Whether the core THREW (its sentence already presented by
        // `applyEdit`), so the lock alert below never follows it.
        var threw = false
        document.applyEdit("Fill", record: record) { current in
            do {
                return try current.rerenderingArtworkLayer(index, updated, anchor: anchor)
            } catch {
                threw = true
                throw error
            }
        }
        if document.doc === beforeEdit, !threw {
            // A TRANSPARENCY lock refuses a re-layout that resizes the raster.
            refuseLockedRerender(layer: index)
        }
        return true
    }
}
