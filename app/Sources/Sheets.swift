import AppKit

// `PreviewRenderer.cancel()` can now return without waiting; fill previews cancel their Rust token,
// other immutable previews discard completion.
//
// Under 12F-1 this file (frozen) gained THREE accessibility identifiers and nothing else
// (`canvas-size-width`, `canvas-size-height`, `canvas-size-apply`), which is the 9H probe case
// `canvas-size-sheet-ui`'s whole contract with it.
//
// `SliderSheetController` takes `accessories` (`FilterSheetAccessory` in this file: a choice popup,
// an integer field, Radial Blur's centre picker) whose values follow the sliders' and which may
// stand alone (Offset has no sliders).
//
// Sheet live previews go through `PreviewRenderer` (final-quality Content-Aware Fill uses its
// dedicated captured `BackgroundWork` review owner instead). A sheet that captures ANY input at
// init — the target layer, the selection's coverage bytes — must hand that same value to its commit
// as well as to its preview: a document-modal sheet does not stop the main run loop, so an agent's
// `set_active_layer` or `select_rect` can land behind it and the commit would otherwise write a
// layer, or fill a region, the user never saw previewed. A captured layer INDEX is not an identity
// — `delete_layer` renumbers everything above it and leaves the index in range, pointing at a
// different layer — so capture `RasterDocument.layerFingerprint` beside it and refuse at commit
// when the two no longer agree.

// MARK: - PreviewRenderer

/// Debounced, coalescing background renderer shared by the live-preview
/// sheets. Requests made while a render is in flight replace any queued one;
/// only the newest pending request runs next.
final class PreviewRenderer {
    private let queue = DispatchQueue(label: "com.kgalligan.doctored.preview")
    private var pending: (() -> CGImage?)?
    // Held here, not in the work item, so `cancel()` releases a not-yet-run
    // request's inputs and reservation at once.
    private var debounced: (() -> CGImage?)?
    private(set) var isRendering = false
    private var debounce: DispatchWorkItem?

    var onRender: ((CGImage?) -> Void)?
    var cancelCompute: (() -> Void)?
    /// Main: after a delivery, when nothing is rendering. Not cleared by `cancel()`.
    var onIdle: (() -> Void)?

    func request(_ compute: @escaping () -> CGImage?) {
        debounce?.cancel()
        debounced = compute
        let work = DispatchWorkItem { [weak self] in
            guard let self = self else { return }
            // An executed debounce must not retain its compute closure (and
            // full-resolution inputs/reservation) until the next UI event.
            self.debounce = nil
            self.pending = self.debounced
            self.debounced = nil
            self.drain()
        }
        debounce = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.06, execute: work)
    }

    func cancel() {
        debounce?.cancel()
        debounce = nil
        debounced = nil
        pending = nil
        onRender = nil
        // Immutable handles and synchronized core caches permit the caller
        // to proceed. The closure owns its inputs until cancellation returns.
        cancelCompute?()
        cancelCompute = nil
    }

    /// Drops the debounced and pending closures; `onRender` and an in-flight render stay.
    func dropPending() {
        debounce?.cancel()
        debounce = nil
        debounced = nil
        pending = nil
    }

    /// Main only: returns once no render is in flight (its delivery still hops to main).
    func waitUntilIdle() { queue.sync {} }

    private func drain() {
        guard !isRendering, let compute = pending else { return }
        pending = nil
        isRendering = true
        queue.async { [weak self] in
            let result = compute()
            DispatchQueue.main.async {
                guard let self = self else { return }
                self.isRendering = false
                self.onRender?(result)
                self.drain()
                if !self.isRendering { self.onIdle?() }
            }
        }
    }
}

// MARK: - Shared layout helpers
//
// Module-wide, not file-private: a dialog too big to live here is its own
// …SheetController file (RotateCanvasSheetController), and it must be
// built from these same pieces or the sheets stop matching each other.

let sheetWidth: CGFloat = 420
let sheetInset: CGFloat = 22

/// Assembles a design-system sheet: 420px card by default (`width:` for a
/// wider one — the Layer Style sheet's two panes), 15px/700 title, optional
/// 12px muted hint, content, then the button row.
func makeSheetView(
    title: String? = nil, hint: String? = nil, content: NSView, buttonRow: NSStackView,
    width: CGFloat = sheetWidth
) -> NSView {
    let container = NSView(frame: NSRect(x: 0, y: 0, width: width, height: 240))
    container.wantsLayer = true
    container.layer?.backgroundColor = DS.chromeBackground.cgColor

    var stackedViews: [NSView] = []
    if let title = title {
        let titleLabel = NSTextField(labelWithString: title)
        titleLabel.font = DS.sans(15, weight: .bold)
        titleLabel.textColor = DS.textStrong
        stackedViews.append(titleLabel)
    }
    if let hint = hint {
        let hintLabel = NSTextField(wrappingLabelWithString: hint)
        hintLabel.font = DS.sans(12)
        hintLabel.textColor = DS.textMuted
        hintLabel.isEditable = false
        hintLabel.preferredMaxLayoutWidth = width - sheetInset * 2
        stackedViews.append(hintLabel)
    }
    stackedViews.append(content)
    let stack = NSStackView(views: stackedViews)
    stack.orientation = .vertical
    stack.alignment = .leading
    stack.spacing = 12
    stack.setCustomSpacing(16, after: stackedViews[stackedViews.count - 2])

    stack.translatesAutoresizingMaskIntoConstraints = false
    buttonRow.translatesAutoresizingMaskIntoConstraints = false
    container.addSubview(stack)
    container.addSubview(buttonRow)
    NSLayoutConstraint.activate([
        container.widthAnchor.constraint(equalToConstant: width),
        stack.topAnchor.constraint(equalTo: container.topAnchor, constant: sheetInset),
        stack.leadingAnchor.constraint(equalTo: container.leadingAnchor, constant: sheetInset),
        stack.trailingAnchor.constraint(
            lessThanOrEqualTo: container.trailingAnchor, constant: -sheetInset),
        buttonRow.topAnchor.constraint(equalTo: stack.bottomAnchor, constant: sheetInset),
        buttonRow.leadingAnchor.constraint(equalTo: container.leadingAnchor, constant: sheetInset),
        buttonRow.trailingAnchor.constraint(equalTo: container.trailingAnchor, constant: -sheetInset),
        buttonRow.bottomAnchor.constraint(equalTo: container.bottomAnchor, constant: -sheetInset),
    ])
    return container
}

