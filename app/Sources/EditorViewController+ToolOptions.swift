import AppKit

// The options bar's content: one ordered cluster list per tool (plus the
// Free Transform takeover), declared as OptionDescriptor bindings into
// ToolOptionsStore and the editor's live state. Priority order IS the
// list order — the bar promotes clusters left to right and folds the rest
// into the More popover, so growth is mechanical: append to the tail.
//
// Every control works at each depth its tool runs at. None is
// validation-disabled for good: Crop's Delete cropped pixels is disabled (and
// reads on) only while its Fill is Content-Aware Fill, which deletes them;
// options that made no sense for this app (Fill's anti-alias and pattern, the
// shapes' path operation, Type's anti-alias and warp, the Gradient's dither,
// midpoint and method) were removed rather than kept dead.
extension EditorViewController {
    // Quick-pick lists for the numeric fields' chevron menus. Pixel fields
    // share one 1–64 spread, percent fields step by ten, and radius-like
    // fields double; fields with special ranges declare their own inline.
    private static let quickPx: [Double] = [1, 2, 3, 4, 6, 8, 12, 16, 24, 32, 48, 64]
    private static let quickPercent: [Double] = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100]
    private static let quickRadius: [Double] = [0, 1, 2, 4, 8, 16, 32, 64, 128]
    private static let quickTolerance: [Double] = [0, 8, 16, 32, 64, 128, 255]

    /// Rebuilds the bar for the current tool or modal session.
    func presentToolOptions() {
        let bar = optionsBar
        if canvas.puppetWarpSession != nil {
            bar.present(icon: "pin", fallback: "PW", title: "Puppet Warp",
                        clusters: puppetWarpClusters())
            return
        }
        if canvas.perspectiveWarpSession != nil {
            bar.present(icon: "perspective", fallback: "PV", title: "Perspective Warp",
                        clusters: perspectiveWarpClusters())
            return
        }
        if isTransforming {
            // The layer COUNT is not repeated here: the status line carries
            // it (`transformStatusText`, MultiLayerEdit.swift), which is the
            // one place that says what the box encloses — one box drawn
            // around three layers looks exactly like one box drawn around
            // one. That number is also the cue for the preview's single
            // approximation: with a non-contiguous set, layers sitting
            // BETWEEN the selected ones draw in the `above` plate
            // (`transformStackComposites`, MultiLayerEdit.swift), so a
            // preview that looks reordered has a number to explain it.
            bar.present(
                icon: "arrow.triangle.2.circlepath", fallback: "FT",
                title: "Free Transform", clusters: transformClusters())
            return
        }
        let tool = currentTool
        bar.present(
            icon: tool.symbol, fallback: tool.fallbackGlyph, title: tool.displayName,
            clusters: optionClusters(for: tool))
    }

    private func optionClusters(for tool: EditorTool) -> [OptionCluster] {
        switch tool {
        case .select, .ellipseSelect, .lasso, .freehandLasso, .subject, .wand:
            return selectClusters(tool)
        case .quickSelect:
            return Array(selectClusters(tool).prefix(2)) + quickSelectionClusters()
        case .magneticLasso:
            return Array(selectClusters(tool).prefix(2)) + magneticLassoClusters()
        case .objectSelect:
            return Array(selectClusters(tool).prefix(2)) + objectSelectionClusters()
        case .crop:
            return cropClusters()
        case .perspectiveCrop:
            return perspectiveCropClusters()
        case .ruler:
            return rulerClusters()
        case .move:
            return moveClusters()
        case .brush, .eraser, .clone, .dodge, .heal, .spotHeal, .blurBrush, .sharpenBrush,
            .smudge, .sponge, .historyBrush:
            return paintClusters(tool)
        case .artHistoryBrush:
            return paintClusters(tool)
        case .remove:
            return removeClusters()
        case .patch:
            return patchClusters()
        case .contentAwareMove:
            return contentAwareMoveClusters()
        case .redEye:
            return redEyeClusters()
        case .fill:
            return fillClusters()
        case .gradient:
            return gradientClusters()
        case .shapeRect, .shapeEllipse, .shapeLine:
            return shapeClusters(tool)
        case .pen:
            return penClusters()
        case .text:
            return textClusters()
        case .eyedropper:
            return eyedropperClusters()
        case .colorSampler:
            return colorSamplerClusters()
        case .zoom, .hand:
            return viewClusters()
        }
    }

    // MARK: - Select group

    private func selectClusters(_ tool: EditorTool) -> [OptionCluster] {
        let wand = tool == .wand
        var clusters: [OptionCluster] = [
            OptionCluster([
                OptionDescriptor(
                    id: "select.mode", overflowLabel: "Mode",
                    kind: .segmented(
                        segments: [
                            ("square.dashed", "N", "New selection"),
                            ("plus", "+", "Add to selection"),
                            ("minus", "−", "Subtract from selection"),
                            ("square.on.square.intersection.dashed", "∩", "Intersect"),
                        ],
                        get: { ToolOptionsStore.shared.select.modeIndex },
                        set: { [weak self] index in
                            ToolOptionsStore.shared.select.modeIndex = index
                            self?.syncCanvasPaintState()
                        },
                        segmentEnabled: nil)),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "select.feather", microLabel: "Feather", overflowLabel: "Feather",
                    kind: .field(
                        width: 52, unit: " px", decimals: 0, min: 0, max: 250,
                        quick: Self.quickRadius,
                        get: { ToolOptionsStore.shared.select.feather },
                        set: { [weak self] value in
                            ToolOptionsStore.shared.select.feather = value
                            self?.syncCanvasPaintState()
                        })),
            ]),
        ]
        // Anti-alias softens only a curved or slanted edge: a rectangle is
        // pixel-exact and the wand and Subject hand over their own coverage,
        // so those tools carry no checkbox.
        if [.ellipseSelect, .lasso, .freehandLasso].contains(tool) {
            clusters.append(OptionCluster([
                OptionDescriptor(
                    id: "select.antialias", overflowLabel: "Anti-alias",
                    kind: .checkbox(
                        label: "Anti-alias",
                        get: { ToolOptionsStore.shared.select.antiAlias },
                        set: { ToolOptionsStore.shared.select.antiAlias = $0 })),
            ]))
        }
        if wand {
            clusters.append(OptionCluster([
                // Sample Size, before Tolerance.
                OptionDescriptor(
                    id: "select.sampleSize", overflowLabel: "Sample Size",
                    kind: .popup(
                        width: 140,
                        items: ["Point Sample"] + SelectToolOptions.wandSampleSizes.dropFirst()
                            .map { "\($0) by \($0) Average" },
                        get: { ToolOptionsStore.shared.select.wandSampleSizeIndex },
                        set: { ToolOptionsStore.shared.select.wandSampleSizeIndex = $0 })),
                OptionDescriptor(
                    id: "select.tolerance", microLabel: "Tolerance", overflowLabel: "Tolerance",
                    kind: .field(
                        width: document?.doc?.isHighDepth == true ? 74 : 46, unit: "",
                        decimals: document?.doc?.isHighDepth == true ? 3 : 0, min: 0, max: 255,
                        quick: Self.quickTolerance,
                        get: { ToolOptionsStore.shared.select.tolerance },
                        set: { ToolOptionsStore.shared.select.tolerance = $0 })),
                OptionDescriptor(
                    id: "select.contiguous", overflowLabel: "Contiguous",
                    kind: .checkbox(
                        label: "Contiguous",
                        get: { ToolOptionsStore.shared.select.contiguous },
                        set: { ToolOptionsStore.shared.select.contiguous = $0 })),
                OptionDescriptor(
                    id: "select.sample", overflowLabel: "Sample",
                    kind: .popup(
                        width: 110, items: ["All layers", "Current layer"],
                        get: { ToolOptionsStore.shared.select.wandSampleAllLayers ? 0 : 1 },
                        set: { ToolOptionsStore.shared.select.wandSampleAllLayers = $0 == 0 })),
            ]))
        }
        // Overflow tail: morphology amounts that also APPLY to the current
        // selection when committed with one on canvas.
        clusters.append(contentsOf: [
            OptionCluster([
                OptionDescriptor(
                    id: "select.grow", microLabel: "Expand", overflowLabel: "Expand / Contract",
                    kind: .field(
                        width: 52, unit: " px", decimals: 0, min: -250, max: 250,
                        quick: [-64, -32, -16, -8, -4, -2, -1, 1, 2, 4, 8, 16, 32, 64],
                        get: { ToolOptionsStore.shared.select.growAmount },
                        set: { [weak self] value in
                            ToolOptionsStore.shared.select.growAmount = value
                            self?.applySelectionMorph(
                                record: .modifySelection(
                                    operation: value >= 0 ? "grow" : "shrink",
                                    radius: abs(value))
                            ) { selection in
                                value >= 0
                                    ? selection.grown(by: value)
                                    : selection.shrunk(by: -value)
                            }
                        })),
                OptionDescriptor(
                    id: "select.border", microLabel: "Border", overflowLabel: "Border width",
                    kind: .field(
                        width: 52, unit: " px", decimals: 0, min: 1, max: 250,
                        quick: [1, 2, 4, 8, 16, 32, 64],
                        get: { ToolOptionsStore.shared.select.borderWidth },
                        set: { [weak self] value in
                            ToolOptionsStore.shared.select.borderWidth = value
                            self?.applySelectionMorph(
                                record: .modifySelection(operation: "border", width: value)
                            ) { $0.bordered(width: value) }
                        })),
                OptionDescriptor(
                    id: "select.smooth", microLabel: "Smooth", overflowLabel: "Smooth",
                    kind: .field(
                        width: 52, unit: " px", decimals: 0, min: 1, max: 250,
                        quick: [1, 2, 4, 8, 16, 32, 64],
                        get: { ToolOptionsStore.shared.select.smoothRadius },
                        set: { [weak self] value in
                            ToolOptionsStore.shared.select.smoothRadius = value
                            self?.applySelectionMorph(
                                record: .modifySelection(operation: "smooth", radius: value)
                            ) { $0.smoothed(by: value) }
                        })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "select.quickmask", overflowLabel: "Quick Mask",
                    kind: .checkbox(
                        label: "Quick Mask",
                        get: { [weak self] in self?.canvas.quickMaskActive ?? false },
                        set: { [weak self] _ in self?.toggleQuickMask(nil) })),
                OptionDescriptor(
                    id: "select.save", overflowLabel: "Save selection",
                    kind: .button(
                        title: "Save…", action: { [weak self] in self?.saveSelectionSheet(nil) }),
                    isEnabled: { [weak self] in self?.canvas.selection != nil }),
            ]),
        ])
        return clusters
    }

    /// A morphology field committed with a selection on canvas: apply the
    /// op to it (the field's value is remembered either way).
    private func applySelectionMorph(
        record: [ActionStep], _ transform: (CanvasSelection) -> CanvasSelection?
    ) {
        guard let selection = canvas.selection, !canvas.quickMaskActive else { return }
        commitCanvasSelection(transform(selection), actionName: "Modify Selection", record: record)
    }

    // MARK: - Crop

    private func cropClusters() -> [OptionCluster] {
        [
            OptionCluster([
                OptionDescriptor(
                    id: "crop.ratio", microLabel: "Ratio", overflowLabel: "Ratio",
                    kind: .popup(
                        width: 92, items: Self.cropRatios.map { $0.title },
                        get: { ToolOptionsStore.shared.crop.ratioIndex },
                        set: { [weak self] index in
                            ToolOptionsStore.shared.crop.ratioIndex = index
                            self?.cropRatioChanged()
                        })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "crop.w", microLabel: "W", overflowLabel: "Width",
                    kind: .field(
                        width: 58, unit: "", decimals: 0, min: 1, max: 100_000,
                        quick: [],
                        get: { [weak self] in self?.cropRectWidth ?? 0 },
                        set: { [weak self] in self?.cropRectWidth = $0 })),
                OptionDescriptor(
                    id: "crop.h", microLabel: "H", overflowLabel: "Height",
                    kind: .field(
                        width: 58, unit: "", decimals: 0, min: 1, max: 100_000,
                        quick: [],
                        get: { [weak self] in self?.cropRectHeight ?? 0 },
                        set: { [weak self] in self?.cropRectHeight = $0 })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "crop.straighten", microLabel: "Straighten", overflowLabel: "Straighten",
                    kind: .field(
                        width: 52, unit: "°", decimals: 1, min: -45, max: 45,
                        quick: [-45, -30, -15, -5, -1, 0, 1, 5, 15, 30, 45],
                        get: { [weak self] in self?.cropStraightenDegrees ?? 0 },
                        set: { [weak self] in self?.cropStraightenDegrees = $0 })),
                OptionDescriptor(
                    id: "crop.straighten.line", overflowLabel: "Straighten by drawing a line",
                    kind: .checkbox(
                        label: "Draw line",
                        get: { [weak self] in self?.cropSession?.drawsStraightenLine ?? false },
                        set: { [weak self] in self?.setCropStraightenLineMode($0) }),
                    isEnabled: { [weak self] in self?.cropSession != nil }),
                OptionDescriptor(
                    id: "crop.straighten.auto", overflowLabel: "Auto straighten",
                    kind: .button(
                        title: "Auto", action: { [weak self] in self?.autoStraightenCrop() }),
                    isEnabled: { [weak self] in self?.cropSession != nil }),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "crop.delete", overflowLabel: "Delete cropped pixels",
                    kind: .checkbox(
                        label: "Delete cropped pixels",
                        get: {
                            let crop = ToolOptionsStore.shared.crop
                            return crop.deleteCroppedPixels
                                || CropFill(index: crop.fillIndex) == .contentAware
                        },
                        set: { ToolOptionsStore.shared.crop.deleteCroppedPixels = $0 }),
                    isEnabled: {
                        CropFill(index: ToolOptionsStore.shared.crop.fillIndex) != .contentAware
                    }),
            ]),
            // What a box past the canvas fills its new area with
            // (CropFill.swift), the Fill popup.
            OptionCluster([
                OptionDescriptor(
                    id: "crop.fill", microLabel: "Fill", overflowLabel: "Fill",
                    kind: .popup(
                        width: 150, items: CropFill.allCases.map(\.title),
                        get: { ToolOptionsStore.shared.crop.fillIndex },
                        set: { [weak self] index in
                            ToolOptionsStore.shared.crop.fillIndex = index
                            self?.cropFillChanged()
                        })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "crop.grid", microLabel: "Grid", overflowLabel: "Grid overlay",
                    kind: .popup(
                        width: 84, items: ["None", "Thirds"],
                        get: { ToolOptionsStore.shared.crop.gridIndex },
                        set: { [weak self] index in
                            ToolOptionsStore.shared.crop.gridIndex = index
                            self?.cropGridChanged()
                        })),
                OptionDescriptor(
                    id: "crop.snap", overflowLabel: "Snap to guides",
                    kind: .checkbox(
                        label: "Snap to guides",
                        get: { ToolOptionsStore.shared.crop.snapToGuides },
                        set: { [weak self] value in
                            ToolOptionsStore.shared.crop.snapToGuides = value
                            self?.syncCanvasPaintState()
                        })),
            ]),
        ]
    }

    // MARK: - Move

    private func moveClusters() -> [OptionCluster] {
        [
            OptionCluster([
                // Two halves of one option: the checkbox says whether a
                // click retargets at all, the popup says what it picks. The
                // popup is meaningless with the checkbox off, so it follows
                // it.
                OptionDescriptor(
                    id: "move.autoselecton", overflowLabel: "Auto-select",
                    kind: .checkbox(
                        label: "Auto-select",
                        get: { ToolOptionsStore.shared.move.autoSelect },
                        set: { ToolOptionsStore.shared.move.autoSelect = $0 })),
                OptionDescriptor(
                    id: "move.autoselect", overflowLabel: "Auto-select target",
                    kind: .popup(
                        width: 76, items: ["Layer", "Group"],
                        get: { ToolOptionsStore.shared.move.autoSelectIndex },
                        set: { ToolOptionsStore.shared.move.autoSelectIndex = $0 }),
                    isEnabled: { ToolOptionsStore.shared.move.autoSelect }),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "move.transformcontrols", overflowLabel: "Show transform controls",
                    kind: .checkbox(
                        label: "Show transform controls",
                        get: { ToolOptionsStore.shared.move.transformControlsShown },
                        set: { [weak self] value in
                            ToolOptionsStore.shared.move.transformControlsShown = value
                            self?.refreshMoveControls()
                        })),
            ]),
            OptionCluster([
                // The six segments are in the core's own `DrAlign` order, so
                // the index goes straight through to the same call Layer ▸
                // Align makes (EditorViewController+Arrange.swift): CONTENT
                // bounds, aligned to the selection's union with two or more
                // entries selected and to the canvas with one.
                OptionDescriptor(
                    id: "move.align", overflowLabel: "Align",
                    kind: .segmented(
                        segments: [
                            ("align.horizontal.left", "⇤", "Align left edges"),
                            ("align.horizontal.center", "↔", "Align horizontal centers"),
                            ("align.horizontal.right", "⇥", "Align right edges"),
                            ("align.vertical.top", "⤒", "Align top edges"),
                            ("align.vertical.center", "↕", "Align vertical centers"),
                            ("align.vertical.bottom", "⤓", "Align bottom edges"),
                        ],
                        get: { -1 },
                        set: { [weak self] index in self?.alignSelection(index) },
                        segmentEnabled: nil),
                    // Enabled at 16 bits and float too.
                    isEnabled: { [weak self] in self?.canAlignLayers ?? false }),
            ]),
            OptionCluster([
                // Equalizing the gaps needs three entries to have a gap
                // between them at all — a rule that changes with the layers
                // panel's selection, not with the tool. So it rides
                // `segmentEnabled`, which the control asks on every click and
                // every redraw, rather than `isEnabled`, which it caches
                // until the bar is refreshed: a control that dims lazily
                // would swallow the very click it should take.
                OptionDescriptor(
                    id: "move.distribute", overflowLabel: "Distribute",
                    kind: .segmented(
                        segments: [
                            ("distribute.horizontal", "⇹", "Distribute horizontal spacing"),
                            ("distribute.vertical", "⇳", "Distribute vertical spacing"),
                        ],
                        get: { -1 },
                        set: { [weak self] index in self?.distributeSelection(index) },
                        // Enabled at 16 bits and float too.
                        segmentEnabled: { [weak self] _ in self?.canDistributeLayers ?? false })),
                OptionDescriptor(
                    id: "move.snaplayers", overflowLabel: "Snap to layers",
                    kind: .checkbox(
                        label: "Snap to layers",
                        get: { ToolOptionsStore.shared.move.snapToLayers },
                        set: { [weak self] value in
                            ToolOptionsStore.shared.move.snapToLayers = value
                            self?.syncCanvasPaintState()
                        })),
                OptionDescriptor(
                    id: "move.nudge", microLabel: "Nudge", overflowLabel: "Nudge step",
                    kind: .field(
                        width: 52, unit: " px", decimals: 0, min: 1, max: 100,
                        quick: Self.quickPx,
                        get: { ToolOptionsStore.shared.move.nudgeStep },
                        set: { [weak self] value in
                            ToolOptionsStore.shared.move.nudgeStep = value
                            self?.syncCanvasPaintState()
                        })),
            ]),
        ]
    }

    // MARK: - Paint tools (brush, eraser, clone, dodge)

    /// The name of a paint tool's amount field (`PaintToolOptions.opacity`):
    /// Dodge / Burn's Exposure, the Strength of Blur, Sharpen, Smudge and
    /// Sponge, and Opacity for the rest, the History Brush included.
    static func paintAmountLabel(_ tool: EditorTool) -> String {
        switch tool {
        case .dodge: return "Exposure"
        case .blurBrush, .sharpenBrush, .smudge, .sponge: return "Strength"
        default: return "Opacity"
        }
    }

    /// The Blend option the beginning stroke composites with: the active
    /// tool's stored mode for brush and clone (the strokes that paint
    /// layer pixels), Normal for everything else. Called from
    /// onStrokeBegin, which latches it for the stroke's lifetime.
    func paintStrokeBlendMode() -> DrBlendMode {
        guard currentTool == .brush || currentTool == .clone,
              let paint = ToolOptionsStore.shared.paintOptions(for: currentTool)
        else { return DR_BLEND_NORMAL }
        // On a Grayscale or Lab Color document a stored mode outside its list strokes as Normal,
        // as the popup shows it (#547 phase 2e, Lab phase 2e); the store is not rewritten.
        let mode = Self.blendMode(fromStored: paint.blendIndex)
        return Self.paintBlendChoices(for: document?.doc).contains { $0.0 == mode }
            ? mode : DR_BLEND_NORMAL
    }

    /// The Brush's and Clone Stamp's Mode list on `doc`: on a Grayscale document its modes
    /// (`NativeLayerBlending.grayModes(for:)`, #547 phase 2e), on a Lab Color document the Lab
    /// list (`NativeLayerBlending.labModes`, #547 Lab phase 2e), else every mode.
    static func paintBlendChoices(for doc: RasterDocument?) -> [(DrBlendMode, String)] {
        if let doc, doc.isLab {
            return NativeLayerBlending.labModes.map { ($0, DrBlendMode.displayName(for: $0)) }
        }
        guard let doc, doc.isGray else { return DrBlendMode.allBlendModes }
        return NativeLayerBlending.grayModes(for: doc).map { ($0, DrBlendMode.displayName(for: $0)) }
    }

    /// The Fill and Gradient tools' Mode list on `doc`: on a Lab Color document the Lab list
    /// (`NativeLayerBlending.labModes`, #547 Lab phase 2f), else every mode.
    static func toolBlendChoices(for doc: RasterDocument?) -> [(DrBlendMode, String)] {
        if let doc, doc.isLab {
            return NativeLayerBlending.labModes.map { ($0, DrBlendMode.displayName(for: $0)) }
        }
        return DrBlendMode.allBlendModes
    }

    /// Rebuilds the bar after a non-live document change (a mode conversion, menu or MCP) when
    /// the Brush, Clone Stamp or Art History Brush is current and the bar does not hold the Mode
    /// popup the document now wants (`paint.blend.gray` / `paint.artMode.gray` on Gray,
    /// `paint.blend.lab` / `paint.artMode.lab` on Lab), and likewise for the Fill and Gradient
    /// tools (`fill.blend.lab` / `gradient.blend.lab` on Lab).
    func toolOptionsDocumentDidChange(_ note: Notification) {
        guard note.userInfo?["isLive"] as? Bool != true else { return }
        let gray = document?.doc?.isGray == true
        let lab = document?.doc?.isLab == true
        let wanted: String
        if currentTool == .brush || currentTool == .clone {
            wanted = lab ? "paint.blend.lab" : gray ? "paint.blend.gray" : "paint.blend"
        } else if currentTool == .artHistoryBrush {
            wanted = lab ? "paint.artMode.lab" : gray ? "paint.artMode.gray" : "paint.artMode"
        } else if currentTool == .fill {
            wanted = lab ? "fill.blend.lab" : "fill.blend"
        } else if currentTool == .gradient {
            wanted = lab ? "gradient.blend.lab" : "gradient.blend"
        } else {
            return
        }
        func holds(_ view: NSView) -> Bool {
            view.identifier?.rawValue == wanted || view.subviews.contains(where: holds)
        }
        if !holds(optionsBar) { presentToolOptions() }
    }

    /// The stored `blendIndex` — a raw DrBlendMode value, stable across
    /// releases — as a mode; Normal for anything unrecognized.
    static func blendMode(fromStored raw: Int) -> DrBlendMode {
        guard raw >= 0, raw <= Int(UInt32.max) else { return DR_BLEND_NORMAL }
        let mode = DrBlendMode(rawValue: UInt32(raw))
        return DrBlendMode.allBlendModes.contains { $0.0 == mode } ? mode : DR_BLEND_NORMAL
    }

    /// The flat blend-popup row for a stored raw mode (Normal for junk) in `choices`.
    private static func blendListIndex(
        of stored: Int, in choices: [(DrBlendMode, String)] = DrBlendMode.allBlendModes
    ) -> Int {
        let mode = blendMode(fromStored: stored)
        return choices.firstIndex { $0.0 == mode } ?? 0
    }

    /// The Art History Brush's own cluster: its Mode (seven,
    /// stored as `blendIndex`), Style, Area and Tolerance.
    private func artHistoryCluster(read: @escaping () -> PaintToolOptions,
                                   write: @escaping (PaintToolOptions) -> Void) -> OptionCluster {
        // On a Grayscale document the Mode list is the tool's Grayscale modes
        // (#547 phase 2e), on a Lab Color one those of `labModes` (#547 Lab 2f; since phase 3,
        // Normal and the HSL four); a stored mode outside it reads as Normal, the store
        // unchanged.
        let doc = document?.doc
        let gray = doc?.isGray == true
        let lab = doc?.isLab == true
        let modes = doc.map { doc in
            lab ? ArtHistoryOptions.modes.filter(NativeLayerBlending.labModes.contains)
                : gray
                ? ArtHistoryOptions.modes.filter(NativeLayerBlending.grayModes(for: doc).contains)
                : ArtHistoryOptions.modes
        } ?? ArtHistoryOptions.modes
        let styles = ArtHistoryStyle.allCases
        return OptionCluster([
            OptionDescriptor(
                id: lab ? "paint.artMode.lab" : gray ? "paint.artMode.gray" : "paint.artMode",
                microLabel: "Mode",
                overflowLabel: "Mode",
                kind: .popup(
                    width: 104, items: modes.map(ArtHistoryOptions.name(of:)),
                    get: {
                        modes.firstIndex(of: DrBlendMode(rawValue: UInt32(max(read().blendIndex, 0))))
                            ?? 0
                    },
                    set: { index in
                        guard modes.indices.contains(index) else { return }
                        var options = read()
                        options.blendIndex = Int(modes[index].rawValue)
                        write(options)
                    })),
            OptionDescriptor(
                id: "paint.artStyle", microLabel: "Style", overflowLabel: "Style",
                kind: .popup(
                    width: 128, items: styles.map(\.displayName),
                    get: { styles.firstIndex(of: read().artStyle ?? .tightShort) ?? 0 },
                    set: { index in
                        guard styles.indices.contains(index) else { return }
                        var options = read()
                        options.artStyle = styles[index]
                        write(options)
                    })),
            OptionDescriptor(
                id: "paint.artArea", microLabel: "Area", overflowLabel: "Area",
                kind: .field(
                    width: 52, unit: " px", decimals: 0, min: 0, max: 500,
                    quick: [0, 10, 25, 50, 100, 200, 500],
                    get: { read().artArea ?? ArtHistoryOptions.defaultArea },
                    set: { value in
                        var options = read()
                        options.artArea = value
                        write(options)
                    })),
            OptionDescriptor(
                id: "paint.artTolerance", microLabel: "Tolerance", overflowLabel: "Tolerance",
                kind: .field(
                    width: 48, unit: "%", decimals: 0, min: 0, max: 100,
                    quick: [0] + Self.quickPercent,
                    get: { read().artTolerance ?? 0 },
                    set: { value in
                        var options = read()
                        options.artTolerance = value
                        write(options)
                    })),
        ])
    }

    private func paintClusters(_ tool: EditorTool) -> [OptionCluster] {
        let read: () -> PaintToolOptions = {
            ToolOptionsStore.shared.paintOptions(for: tool) ?? PaintToolOptions()
        }
        let write: (PaintToolOptions) -> Void = {
            ToolOptionsStore.shared.setPaintOptions($0, for: tool)
        }
        // The Blend popup's list: the Grayscale modes on a Gray document (#547 phase 2e), the
        // Lab modes on a Lab document (#547 Lab phase 2e).
        let blendChoices = Self.paintBlendChoices(for: document?.doc)
        let blendChoicesID = document?.doc?.isLab == true ? "paint.blend.lab"
            : document?.doc?.isGray == true ? "paint.blend.gray" : "paint.blend"
        var clusters: [OptionCluster] = [
            OptionCluster([
                OptionDescriptor(
                    id: "paint.preset", overflowLabel: "Preset",
                    kind: .popup(
                        width: 100, items: BrushPreset.popupItems,
                        get: { BrushPreset.matchIndex(of: read()) },
                        set: { [weak self] index in
                            // Row 0 is "Custom" — what shows when the values
                            // match no preset; picking it changes nothing.
                            guard index >= 1,
                                  BrushPreset.builtIns.indices.contains(index - 1)
                            else { return }
                            // End any live field edit FIRST: a field editor
                            // skips value refreshes while active, and its
                            // stale text would commit right back over the
                            // preset at the next blur.
                            self?.view.window?.makeFirstResponder(nil)
                            write(BrushPreset.builtIns[index - 1].applied(to: read()))
                            self?.syncCanvasPaintState()
                        })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "paint.size", microLabel: "Size", overflowLabel: "Size",
                    kind: .field(
                        // The floor is the tool's own: three for the two
                        // healing brushes, whose solve keeps the destination
                        // at every covered pixel touching an uncovered one,
                        // so a footprint two pixels across is all boundary
                        // and heals nothing (EditorTool.minimumBrushSize).
                        width: 56, unit: " px", decimals: 0,
                        min: tool.minimumBrushSize, max: tool.maximumBrushSize,
                        quick: Self.quickPx,
                        get: { read().size },
                        set: { [weak self] value in
                            var options = read()
                            options.size = value
                            write(options)
                            self?.syncCanvasPaintState()
                        })),
            ]),
            OptionCluster([
                // On the two healing brushes hardness SHAPES the footprint —
                // it shrinks the covered area inside the brush circle — and
                // does not fade the heal: the Poisson join is already
                // seamless at the covered set's edge, so there is nothing
                // left to feather (§0.4).
                OptionDescriptor(
                    id: "paint.hardness", microLabel: "Hardness", overflowLabel: "Hardness",
                    kind: .field(
                        width: 48, unit: "%", decimals: 0, min: 0, max: 100,
                        quick: [0] + Self.quickPercent,
                        get: { read().hardness },
                        set: { [weak self] value in
                            var options = read()
                            options.hardness = value
                            write(options)
                            self?.syncCanvasPaintState()
                        })),
            ]),
            OptionCluster([
                // Dodge/burn reads this as its exposure and the two healing
                // brushes as the heal's STRENGTH — both carry their amount in
                // the core op rather than in the stroke's alpha, which is why
                // neither has a usable Flow (below).
                OptionDescriptor(
                    id: "paint.opacity",
                    microLabel: Self.paintAmountLabel(tool),
                    overflowLabel: Self.paintAmountLabel(tool),
                    kind: .field(
                        width: 52, unit: "%", decimals: 0, min: 1, max: 100,
                        quick: Self.quickPercent,
                        get: { read().opacity },
                        set: { [weak self] value in
                            var options = read()
                            options.opacity = value
                            write(options)
                            self?.syncCanvasPaintState()
                        })),
            ]),
        ]
        if tool == .clone || tool == .heal || tool == .spotHeal {
            // Aligned is the Healing Brush's alone (the Clone Stamp re-aims
            // every mouse-down and spot healing has no source point at all);
            // Sample, Ignore Adjustment Layers and the output target belong
            // to all three.
            var healing: [OptionDescriptor] = []
            if tool == .spotHeal {
                // Type, first in the cluster; nil is Content-Aware.
                healing.append(
                    OptionDescriptor(
                        id: "paint.spotHealType", microLabel: "Type", overflowLabel: "Type",
                        kind: .popup(
                            width: 128, items: SpotHealType.allCases.map(\.displayName),
                            get: {
                                SpotHealType.allCases.firstIndex(
                                    of: read().spotHealType ?? .contentAware) ?? 0
                            },
                            set: { [weak self] index in
                                var options = read()
                                let type = SpotHealType.allCases.indices.contains(index)
                                    ? SpotHealType.allCases[index] : .contentAware
                                options.spotHealType = type == .contentAware ? nil : type
                                write(options)
                                self?.syncCanvasPaintState()
                            })))
            }
            if tool == .heal {
                healing.append(
                    OptionDescriptor(
                        id: "paint.aligned", overflowLabel: "Aligned",
                        kind: .checkbox(
                            label: "Aligned",
                            get: { read().aligned },
                            set: { [weak self] value in
                                var options = read()
                                options.aligned = value
                                write(options)
                                self?.syncCanvasPaintState()
                            })))
            }
            healing.append(contentsOf: Self.samplingDescriptors(
                prefix: "paint",
                get: { RetouchSamplingChoice(
                    mode: read().sample, ignoreAdjustments: read().ignoreAdjustments) },
                set: { [weak self] choice in
                    var options = read()
                    options.sample = choice.mode
                    options.ignoreAdjustments = choice.ignoreAdjustments
                    write(options)
                    self?.syncCanvasPaintState()
                }))
            healing.append(
                OptionDescriptor(
                    id: "paint.output", microLabel: "To", overflowLabel: "Retouch to",
                    kind: .popup(
                        width: 116, items: RetouchOutput.allCases.map(\.displayName),
                        get: { read().outputToNewLayer ? 1 : 0 },
                        set: { [weak self] index in
                            var options = read()
                            options.outputToNewLayer = index == 1
                            write(options)
                            self?.syncCanvasPaintState()
                        })))
            healing += Self.retouchGeometryDescriptors(tool: tool, read: read, write: write)
            clusters.append(OptionCluster(healing))
        }
        if tool == .sponge {
            // Desaturate first, as it is the default (`saturate` nil).
            clusters.append(OptionCluster([
                OptionDescriptor(
                    id: "paint.spongeMode", overflowLabel: "Mode",
                    kind: .popup(
                        width: 104, items: ["Desaturate", "Saturate"],
                        get: { read().saturate == true ? 1 : 0 },
                        set: { index in
                            var options = read()
                            options.saturate = index == 1
                            write(options)
                        })),
                // Vibrance, on by default (`vibrance` nil).
                OptionDescriptor(
                    id: "paint.vibrance", overflowLabel: "Vibrance",
                    kind: .checkbox(
                        label: "Vibrance",
                        get: { read().vibrance != false },
                        set: { on in
                            var options = read()
                            options.vibrance = on
                            write(options)
                        })),
            ]))
        }
        if tool == .blurBrush || tool == .sharpenBrush || tool == .smudge {
            // Sample All Layers reads the picture of every visible layer
            // (off by default).
            clusters.append(OptionCluster([
                OptionDescriptor(
                    id: "paint.sampleAllLayers", overflowLabel: "Sample All Layers",
                    kind: .checkbox(
                        label: "Sample All Layers",
                        get: { read().sampleAllLayers == true },
                        set: { on in
                            var options = read()
                            options.sampleAllLayers = on
                            write(options)
                        })),
            ]))
        }
        if tool == .artHistoryBrush {
            clusters.append(artHistoryCluster(read: read, write: write))
        }
        if tool == .dodge {
            clusters.append(OptionCluster([
                OptionDescriptor(
                    id: "paint.mode", overflowLabel: "Mode",
                    kind: .popup(
                        width: 76, items: ["Dodge", "Burn"],
                        get: { read().burn ? 1 : 0 },
                        set: { index in
                            var options = read()
                            options.burn = index == 1
                            write(options)
                        })),
                OptionDescriptor(
                    id: "paint.range", microLabel: "Range", overflowLabel: "Range",
                    kind: .popup(
                        width: 96, items: ["Shadows", "Midtones", "Highlights"],
                        get: { read().rangeIndex },
                        set: { index in
                            var options = read()
                            options.rangeIndex = index
                            write(options)
                        })),
                // #547: Protect Tones moves a pixel's channels together (off by default).
                OptionDescriptor(
                    id: "paint.protectTones", overflowLabel: "Protect Tones",
                    kind: .checkbox(
                        label: "Protect Tones",
                        get: { read().protectTones == true },
                        set: { on in
                            var options = read()
                            options.protectTones = on
                            write(options)
                        })),
            ]))
        }
        clusters.append(contentsOf: [
            OptionCluster([
                OptionDescriptor(
                    id: "paint.flow", microLabel: "Flow", overflowLabel: "Flow",
                    kind: .field(
                        width: 52, unit: "%", decimals: 0, min: 1, max: 100,
                        quick: Self.quickPercent,
                        get: { read().flow },
                        set: { [weak self] value in
                            var options = read()
                            options.flow = value
                            write(options)
                            self?.syncCanvasPaintState()
                        }),
                    // The healing brushes' overlay ALPHA is the solve's own
                    // domain (covered = alpha >= 128, §0.4), so a flowed-down
                    // dab would empty it and the heal would refuse outright.
                    // The stroke always deposits full coverage and Opacity
                    // carries the strength; the canvas pins flow to 1 for
                    // these two tools to match.
                    isEnabled: { tool != .heal && tool != .spotHeal }),
                OptionDescriptor(
                    id: blendChoicesID, overflowLabel: "Blend",
                    kind: .popup(
                        width: 110, items: blendChoices.map { $0.1 },
                        get: { Self.blendListIndex(of: read().blendIndex, in: blendChoices) },
                        set: { index in
                            guard blendChoices.indices.contains(index) else { return }
                            var options = read()
                            options.blendIndex = Int(blendChoices[index].0.rawValue)
                            write(options)
                        }),
                    // Blend applies where a stroke PAINTS layer pixels. The
                    // eraser is its own composite op and dodge a retouch op,
                    // so their popups stay pinned to Normal.
                    isEnabled: { tool == .brush || tool == .clone }),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "paint.spacing", microLabel: "Spacing", overflowLabel: "Spacing",
                    kind: .field(
                        width: 48, unit: "%", decimals: 0, min: 1, max: 200,
                        quick: Self.quickPercent + [150, 200],
                        get: { read().spacing },
                        set: { [weak self] value in
                            var options = read()
                            options.spacing = value
                            write(options)
                            self?.syncCanvasPaintState()
                        })),
                OptionDescriptor(
                    id: "paint.angle", microLabel: "Angle", overflowLabel: "Angle",
                    kind: .field(
                        width: 48, unit: "°", decimals: 0, min: -180, max: 180,
                        quick: [-135, -90, -45, 0, 45, 90, 135, 180],
                        get: { read().angle },
                        set: { [weak self] value in
                            var options = read()
                            options.angle = value
                            write(options)
                            self?.syncCanvasPaintState()
                        })),
                OptionDescriptor(
                    id: "paint.roundness", microLabel: "Round", overflowLabel: "Roundness",
                    kind: .field(
                        width: 48, unit: "%", decimals: 0, min: 1, max: 100,
                        quick: Self.quickPercent,
                        get: { read().roundness },
                        set: { [weak self] value in
                            var options = read()
                            options.roundness = value
                            write(options)
                            self?.syncCanvasPaintState()
                        })),
                OptionDescriptor(
                    id: "paint.smoothing", microLabel: "Smooth", overflowLabel: "Smoothing",
                    kind: .field(
                        width: 48, unit: "%", decimals: 0, min: 0, max: 100,
                        quick: [0] + Self.quickPercent,
                        get: { read().smoothing },
                        set: { [weak self] value in
                            var options = read()
                            options.smoothing = value
                            write(options)
                            self?.syncCanvasPaintState()
                        })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "paint.pressure", overflowLabel: "Pressure size",
                    kind: .checkbox(
                        label: "Pressure size",
                        get: { read().pressureSize },
                        set: { [weak self] value in
                            var options = read()
                            options.pressureSize = value
                            write(options)
                            self?.syncCanvasPaintState()
                        })),
                OptionDescriptor(
                    id: "paint.airbrush", overflowLabel: "Airbrush",
                    kind: .checkbox(
                        label: "Airbrush",
                        get: { read().airbrush },
                        set: { [weak self] value in
                            var options = read()
                            options.airbrush = value
                            write(options)
                            self?.syncCanvasPaintState()
                        })),
            ]),
        ])
        clusters.append(OptionCluster([
            OptionDescriptor(id: "paint.pressureOpacity", overflowLabel: "Pressure opacity",
                kind: .checkbox(label: "Pressure opacity", get: { read().pressureOpacity ?? false },
                    set: { [weak self] value in
                        var options = read(); options.pressureOpacity = value; write(options)
                        self?.syncCanvasPaintState()
                    }))
        ]))
        return clusters
    }

    // MARK: - Remove

    /// The Remove tool: the brush's width, whether the fill samples
    /// every layer or the target's own pixels, and where the result lands.
    /// No tip cluster: the stroke is a hard round, and its coverage is a
    /// Content-Aware Fill hole, not paint.
    private func removeClusters() -> [OptionCluster] {
        let read: () -> PaintToolOptions = {
            ToolOptionsStore.shared.paintOptions(for: .remove) ?? PaintToolOptions()
        }
        let write: (PaintToolOptions) -> Void = {
            ToolOptionsStore.shared.setPaintOptions($0, for: .remove)
        }
        return [
            // Model (the local inpainting model, the default) or Content-Aware.
            OptionCluster([
                OptionDescriptor(
                    id: "remove.mode", microLabel: "Mode", overflowLabel: "Mode",
                    kind: .popup(
                        width: 116, items: ["Model", "Content-Aware"],
                        get: { ToolOptionsStore.shared.removeTool.mode == .model ? 0 : 1 },
                        set: { index in
                            ToolOptionsStore.shared.removeTool.mode = index == 0 ? .model : .contentAware
                        })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "remove.size", microLabel: "Size", overflowLabel: "Size",
                    kind: .field(
                        width: 56, unit: " px", decimals: 0,
                        min: EditorTool.remove.minimumBrushSize,
                        max: EditorTool.remove.maximumBrushSize,
                        quick: Self.quickPx + [100, 200, 400],
                        get: { read().size },
                        set: { [weak self] value in
                            var options = read()
                            options.size = value
                            write(options)
                            self?.syncCanvasPaintState()
                        })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "remove.samplealllayers", overflowLabel: "Sample all layers",
                    kind: .checkbox(
                        label: "Sample all layers",
                        get: { read().sample == .all },
                        set: { value in
                            var options = read()
                            options.sample = value ? .all : .current
                            write(options)
                        })),
                OptionDescriptor(
                    id: "remove.output", microLabel: "To", overflowLabel: "Output",
                    kind: .popup(
                        width: 116, items: ["Current Layer", "New Layer"],
                        get: { read().outputToNewLayer ? 1 : 0 },
                        set: { index in
                            var options = read()
                            options.outputToNewLayer = index == 1
                            write(options)
                        })),
            ]),
            // "Remove after each stroke"; off, strokes are held until ✓
            // (Return) applies them as one step or ✕ (Escape) drops them.
            OptionCluster([
                OptionDescriptor(
                    id: "remove.aftereachstroke", overflowLabel: "Remove after each stroke",
                    kind: .checkbox(
                        label: "Remove after each stroke",
                        get: { ToolOptionsStore.shared.removeTool.removeAfterEachStroke },
                        set: { ToolOptionsStore.shared.removeTool.removeAfterEachStroke = $0 })),
                OptionDescriptor(
                    id: "remove.apply", overflowLabel: "Apply",
                    kind: .button(
                        title: "✓", action: { [weak self] in self?.applyPendingRemove() }),
                    isEnabled: { [weak self] in self?.canvas.removeSession.hasPending ?? false }),
                OptionDescriptor(
                    id: "remove.cancel", overflowLabel: "Cancel",
                    kind: .button(
                        title: "✕", action: { [weak self] in self?.cancelPendingRemove() }),
                    isEnabled: { [weak self] in self?.canvas.removeSession.hasPending ?? false }),
            ]),
        ]
    }

    // MARK: - Patch

    /// The Patch tool: which way the drag runs, and where the texture is
    /// taken from. The outline itself is a gesture, not an option.
    private func patchClusters() -> [OptionCluster] {
        [
            OptionCluster([
                OptionDescriptor(
                    id: "patch.direction", microLabel: "Patch", overflowLabel: "Patch",
                    kind: .popup(
                        width: 116, items: ["Source", "Destination"],
                        get: { min(max(ToolOptionsStore.shared.patch.directionIndex, 0), 1) },
                        set: { ToolOptionsStore.shared.patch.directionIndex = $0 })),
            ]),
            OptionCluster(Self.samplingDescriptors(
                prefix: "patch",
                get: { RetouchSamplingChoice(
                    mode: ToolOptionsStore.shared.patch.sample,
                    ignoreAdjustments: ToolOptionsStore.shared.patch.ignoreAdjustments) },
                set: { choice in
                    ToolOptionsStore.shared.patch.sample = choice.mode
                    ToolOptionsStore.shared.patch.ignoreAdjustments = choice.ignoreAdjustments
                })),
        ]
    }

    // MARK: - Content-Aware Move

    /// The Content-Aware Move tool: Move or Extend, and where the fill
    /// and the heal read from. The outline and the drag are Patch's gesture.
    private func contentAwareMoveClusters() -> [OptionCluster] {
        [
            OptionCluster([
                OptionDescriptor(
                    id: "contentawaremove.mode", microLabel: "Mode", overflowLabel: "Mode",
                    kind: .popup(
                        width: 96, items: ["Move", "Extend"],
                        get: { min(max(ToolOptionsStore.shared.contentAwareMove.modeIndex, 0), 1) },
                        set: { ToolOptionsStore.shared.contentAwareMove.modeIndex = $0 })),
            ]),
            OptionCluster(Self.samplingDescriptors(
                prefix: "contentawaremove",
                get: { RetouchSamplingChoice(
                    mode: ToolOptionsStore.shared.contentAwareMove.sample,
                    ignoreAdjustments: ToolOptionsStore.shared.contentAwareMove.ignoreAdjustments) },
                set: { choice in
                    ToolOptionsStore.shared.contentAwareMove.sample = choice.mode
                    ToolOptionsStore.shared.contentAwareMove.ignoreAdjustments =
                        choice.ignoreAdjustments
                })),
        ]
    }

    /// The two sampling controls every retouch tool shares: WHICH pixels the
    /// gesture reads, and whether adjustment layers above the target are left
    /// out of them.
    ///
    /// One builder, because the three brushes, the Patch tool and
    /// Content-Aware Fill must offer the same vocabulary in the same order —
    /// a tool whose menu said something slightly different would be a tool
    /// whose recorded Action means something slightly different.
    ///
    /// Ignore Adjustment Layers is deliberately shown DISABLED rather than
    /// hidden outside All Layers: it is a provable identity in the other two
    /// modes (they already exclude everything above the target), and a
    /// control that vanished would read as a control that had been lost.
    static func samplingDescriptors(
        prefix: String, get: @escaping () -> RetouchSamplingChoice,
        set: @escaping (RetouchSamplingChoice) -> Void
    ) -> [OptionDescriptor] {
        [
            OptionDescriptor(
                id: "\(prefix).sample", microLabel: "Sample", overflowLabel: "Sample",
                kind: .popup(
                    width: 136, items: RetouchSampling.allCases.map(\.displayName),
                    get: { RetouchSampling.allCases.firstIndex(of: get().mode) ?? 0 },
                    set: { index in
                        var choice = get()
                        choice.mode = RetouchSampling.allCases[
                            min(max(index, 0), RetouchSampling.allCases.count - 1)]
                        set(choice)
                    })),
            OptionDescriptor(
                id: "\(prefix).ignoreadjustments",
                overflowLabel: "Ignore adjustment layers",
                kind: .checkbox(
                    label: "Ignore adjustment layers",
                    get: { get().ignoreAdjustments },
                    set: { value in
                        var choice = get()
                        choice.ignoreAdjustments = value
                        set(choice)
                    }),
                isEnabled: { get().mode == .all }),
        ]
    }

    // MARK: - Red Eye

    private func redEyeClusters() -> [OptionCluster] {
        [
            OptionCluster([
                // 100 % is the shipped default on purpose: the dial REJECTS a
                // red region larger than this fraction of the rectangle's
                // shorter side, and the gesture the app documents is a tight
                // rectangle over one eye. Turn it down when something red got
                // caught by a sloppy rectangle.
                OptionDescriptor(
                    id: "redeye.pupil", microLabel: "Pupil Size", overflowLabel: "Pupil Size",
                    kind: .field(
                        width: 52, unit: "%", decimals: 0, min: 1, max: 100,
                        quick: Self.quickPercent,
                        get: { ToolOptionsStore.shared.redEye.pupilSize },
                        set: { ToolOptionsStore.shared.redEye.pupilSize = $0 })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "redeye.darken", microLabel: "Darken", overflowLabel: "Darken Amount",
                    kind: .field(
                        width: 52, unit: "%", decimals: 0, min: 0, max: 100,
                        quick: [0] + Self.quickPercent,
                        get: { ToolOptionsStore.shared.redEye.darken },
                        set: { ToolOptionsStore.shared.redEye.darken = $0 })),
            ]),
        ]
    }

    // MARK: - Fill

    private func fillClusters() -> [OptionCluster] {
        let modes = Self.toolBlendChoices(for: document?.doc)
        let lab = document?.doc?.isLab == true
        return [
            OptionCluster([
                OptionDescriptor(
                    id: "fill.contents", overflowLabel: "Contents",
                    kind: .popup(
                        width: 104, items: ["Foreground", "Background"],
                        get: { min(max(ToolOptionsStore.shared.fill.contentsIndex, 0), 1) },
                        set: { ToolOptionsStore.shared.fill.contentsIndex = $0 })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "fill.tolerance", microLabel: "Tolerance", overflowLabel: "Tolerance",
                    kind: .field(
                        width: 46, unit: "", decimals: 0, min: 0, max: 255,
                        quick: Self.quickTolerance,
                        get: { ToolOptionsStore.shared.fill.tolerance },
                        set: { ToolOptionsStore.shared.fill.tolerance = $0 })),
                OptionDescriptor(
                    id: "fill.contiguous", overflowLabel: "Contiguous",
                    kind: .checkbox(
                        label: "Contiguous",
                        get: { ToolOptionsStore.shared.fill.contiguous },
                        set: { ToolOptionsStore.shared.fill.contiguous = $0 })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "fill.opacity", microLabel: "Opacity", overflowLabel: "Opacity",
                    kind: .field(
                        width: 52, unit: "%", decimals: 0, min: 1, max: 100,
                        quick: Self.quickPercent,
                        get: { ToolOptionsStore.shared.fill.opacity },
                        set: { ToolOptionsStore.shared.fill.opacity = $0 })),
                OptionDescriptor(
                    id: lab ? "fill.blend.lab" : "fill.blend", microLabel: "Mode",
                    overflowLabel: "Mode",
                    kind: .popup(
                        width: 112, items: modes.map { $0.1 },
                        get: {
                            Self.blendListIndex(
                                of: ToolOptionsStore.shared.fill.blendIndex, in: modes)
                        },
                        set: { index in
                            guard modes.indices.contains(index) else { return }
                            ToolOptionsStore.shared.fill.blendIndex = Int(modes[index].0.rawValue)
                        })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "fill.samplealllayers", overflowLabel: "Sample all layers",
                    kind: .checkbox(
                        label: "Sample all layers",
                        get: { ToolOptionsStore.shared.fill.sampleAllLayers },
                        set: { ToolOptionsStore.shared.fill.sampleAllLayers = $0 })),
            ]),
        ]
    }

    // MARK: - Gradient

    private func gradientClusters() -> [OptionCluster] {
        let modes = Self.toolBlendChoices(for: document?.doc)
        let lab = document?.doc?.isLab == true
        return [
            OptionCluster([
                OptionDescriptor(
                    id: "gradient.preview", overflowLabel: "Gradient",
                    kind: .gradientPreview(get: { [weak self] in
                        self?.gradientToolRamp.stops ?? [(0, .black), (1, .white)]
                    })),
                OptionDescriptor(
                    id: "gradient.stops", overflowLabel: "Stops",
                    kind: .button(title: "Edit…", action: { [weak self] in
                        self?.showGradientStops()
                    })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "gradient.type", overflowLabel: "Type",
                    kind: .segmented(
                        segments: [
                            ("circle.lefthalf.filled", "L", "Linear"),
                            ("smallcircle.filled.circle", "R", "Radial"),
                            ("arrow.trianglehead.2.clockwise.rotate.90", "A", "Angle"),
                            ("rectangle.split.2x1", "M", "Reflected"),
                            ("diamond", "D", "Diamond"),
                        ],
                        get: { min(max(ToolOptionsStore.shared.gradient.typeIndex, 0), 4) },
                        set: { ToolOptionsStore.shared.gradient.typeIndex = $0 },
                        segmentEnabled: nil)),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "gradient.opacity", microLabel: "Opacity", overflowLabel: "Opacity",
                    kind: .field(
                        width: 52, unit: "%", decimals: 0, min: 1, max: 100,
                        quick: Self.quickPercent,
                        get: { ToolOptionsStore.shared.gradient.opacity },
                        set: { ToolOptionsStore.shared.gradient.opacity = $0 })),
                // The Fill tool's Mode popup (`fill.blend`), for the drag.
                OptionDescriptor(
                    id: lab ? "gradient.blend.lab" : "gradient.blend", microLabel: "Mode",
                    overflowLabel: "Mode",
                    kind: .popup(
                        width: 112, items: modes.map { $0.1 },
                        get: {
                            Self.blendListIndex(
                                of: ToolOptionsStore.shared.gradient.blendIndex, in: modes)
                        },
                        set: { index in
                            guard modes.indices.contains(index) else { return }
                            ToolOptionsStore.shared.gradient.blendIndex =
                                Int(modes[index].0.rawValue)
                        })),
                OptionDescriptor(
                    id: "gradient.reverse", overflowLabel: "Reverse",
                    kind: .checkbox(
                        label: "Reverse",
                        get: { ToolOptionsStore.shared.gradient.reverse },
                        set: { ToolOptionsStore.shared.gradient.reverse = $0 })),
                OptionDescriptor(
                    id: "gradient.transparency", overflowLabel: "Transparency",
                    kind: .checkbox(
                        label: "Transparency",
                        get: { ToolOptionsStore.shared.gradient.transparency },
                        set: { ToolOptionsStore.shared.gradient.transparency = $0 })),
            ]),
        ]
    }

    // MARK: - Shapes

    private func shapeClusters(_ tool: EditorTool) -> [OptionCluster] {
        [
            OptionCluster([
                OptionDescriptor(
                    id: "shape.fill", microLabel: "Fill", overflowLabel: "Fill",
                    kind: .swatch(
                        get: { [weak self] in
                            ToolOptionsStore.shared.shape.fillColor(
                                float: self?.document?.doc?.isFloat == true) ?? .clear
                        },
                        set: { [weak self] color in
                            ToolOptionsStore.shared.shape.setFill(color)
                            self?.shapeStyleEdited()
                        },
                        picker: { [weak self] current, commit, systemColors in
                            // #547: a CMYK document picks the fill as ink.
                            self?.presentSwatchPicker(
                                title: "Color Picker (Fill Color)", color: current,
                                commit: commit, systemColors: systemColors,
                                commitInk: { color, ink in
                                    ToolOptionsStore.shared.shape.setFill(color, ink: ink)
                                    self?.shapeStyleEdited()
                                    self?.optionsBar.refreshValues()
                                },
                                ink: ToolOptionsStore.shared.shape.fillInk) ?? false
                        }),
                    // A line is stroke only.
                    isEnabled: { tool != .shapeLine }),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "shape.stroke", microLabel: "Stroke", overflowLabel: "Stroke",
                    kind: .swatch(
                        get: { [weak self] in
                            ToolOptionsStore.shared.shape.strokeColor(
                                float: self?.document?.doc?.isFloat == true) ?? .clear
                        },
                        set: { [weak self] color in
                            ToolOptionsStore.shared.shape.setStroke(color)
                            self?.shapeStyleEdited()
                        },
                        picker: { [weak self] current, commit, systemColors in
                            // #547: a CMYK document picks the stroke as ink.
                            self?.presentSwatchPicker(
                                title: "Color Picker (Stroke Color)", color: current,
                                commit: commit, systemColors: systemColors,
                                commitInk: { color, ink in
                                    ToolOptionsStore.shared.shape.setStroke(color, ink: ink)
                                    self?.shapeStyleEdited()
                                    self?.optionsBar.refreshValues()
                                },
                                ink: ToolOptionsStore.shared.shape.strokeInk) ?? false
                        })),
                OptionDescriptor(
                    id: "shape.weight", overflowLabel: "Stroke weight",
                    kind: .field(
                        width: 50, unit: " px", decimals: 0, min: 0, max: 200,
                        quick: Self.quickPx,
                        get: { ToolOptionsStore.shared.shape.strokeWidth },
                        set: { [weak self] value in
                            ToolOptionsStore.shared.shape.strokeWidth = value
                            self?.shapeStyleEdited()
                        })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "shape.radius", microLabel: "Radius", overflowLabel: "Corner radius",
                    kind: .field(
                        width: 50, unit: " px", decimals: 0, min: 0, max: 500,
                        quick: Self.quickRadius,
                        get: { ToolOptionsStore.shared.shape.radius },
                        set: { [weak self] value in
                            ToolOptionsStore.shared.shape.radius = value
                            self?.shapeStyleEdited()
                        }),
                    // Radius only rounds rectangles.
                    isEnabled: { tool == .shapeRect }),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "shape.cornerstyle", overflowLabel: "Corner style",
                    kind: .popup(
                        width: 84, items: ShapeStrokeJoin.allCases.map { $0.title },
                        get: { ToolOptionsStore.shared.shape.strokeJoin.index },
                        set: { [weak self] index in
                            ToolOptionsStore.shared.shape.strokeJoinIndex = index
                            self?.shapeStyleEdited()
                        })),
                OptionDescriptor(
                    id: "shape.dash", overflowLabel: "Dash pattern",
                    kind: .popup(
                        width: 84, items: ShapeStrokeDash.allCases.map { $0.title },
                        get: { ToolOptionsStore.shared.shape.strokeDash.index },
                        set: { [weak self] index in
                            ToolOptionsStore.shared.shape.strokeDashIndex = index
                            self?.shapeStyleEdited()
                        })),
                OptionDescriptor(
                    id: "shape.alignstroke", overflowLabel: "Align stroke",
                    kind: .popup(
                        width: 84, items: ShapeStrokeAlign.allCases.map { $0.title },
                        get: { ToolOptionsStore.shared.shape.strokeAlign.index },
                        set: { [weak self] index in
                            ToolOptionsStore.shared.shape.strokeAlignIndex = index
                            self?.shapeStyleEdited()
                        }),
                    // A line has no inside or outside: its stroke is centred.
                    isEnabled: { tool != .shapeLine }),
            ]),
        ]
    }

    // MARK: - Text

    private static let textWeights: [(title: String, weight: Int)] = [
        ("Light", 3), ("Regular", 5), ("Medium", 6), ("Semibold", 8), ("Bold", 9),
    ]

    /// The popup row for an NSFontManager weight: the row whose weight is
    /// NEAREST, so a description carrying a weight the popup has no row for
    /// (an agent's `weight: 7`) reads as "Medium" rather than as "Regular"
    /// while the store keeps the exact value until the user picks a row.
    /// Ties go to the lighter row (the first in the table).
    private static func textWeightRow(_ weight: Int) -> Int {
        var best = 1
        var bestDistance = Int.max
        for (index, row) in textWeights.enumerated() where abs(row.weight - weight) < bestDistance {
            best = index
            bestDistance = abs(row.weight - weight)
        }
        return best
    }

    /// A typography toggle as a ONE-cell segmented control: the cell
    /// highlights when `get` returns its index (0) and nothing when it
    /// returns −1, and `set` fires on every click, so a single cell is a
    /// button-style toggle with no new control kind (OptionSegmentedControl).
    /// The setter writes the store only: the bar's onAnyEdit →
    /// `toolOptionsEdited()` rebuilds `canvas.textStyle` from the store
    /// (`currentTextStyle`) and restyles a live session, which is the one
    /// path every text option takes.
    private func textToggle(
        id: String, symbol: String, fallback: String, label: String,
        get: @escaping () -> Bool, toggle: @escaping () -> Void
    ) -> OptionDescriptor {
        OptionDescriptor(
            id: id, overflowLabel: label,
            kind: .segmented(
                segments: [(symbol, fallback, label)],
                get: { get() ? 0 : -1 },
                set: { _ in toggle() },
                segmentEnabled: nil))
    }

    private func textClusters() -> [OptionCluster] {
        let families = NSFontManager.shared.availableFontFamilies.sorted()
        return [
            OptionCluster([
                OptionDescriptor(
                    id: "text.font", overflowLabel: "Font",
                    kind: .popup(
                        width: 150, items: families,
                        get: { [weak self] in
                            guard let self = self else { return 0 }
                            return families.firstIndex(of: self.fontFamily) ?? 0
                        },
                        set: { [weak self] index in
                            guard let self = self, families.indices.contains(index)
                            else { return }
                            self.fontFamily = families[index]
                            ToolOptionsStore.shared.text.family = families[index]
                        })),
                OptionDescriptor(
                    id: "text.weight", overflowLabel: "Weight",
                    kind: .popup(
                        width: 92, items: Self.textWeights.map { $0.title },
                        get: { Self.textWeightRow(ToolOptionsStore.shared.text.weight) },
                        set: { index in
                            guard Self.textWeights.indices.contains(index) else { return }
                            ToolOptionsStore.shared.text.weight = Self.textWeights[index].weight
                        })),
            ]),
            // The style toggles sit right after the face they modify. A
            // family without an italic member previews and commits its
            // regular face for Italic (TextStyle.nsFont) — preview and
            // commit agree by construction, so the toggle stays live.
            OptionCluster([
                textToggle(
                    id: "text.italic", symbol: "italic", fallback: "I", label: "Italic",
                    get: { ToolOptionsStore.shared.text.italic },
                    toggle: { ToolOptionsStore.shared.text.italic.toggle() }),
                textToggle(
                    id: "text.underline", symbol: "underline", fallback: "U", label: "Underline",
                    get: { ToolOptionsStore.shared.text.underline },
                    toggle: { ToolOptionsStore.shared.text.underline.toggle() }),
                textToggle(
                    id: "text.strikethrough", symbol: "strikethrough", fallback: "S",
                    label: "Strikethrough",
                    get: { ToolOptionsStore.shared.text.strikethrough },
                    toggle: { ToolOptionsStore.shared.text.strikethrough.toggle() }),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "text.size", microLabel: "Size", overflowLabel: "Size",
                    kind: .field(
                        width: 52, unit: " pt", decimals: 0, min: 6, max: 500,
                        quick: [9, 10, 12, 14, 18, 24, 36, 48, 72, 96, 144],
                        get: { [weak self] in Double(self?.fontSize ?? 48) },
                        set: { [weak self] value in
                            self?.fontSize = CGFloat(value)
                            ToolOptionsStore.shared.text.size = value
                        })),
                // Tracking is the `.kern` attribute in px (TextToolOptions):
                // it changes line breaks, which is why the session restyles
                // its whole storage on every edit rather than just the
                // typing attributes.
                OptionDescriptor(
                    id: "text.tracking", microLabel: "Track", overflowLabel: "Tracking",
                    kind: .field(
                        width: 46, unit: "", decimals: 0,
                        min: TextToolOptions.trackingRange.lowerBound,
                        max: TextToolOptions.trackingRange.upperBound,
                        quick: [-50, -25, -10, -5, 0, 5, 10, 25, 50],
                        get: { ToolOptionsStore.shared.text.tracking },
                        set: { ToolOptionsStore.shared.text.tracking = $0 })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "text.align", overflowLabel: "Alignment",
                    kind: .segmented(
                        segments: [
                            ("text.alignleft", "L", "Align left"),
                            ("text.aligncenter", "C", "Align center"),
                            ("text.alignright", "R", "Align right"),
                        ],
                        get: { [weak self] in
                            Self.alignmentIndex(self?.textAlignment ?? .left)
                        },
                        set: { [weak self] index in
                            let clamped = min(max(index, 0), 2)
                            self?.textAlignment = Self.alignmentSegmentValues[clamped]
                            ToolOptionsStore.shared.text.alignmentIndex = clamped
                        },
                        segmentEnabled: nil)),
                OptionDescriptor(
                    id: "text.color", overflowLabel: "Color",
                    kind: .swatch(
                        get: { [weak self] in self?.paintColor ?? .black },
                        set: { [weak self] color in self?.setPaintColor(color) },
                        picker: { [weak self] current, commit, systemColors in
                            // #547: a CMYK document picks the text colour as ink, kept as
                            // the foreground's stored ink (`setPaintInk`).
                            self?.presentSwatchPicker(
                                title: "Color Picker (Text Color)", color: current,
                                commit: commit, systemColors: systemColors,
                                commitInk: { color, ink in
                                    self?.setPaintInk(ink, color: color)
                                    self?.optionsBar.refreshValues()
                                },
                                ink: ToolOptionsStore.shared.sharedState.foregroundInk) ?? false
                        })),
            ]),
            OptionCluster([
                // Leading is the line height in pt, 0 = the font's natural
                // height (TextStyle.leading); the quick list is the classic
                // headline spread. A leading tighter than the natural height
                // overlaps lines, and the renderer keeps the ascent that
                // then rises above the block (TextLayer.layout's ink-safe
                // rect).
                OptionDescriptor(
                    id: "text.leading", microLabel: "Leading", overflowLabel: "Leading",
                    kind: .field(
                        width: 52, unit: " pt", decimals: 0,
                        min: TextToolOptions.leadingRange.lowerBound,
                        max: TextToolOptions.leadingRange.upperBound,
                        quick: [0, 12, 14, 18, 24, 36, 48, 60, 72, 96, 144],
                        get: { ToolOptionsStore.shared.text.leading },
                        set: { ToolOptionsStore.shared.text.leading = $0 })),
                // Baseline shift in px, positive raises (`.baselineOffset`).
                OptionDescriptor(
                    id: "text.baseline", microLabel: "Baseline", overflowLabel: "Baseline shift",
                    kind: .field(
                        width: 46, unit: "", decimals: 0,
                        min: TextToolOptions.baselineShiftRange.lowerBound,
                        max: TextToolOptions.baselineShiftRange.upperBound,
                        quick: [-50, -25, -10, -5, 0, 5, 10, 25, 50],
                        get: { ToolOptionsStore.shared.text.baselineShift },
                        set: { ToolOptionsStore.shared.text.baselineShift = $0 })),
            ]),
        ]
    }

    // MARK: - Eyedropper

    private func eyedropperClusters() -> [OptionCluster] {
        [
            OptionCluster([
                OptionDescriptor(
                    id: "sample.size", microLabel: "Sample", overflowLabel: "Sample size",
                    kind: .popup(
                        width: 76, items: ["Point", "3 × 3", "5 × 5"],
                        get: { ToolOptionsStore.shared.sample.sampleSizeIndex },
                        set: { ToolOptionsStore.shared.sample.sampleSizeIndex = $0 })),
                OptionDescriptor(
                    id: "sample.from", microLabel: "From", overflowLabel: "Sample from",
                    kind: .popup(
                        width: 110, items: ["Current layer", "All layers"],
                        get: { ToolOptionsStore.shared.sample.fromIndex },
                        set: { ToolOptionsStore.shared.sample.fromIndex = $0 })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "sample.ring", overflowLabel: "Show sampling ring",
                    kind: .checkbox(
                        label: "Show sampling ring",
                        get: { ToolOptionsStore.shared.sample.showRing },
                        set: { ToolOptionsStore.shared.sample.showRing = $0 })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "sample.picked", microLabel: "Picked", overflowLabel: "Picked",
                    kind: .display(
                        width: 84, get: { [weak self] in self?.lastSampleText ?? "—" })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "sample.copy", overflowLabel: "Copy on pick",
                    kind: .checkbox(
                        label: "Copy on pick",
                        get: { ToolOptionsStore.shared.sample.copyOnPick },
                        set: { ToolOptionsStore.shared.sample.copyOnPick = $0 })),
                OptionDescriptor(
                    id: "sample.recent", microLabel: "Recent", overflowLabel: "Recent swatches",
                    kind: .popup(
                        width: 96, items: RecentSwatches.titles,
                        get: { -1 },
                        set: { [weak self] index in self?.chooseRecentSwatch(index) }),
                    isEnabled: { !RecentSwatches.colors.isEmpty }),
            ]),
        ]
    }

    // MARK: - View (zoom / hand)

    private func viewClusters() -> [OptionCluster] {
        [
            OptionCluster([
                OptionDescriptor(
                    id: "view.fit", overflowLabel: "Fit",
                    kind: .button(title: "Fit", action: { [weak self] in self?.zoomToFit() })),
                OptionDescriptor(
                    id: "view.actual", overflowLabel: "100%",
                    kind: .button(title: "100%", action: { [weak self] in self?.zoomActual() })),
                OptionDescriptor(
                    id: "view.fill", overflowLabel: "Fill",
                    kind: .button(title: "Fill", action: { [weak self] in self?.zoomToFill() })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "view.zoom", microLabel: "Zoom", overflowLabel: "Zoom",
                    kind: .field(
                        width: 56, unit: "%", decimals: 0, min: 2, max: 3200,
                        quick: [25, 50, 100, 200, 400, 800, 1600, 3200],
                        get: { [weak self] in self?.zoomPercent ?? 100 },
                        set: { [weak self] in self?.zoomPercent = $0 })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "view.scrubby", overflowLabel: "Scrubby zoom",
                    kind: .checkbox(
                        label: "Scrubby zoom",
                        get: { ToolOptionsStore.shared.view.scrubbyZoom },
                        set: { [weak self] value in
                            ToolOptionsStore.shared.view.scrubbyZoom = value
                            self?.syncCanvasPaintState()
                        })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "view.rulers", overflowLabel: "Rulers",
                    kind: .checkbox(
                        label: "Rulers",
                        get: { ToolOptionsStore.shared.view.rulers },
                        set: { [weak self] value in
                            ToolOptionsStore.shared.view.rulers = value
                            // The broadcast relayouts every window, this
                            // one included (EditorViewController+Rulers).
                            self?.viewChromeChanged()
                        })),
                OptionDescriptor(
                    id: "view.guides", overflowLabel: "Guides",
                    kind: .checkbox(
                        label: "Guides",
                        get: { ToolOptionsStore.shared.view.guides },
                        set: { [weak self] value in
                            ToolOptionsStore.shared.view.guides = value
                            self?.viewChromeChanged()
                        })),
                OptionDescriptor(
                    id: "view.pixelgrid", overflowLabel: "Pixel grid",
                    kind: .popup(
                        width: 76, items: ["Auto", "On", "Off"],
                        get: { ToolOptionsStore.shared.view.pixelGridIndex },
                        set: { [weak self] index in
                            ToolOptionsStore.shared.view.pixelGridIndex = index
                            self?.viewChromeChanged()
                        })),
            ]),
        ]
    }

    // MARK: - Free Transform takeover

    private func transformClusters() -> [OptionCluster] {
        [
            OptionCluster([
                OptionDescriptor(
                    id: "transform.angle", microLabel: "Angle", overflowLabel: "Angle",
                    kind: .field(
                        width: 56, unit: "°", decimals: 2, min: -360, max: 360,
                        quick: [-90, -45, -30, -15, 0, 15, 30, 45, 90, 180],
                        get: { [weak self] in self?.transformDegrees ?? 0 },
                        set: { [weak self] in self?.transformDegrees = $0 })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "transform.scalex", microLabel: "Scale X", overflowLabel: "Scale X",
                    kind: .field(
                        width: 60, unit: "%", decimals: 2,
                        min: -Self.maxScalePercent, max: Self.maxScalePercent,
                        quick: [25, 50, 75, 100, 150, 200],
                        get: { [weak self] in self?.transformScaleXPercent ?? 100 },
                        set: { [weak self] in self?.transformScaleXPercent = $0 })),
                OptionDescriptor(
                    id: "transform.scaley", microLabel: "Y", overflowLabel: "Scale Y",
                    kind: .field(
                        width: 60, unit: "%", decimals: 2,
                        min: -Self.maxScalePercent, max: Self.maxScalePercent,
                        quick: [25, 50, 75, 100, 150, 200],
                        get: { [weak self] in self?.transformScaleYPercent ?? 100 },
                        set: { [weak self] in self?.transformScaleYPercent = $0 })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "transform.w", microLabel: "W", overflowLabel: "Width",
                    kind: .field(
                        width: 58, unit: "", decimals: 0, min: 1, max: 100_000_000,
                        quick: [],
                        get: { [weak self] in self?.transformWidthPixels ?? 0 },
                        set: { [weak self] in self?.transformWidthPixels = $0 })),
                OptionDescriptor(
                    id: "transform.h", microLabel: "H", overflowLabel: "Height",
                    kind: .field(
                        width: 58, unit: "", decimals: 0, min: 1, max: 100_000_000,
                        quick: [],
                        get: { [weak self] in self?.transformHeightPixels ?? 0 },
                        set: { [weak self] in self?.transformHeightPixels = $0 })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "transform.sampler", microLabel: "Sampler", overflowLabel: "Sampler",
                    kind: .popup(
                        width: 170, items: Self.transformSamplers.map { $0.title },
                        get: { [weak self] in self?.transformSamplerListIndex ?? 0 },
                        set: { [weak self] in self?.transformSamplerListIndex = $0 })),
            ]),
        ]
    }
}
