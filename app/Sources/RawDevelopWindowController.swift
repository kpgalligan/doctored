import AppKit

/// Shared initial-development and redevelopment controls. A retained snapshot
/// and captured decoder recipe supply every preview and the final operation.
final class RawDevelopWindowController: NSObject {
    /// Puts the dialog up for `probe` and returns the settings to develop
    /// with, or nil when the user cancelled.
    static func run(_ probe: RawProbe, descriptor: RawSourceDescriptor? = nil,
                    snapshot: RawSourceSnapshot? = nil, redevelop: Bool = false,
                    space: CGColorSpace? = nil, beforeSource: RasterPrecisionImage? = nil) -> RawDevelopSettings? {
        RawDevelopWindowController(probe: probe, descriptor: descriptor, snapshot: snapshot,
                                   redevelop: redevelop, space: space, beforeSource: beforeSource).run()
    }

    static func runImport(_ probe: RawProbe, descriptor: RawSourceDescriptor,
                          snapshot: RawSourceSnapshot) -> (settings: RawDevelopSettings, highDepth: Bool)? {
        let controller = RawDevelopWindowController(probe: probe, descriptor: descriptor,
            snapshot: snapshot, redevelop: false, space: nil)
        guard let settings = controller.run() else { return nil }
        return (settings, controller.openHighDepth)
    }

    /// Layer ▸ Place Embedded…/Place Linked… and Replace Contents of a camera
    /// file: the layer takes the document's depth, so no depth row;
    /// `verb` titles the primary button ("Place" or "Replace").
    /// `documentIsEightBit` is the receiving document's depth, which decides
    /// the output note (`refreshOutputNote`). `documentIsFloat` adds the
    /// "Development:" row and opens it on the 32-bit float (linear)
    /// development (`RawSourcePlacement.defaultEncoding`); the returned
    /// patch names an encoding only when the user changed that row.
    static func runPlace(_ probe: RawProbe, descriptor: RawSourceDescriptor,
                         snapshot: RawSourceSnapshot, verb: String,
                         documentIsEightBit: Bool, documentIsFloat: Bool) -> RawDevelopSettings? {
        RawDevelopWindowController(probe: probe, descriptor: descriptor, snapshot: snapshot,
                                   redevelop: false, space: nil, placing: verb,
                                   documentIsEightBit: documentIsEightBit,
                                   documentIsFloat: documentIsFloat).run()
    }

    /// One continuous control, described once so the thirteen rows are built,
    /// reset and enabled by the same three lines instead of thirteen copies.
    struct Knob {
        let label: String
        let range: ClosedRange<Double>
        let format: (Double) -> String
        /// False when THIS file's decoder does not support the control: the
        /// row is disabled and still shows the file's own value, and the
        /// card's footnote names it.
        let supported: Bool
        /// True for temperature and tint, which are additionally gated on
        /// the white-balance pop-up being on Custom.
        let whiteBalance: Bool
        let read: (RawDevelopValues) -> Double
        let write: (inout RawDevelopValues, Double) -> Void
        /// True for Shadows, which `CIRAWFilter` documents as having no
        /// effect while `boostAmount` (Tone curve) is 0. The row is disabled
        /// there rather than left live: a slider that can be dragged across
        /// its whole range while the preview never moves is the one thing a
        /// live preview must not do.
        var inertWhileLinear = false
        /// True for Tone curve, which a development entering the 32-bit
        /// float (linear) encoding starts at 0, scene-linear; the row
        /// stays live there and any value is kept.
        var startsAtZeroAtFloat = false
    }