/// When a row overflows, the `leading` views give way before Cancel and Apply,
/// which keep their whole titles.
func makeButtonRow(
    cancel: NSButton, apply: NSButton, leading: [NSView] = []
) -> NSStackView {
    cancel.keyEquivalent = "\u{1b}"
    apply.keyEquivalent = "\r"
    for view in leading {
        view.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
    }
    let spacer = NSView()
    spacer.setContentHuggingPriority(.defaultLow - 1, for: .horizontal)
    let row = NSStackView(views: leading + [spacer, cancel, apply])
    row.orientation = .horizontal
    row.alignment = .centerY
    row.spacing = 8
    return row
}

/// 13px row label for the sheet grids' 106px right-aligned label column.
func fieldLabel(_ text: String) -> NSTextField {
    let label = NSTextField(labelWithString: text)
    label.font = DS.sans(13)
    label.textColor = DS.textStrong
    return label
}

/// Bottom-left 10px mono footnote carried in the button row's leading slot.
///
/// `makeButtonRow` pins the row to both edges of the card and gives Cancel
/// and Apply their intrinsic widths, so the footnote gets whatever is left —
/// about 210 pt on the standard 420 pt card. A label built by
/// `NSTextField(labelWithString:)` breaks by CLIPPING, so a longer footnote
/// used to lose its tail mid-glyph with nothing to show it had: it ends
/// with a tail ellipsis and carries the whole string as a tooltip instead.
func sheetFootnote(_ text: String) -> NSTextField {
    let label = NSTextField(labelWithString: text)
    label.font = DS.mono(10)
    label.textColor = DS.textFaint
    label.lineBreakMode = .byTruncatingTail
    label.toolTip = text
    return label
}

func sheetCancelButton(target: AnyObject, action: Selector) -> NSButton {
    StickerButton(title: "Cancel", style: .secondary, target: target, action: action)
}

func sheetApplyButton(target: AnyObject, action: Selector) -> NSButton {
    StickerButton(title: "Apply", style: .primary, target: target, action: action)
}

// MARK: - CanvasSizeSheetController

/// The Canvas Size dialog's 3x3 anchor grid: the selected
/// cell marks where the existing content is pinned in the new canvas, and
/// neighboring cells show arrows pointing away from it.
private final class AnchorGridView: NSView {
    /// (column, row), 0-based, top-left origin. Center by default.
    private(set) var anchor = (col: 1, row: 1)

    private var buttons: [[NSButton]] = []

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        let grid = NSGridView()
        grid.translatesAutoresizingMaskIntoConstraints = false
        grid.rowSpacing = 2
        grid.columnSpacing = 2
        for row in 0..<3 {
            var rowButtons: [NSButton] = []
            for col in 0..<3 {
                let button = NSButton(title: "", target: self, action: #selector(cellClicked(_:)))
                button.setButtonType(.momentaryPushIn)
                button.isBordered = false
                button.wantsLayer = true
                button.layer?.cornerRadius = DS.buttonRadius
                button.layer?.borderWidth = 1
                button.font = NSFont.systemFont(ofSize: 13)
                button.tag = row * 3 + col
                button.widthAnchor.constraint(equalToConstant: 32).isActive = true
                button.heightAnchor.constraint(equalToConstant: 28).isActive = true
                rowButtons.append(button)
            }
            buttons.append(rowButtons)
            grid.addRow(with: rowButtons)
        }
        addSubview(grid)
        NSLayoutConstraint.activate([
            grid.topAnchor.constraint(equalTo: topAnchor),
            grid.bottomAnchor.constraint(equalTo: bottomAnchor),
            grid.leadingAnchor.constraint(equalTo: leadingAnchor),
            grid.trailingAnchor.constraint(equalTo: trailingAnchor),
        ])
        refreshCells()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("AnchorGridView does not support NSCoder")
    }

    @objc private func cellClicked(_ sender: NSButton) {
        anchor = (col: sender.tag % 3, row: sender.tag / 3)
        refreshCells()
    }

    private func refreshCells() {
        // Arrows radiate outward from the anchor; distant cells stay blank.
        let arrows: [[String]] = [
            ["↖", "↑", "↗"],
            ["←", "·", "→"],
            ["↙", "↓", "↘"],
        ]
        for row in 0..<3 {
            for col in 0..<3 {
                let button = buttons[row][col]
                let isAnchor = row == anchor.row && col == anchor.col
                let glyph: String
                if isAnchor {
                    glyph = "●"
                } else if abs(row - anchor.row) <= 1 && abs(col - anchor.col) <= 1 {
                    glyph = arrows[row - anchor.row + 1][col - anchor.col + 1]
                } else {
                    glyph = ""
                }
                button.layer?.borderColor = DS.borderStrong.cgColor
                button.layer?.backgroundColor =
                    (isAnchor ? DS.accent : DS.chromeBackground).cgColor
                button.attributedTitle = NSAttributedString(
                    string: glyph,
                    attributes: [
                        .font: NSFont.systemFont(ofSize: isAnchor ? 10 : 13),
                        .foregroundColor: isAnchor ? DS.onAccent : DS.textMuted,
                    ])
            }
        }
    }
}

final class CanvasSizeSheetController: NSViewController {
    private let document: ImageDocument
    private let originalWidth: Int
    private let originalHeight: Int

    private let widthField = NSTextField(string: "")
    private let heightField = NSTextField(string: "")
    private let anchorGrid = AnchorGridView(frame: .zero)

    init(document: ImageDocument) {
        self.document = document
        self.originalWidth = document.doc?.width ?? 1
        self.originalHeight = document.doc?.height ?? 1
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("CanvasSizeSheetController does not support NSCoder")
    }

    override func loadView() {
        for field in [widthField, heightField] {
            let formatter = NumberFormatter()
            formatter.numberStyle = .none
            formatter.allowsFloats = false
            formatter.minimum = 1
            formatter.maximum = 20000
            field.formatter = formatter
            field.widthAnchor.constraint(equalToConstant: 90).isActive = true
            DSField.style(field)
        }
        widthField.integerValue = originalWidth
        heightField.integerValue = originalHeight
        widthField.identifier = NSUserInterfaceItemIdentifier("canvas-size-width")
        heightField.identifier = NSUserInterfaceItemIdentifier("canvas-size-height")

        let currentLabel = fieldLabel("\(originalWidth) × \(originalHeight) px")
        currentLabel.font = DS.mono(13)
        currentLabel.textColor = DS.textMuted

        let grid = NSGridView(views: [
            [fieldLabel("Current size:"), currentLabel],
            [fieldLabel("Width:"), widthField],
            [fieldLabel("Height:"), heightField],
            [fieldLabel("Anchor:"), anchorGrid],
        ])
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106
        grid.cell(for: anchorGrid)?.yPlacement = .top

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        applyButton.identifier = NSUserInterfaceItemIdentifier("canvas-size-apply")
        view = makeSheetView(
            title: "Canvas size",
            hint: "Grows or trims the canvas without scaling. Layers keep "
                + "their pixels and can be revealed again later.",
            content: grid,
            buttonRow: makeButtonRow(
                cancel: cancelButton, apply: applyButton,
                leading: [sheetFootnote("⌥⌘C")]))
    }

