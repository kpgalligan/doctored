import AppKit

// The typed (16-bit) layer style sheet is the shared `LayerStyleSheetController` opened with a
// `PrecisionEditorCapture`: a live preview at 16 bits through `PrecisionStylePreview` (none
// at 32-bit float), the document's light bound as at 8 bits, and one
// publication of the style and the light through `capture.requireCurrent` / `withLayerStylePayload`
// / `withGlobalLight` / `capture.publish` with no `try?` on that path; `editLayerStyle` builds the
// capture, `canEditLayerStyle` admits every layer, a Pass Through group included, and
// `set_global_light` takes the typed `{op:"global_light"}` edit at 16 bits.

/// Layer > Layer Style… — the ONE sheet for a layer's effects and blending
/// options: a checklist down the left (Blending Options, then the nine
/// effects, each with its checkbox) and one pane on the right for the
/// selected row, live-previewed through the normal projection — the core
/// renders the effects, so what the canvas shows while the sheet is up is
/// exactly what Apply commits, as one undo step.
///
/// Its own file because Sheets.swift is frozen; it is built on the shared
/// `makeSheetView` (at 640 pt through the builder's `width:`), so it matches
/// every other dialog. The panes are the `<Effect>Pane` classes, one file
/// each; only the Blending Options pane lives here. The checklist rows and the
/// instance bar are `LayerStyleList.swift`'s.
///
/// A repeating kind (Drop Shadow, Inner Shadow, Stroke, Color Overlay,
/// Gradient Overlay) shows one row per instance, TOP first (the instance that
/// draws on top), or one unchecked row when it has none; its "+" adds a copy
/// above the row, and the instance bar over its pane moves it up or down
/// within the kind or deletes it. The list rebuilds whenever the instances
/// change.
final class LayerStyleSheetController: NSViewController {
    /// A checklist row: an effect row names its instance (render order).
    private enum Row: Hashable {
        case blendingOptions
        case effect(LayerStyleEffectKind, Int)
    }

    /// 640 pt: the checklist (176) + 16 + the pane host (404) inside the
    /// builder's 22 pt insets — every pane grid (106 + 12 + 180 + 12 + 52 =
    /// 362) fits with room for a merged checkbox title.
    private static let cardWidth: CGFloat = 640
    private static let listWidth: CGFloat = 176
    private static let paneWidth: CGFloat = 404
    /// Tall enough for the nine-row Drop Shadow pane, so switching panes
    /// normally leaves the sheet's height alone; a taller pane still grows it.
    private static let paneMinHeight: CGFloat = 320

    private let document: ImageDocument
    private weak var canvas: ImageCanvasView?
    private let layerIndex: Int
    /// A 16-bit or float document's captured revision: the sheet edits the
    /// same model and the document's light and publishes once through the
    /// core's `layer_style` and `global_light` edits (`NativeLayerStyle`).
    private let capture: PrecisionEditorCapture?
    /// The 16-bit live preview; nil at 8 bits, which previews through
    /// `renderer`, and at 32-bit float, which previews nothing (the typed
    /// compositor renders on Apply).
    private var typedPreview: PrecisionStylePreview?

    /// The preview base and what Cancel must leave behind, captured at init:
    /// the sheet blocks every other UI edit while it is open (agent edits
    /// are re-validated at Apply).
    private let baseDoc: RasterDocument?
    /// Whether the sheet opened on an adjustment layer: Apply's
    /// stale-target guard compares it, so an agent edit that swapped the
    /// stack under the sheet cannot land the style on another kind of layer.
    private let openedOnAdjustment: Bool
    /// A CMYK document's inks as the colours the panes show (#1).
    private let shownInks: CmykShownInks?
    private let original: LayerStyle?
    private let originalLight: GlobalLight

    private var style: LayerStyle
    private var light: GlobalLight
    private let renderer = PreviewRenderer()
    /// The last preview document: each tick styles the PREVIOUS preview
    /// rather than the base, so the core's cache hand-over (set_layer_style
    /// moves the rendered planes from the style being replaced to its
    /// successor — a colour, opacity or blend edit is re-stamped, only an
    /// effect whose geometry changed is re-rendered) makes a slider drag
    /// cost no blur, and Apply commits from it with its planes warm.
    private let chain = PreviewChain()
    private var didRequestInitialPreview = false