    let descriptor: RawSourceDescriptor?
    let snapshot: RawSourceSnapshot?
    let redevelop: Bool
    /// The primary button's verb when placing a camera file; nil otherwise.
    let placing: String?
    /// The depth of the document the development lands in, when the caller
    /// knows it (a place, a redevelop); nil for an open, whose depth is the
    /// dialog's own Document depth row.
    let documentIsEightBit: Bool?
    /// True when the development lands in a 32-bit float document (a place or
    /// a redevelop there): the card then has the "Development:" row.
    let documentIsFloat: Bool
    /// The encoding the card opened on, and the one it shows now. An open
    /// starts at the 16-bit master and takes the float development from its
    /// Document depth row; a place into a float document opens on the float
    /// development; a redevelop opens on the layer's own.
    let initialEncoding: RawOutputEncoding
    var encoding: RawOutputEncoding
    /// The Tone curve value the card held before entering the float encoding
    /// set it to 0, restored when the encoding goes back to the 16-bit master.
    /// Non-nil is "the pin is in force": `developSettings` then names the
    /// curve explicitly, since the recipe would otherwise start it at 0.
    var toneCurveBeforeFloat: Double?
    var encodingRow: CardControlRow?
    var floatNoteRow: NSView?
    private var openHighDepth = true
    private let previewSpace: CGColorSpace
    let initialValues: RawDevelopValues
    let probe: RawProbe
    var values: RawDevelopValues
    let knobs: [Knob]
    var knobRows: [CardControlRow] = []
    var lensRow: CardControlRow?
    var highlightRow: CardControlRow?
    var whiteBalanceRow: CardControlRow?
    let beforeSource: RasterPrecisionImage?
    var outputSpace: RawOutputSpace
    var outputRow: CardControlRow?
    /// The note under "Develop output:" and its label (`refreshOutputNote`).
    var outputNoteRow: NSView?
    var outputNoteLabel: NSTextField?
    var workflowPanel: RawDevelopPreviewPanel?
    var workflowSession: RawDevelopPreviewSession?
    var applyButton: NSButton?
    var previewIsReady = false
    var presetPopup: NSPopUpButton?
    var presets: [RawPreset] = []
    var enhancementControls: RawEnhancementControls?
    var localSession: RawLocalDevelopSession?
    var localControls: RawLocalControls?
    var localGesture: RawLocalGestureView?
    var localResult: RawLocalDevelopResult?
    var localValidate: (() throws -> Void)?
    var localSection = false
    var localTool = RawLocalGestureView.Tool.pan
    var globalControlsView: NSView?
    var localSidebarView: NSStackView?
    weak var controlsScroll: NSScrollView?
    private let previewView = NSImageView()
    private let renderer = PreviewRenderer()
    var modal: ModalCardWindowController?

    /// The preview's size. 516 pt is the 560 pt card less its 2 × 22
    /// insets, and 344 would be exactly 3:2 — the aspect ratio of nearly
    /// every camera RAW — so at 340 a landscape frame all but fills the box
    /// and a portrait one letterboxes symmetrically. Fixed rather than
    /// fitted, because the window must not change size when the picture
    /// behind it does.
    private static let previewSize = NSSize(width: 516, height: 340)

    /// How short the preview may get on a small screen (see
    /// `fitToScreen(_:)`). Below this it is no longer a preview.
    private static let minimumPreviewHeight: CGFloat = 160

    /// Room left for the title bar plus a margin above and below the
    /// window, since the card is measured without its window.
    private static let windowChrome: CGFloat = 60

    private var previewHeightConstraint: NSLayoutConstraint?