    @objc private func applyClicked(_ sender: Any?) {
        let w = widthField.integerValue
        let h = heightField.integerValue
        guard w >= 1, h >= 1 else {
            NSSound.beep()
            return
        }
        guard w * h <= RasterImage.maxResizePixels else {
            let alert = NSAlert()
            alert.messageText = "Size Too Large"
            alert.informativeText = RasterDocument.canvasTooLargeRefusal
            if let window = view.window {
                alert.beginSheetModal(for: window)
            } else {
                alert.runModal()
            }
            return
        }
        // Same refusal, same reason (ChannelBudget): growing the canvas grows
        // every channel with it.
        if let reason = document.doc?.channelBudgetRefusal(width: w, height: h) {
            presentChannelBudgetAlert(reason)
            return
        }
        // The anchor pins the existing content: its column/row chooses how
        // much of the size delta lands left/above the old canvas origin.
        let (originX, originY) = RasterDocument.canvasSizeOrigin(oldWidth: originalWidth,
            oldHeight: originalHeight, newWidth: w, newHeight: h, anchor: anchorGrid.anchor)
        dismiss(self)
        guard w != originalWidth || h != originalHeight else { return }
        document.applyEdit(
            "Canvas Size",
            record: .canvasSize(width: w, height: h, anchor: anchorGrid.anchor),
            resultingSelection: document.selectionCarried(
                canvasWidth: w, height: h, originX: originX, originY: originY)
        ) {
            try $0.canvasResized(w: w, h: h, originX: originX, originY: originY,
                                 indexedBackground: IndexedDocument.swatch)
        }
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}

// MARK: - FeatherSheetController

/// Select > Feather Selection…: Gaussian-softens the current selection's
/// coverage mask. Selections are not undoable and no pixels change, so
/// there is no live preview — the marquee and dimming update on Apply.
final class FeatherSheetController: NSViewController {
    private weak var canvas: ImageCanvasView?

    // Log-feel radius: the slider runs log10(0.5)…log10(250) so the
    // useful small radii get most of the travel.
    private let radiusSlider = NSSlider(
        value: log10(4.0), minValue: log10(0.5), maxValue: log10(250.0),
        target: nil, action: nil)
    private let radiusValue = NSTextField(labelWithString: "4.0 px")

    init(canvas: ImageCanvasView) {
        self.canvas = canvas
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("FeatherSheetController does not support NSCoder")
    }

    override func loadView() {
        radiusSlider.isContinuous = true
        radiusSlider.target = self
        radiusSlider.action = #selector(sliderChanged(_:))
        radiusSlider.widthAnchor.constraint(equalToConstant: 180).isActive = true

        radiusValue.font = DS.mono(12)
        radiusValue.textColor = DS.textMuted
        radiusValue.alignment = .right
        radiusValue.widthAnchor.constraint(equalToConstant: 52).isActive = true

        let grid = NSGridView(views: [
            [fieldLabel("Radius:"), radiusSlider, radiusValue]
        ])
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        view = makeSheetView(
            title: "Feather selection",
            hint: "Softens the selection edge, so fills, gradients, and "
                + "strokes fade out across the radius.",
            content: grid,
            buttonRow: makeButtonRow(
                cancel: cancelButton, apply: applyButton,
                leading: [sheetFootnote("0.5 – 250 px")]))
    }

    private var radius: Double {
        pow(10, radiusSlider.doubleValue)
    }

    @objc private func sliderChanged(_ sender: Any?) {
        radiusValue.stringValue = String(format: "%.1f px", radius)
    }

    @objc private func applyClicked(_ sender: Any?) {
        let radius = radius
        dismiss(self)
        guard let canvas = canvas, let selection = canvas.selection else {
            NSSound.beep()
            return
        }
        // A feather that spreads all coverage to zero comes back nil and
        // deselects — the same all-empty rule as the combine modes.
        canvas.commitSelectionChange(selection.feathered(by: radius), actionName: "Feather Selection",
                                     record: .modifySelection(operation: "feather", radius: radius))
        // Recorded HERE and not in `setSelection`: that setter is a per-tick
        // side effect (a marquee drag calls it hundreds of times, and every
        // canvas resize calls it to clear a selection that no longer means
        // anything), while this is the command boundary.
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}

// MARK: - SelectionMorphSheetController

/// Select > Modify > Expand/Contract/Border/Smooth…: FeatherSheetController's
/// numeric dialog, parameterized over the selection-morphology op Apply
/// runs. Selections are not undoable and no pixels change, so there is no
/// live preview — the marquee and dimming update on Apply.
final class SelectionMorphSheetController: NSViewController {
    private weak var canvas: ImageCanvasView?
    private let sheetTitle: String
    private let valueLabel: String
    private let hint: String
    private let transform: (CanvasSelection, Double) -> CanvasSelection?
    /// What Apply records, from the chosen value. A parameter for the same
    /// reason SliderSheetController has one: this class is generic over four
    /// Select-menu commands and only its caller knows which `operation` its
    /// number belongs to.
    private let step: (Double) -> [ActionStep]

    // The same log-feel travel as Feather: log10(0.5)…log10(250), so the
    // useful small values get most of the slider.
    private let valueSlider = NSSlider(
        value: log10(4.0), minValue: log10(0.5), maxValue: log10(250.0),
        target: nil, action: nil)
    private let valueField = NSTextField(labelWithString: "4.0 px")

    /// `label` is the parameter's row label ("Radius:", or Border's
    /// "Width:"); `transform` maps the current selection and the chosen
    /// value to the new selection (nil deselects).
    init(
        canvas: ImageCanvasView, title: String, label: String, hint: String,
        step: @escaping (Double) -> [ActionStep],
        transform: @escaping (CanvasSelection, Double) -> CanvasSelection?
    ) {
        self.canvas = canvas
        self.sheetTitle = title
        self.valueLabel = label
        self.hint = hint
        self.step = step
        self.transform = transform
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("SelectionMorphSheetController does not support NSCoder")
    }