    private var selectedRow: Row
    private var listRows: [(row: Row, view: StyleListRow)] = []
    private let listStack = NSStackView()
    private let instanceBar = LayerStyleInstanceBar()
    private var effectPanes: [Row: LayerStylePane] = [:]
    private var blendingPane: BlendingOptionsPane?
    private let paneHost = NSView()
    private var visiblePane: LayerStylePane?
    /// The typed publish path's refusal, shown IN the sheet (the deleted
    /// Blend If sheet's shape): a refused Apply must not tear the sheet down
    /// and drop the authored style.
    private let status = NSTextField(wrappingLabelWithString: "")

    init(
        document: ImageDocument, canvas: ImageCanvasView?, layer: Int,
        initial: LayerStyleEffectKind?, capture: PrecisionEditorCapture? = nil
    ) {
        self.document = document
        self.canvas = canvas
        self.layerIndex = layer
        self.capture = capture
        let base: RasterDocument? = capture?.facade ?? document.doc
        var original = base?.layerStylePayload(layer)
        let originalLight = base?.globalLight ?? GlobalLight()
        // A CMYK document (#1): each inked slot shows its ink's colour,
        // remembered so a slot left alone sends its ink back exactly.
        var shownInks: CmykShownInks?
        if base?.isCmyk == true, let precision = base?.precision {
            var inks = CmykShownInks(document: precision, space: .sRGB)
            original = original?.mappingColourSlots { color, ink in
                if let value = ink { color = inks.shownHex(ofInk: value) }
            }
            shownInks = inks
        }
        self.shownInks = shownInks
        var style = original ?? LayerStyle()
        // Opening on a specific effect turns it on, so the first preview
        // already shows it.
        // A kind that already has instances turns on only its TOP one, the
        // one selected below; the others keep their own state.
        if let initial = initial, !style.effectEnabled(initial) {
            let count = style.instanceCount(initial)
            if count > 0 {
                style.setEffectEnabled(initial, instance: count - 1, true)
            } else {
                Self.enable(initial, in: &style, float: base?.isFloat == true)
            }
        }
        self.baseDoc = base
        self.openedOnAdjustment = base?.layerIsAdjustment(layer) ?? false
        self.original = original
        self.originalLight = originalLight
        self.style = style
        self.light = originalLight
        // Opening on a kind selects its TOP instance.
        self.selectedRow = initial.map { .effect($0, max(style.instanceCount($0) - 1, 0)) }
            ?? .blendingOptions
        super.init(nibName: nil, bundle: nil)
        renderer.onRender = { [weak self] cgImage in
            self?.canvas?.previewImage = cgImage
        }
        if let capture, !capture.facade.isFloat {
            let preview = PrecisionStylePreview(capture: capture, layer: layer, document: document)
            preview.onRender = { [weak self] image in self?.typedPreviewLanded(image) }
            preview.onStatus = { [weak self] message in self?.typedPreviewStatus(message) }
            typedPreview = preview
        }
    }

    private func typedPreviewLanded(_ image: CGImage?) {
        canvas?.previewImage = image
    }

    /// The reduced-resolution note is information, not a refusal: muted. Every
    /// other sentence on the line is a refusal and reads in red.
    private func typedPreviewStatus(_ message: String) {
        let isNote = message == PrecisionStylePreview.reducedStatus
        status.textColor = isNote ? DS.textMuted : .systemRed
        status.stringValue = message
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("LayerStyleSheetController does not support NSCoder")
    }