    init(probe: RawProbe, descriptor: RawSourceDescriptor?, snapshot: RawSourceSnapshot?,
                 redevelop: Bool, space: CGColorSpace?, beforeSource: RasterPrecisionImage? = nil,
                 placing: String? = nil, documentIsEightBit: Bool? = nil,
                 documentIsFloat: Bool = false) {
        self.probe = probe
        self.descriptor = descriptor
        self.snapshot = snapshot
        self.redevelop = redevelop
        self.placing = placing
        self.documentIsEightBit = documentIsEightBit
        self.documentIsFloat = documentIsFloat
        self.beforeSource = beforeSource
        outputSpace = descriptor?.recipe.outputColorSpace ?? .sRGB
        previewSpace = space ?? WorkingSpace.sRGB.cgSpace
        initialValues = descriptor?.recipe.values ?? probe.capabilities.developDefaults
        let capabilities = probe.capabilities
        let opening: RawOutputEncoding = placing != nil && documentIsFloat
            ? .linearFloat : descriptor?.recipe.outputEncoding ?? .normalized16
        initialEncoding = opening
        encoding = opening
        var openingValues = initialValues
        // Only a card that enters the float encoding from a 16-bit recipe (a
        // place into a float document) starts the curve at 0; a float
        // layer's redevelop opens on its saved curve.
        if opening == .linearFloat, descriptor?.recipe.outputEncoding != .linearFloat {
            toneCurveBeforeFloat = openingValues.toneCurve
            openingValues.toneCurve = 0
        }
        values = openingValues
        // The order here IS the dialog's top-to-bottom order and the order
        // RawCapabilities.unsupportedLabels lists the footnote in.
        knobs = [
            Knob(
                label: "Exposure:", range: RawDevelopRange.exposure,
                format: { String(format: "%+.2f EV", $0) }, supported: true,
                whiteBalance: false, read: { $0.exposure }, write: { $0.exposure = $1 }),
            Knob(
                label: "Temperature:", range: RawDevelopRange.temperature,
                format: { String(format: "%.0f K", $0) }, supported: true,
                whiteBalance: true, read: { $0.temperature }, write: { $0.temperature = $1 }),
            Knob(
                label: "Tint:", range: RawDevelopRange.tint,
                format: { String(format: "%.0f", $0) }, supported: true,
                whiteBalance: true, read: { $0.tint }, write: { $0.tint = $1 }),
            Knob(
                label: "Tone curve:", range: RawDevelopRange.toneCurve,
                format: { String(format: "%.2f", $0) }, supported: true,
                whiteBalance: false, read: { $0.toneCurve }, write: { $0.toneCurve = $1 },
                startsAtZeroAtFloat: true),
            Knob(
                label: "Shadows:", range: RawDevelopRange.shadows,
                format: { String(format: "%.2f", $0) }, supported: true,
                whiteBalance: false, read: { $0.shadows }, write: { $0.shadows = $1 },
                inertWhileLinear: true),
            Knob(
                label: "Local contrast:", range: RawDevelopRange.contrast,
                format: { String(format: "%.2f", $0) }, supported: capabilities.contrast,
                whiteBalance: false, read: { $0.contrast }, write: { $0.contrast = $1 }),
            Knob(
                label: "Sharpness:", range: RawDevelopRange.sharpness,
                format: { String(format: "%.2f", $0) }, supported: capabilities.sharpness,
                whiteBalance: false, read: { $0.sharpness }, write: { $0.sharpness = $1 }),
            Knob(
                label: "Detail:", range: RawDevelopRange.detail,
                format: { String(format: "%.2f", $0) }, supported: capabilities.detail,
                whiteBalance: false, read: { $0.detail }, write: { $0.detail = $1 }),
            Knob(
                label: "Luminance noise:", range: RawDevelopRange.luminanceNoise,
                format: { String(format: "%.2f", $0) }, supported: capabilities.luminanceNoise,
                whiteBalance: false, read: { $0.luminanceNoise },
                write: { $0.luminanceNoise = $1 }),
            Knob(
                label: "Colour noise:", range: RawDevelopRange.colorNoise,
                format: { String(format: "%.2f", $0) }, supported: capabilities.colorNoise,
                whiteBalance: false, read: { $0.colorNoise }, write: { $0.colorNoise = $1 }),
        ]
        super.init()
        renderer.onRender = { [weak self] image in
            guard let self = self, let image = image else { return }
            // .zero asks NSImage to take the CGImage's pixel dimensions as
            // its size, which is what the proportional scaling below wants.
            self.previewView.image = NSImage(cgImage: image, size: .zero)
        }
    }

    func run() -> RawDevelopSettings? {
        let modal = ModalCardWindowController(title: "Develop RAW", card: makeCard())
        self.modal = modal
        configureWorkflowSession()
        requestPreview()
        let response = modal.runModal()
        if response == .OK { localResult = localSession?.result }
        localSession?.cancel()
        localSession = nil
        localGesture?.clear()
        localControls?.clear()
        localValidate = nil
        // The preview session owns in-flight immutable inputs until its worker
        // finishes, even when Cancel closes the window during camera decoding.
        workflowSession?.cancel()
        workflowSession = nil
        workflowPanel?.clear()
        renderer.cancel()
        self.modal = nil
        guard response == .OK else { return nil }
        return pendingSettings()
    }

    // MARK: - The card