    override func loadView() {
        valueSlider.isContinuous = true
        valueSlider.target = self
        valueSlider.action = #selector(sliderChanged(_:))
        valueSlider.widthAnchor.constraint(equalToConstant: 180).isActive = true

        valueField.font = DS.mono(12)
        valueField.textColor = DS.textMuted
        valueField.alignment = .right
        valueField.widthAnchor.constraint(equalToConstant: 52).isActive = true

        let grid = NSGridView(views: [
            [fieldLabel(valueLabel), valueSlider, valueField]
        ])
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        view = makeSheetView(
            title: sheetTitle,
            hint: hint,
            content: grid,
            buttonRow: makeButtonRow(
                cancel: cancelButton, apply: applyButton,
                leading: [sheetFootnote("0.5 – 250 px")]))
    }

    private var value: Double {
        pow(10, valueSlider.doubleValue)
    }

    @objc private func sliderChanged(_ sender: Any?) {
        valueField.stringValue = String(format: "%.1f px", value)
    }

    @objc private func applyClicked(_ sender: Any?) {
        let value = value
        dismiss(self)
        guard let canvas = canvas, let selection = canvas.selection else {
            NSSound.beep()
            return
        }
        // An op that leaves no coverage (a shrink past the middle, a border
        // of an empty contour) comes back nil and deselects — the same
        // all-empty rule as Feather and the combine modes.
        canvas.commitSelectionChange(transform(selection, value), actionName: "Modify Selection",
                                     record: step(value))
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}

// MARK: - SliderSheetController

/// One slider row of a SliderSheetController: label, slider-space range, an
/// optional slider→value mapping (log-feel gamma), and a value formatter.
struct FilterSliderDescriptor {
    let label: String
    let sliderMin: Double
    let sliderMax: Double
    let sliderInitial: Double
    let isInteger: Bool
    let map: (Double) -> Double
    let format: (Double) -> String

    init(
        label: String, min: Double, max: Double, initial: Double,
        isInteger: Bool = false,
        map: @escaping (Double) -> Double = { $0 },
        format: @escaping (Double) -> String
    ) {
        self.label = label
        self.sliderMin = min
        self.sliderMax = max
        self.sliderInitial = initial
        self.isInteger = isInteger
        self.map = map
        self.format = format
    }
}

/// A non-slider control under a SliderSheetController's grid (a choice, an
/// integer field, a centre picker): its values follow the sliders' mapped
/// values, and `onChange` re-previews (`FilterSheetAccessories.swift`).
protocol FilterSheetAccessory: AnyObject {
    var view: NSView { get }
    var values: [Double] { get }
    var onChange: (() -> Void)? { get set }
}

/// Generic live-preview filter sheet: N sliders and a compute closure that
/// filters the ACTIVE layer. The preview shows the edit in context (the
/// filtered layer swapped into the captured doc handle, flattened); Apply
/// routes through applyToActiveLayer as one undo step.
final class SliderSheetController: NSViewController {
    private let document: ImageDocument
    private weak var canvas: ImageCanvasView?
    private let sheetTitle: String
    private let actionName: String
    private let descriptors: [FilterSliderDescriptor]
    private let accessories: [FilterSheetAccessory]
    private let compute: (RasterImage, [Double], RasterJobControl?) -> RasterImage?
    /// Proxy values from the mapped values and the proxy factor (nil: none).
    private let proxy: (([Double], Double) -> [Double])?
    private let willChange: ((Int, inout [Double]) -> Void)?
    /// What this sheet's Apply records, from the sliders' MAPPED values. It
    /// is a parameter because the class is generic over five filters: only
    /// the factory that built it knows which tool and which argument names
    /// its numbers belong to (`ActionSteps.swift`).
    private let step: ([Double]) -> [ActionStep]
    /// The 8-bit `dr_image_filter_op` request of the mapped values, which
    /// the exact preview reserves and Apply admits (nil: neither).
    private let request: (([Double]) -> ByteFilterRequest)?
    private let capture: OperationCapture?
    private let renderer: ByteFilterPreview
    private let previewStatus = NSTextField(labelWithString: "")
    private let precisionSession: PrecisionAdjustmentSession?
    private var sliders: [NSSlider] = []
    private var valueLabels: [NSTextField] = []
    private var didRequestInitialPreview = false
    private var fineControls: FineToneControls?
    /// Set on a sheet presented again after a late busy refusal;
    /// shown on its status line when it appears.
    private var lateBusyStatus: String?

    /// `willChange` runs before a slider change is committed; it may adjust
    /// the OTHER sliders' (slider-space) values.
    init(
        document: ImageDocument, canvas: ImageCanvasView,
        title: String, actionName: String,
        sliders: [FilterSliderDescriptor],
        accessories: [FilterSheetAccessory] = [],
        willChange: ((Int, inout [Double]) -> Void)? = nil,
        step: @escaping ([Double]) -> [ActionStep],
        proxy: (([Double], Double) -> [Double])? = nil,
        request: (([Double]) -> ByteFilterRequest)? = nil,
        compute: @escaping (RasterImage, [Double], RasterJobControl?) -> RasterImage?
    ) {
        self.document = document
        self.request = request
        self.canvas = canvas
        self.sheetTitle = title
        self.actionName = actionName
        self.descriptors = sliders
        self.accessories = accessories
        self.willChange = willChange
        self.step = step
        self.compute = compute
        self.proxy = proxy
        self.capture = document.captureOperation(previewBase: canvas.channelDisplay?.baseKind)
        renderer = ByteFilterPreview(capture: capture, proxied: proxy != nil)
        precisionSession = PrecisionAdjustmentSession(document, canvas: canvas, mode: .destructive)
        if proxy != nil { precisionSession?.prepareProxy() }
        super.init(nibName: nil, bundle: nil)
        renderer.onRender = { [weak self] cgImage in
            self?.canvas?.previewImage = cgImage
        }
        renderer.onRefusal = { [weak self] message in
            self?.previewStatus.stringValue = message ?? ""
            self?.previewStatus.isHidden = message == nil
        }
        for accessory in accessories { accessory.onChange = { [weak self] in self?.fineValuesChanged() } }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("SliderSheetController does not support NSCoder")
    }