    override func loadView() {
        let list = listStack
        list.orientation = .vertical
        list.alignment = .leading
        list.spacing = 2
        list.widthAnchor.constraint(equalToConstant: Self.listWidth).isActive = true
        rebuildList()
        instanceBar.onUp = { [weak self] in self?.moveSelectedInstance(by: 1) }
        instanceBar.onDown = { [weak self] in self?.moveSelectedInstance(by: -1) }
        instanceBar.onRemove = { [weak self] in self?.removeSelectedInstance() }

        paneHost.translatesAutoresizingMaskIntoConstraints = false
        paneHost.widthAnchor.constraint(equalToConstant: Self.paneWidth).isActive = true
        paneHost.heightAnchor.constraint(greaterThanOrEqualToConstant: Self.paneMinHeight)
            .isActive = true
        // Low priority: the host settles at the minimum unless a pane needs
        // more, which keeps the height determinate.
        let preferred = paneHost.heightAnchor.constraint(equalToConstant: Self.paneMinHeight)
        preferred.priority = .defaultLow
        preferred.isActive = true

        let right = NSStackView(views: [instanceBar, paneHost])
        right.orientation = .vertical
        right.alignment = .leading
        right.spacing = 8
        let columns = NSStackView(views: [list, right])
        columns.orientation = .horizontal
        columns.alignment = .top
        columns.spacing = 16
        status.textColor = .systemRed
        status.preferredMaxLayoutWidth = Self.cardWidth - 44
        // A UI-source probe reads the preview's status here.
        status.setAccessibilityIdentifier("layer-style.status")
        let content = NSStackView(views: [columns, status])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 8

        let resetButton = StickerButton(
            title: "Reset", style: .secondary, target: self, action: #selector(resetClicked(_:)))
        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        // A UI-source probe commits without a mouse (Phase 12E-3b).
        applyButton.identifier = LayerStyleKnockoutRows.applyIdentifier
        let hint = "Effects render from the layer's shape — its alpha times its mask — every time "
            + "the canvas composites, so they follow every move and edit. Fill opacity "
            + "scales the pixels but not the effects."
        view = makeSheetView(
            title: "Layer Style",
            hint: capture == nil || typedPreview != nil
                ? hint
                : hint + " On a 32-bit float document there is no live preview: the "
                    + "typed compositor renders the style and the global light on Apply.",
            content: content,
            buttonRow: makeButtonRow(
                cancel: cancelButton, apply: applyButton,
                leading: [resetButton, sheetFootnote("one undo step")]),
            width: Self.cardWidth)
        refreshChecklist()
        select(selectedRow)
    }

    /// Preview immediately, so what the canvas shows is what Apply produces
    /// even before the first control moves.
    override func viewDidAppear() {
        super.viewDidAppear()
        guard !didRequestInitialPreview else { return }
        didRequestInitialPreview = true
        requestPreview()
    }

    // MARK: - Checklist

    /// The rows `style` lists: Blending Options, then each kind in checklist
    /// order — a repeating kind's instances top first, or its one row.
    private func rows(for style: LayerStyle) -> [Row] {
        var out: [Row] = [.blendingOptions]
        for kind in LayerStyleEffectKind.allCases {
            let count = style.instanceCount(kind)
            if kind.repeats, count > 1 {
                out += (0..<count).reversed().map { .effect(kind, $0) }
            } else {
                out.append(.effect(kind, 0))
            }
        }
        return out
    }

    /// Rebuilds the checklist's row views when the instances changed;
    /// a no-op when the row set is the same.
    private func rebuildList() {
        let wanted = rows(for: style)
        guard wanted != listRows.map(\.row) else { return }
        for view in listStack.arrangedSubviews {
            listStack.removeArrangedSubview(view)
            view.removeFromSuperview()
        }
        listRows = wanted.map { row in (row, makeRowView(row)) }
        for entry in listRows {
            listStack.addArrangedSubview(entry.view)
        }
        refreshChecklist()
    }

    private func makeRowView(_ row: Row) -> StyleListRow {
        guard case .effect(let kind, let instance) = row else {
            let blending = StyleListRow(title: "Blending Options", hasCheckbox: false)
            blending.onSelect = { [weak self] in self?.select(.blendingOptions) }
            return blending
        }
        let view = StyleListRow(title: kind.title, hasCheckbox: true, hasAdd: kind.repeats)
        view.onSelect = { [weak self] in self?.selectEffect(kind, instance) }
        view.onToggle = { [weak self] on in self?.setEffectEnabled(kind, instance, on) }
        view.onAdd = { [weak self] in self?.addInstance(kind, instance) }
        // A 32-bit float document disables an effect its operator table
        // refuses, the core's sentence as the tooltip.
        if isFloat, !FloatDocument.admitsEffect(kind) {
            view.isAvailable = false
            view.toolTip = FloatDocument.effectRefusal(kind)
        }
        return view
    }

    private func select(_ row: Row) {
        selectedRow = row
        for entry in listRows {
            entry.view.isSelected = entry.row == row
        }
        if case .effect(let kind, let instance) = row, kind.repeats,
           style.instanceCount(kind) > 0 {
            instanceBar.isHidden = false
            instanceBar.update(instance: instance, count: style.instanceCount(kind))
        } else {
            instanceBar.isHidden = true
        }
        let pane = self.pane(for: row)
        pane.reload()
        showPane(pane)
    }

    /// Clicking an effect's NAME turns that instance on as well as showing its
    /// pane — a pane editing an effect the preview does
    /// not show would be confusing.
    private func selectEffect(_ kind: LayerStyleEffectKind, _ instance: Int) {
        if !style.effectEnabled(kind, instance: instance) {
            turnOn(kind, instance)
            styleDidChange()
        }
        select(.effect(kind, instance))
    }

    private func setEffectEnabled(_ kind: LayerStyleEffectKind, _ instance: Int, _ on: Bool) {
        if on {
            turnOn(kind, instance)
        } else {
            style.setEffectEnabled(kind, instance: instance, false)
        }
        styleDidChange()
    }

    /// One instance on; a kind with none starts its first through `enable`.
    private func turnOn(_ kind: LayerStyleEffectKind, _ instance: Int) {
        if style.hasEffect(kind) {
            style.setEffectEnabled(kind, instance: instance, true)
        } else {
            Self.enable(kind, in: &style, float: isFloat)
        }
    }

    /// "+": a copy above `instance` (the kind's first when it has none),
    /// selected; refused with a beep at `LayerStyle.maxInstances`.
    private func addInstance(_ kind: LayerStyleEffectKind, _ instance: Int) {
        guard style.hasEffect(kind) else {
            selectEffect(kind, 0)
            return
        }
        guard let copy = style.duplicateInstance(kind, instance) else {
            NSSound.beep()
            return
        }
        styleDidChange()
        select(.effect(kind, copy))
    }

    /// ↑ (`by` +1: draws one place higher) or ↓ (-1) for the selected instance.
    private func moveSelectedInstance(by step: Int) {
        guard case .effect(let kind, let instance) = selectedRow else { return }
        let to = instance + step
        guard (0..<style.instanceCount(kind)).contains(to) else {
            NSSound.beep()
            return
        }
        style.moveInstance(kind, from: instance, to: to)
        styleDidChange()
        select(.effect(kind, to))
    }

    /// The trash: removes the selected instance and selects the kind's top
    /// remaining one (its unchecked row when none is left).
    private func removeSelectedInstance() {
        guard case .effect(let kind, let instance) = selectedRow else { return }
        style.removeInstance(kind, instance)
        styleDidChange()
        select(.effect(kind, max(style.instanceCount(kind) - 1, 0)))
    }

    private var isFloat: Bool { baseDoc?.isFloat == true }
    private var isCmyk: Bool { baseDoc?.isCmyk == true }
    private var isGray: Bool { baseDoc?.isGray == true }
    private var isLab: Bool { baseDoc?.isLab == true }

    /// What the sheet sends for `style`: on a CMYK document every colour slot
    /// as its ink (`CmykShownInks`: a slot left alone keeps its ink exactly),
    /// except an ink-less black, which the core stores as K100
    /// where that is the slot's default (take1's D-K, `cmyk_style`); else the
    /// style as edited.
    private func sent(_ style: LayerStyle) -> LayerStyle {
        guard let inks = shownInks else { return style }
        return style.mappingColourSlots { color, ink in
            if ink == nil, color.lowercased() == "#000000" { return }
            ink = inks.ink(ofShown: color)
        }
    }

    /// Turns `kind` on; an effect NEW to the style in a 32-bit float document
    /// starts in its 32-bit default mode where it differs
    /// (`FloatDocument.effectDefaultModes`: Linear Dodge for the glows and a
    /// Bevel's highlight, whose 8/16-bit Screen has no 32-bit mode).
    private static func enable(_ kind: LayerStyleEffectKind, in style: inout LayerStyle,
                               float: Bool) {
        let isNew = !style.hasEffect(kind)
        style.setEffectEnabled(kind, true)
        guard float, isNew, let mode = FloatDocument.effectDefaultModes[kind] else { return }
        // A new kind holds exactly one instance.
        switch kind {
        case .dropShadow: style.dropShadows[0].blend = mode
        case .innerShadow: style.innerShadows[0].blend = mode
        case .outerGlow: style.outerGlow?.blend = mode
        case .innerGlow: style.innerGlow?.blend = mode
        case .stroke: style.strokes[0].blend = mode
        case .colorOverlay: style.colorOverlays[0].blend = mode
        case .gradientOverlay: style.gradientOverlays[0].blend = mode
        case .patternOverlay: style.patternOverlays[0].blend = mode
        case .bevelEmboss: style.bevelEmboss?.highlightBlend = mode
        case .satin: style.satin?.blend = mode
        }
    }

    private func refreshChecklist() {
        for entry in listRows {
            if case .effect(let kind, let instance) = entry.row {
                entry.view.isChecked = style.effectEnabled(kind, instance: instance)
                entry.view.canAdd = style.instanceCount(kind) < LayerStyle.maxInstances
            }
        }
    }

    private func showPane(_ pane: LayerStylePane) {
        visiblePane?.view.removeFromSuperview()
        visiblePane = pane
        let paneView = pane.view
        paneView.translatesAutoresizingMaskIntoConstraints = false
        paneHost.addSubview(paneView)
        NSLayoutConstraint.activate([
            paneView.topAnchor.constraint(equalTo: paneHost.topAnchor),
            paneView.leadingAnchor.constraint(equalTo: paneHost.leadingAnchor),
            paneView.trailingAnchor.constraint(lessThanOrEqualTo: paneHost.trailingAnchor),
            paneView.bottomAnchor.constraint(lessThanOrEqualTo: paneHost.bottomAnchor),
        ])
    }

    // MARK: - Panes

    private func pane(for row: Row) -> LayerStylePane {
        switch row {
        case .blendingOptions:
            if let pane = blendingPane { return pane }
            let pane = BlendingOptionsPane(
                fill: EffectBinding(
                    get: { [weak self] in self?.style.fillOpacity ?? 1 },
                    set: { [weak self] value in
                        self?.style.fillOpacity = value
                        self?.styleDidChange()
                    }),
                blendIf: EffectBinding(
                    get: { [weak self] in self?.style.blendIf },
                    set: { [weak self] value in
                        self?.style.blendIf = value
                        self?.styleDidChange()
                    }),
                knockout: EffectBinding(
                    get: { [weak self] in self?.style.knockout ?? .none },
                    set: { [weak self] value in
                        self?.style.knockout = value
                        self?.styleDidChange()
                    }),
                blendInterior: EffectBinding(
                    get: { [weak self] in self?.style.blendInteriorAsGroup ?? false },
                    set: { [weak self] value in
                        self?.style.blendInteriorAsGroup = value
                        self?.styleDidChange()
                    }),
                context: paneContext,
                cmyk: isCmyk, gray: isGray, lab: isLab)
            blendingPane = pane
            return pane
        case .effect(let kind, let instance):
            if let pane = effectPanes[row] { return pane }
            let pane = makePane(for: kind, instance: instance)
            effectPanes[row] = pane
            return pane
        }
    }

    private func makePane(for kind: LayerStyleEffectKind, instance: Int) -> LayerStylePane {
        let context = paneContext
        switch kind {
        case .dropShadow:
            return DropShadowPane(
                binding: binding(\.dropShadows, instance: instance, default: DropShadowEffect()),
                context: context)
        case .innerShadow:
            return InnerShadowPane(
                binding: binding(\.innerShadows, instance: instance, default: InnerShadowEffect()),
                context: context)
        case .outerGlow:
            return OuterGlowPane(
                binding: binding(\.outerGlow, default: OuterGlowEffect()), context: context)
        case .innerGlow:
            return InnerGlowPane(
                binding: binding(\.innerGlow, default: InnerGlowEffect()), context: context)
        case .stroke:
            return StrokePane(
                binding: binding(\.strokes, instance: instance, default: StrokeEffect()),
                context: context, layerOrigin: layerOriginReader(),
                layerOffset: layerOffsetReader())
        case .colorOverlay:
            return ColorOverlayPane(
                binding: binding(\.colorOverlays, instance: instance, default: ColorOverlayEffect()),
                context: context)
        case .gradientOverlay:
            return GradientOverlayPane(
                binding: binding(
                    \.gradientOverlays, instance: instance, default: GradientOverlayEffect()),
                context: context)
        case .patternOverlay:
            return PatternOverlayPane(
                binding: binding(
                    \.patternOverlays, instance: instance, default: PatternOverlayEffect()),
                context: context, layerOrigin: layerOriginReader(),
                layerOffset: layerOffsetReader())
        case .bevelEmboss:
            return BevelEmbossPane(
                binding: binding(\.bevelEmboss, default: BevelEmbossEffect()), context: context,
                layerOrigin: layerOriginReader(), layerOffset: layerOffsetReader())
        case .satin:
            return SatinPane(binding: binding(\.satin, default: SatinEffect()), context: context)
        }
    }

    /// The layer content's top left in canvas px (nil when it has none): a
    /// pattern fill's Snap to Origin (`PatternFillRows`).
    private func layerOriginReader() -> () -> CGPoint? {
        let layer = layerIndex
        return { [weak self] in
            guard let bounds = self?.baseDoc?.layerBounds(layer) else { return nil }
            return CGPoint(x: bounds.x, y: bounds.y)
        }
    }

    /// The layer's offset (nil when unknown): a linked pattern fill's Snap to
    /// Origin anchors there (`PatternFillRows`).
    private func layerOffsetReader() -> () -> (x: Int, y: Int)? {
        let layer = layerIndex
        return { [weak self] in
            guard let info = self?.baseDoc?.layerInfo(layer) else { return nil }
            return (info.offsetX, info.offsetY)
        }
    }

    /// A binding to one effect slot: reads the effect (its defaults while
    /// absent) and writes it back, re-previewing.
    private func binding<Effect>(
        _ keyPath: WritableKeyPath<LayerStyle, Effect?>,
        default makeDefault: @autoclosure @escaping () -> Effect
    ) -> EffectBinding<Effect> {
        EffectBinding(
            get: { [weak self] in self?.style[keyPath: keyPath] ?? makeDefault() },
            set: { [weak self] value in
                self?.style[keyPath: keyPath] = value
                self?.styleDidChange()
            })
    }

    /// A binding to one instance of a repeating kind: reads the
    /// instance (the defaults while it does not exist) and writes it back,
    /// creating the kind's first instance when it has none.
    private func binding<Effect>(
        _ keyPath: WritableKeyPath<LayerStyle, [Effect]>, instance: Int,
        default makeDefault: @autoclosure @escaping () -> Effect
    ) -> EffectBinding<Effect> {
        EffectBinding(
            get: { [weak self] in
                guard let list = self?.style[keyPath: keyPath], list.indices.contains(instance)
                else { return makeDefault() }
                return list[instance]
            },
            set: { [weak self] value in
                guard let self else { return }
                if self.style[keyPath: keyPath].indices.contains(instance) {
                    self.style[keyPath: keyPath][instance] = value
                } else if instance == 0, self.style[keyPath: keyPath].isEmpty {
                    self.style[keyPath: keyPath].append(value)
                }
                self.styleDidChange()
            })
    }

    private var paneContext: LayerStylePaneContext {
        // The document's light at both depths: a 16-bit edit is
        // published on Apply with the style.
        let light: EffectBinding<GlobalLight> = EffectBinding(
            get: { [weak self] in self?.light ?? GlobalLight() },
            set: { [weak self] value in self?.lightDidChange(value) })
        return LayerStylePaneContext(
            globalLight: light, canvasSize: baseDoc?.canvasSize ?? .zero,
            // A Grayscale document's effects take its own list (#1 phase 2d).
            blendModes: isGray ? baseDoc.map { GrayDocument.effectModes(for: $0) }
                : isFloat ? FloatDocument.effectModes : nil)
    }

    /// A global-light edit from a pane's Angle/Altitude slider. The pane
    /// that wrote it already shows the value, and the other panes re-read
    /// the light when they are next shown, so no reload happens here — a
    /// reload mid-drag would snap the slider under the pointer.
    private func lightDidChange(_ light: GlobalLight) {
        self.light = light
        styleDidChange()
    }

    private func styleDidChange() {
        rebuildList()
        refreshChecklist()
        for entry in listRows {
            entry.view.isSelected = entry.row == selectedRow
        }
        requestPreview()
    }

    // MARK: - Preview and commit

    /// The edited style and light on the last preview (the captured base
    /// for the first), flattened on the renderer's queue — only captured
    /// values and the chain cross.
    private func requestPreview() {
        // 16 bits: the typed preview's own bookkeeping edits and reservation
        // (`PrecisionStylePreview`).
        if let typedPreview {
            typedPreview.request(style: sent(style), light: light)
            return
        }
        // No float preview: the styled composite is rendered by the core on
        // Apply.
        guard capture == nil, let baseDoc = baseDoc else { return }
        let style = self.style
        let light = self.light
        let idx = layerIndex
        let chain = self.chain
        renderer.request {
            // `try?` on a throwing `RasterDocument?` flattens to
            // `RasterDocument?`; nil means "same style as the previous
            // preview" (the core's equal-value refusal) or an invalid style,
            // and either way the previous preview is the right thing to
            // draw. The light setter refuses an unchanged value the same
            // way, and is always asked so a light edited and then put back
            // is put back on the chain too.
            let previous = chain.last ?? baseDoc
            let styled = (try? previous.withLayerStylePayload(idx, style)) ?? previous
            let lit = (try? styled.withGlobalLight(light)) ?? styled
            chain.last = lit
            // The styled composite is document pixels: tagged with that
            // document's own space so the preview and the canvas behind it
            // agree. `lit` is derived from `baseDoc`, so it carries the
            // same profile.
            return lit.flattened()?.makeCGImage(in: lit.colorSpace)
        }
    }

    @objc private func resetClicked(_ sender: Any?) {
        style = original ?? LayerStyle()
        light = originalLight
        rebuildList()
        refreshChecklist()
        // The selected instance may be gone: keep the row when it is listed.
        select(listRows.contains { $0.row == selectedRow } ? selectedRow : .blendingOptions)
        requestPreview()
    }

    @objc private func applyClicked(_ sender: Any?) {
        let style = self.style
        let light = self.light
        let originalLight = self.originalLight
        let idx = layerIndex
        let baseDoc = self.baseDoc
        let chain = self.chain
        let openedOnAdjustment = self.openedOnAdjustment
        // cancel() drains the renderer's queue, so the chain is quiescent
        // from here on. The typed preview's reservation is released before
        // the publish below admits its own display preparation.
        renderer.cancel()
        typedPreview?.drain()
        canvas?.previewImage = nil
        // An identity style clears (the core's rule; normalized here so the
        // "nothing changed" compare is exact for an unstyled layer).
        let target: LayerStyle? = style.isIdentity ? nil : style
        // What the core receives: a CMYK document's slots as inks.
        let written = target.map { sent($0) }
        if let capture {
            // The typed path: the captured revision must still be current,
            // the core validates and canonicalizes the whole style and the
            // light, and the result is published once (no `try?` on this
            // path). A refusal — a background operation still running
            // included — keeps the sheet up with the authored style intact
            // and reports itself here, so nothing is lost; only success
            // dismisses.
            guard target != original || light != originalLight else { dismiss(self); return }
            do {
                try capture.requireCurrent(document)
                var next = capture.facade
                if target != original {
                    next = try next.withLayerStylePayload(idx, written) ?? next
                }
                if light != originalLight {
                    next = try next.withGlobalLight(light) ?? next
                }
                var record: [ActionStep] = .setLayerStyle(json: written?.json(),
                    layer: capture.facade.layerRef(idx),
                    note: target == nil ? "Layer ▸ Layer Style (cleared)" : "Layer ▸ Layer Style…")
                if light != originalLight {
                    record += .setGlobalLight(angle: light.angle, altitude: light.altitude)
                }
                try capture.publish(next === capture.facade ? nil : next.precision, to: document,
                    name: "Layer Style", record: record)
                dismiss(self)
            } catch {
                status.textColor = .systemRed
                status.stringValue = error.localizedDescription
            }
            return
        }
        dismiss(self)
        guard target != original || light != originalLight else { return }
        // Two tools, one commit: the sheet writes the layer's style and the
        // document's global light together, and `[ActionStep]` is an array
        // exactly so one gesture can record both. The light is recorded only
        // when it moved, since it is document-wide.
        var record: [ActionStep] = .setLayerStyle(
            json: target?.json(), layer: document.doc?.layerRef(idx),
            note: target == nil ? "Layer ▸ Layer Style (cleared)" : "Layer ▸ Layer Style…")
        if light != originalLight {
            record += .setGlobalLight(angle: light.angle, altitude: light.altitude)
        }
        document.applyEdit("Layer Style", record: record) { doc in
            // The layer must still be what the sheet opened on: agent edits
            // can land under a sheet and move the stack.
            guard idx < doc.layerCount, doc.layerIsAdjustment(idx) == openedOnAdjustment else {
                return nil
            }
            // Commit from the last preview while the document is still the
            // one it was rendered on: its planes are warm, so Apply
            // re-renders nothing the preview already showed. Both setters
            // refuse an unchanged value, so a preview that already matches
            // is committed as is (a pending tick may not have run).
            let start = (doc === baseDoc ? chain.last : nil) ?? doc
            var out = (try? start.withLayerStylePayload(idx, target)) ?? start
            out = (try? out.withGlobalLight(light)) ?? out
            return out === doc ? nil : out
        }
    }

    @objc private func cancelClicked(_ sender: Any?) {
        renderer.cancel()
        typedPreview?.drain()
        typedPreview?.cancel()
        canvas?.previewImage = nil
        dismiss(self)
    }
}

// MARK: - Preview chain

/// The document the last preview was rendered from, handed from one
/// renderer-queue tick to the next and read on the main thread at Apply
/// (after `PreviewRenderer.cancel()` has drained the queue). Locked so the
/// two threads can never race even if that ordering changes.
private final class PreviewChain {
    private let lock = NSLock()
    private var doc: RasterDocument?