    private func makeCard() -> NSView {
        previewView.imageScaling = .scaleProportionallyUpOrDown
        previewView.imageAlignment = .alignCenter
        previewView.wantsLayer = true
        previewView.layer?.backgroundColor = DS.canvasVoid.cgColor
        previewView.widthAnchor.constraint(
            equalToConstant: Self.previewSize.width).isActive = true
        let previewHeight = previewView.heightAnchor.constraint(
            equalToConstant: Self.previewSize.height)
        previewHeight.isActive = true
        previewHeightConstraint = previewHeight

        var rows: [NSView] = []
        if descriptor != nil, snapshot != nil { rows.append(presetControls()) }
        if !redevelop && placing == nil {
            // Indices 0 and 1 are fixed (probes select them); the float
            // development is offered where the retained decode seam is.
            var titles = ["16-bit", "8-bit (retain 16-bit source)"]
            if descriptor != nil, snapshot != nil { titles.append(RawOutputEncoding.linearFloat.displayName) }
            let depth = cardPopupRow(label: "Document depth:",
                titles: titles, selected: 0) { [weak self] index in
                self?.documentDepthChanged(index)
            }
            depth.setAccessibilityIdentifier("raw-develop-document-depth")
            rows.append(depth)
        } else if documentIsFloat, descriptor != nil {
            // Layer ▸ Redevelop RAW… and a place in a 32-bit float document:
            // which master the layer keeps.
            let development = cardPopupRow(label: "Development:",
                titles: RawOutputEncoding.allCases.map(\.displayName),
                selected: RawOutputEncoding.allCases.firstIndex(of: encoding) ?? 0) { [weak self] index in
                    guard RawOutputEncoding.allCases.indices.contains(index) else { return }
                    self?.setEncoding(RawOutputEncoding.allCases[index])
                }
            development.setAccessibilityIdentifier("raw-develop-encoding")
            encodingRow = development
            rows.append(development)
        }
        if descriptor != nil { rows.append(floatNoteLine()) }
        let output = cardPopupRow(label: "Develop output:",
            titles: RawOutputSpace.allCases.map(\.displayName),
            selected: RawOutputSpace.allCases.firstIndex(of: outputSpace) ?? 0) { [weak self] index in
                self?.outputSpaceChanged(index)
            }
        output.setAccessibilityIdentifier("raw-develop-output-space")
        outputRow = output
        if descriptor != nil {
            rows.append(output)
            // Its first state is set here, before `workflowContent` measures
            // the controls for the scroll document.
            rows.append(outputNoteLine())
        }
        if let descriptor {
            let enhancements = RawEnhancementControls(recipe: descriptor.recipe)
            enhancements.presenceAvailable = encoding != .linearFloat
            enhancements.onChange = { [weak self] in self?.requestPreview() }
            enhancementControls = enhancements
            rows.append(enhancements.view)
        }
        let whiteBalance = cardPopupRow(
            label: "White balance:", titles: ["As Shot", "Custom"], selected: values.customWhiteBalance ? 1 : 0
        ) { [weak self] index in
            self?.setCustomWhiteBalance(index == 1)
        }
        whiteBalanceRow = whiteBalance

        var placedWhiteBalance = false
        for (index, knob) in knobs.enumerated() {
            let row = cardSliderRow(
                label: knob.label, min: knob.range.lowerBound, max: knob.range.upperBound,
                value: knob.read(values), format: knob.format
            ) { [weak self] value in
                self?.knobChanged(index, value)
            }
            knobRows.append(row)
            // The pop-up sits directly above the pair it governs.
            if knob.whiteBalance && !placedWhiteBalance {
                placedWhiteBalance = true
                rows.append(whiteBalance)
            }
            rows.append(row)
        }
        if let presenceView = enhancementControls?.presenceView { rows.append(presenceView) }
        if let grainView = enhancementControls?.grainView { rows.append(grainView) }

        // The decoder's own lens profile (distortion, chromatic aberration,
        // vignetting), on by default when the file carries one.
        let lens = cardCheckboxRow(
            label: "Profile corrections:", title: "Enable Profile Corrections",
            on: values.lensCorrection
        ) { [weak self] on in
            self?.values.lensCorrection = on
            self?.requestPreview()
        }
        lensRow = lens
        rows.append(lens)
        rows.append(lensProfileLine())

        // macOS 26+ only; gated because the deployment floor is 15. The row
        // is HIDDEN below 26 rather than disabled — a control that can never
        // come alive on this OS is noise, and the footnote is for controls
        // the FILE cannot support.
        if #available(macOS 26.0, *) {
            let highlights = cardCheckboxRow(
                label: "Highlights:", title: "Recover clipped highlights",
                on: values.highlightRecovery
            ) { [weak self] on in
                self?.values.highlightRecovery = on
                self?.requestPreview()
            }
            highlightRow = highlights
            rows.append(highlights)
        }

        let stack = NSStackView(views: rows)
        stack.orientation = .vertical
        stack.alignment = .leading
        // The sheet grids' own row spacing, and `makeSheetView`'s own gap
        // between a card's parts — so a modal card reads as a sheet.
        stack.spacing = 10