    override func loadView() {
        var rows: [[NSView]] = []
        for descriptor in descriptors {
            let slider = NSSlider(
                value: descriptor.sliderInitial, minValue: descriptor.sliderMin,
                maxValue: descriptor.sliderMax, target: self, action: #selector(sliderChanged(_:)))
            slider.isContinuous = true
            slider.widthAnchor.constraint(equalToConstant: 180).isActive = true

            let label = NSTextField(
                labelWithString: descriptor.format(descriptor.map(descriptor.sliderInitial)))
            label.font = DS.mono(12)
            label.textColor = DS.textMuted
            label.alignment = .right
            label.widthAnchor.constraint(equalToConstant: 52).isActive = true

            sliders.append(slider)
            valueLabels.append(label)
            rows.append([fieldLabel(descriptor.label), slider, label])
        }
        let grid = NSGridView(views: rows)
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        if !rows.isEmpty {  // a sheet of accessories alone has no grid columns
            grid.column(at: 0).xPlacement = .trailing
            grid.column(at: 0).width = 106
        }

        fineControls = FineToneControls.forSliders(id: sheetTitle.lowercased(), native: document.doc.isHighDepth,
            sliders: sliders) { [weak self] in self?.fineValuesChanged() }
        let content = NSStackView(views: (rows.isEmpty ? [] : [grid]) + accessories.map(\.view)
            + (fineControls.map { [$0] } ?? [])); content.orientation = .vertical
        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        view = makeSheetView(
            title: sheetTitle, content: precisionSession?.content(content) ?? (request == nil ? content : withPreviewStatus(content)),
            buttonRow: makeButtonRow(
                cancel: cancelButton, apply: applyButton,
                leading: [sheetFootnote("one undo step")]))
    }

    override func viewDidAppear() {
        super.viewDidAppear()
        // Non-identity defaults (threshold, posterize, …) must preview
        // immediately so what the user sees is what Apply produces.
        guard !didRequestInitialPreview else { return }
        didRequestInitialPreview = true
        if let lateBusyStatus {
            let status = precisionSession?.previewStatus ?? previewStatus
            status.stringValue = lateBusyStatus
            status.isHidden = false
            // While the slot is held a preview is refused, and its refusal
            // would replace this line; the next control change previews.
            if BackgroundWork.occupied { return }
        }
        requestPreview()
    }

    /// What Apply's commit runs when the worker slot was taken
    /// after this sheet's own slot wait (behind the rasterize prompt): a fresh
    /// sheet from the same arguments and accessory objects, its sliders at the
    /// refused positions and `BackgroundWork.busyMessage` on its status line.
    private func lateBusyReopener(presenter: NSViewController?) -> () -> Void {
        let rows = zip(descriptors, sliders).map { row, slider in
            FilterSliderDescriptor(label: row.label, min: row.sliderMin, max: row.sliderMax,
                initial: slider.doubleValue, isInteger: row.isInteger, map: row.map, format: row.format)
        }
        let document = document, title = sheetTitle, actionName = actionName, accessories = accessories
        let willChange = willChange, step = step, proxy = proxy, request = request, compute = compute
        return { [weak presenter, weak canvas] in
            guard let presenter, let canvas else { return }
            let sheet = SliderSheetController(document: document, canvas: canvas, title: title,
                actionName: actionName, sliders: rows, accessories: accessories, willChange: willChange,
                step: step, proxy: proxy, request: request, compute: compute)
            sheet.lateBusyStatus = BackgroundWork.busyMessage
            presenter.presentAsSheet(sheet)
        }
    }

    private func mappedValues() -> [Double] {
        zip(sliders, descriptors).map { slider, descriptor in
            descriptor.map(slider.doubleValue)
        } + accessories.flatMap(\.values)
    }

    private func updateValueLabels() {
        for (index, descriptor) in descriptors.enumerated() {
            valueLabels[index].stringValue = descriptor.format(descriptor.map(sliders[index].doubleValue))
        }
    }

    private func fineValuesChanged() {
        updateValueLabels()
        requestPreview()
    }

    private func requestPreview() {
        let values = mappedValues(), compute = compute, step = step
        if let precisionSession { precisionSession.request(steps: step(values), proxySteps: proxy.map { map in { step(map(values, $0)) } }); return }
        let reserve: ((OperationCapture) throws -> SelectionWorkBudget.Reservation)? = request.map { make in
            { try ByteFilterAdmission.reservePreview(make(values), capture: $0) } }
        renderer.request(op: { compute($0, values, $1) }, proxyOp: proxy.map { map in { compute($0, map(values, $1), nil) } },
                         reserve: reserve)
    }

    /// The 8-bit sheets' refusal line; the 16-bit session shows its own.
    private func withPreviewStatus(_ content: NSView) -> NSView {
        previewStatus.textColor = .systemRed
        previewStatus.font = DS.sans(11)
        previewStatus.lineBreakMode = .byWordWrapping
        previewStatus.maximumNumberOfLines = 3
        previewStatus.setAccessibilityIdentifier("tone.preview.status")
        previewStatus.widthAnchor.constraint(lessThanOrEqualToConstant: 360).isActive = true
        previewStatus.isHidden = true  // shown with the first refusal; the stack drops hidden views
        let stack = NSStackView(views: [content, previewStatus])
        stack.orientation = .vertical; stack.alignment = .leading; stack.spacing = 6
        return stack
    }

    @objc private func sliderChanged(_ sender: Any?) {
        guard let slider = sender as? NSSlider,
              let index = sliders.firstIndex(where: { $0 === slider })
        else { return }
        if descriptors[index].isInteger {
            slider.doubleValue = slider.doubleValue.rounded()
        }
        if let willChange = willChange {
            var values = sliders.map { $0.doubleValue }
            willChange(index, &values)
            for (i, value) in values.enumerated() where sliders[i].doubleValue != value {
                sliders[i].doubleValue = value
            }
        }
        updateValueLabels()
        fineControls?.refresh()
        requestPreview()
    }

    @objc private func applyClicked(_ sender: Any?) {
        guard fineControls?.accept() != false else { return }
        let values = mappedValues()
        let compute = compute
        // A held worker slot keeps the sheet open with its values; an
        // exact bypass (plan 0) runs nothing and never waits for the slot.
        let bypass = precisionSession == nil && capture.flatMap { capture in
            request.map { ByteFilterAdmission.isExactBypass($0(values), on: capture.source) } } == true
        if precisionSession != nil || (request != nil && !bypass),
           !BackgroundWork.awaitSlotForSheetApply(status: precisionSession?.previewStatus ?? previewStatus) { return }
        // The preview's reservation is released before Apply admits.
        renderer.drain()
        canvas?.previewImage = nil
        let reopen = lateBusyReopener(presenter: presentingViewController)
        dismiss(self)
        if let precisionSession { precisionSession.commit(steps: step(values), name: actionName, onLateBusy: reopen); return }
        if let request, let capture {
            document.applyCapturedByteFilter(capture, actionName, record: step(values), request: request(values),
                                             onLateBusy: reopen)
            return
        }
        document.applyCapturedOperation(capture, actionName, record: step(values)) { compute($0, values, nil) }
    }

    @objc private func cancelClicked(_ sender: Any?) {
        precisionSession?.cancel()
        renderer.cancel()
        canvas?.previewImage = nil
        dismiss(self)
    }
}

// MARK: - AdjustmentLayerSheetController

/// Live-preview sheet for a PARAMETERIZED adjustment-layer op, in either of
/// two modes: creating a NEW adjustment layer above the active one, or
/// re-editing an existing adjustment layer's meta. The slider rows reuse the
/// destructive dialogs' exact labels, ranges and mappings
/// (the SliderSheetController factories) so the
/// destructive and non-destructive variants feel identical; only the commit
/// target differs. The preview chains the same PURE ops the commit will run,
/// on a doc handle captured at init — the document itself is untouched until
/// Apply (one undo step), so Cancel is nothing more than dropping the
/// preview: no undo registration, no edit count.
///
/// This class is also the dialog seam: `make(op:...)` is the one entry
/// point the editor calls, and it hands `.curves` to
/// CurvesAdjustmentSheetController (the curve editor under the same modes
/// and completion contract, in its own file) instead of building slider
/// rows. A `preset` (Layer ▸ New Adjustment Layer ▸ From Photoshop Preset…)
/// prefills a `.create` sheet of the same op.
final class AdjustmentLayerSheetController: NSViewController {
    /// What Apply commits.
    enum Mode {
        /// Insert a new adjustment layer above the ACTIVE layer. Its mask
        /// comes from `selection` (the marquee's canvas-sized coverage,
        /// captured when the sheet opened and left up afterwards, exactly
        /// like Layer > Mask > From Selection) or is reveal-all when nil.
        case create(selection: [UInt8]?)
        /// Replace ONLY layer `layer`'s meta; `original` prefills the
        /// sliders, and an unchanged meta commits nothing.
        case edit(layer: Int, original: AdjustmentLayerPayload)
    }