    var last: RasterDocument? {
        get { lock.withLock { doc } }
        set { lock.withLock { doc = newValue } }
    }
}

// MARK: - Blending Options pane

/// Blending Options: fill opacity (the pixels only, never the effects), then
/// the two remaining blending options — Knockout and Blend Interior
/// Effects as Group (`LayerStyleKnockoutRows`, Phase 12E-3b) — over the
/// embedded Blend If pane. The same rows at 8 and 16 bits; `cmyk` (a CMYK
/// document, #1) offers Blend If on Gray and the four plates; `gray` (a
/// Grayscale document, #1 phase 2d) on Gray only; `lab` (a Lab Color
/// document, #1 phase 2f) on Lightness, a and b.
private final class BlendingOptionsPane: NSObject, LayerStylePane {
    let view: NSView
    private let rows: [PaneRow]
    private let blendIfPane: BlendIfPane

    init(
        fill: EffectBinding<Double>, blendIf: EffectBinding<BlendIf?>,
        knockout: EffectBinding<Knockout>, blendInterior: EffectBinding<Bool>,
        context: LayerStylePaneContext, cmyk: Bool = false, gray: Bool = false, lab: Bool = false
    ) {
        let fillRow = PaneRows.percent(
            "Fill Opacity:", get: { fill.read() }, set: { fill.write($0) })
        rows = [fillRow]
            + LayerStyleKnockoutRows.rows(knockout: knockout, blendInterior: blendInterior)
        let blendIfPane = BlendIfPane(
            binding: blendIf, context: context, cmyk: cmyk, gray: gray, lab: lab)
        self.blendIfPane = blendIfPane
        let heading = fieldLabel("Blend If")
        let stack = NSStackView(views: [PaneRows.grid(rows), heading, blendIfPane.view])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 12
        view = stack
        super.init()
    }

    func reload() {
        rows.forEach { $0.reload() }
        blendIfPane.reload()
    }
}