        let reset = StickerButton(
            title: "Reset", style: .secondary, target: self, action: #selector(resetClicked(_:)))
        let cancel = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let open = StickerButton(
            title: redevelop ? "Apply" : placing ?? "Open", style: .primary, target: self, action: #selector(openClicked(_:)))
        applyButton = open
        open.setAccessibilityIdentifier("raw-develop-apply")
        open.isEnabled = descriptor == nil || snapshot == nil
        var leading: [NSView] = [reset]
        let unsupported = probe.capabilities.unsupportedLabels
        if !unsupported.isEmpty {
            leading.append(
                sheetFootnote("This file does not support: "
                    + unsupported.joined(separator: ", ") + "."))
        }
        applyEnablement()
        let card = makeSheetView(
            title: "Develop \(probe.url.lastPathComponent)", hint: subtitle,
            content: workflowContent(controls: stack, legacyPreview: previewView),
            buttonRow: makeButtonRow(cancel: cancel, apply: open, leading: leading),
            // Wider than the house 420 pt card: a preview you cannot judge
            // an exposure or a white balance from is not worth showing, and
            // the two-pane Layer Style sheet already set the precedent for a
            // card that asks for more room when its content earns it.
            width: workflowCardWidth)
        if workflowPanel == nil { fitToScreen(card) }
        return card
    }

    /// The Lensfun profile Enable Profile Corrections switches (v1-lensfun): the recipe's, matched
    /// at its first development when the decoder carries no lens profile.
    var lensfunProfile: RawLensfunProfile? {
        probe.capabilities.lensCorrection ? nil : descriptor?.recipe.lensProfile
    }

    /// The line under Enable Profile Corrections: which lens the file names,
    /// and whether its profile is what the switch applies.
    private func lensProfileLine() -> NSView {
        let lens = probe.lensModel ?? "not recorded"
        let tag = probe.lensfun?.tag ?? "v0.3.4"
        let text = probe.capabilities.lensCorrection
            ? "Lens: \(lens) \u{2014} corrected by the profile embedded in this camera file."
            : lensfunProfile.map { "Lens: \(lens) \u{2014} corrected by the Lensfun profile \($0.lens) "
                + "(Lensfun database \(tag)); this file carries no lens profile of its own." }
            ?? "Lens: \(lens) \u{2014} this file carries no lens profile and no Lensfun profile matches it; the manual lens controls below still apply."
        let label = NSTextField(wrappingLabelWithString: text)
        label.font = DS.sans(11)
        label.textColor = DS.textMuted
        label.preferredMaxLayoutWidth = 360
        label.setAccessibilityIdentifier("raw-develop-lens-profile")
        // Under the checkbox, past the rows' 106 pt label column and its gap.
        let spacer = NSView()
        spacer.widthAnchor.constraint(equalToConstant: 106).isActive = true
        let row = NSStackView(views: [spacer, label])
        row.orientation = .horizontal
        row.spacing = 12
        return row
    }

    /// What the note under "Develop output:" says now, or nil for no note:
    /// `RawOutputSpace.depthNote` at the receiving document's depth — the
    /// caller's when it gave one, else the dialog's own Document depth row.
    private var outputNote: String? {
        outputSpace.depthNote(eightBit: documentIsEightBit ?? !openHighDepth)
    }

    /// The line under "Develop output:" (the `lensProfileLine` shape), hidden
    /// unless the chosen space has something to say about the document depth.
    private func outputNoteLine() -> NSView {
        let note = outputNote
        let label = NSTextField(wrappingLabelWithString: note ?? "")
        label.font = DS.sans(11)
        label.textColor = DS.textMuted
        label.preferredMaxLayoutWidth = 360
        label.setAccessibilityIdentifier("raw-develop-output-note")
        let spacer = NSView()
        spacer.widthAnchor.constraint(equalToConstant: 106).isActive = true
        let row = NSStackView(views: [spacer, label])
        row.orientation = .horizontal
        row.spacing = 12
        row.isHidden = note == nil
        outputNoteLabel = label
        outputNoteRow = row
        return row
    }

    /// Sets the output note's text and visibility from the output space and
    /// the document depth. The controls' scroll document is frame-sized once,
    /// in `workflowContent`, so a change of the row's `isHidden` resizes it
    /// through `refitControlsDocument()`.
    func refreshOutputNote() {
        guard let row = outputNoteRow else { return }
        let note = outputNote
        outputNoteLabel?.stringValue = note ?? ""
        let hidden = note == nil
        guard row.isHidden != hidden else { return }
        row.isHidden = hidden
        refitControlsDocument()
    }

    private func outputSpaceChanged(_ index: Int) {
        guard RawOutputSpace.allCases.indices.contains(index) else { return }
        outputSpace = RawOutputSpace.allCases[index]
        refreshOutputNote()
        requestPreview()
    }

    private func documentDepthChanged(_ index: Int) {
        openHighDepth = index != 1
        refreshOutputNote()
        setEncoding(index == 2 ? .linearFloat : .normalized16)
    }

    /// The float encoding's note (`RawOutputEncoding.linearFloatNote`), the
    /// `outputNoteLine` shape, hidden at the 16-bit master.
    private func floatNoteLine() -> NSView {
        let label = NSTextField(wrappingLabelWithString: RawOutputEncoding.linearFloatNote)
        label.font = DS.sans(11)
        label.textColor = DS.textMuted
        label.preferredMaxLayoutWidth = 360
        label.setAccessibilityIdentifier("raw-develop-float-note")
        let spacer = NSView()
        spacer.widthAnchor.constraint(equalToConstant: 106).isActive = true
        let row = NSStackView(views: [spacer, label])
        row.orientation = .horizontal
        row.spacing = 12
        row.isHidden = encoding != .linearFloat
        floatNoteRow = row
        return row
    }

    /// Switches the card between the 16-bit SDR master and the 32-bit float
    /// (linear) development. Entering float sets the tone curve to 0,
    /// scene-linear, and leaves its row live (Shadows stays inert while
    /// it is 0); grain stays live (#680, on the display encoding); the
    /// Presence rows (Texture, Clarity, Dehaze) and the local tone rows are
    /// disabled. Back at 16 bits the curve
    /// returns to the value it had, or the file's own.
    func setEncoding(_ next: RawOutputEncoding) {
        guard next != encoding else { return }
        encoding = next
        if next == .linearFloat {
            toneCurveBeforeFloat = values.toneCurve
            values.toneCurve = 0
        } else {
            values.toneCurve = toneCurveBeforeFloat
                ?? descriptor?.recipe.capabilities.defaults.toneCurve
                ?? probe.capabilities.defaults.toneCurve
            toneCurveBeforeFloat = nil
        }
        refreshEncodingControls()
        requestPreview()
    }

    /// The rows the encoding governs, brought in line with `encoding` and
    /// `values`.
    func refreshEncodingControls() {
        for (index, knob) in knobs.enumerated() where knob.startsAtZeroAtFloat {
            knobRows[index].setValue(knob.read(values))
        }
        enhancementControls?.presenceAvailable = encoding != .linearFloat
        localControls?.toneAvailable = encoding != .linearFloat
        encodingRow?.setSelected(RawOutputEncoding.allCases.firstIndex(of: encoding) ?? 0)
        if let row = floatNoteRow, row.isHidden != (encoding != .linearFloat) {
            row.isHidden = encoding != .linearFloat
            refitControlsDocument()
        }
        applyEnablement()
    }

    /// Gives the preview back whatever height the screen cannot spare.
    ///
    /// Measured, the card stands 916 pt tall with the full 340 pt preview
    /// and the macOS 26 highlight row — comfortable on a default display
    /// mode, and more than a 13" laptop turned up to its largest text size
    /// has to give. There are thirteen controls and two buttons under the
    /// picture, none of which may fall off the bottom of a window that
    /// cannot scroll, so the picture is the part that yields.
    private func fitToScreen(_ card: NSView) {
        guard let constraint = previewHeightConstraint,
              let available = NSScreen.main?.visibleFrame.height
        else { return }
        let overflow = card.fittingSize.height + Self.windowChrome - available
        guard overflow > 0 else { return }
        constraint.constant = max(
            Self.minimumPreviewHeight, Self.previewSize.height - overflow)
    }

    /// "Canon EOS R5 · 8192 × 5464 pixels" — which file is being developed,
    /// which matters when a folder of RAWs was dropped and three dialogs are
    /// queued behind this one.
    private var subtitle: String {
        let size = String(
            format: "%.0f × %.0f pixels", probe.pixelSize.width, probe.pixelSize.height)
        guard let model = probe.cameraModel else { return size }
        return "\(model) · \(size)"
    }

    // MARK: - Control state

    private func knobChanged(_ index: Int, _ value: Double) {
        guard knobs.indices.contains(index) else { return }
        let knob = knobs[index]
        knob.write(&values, knob.range.clamping(value, fallback: knob.read(values)))
        // Tone curve gates Shadows, so every knob change re-asks: thirteen
        // rows is nothing beside the render this is about to schedule.
        applyEnablement()
        requestPreview()
    }

    private func setCustomWhiteBalance(_ custom: Bool) {
        values.customWhiteBalance = custom
        if !custom {
            // Back to As Shot: the sliders show the file's own numbers again,
            // and the decode writes neither property.
            values.temperature = descriptor?.recipe.capabilities.defaults.temperature ?? probe.capabilities.defaults.temperature
            values.tint = descriptor?.recipe.capabilities.defaults.tint ?? probe.capabilities.defaults.tint
            for (index, knob) in knobs.enumerated() where knob.whiteBalance {
                knobRows[index].setValue(knob.read(values))
            }
        }
        applyEnablement()
        requestPreview()
    }

    /// A gated control whose flag is false is disabled and still shows the
    /// file's value; temperature and tint are additionally disabled while
    /// the white balance is As Shot, and Shadows while Tone curve is 0 —
    /// `boostShadowAmount` "has no effect if the boostAmount is 0"
    /// (CIRAWFilter.h), so the dead control reads as dead instead of moving
    /// a preview that cannot change.
    func applyEnablement() {
        for (index, knob) in knobs.enumerated() {
            knobRows[index].isEnabled =
                knob.supported && (!knob.whiteBalance || values.customWhiteBalance)
                && (!knob.inertWhileLinear || values.toneCurve > 0)
        }
        lensRow?.isEnabled = probe.capabilities.lensCorrection || lensfunProfile != nil
        highlightRow?.isEnabled = probe.capabilities.highlightRecovery
    }

    func requestPreview() {
        if workflowPanel != nil { requestWorkflowPreview(); return }
        let url = probe.url
        let values = self.values
        let capabilities = probe.capabilities
        let space = previewSpace
        let descriptor = self.descriptor
        let snapshot = self.snapshot
        renderer.request {
            withExtendedLifetime(snapshot) {
                if let descriptor {
                    return RetainedRawDecoder.preview(url: url, descriptor: descriptor, values: values, space: space)
                }
                return RawImage.previewImage(url: url, values: values, capabilities: capabilities, space: space)
            }
        }
    }

    // MARK: - Buttons

    @objc private func resetClicked(_ sender: Any?) {
        values = initialValues
        // An open keeps the depth its row shows; a redevelop or a place goes
        // back to the encoding the card opened on. Either way a float card
        // over a 16-bit recipe starts the tone curve at 0 again.
        if encodingRow != nil { encoding = initialEncoding }
        toneCurveBeforeFloat = nil
        if encoding == .linearFloat, descriptor?.recipe.outputEncoding != .linearFloat {
            toneCurveBeforeFloat = values.toneCurve
            values.toneCurve = 0
        }
        outputSpace = descriptor?.recipe.outputColorSpace ?? .sRGB
        outputRow?.setSelected(RawOutputSpace.allCases.firstIndex(of: outputSpace) ?? 0)
        refreshOutputNote()
        for (index, knob) in knobs.enumerated() {
            knobRows[index].setValue(knob.read(values))
        }
        lensRow?.setOn(values.lensCorrection)
        highlightRow?.setOn(values.highlightRecovery)
        whiteBalanceRow?.setSelected(values.customWhiteBalance ? 1 : 0)
        enhancementControls?.reset()
        localSession?.reset()
        refreshEncodingControls()
        requestPreview()
    }

    @objc private func openClicked(_ sender: Any?) {
        guard workflowPanel == nil || previewIsReady else { NSSound.beep(); return }
        guard localSession == nil || localSession?.result != nil else { NSSound.beep(); return }
        guard localSession == nil || localSession?.canApply == true else { NSSound.beep(); return }
        do { try localValidate?() }
        catch {
            workflowPanel?.setStatus(error.localizedDescription, ready: false)
            previewIsReady = false; applyButton?.isEnabled = false
            NSSound.beep(); return
        }
        modal?.end(.OK)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        modal?.end(.cancel)
    }
}