    /// The op-specific pieces of a slider-built dialog.
    private struct Config {
        let sliders: [FilterSliderDescriptor]
        let willChange: ((Int, inout [Double]) -> Void)?
        /// Meta params from the sliders' MAPPED values (schema ranges are
        /// guaranteed by the slider ranges themselves).
        let params: ([Double]) -> [String: Any]
        /// SLIDER-space positions from a layer's params (edit-mode prefill),
        /// clamped defensively into the sliders' ranges.
        let sliderValues: (AdjustmentLayerPayload) -> [Double]
    }

    private static func config(for op: AdjustmentLayerOp) -> Config? {
        switch op {
        case .hueRotate:
            return Config(
                sliders: [
                    FilterSliderDescriptor(
                        label: "Angle:", min: -180, max: 180, initial: 0,
                        format: { String(format: "%.0f°", $0) })
                ],
                willChange: nil,
                params: { ["degrees": $0[0]] },
                sliderValues: { payload in
                    // Any finite angle is legal in meta; show its wrap into
                    // the dialog's -180…180 range (rotation is mod 360).
                    var degrees = payload.number("degrees", default: 0)
                        .truncatingRemainder(dividingBy: 360)
                    if degrees > 180 { degrees -= 360 }
                    if degrees < -180 { degrees += 360 }
                    return [degrees]
                })
        case .threshold:
            return Config(
                sliders: [
                    FilterSliderDescriptor(
                        label: "Level:", min: 0, max: 1, initial: 0.5,
                        format: { String(format: "%.2f", $0) })
                ],
                willChange: nil,
                params: { ["level": $0[0]] },
                sliderValues: { [$0.number("level", default: 0.5).clamped(0, 1)] })
        case .posterize:
            return Config(
                sliders: [
                    FilterSliderDescriptor(
                        label: "Levels:", min: 2, max: 16, initial: 4, isInteger: true,
                        format: { String(Int($0.rounded())) })
                ],
                willChange: nil,
                params: { ["levels": Int($0[0].rounded())] },
                sliderValues: { [$0.number("levels", default: 4).rounded().clamped(2, 16)] })
        case .curves, .levels:
            // Curves and Levels have dialogs, but bespoke ones (the curve
            // editor in CurvesAdjustmentSheetController, the channel stages
            // in LevelsAdjustmentEditor), not slider rows.
            return nil
        case .bcs, .exposure, .vibrance, .hueSaturation, .colorBalance, .blackAndWhite,
            .photoFilter, .channelMixer, .selectiveColor, .shadowsHighlights,
            .whiteBalance, .gradientMap, .colorLookup, .replaceColor:
            // Brightness/Contrast (`BrightnessContrastSheet`) and the
            // phase-5 ops have one sheet class each, built from
            // AdjustmentSchema's table (AdjustmentSheets.make below): their
            // controls are popups, swatches and gradient editors, not rows
            // of sliders.
            return nil
        case .invert, .grayscale, .sepia:
            // Parameterless: created directly, nothing to dialog.
            return nil
        }
    }

    /// Whether `op` has a dialog to open — what enables Adjustment Options…
    /// (parameterless ops have none).
    static func opHasDialog(_ op: AdjustmentLayerOp) -> Bool {
        op == .curves || op == .levels || AdjustmentSheets.hasDialog(op) || config(for: op) != nil
    }

    /// The one entry point the editor calls for both creation and re-edit;
    /// nil for an op without a dialog. `onCommitted` fires after a
    /// successful Apply with the created/edited layer's index (the editor
    /// selects it and refreshes).
    static func make(
        op: AdjustmentLayerOp, document: ImageDocument, canvas: ImageCanvasView,
        mode: Mode, preset: AdjustmentLayerPayload? = nil, onCommitted: ((Int) -> Void)? = nil
    ) -> NSViewController? {
        if let sheet = AdjustmentSheets.make(
            op: op, document: document, canvas: canvas,
            mode: AdjustmentSheetMode.from(mode), preset: preset, onCommitted: onCommitted)
        {
            return sheet
        }
        if op == .curves {
            let controller = CurvesAdjustmentSheetController(
                document: document, canvas: canvas, mode: AdjustmentSheetMode.from(mode),
                preset: preset)
            controller.onCommitted = onCommitted
            return controller
        }
        if op == .levels {
            let controller = LevelsAdjustmentEditor(
                document: document, canvas: canvas, mode: AdjustmentSheetMode.from(mode),
                preset: preset)
            controller.onCommitted = onCommitted
            return controller
        }
        guard let config = config(for: op) else { return nil }
        let controller = AdjustmentLayerSheetController(
            op: op, config: config, document: document, canvas: canvas, mode: mode,
            preset: preset?.op == op.rawValue ? preset : nil)
        controller.onCommitted = onCommitted
        return controller
    }

    private let document: ImageDocument
    private weak var canvas: ImageCanvasView?
    private let op: AdjustmentLayerOp
    private let config: Config
    private let mode: Mode
    private let preset: AdjustmentLayerPayload?
    // In-context preview base: the chained ops are applied to this captured
    // handle and the flattened result is shown, so masks, blend modes and
    // the other layers stay visible while scrubbing.
    private let baseDoc: RasterDocument?
    private let aboveIndex: Int
    private let renderer = PreviewRenderer()
    private let precisionSession: PrecisionAdjustmentSession?
    private var sliders: [NSSlider] = []
    private var valueLabels: [NSTextField] = []
    private var didRequestInitialPreview = false
    private var fineControls: FineToneControls?
    private var onCommitted: ((Int) -> Void)?
    private let feedback = ToneSheetFeedback(active: false)

    private init(
        op: AdjustmentLayerOp, config: Config, document: ImageDocument,
        canvas: ImageCanvasView, mode: Mode, preset: AdjustmentLayerPayload?
    ) {
        self.op = op
        self.config = config
        self.document = document
        self.canvas = canvas
        self.mode = mode
        self.preset = preset
        self.baseDoc = document.doc
        self.aboveIndex = document.activeLayerIndex
        precisionSession = PrecisionAdjustmentSession(document, canvas: canvas, mode: AdjustmentSheetMode.from(mode))
        super.init(nibName: nil, bundle: nil)
        renderer.onRender = { [weak self] cgImage in
            self?.feedback.present(cgImage, on: self?.canvas)
        }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("AdjustmentLayerSheetController does not support NSCoder")
    }

    override func loadView() {
        // Edit mode prefills from the layer's meta; create mode starts at
        // the descriptors' defaults, like the destructive dialogs, or at a
        // Photoshop preset's values when one was opened.
        var initialValues = config.sliders.map { $0.sliderInitial }
        var source: AdjustmentLayerPayload?
        if case .edit(_, let original) = mode { source = original }
        if case .create = mode { source = preset }
        if let source {
            let prefill = config.sliderValues(source)
            if prefill.count == initialValues.count { initialValues = prefill }
        }
        var rows: [[NSView]] = []
        for (index, descriptor) in config.sliders.enumerated() {
            let slider = ClipTrackingSlider(
                value: initialValues[index], minValue: descriptor.sliderMin,
                maxValue: descriptor.sliderMax, target: self,
                action: #selector(sliderChanged(_:)))
            slider.isContinuous = true
            slider.widthAnchor.constraint(equalToConstant: 180).isActive = true

            let label = NSTextField(
                labelWithString: descriptor.format(descriptor.map(initialValues[index])))
            label.font = DS.mono(12)
            label.textColor = DS.textMuted
            label.alignment = .right
            label.widthAnchor.constraint(equalToConstant: 52).isActive = true

            sliders.append(slider)
            valueLabels.append(label)
            rows.append([fieldLabel(descriptor.label), slider, label])
        }
        let grid = NSGridView(views: rows)
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106

        var content: NSView = grid

        fineControls = FineToneControls.forSliders(id: op.rawValue, native: document.doc.isHighDepth,
            sliders: sliders) { [weak self] in self?.fineValuesChanged() }
        if let fineControls { let stack = NSStackView(views: [content, fineControls]); stack.orientation = .vertical; content = stack }
        let title: String
        if case .edit = mode {
            title = "\(op.displayName) options"
        } else {
            title = "New \(op.displayName) layer"
        }
        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        view = makeSheetView(
            title: title, content: precisionSession?.content(content) ?? content,
            buttonRow: makeButtonRow(
                cancel: cancelButton, apply: applyButton,
                leading: [sheetFootnote("one undo step")]))
    }

    override func viewDidAppear() {
        super.viewDidAppear()
        // A newly inserted layer with non-identity defaults (threshold,
        // posterize) must preview immediately so what the user sees is what
        // Apply produces.
        guard !didRequestInitialPreview else { return }
        didRequestInitialPreview = true
        requestPreview()
    }

    private func mappedValues() -> [Double] {
        zip(sliders, config.sliders).map { slider, descriptor in
            descriptor.map(slider.doubleValue)
        }
    }

    private func updateValueLabels() {
        for (index, descriptor) in config.sliders.enumerated() {
            valueLabels[index].stringValue =
                descriptor.format(descriptor.map(sliders[index].doubleValue))
        }
    }

    private func metaJSON(_ values: [Double]) -> String? {
        AdjustmentLayerPayload(op: op, params: config.params(values)).json()
    }

    /// Previews the SAME chained pure ops Apply will commit, applied to the
    /// captured base and flattened (self stays off the render queue; only
    /// captured values cross).
    private func fineValuesChanged() {
        updateValueLabels()
        requestPreview()
    }

    private func requestPreview() {
        if let precisionSession { precisionSession.request(meta: metaJSON(mappedValues())); return }
        guard let baseDoc = baseDoc, let meta = metaJSON(mappedValues()) else { return }
        let clip = feedback.clip, box = feedback.isActive ? feedback.box : nil
        let mode = mode
        let above = aboveIndex
        let name = op.displayName
        renderer.request {
            let previewDoc: RasterDocument?
            switch mode {
            case .create(let selection):
                previewDoc = baseDoc.addingAdjustmentLayer(
                    active: above, name: name, meta: meta, selection: selection)
            case .edit(let idx, _):
                previewDoc = baseDoc.withLayerMeta(idx, meta)
            }
            return ToneSheetFeedback.byteFrame(previewDoc?.flattened(), colorSpace: baseDoc.colorSpace, clip: clip, box: box)
        }
    }

    @objc private func sliderChanged(_ sender: Any?) {
        guard let slider = sender as? NSSlider,
              let index = sliders.firstIndex(where: { $0 === slider })
        else { return }
        if config.sliders[index].isInteger {
            slider.doubleValue = slider.doubleValue.rounded()
        }
        if let willChange = config.willChange {
            var values = sliders.map { $0.doubleValue }
            willChange(index, &values)
            for (i, value) in values.enumerated() where sliders[i].doubleValue != value {
                sliders[i].doubleValue = value
            }
        }
        updateValueLabels()
        fineControls?.refresh()
        requestPreview()
    }

    @objc private func applyClicked(_ sender: Any?) {
        guard fineControls?.accept() != false else { return }
        let values = mappedValues()
        renderer.cancel()
        canvas?.previewImage = nil
        dismiss(self)
        if let precisionSession { precisionSession.commit(meta: metaJSON(values), onCommitted: onCommitted); return }
        guard let meta = metaJSON(values) else {
            NSSound.beep()
            return
        }
        switch mode {
        case .create(let selection):
            // Compose on the document's CURRENT doc and active layer, not
            // the captured preview base, so any edit that slipped in while
            // the sheet was open (an agent's) survives.
            let below = document.activeLayerIndex
            let name = op.displayName
            let before = document.doc
            // An active GROUP takes the new entry as its top child,
            // so the core answers where it went (§4.5).
            let landing = before?.newEntryLandingIndex(active: below) ?? below + 1
            document.applyEdit(
                "New \(name) Layer", record: .addAdjustmentLayer(
                    meta: meta, name: name,
                    position: before?.newEntryPosition(active: below) ?? .above)
            ) {
                $0.addingAdjustmentLayer(
                    active: below, name: name, meta: meta, selection: selection)
            }
            guard document.doc !== before, let doc = document.doc else { return }
            onCommitted?(min(landing, doc.layerCount - 1))
        case .edit(let idx, let original):
            // Closing the dialog with unchanged values must not register an
            // undo step or dirty the file (sorted-key encoding makes the
            // comparison byte-stable).
            guard meta != original.json() else { return }
            document.applyEdit(
                "Edit \(op.displayName) Layer",
                record: .editAdjustmentLayer(
                    meta: meta, layer: document.doc?.layerRef(idx))
            ) { doc in
                // The layer must still BE an adjustment layer (only an agent
                // edit can move the stack under an open sheet).
                guard doc.layerIsAdjustment(idx) else { return nil }
                return doc.withLayerMeta(idx, meta)
            }
            onCommitted?(idx)
        }
    }

    @objc private func cancelClicked(_ sender: Any?) {
        precisionSession?.cancel()
        renderer.cancel()
        canvas?.previewImage = nil
        dismiss(self)
    }
}

extension Double {
    fileprivate func clamped(_ low: Double, _ high: Double) -> Double {
        Swift.min(Swift.max(self, low), high)
    }
}

// MARK: - Filter sheet factories

extension SliderSheetController {
    static func hueRotate(document: ImageDocument, canvas: ImageCanvasView) -> SliderSheetController {
        // The ambient plane/channel target, captured with the sheet: a
        // recorded filter step names its target explicitly, because the UI
        // reads it from the document where the agent reads an argument.
        let target = document.planeEditTarget.layerEditAgentName(in: document.doc)
        return SliderSheetController(
            document: document, canvas: canvas,
            title: "Hue Rotate", actionName: "Hue Rotate",
            sliders: [
                FilterSliderDescriptor(
                    label: "Angle:", min: -180, max: 180, initial: 0,
                    format: { String(format: "%.0f°", $0) })
            ],
            step: { values in
                .filter(
                    "hue_rotate", ["degrees": ActionArgs.number(values[0])], target: target)
            },
            compute: { image, values, _ in
                image.hueRotated(degrees: values[0])
            })
    }

    static func threshold(document: ImageDocument, canvas: ImageCanvasView) -> SliderSheetController {
        // The ambient plane/channel target, captured with the sheet: a
        // recorded filter step names its target explicitly, because the UI
        // reads it from the document where the agent reads an argument.
        let target = document.planeEditTarget.layerEditAgentName(in: document.doc)
        return SliderSheetController(
            document: document, canvas: canvas,
            title: "Threshold", actionName: "Threshold",
            sliders: [
                FilterSliderDescriptor(
                    label: "Level:", min: 0, max: 1, initial: 0.5,
                    format: { String(format: "%.2f", $0) })
            ],
            step: { values in
                .filter("threshold", ["level": values[0]], target: target)
            },
            compute: { image, values, _ in
                image.thresholded(level: values[0])
            })
    }

    static func posterize(document: ImageDocument, canvas: ImageCanvasView) -> SliderSheetController {
        // The ambient plane/channel target, captured with the sheet: a
        // recorded filter step names its target explicitly, because the UI
        // reads it from the document where the agent reads an argument.
        let target = document.planeEditTarget.layerEditAgentName(in: document.doc)
        return SliderSheetController(
            document: document, canvas: canvas,
            title: "Posterize", actionName: "Posterize",
            sliders: [
                FilterSliderDescriptor(
                    label: "Levels:", min: 2, max: 16, initial: 4, isInteger: true,
                    format: { String(Int($0.rounded())) })
            ],
            step: { values in
                .filter("posterize", ["levels": Int(values[0].rounded())], target: target)
            },
            compute: { image, values, _ in
                image.posterized(levels: Int(values[0].rounded()))
            })
    }

    static func pixelate(document: ImageDocument, canvas: ImageCanvasView) -> SliderSheetController {
        // The ambient plane/channel target, captured with the sheet: a
        // recorded filter step names its target explicitly, because the UI
        // reads it from the document where the agent reads an argument.
        let target = document.planeEditTarget.layerEditAgentName(in: document.doc)
        return SliderSheetController(
            document: document, canvas: canvas,
            title: "Pixelate", actionName: "Pixelate",
            sliders: [
                FilterSliderDescriptor(
                    label: "Block size:", min: 2, max: 64, initial: 8, isInteger: true,
                    format: { String(Int($0.rounded())) })
            ],
            step: { values in
                .filter("pixelate", ["block": Int(values[0].rounded())], target: target)
            },
            compute: { image, values, _ in
                image.pixelated(block: Int(values[0].rounded()))
            })
    }

    static func addNoise(document: ImageDocument, canvas: ImageCanvasView) -> SliderSheetController {
        // One seed per sheet open, shared by preview and Apply, so the
        // committed noise is exactly what was previewed.
        let seed = UInt64.random(in: UInt64.min ... UInt64.max)
        // See the sibling factories: the target is captured with the sheet.
        let target = document.planeEditTarget.layerEditAgentName(in: document.doc)
        return SliderSheetController(
            document: document, canvas: canvas,
            title: "Add Noise", actionName: "Add Noise",
            sliders: [
                FilterSliderDescriptor(
                    label: "Amount:", min: 0.02, max: 1, initial: 0.1,
                    format: { String(format: "%.2f", $0) })
            ],
            step: { values in
                // The seed crosses as the Int64 BIT PATTERN the tool reads it
                // back as (`UInt64(bitPattern: Int64(intArg))`), so a replayed
                // Add Noise lands the identical grain.
                .filter(
                    "add_noise",
                    [
                        "amount": ActionArgs.number(values[0]),
                        "seed": Int(bitPattern: UInt(seed)),
                    ], target: target)
            },
            compute: { image, values, _ in
                image.noised(amount: values[0], seed: seed)
            })
    }
}
