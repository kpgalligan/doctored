import AppKit

// The ⌥⌫ / ⌘⌫ fill chords' actions are in `EditorViewController+ColorPair.swift`, one body
// `fillWithPair`; Fill and Stroke work at 8, 16 and 32-bit float outside Quick Mask.
//
// The 8-bit fill path: `FillContents` / `FillSettings` are the vocabulary, `applyFill` the ONE
// commit (opacity 0…100 in the sheet, divided to the core's 0…1 there; a coverage target fills with
// `PlaneAlgebra.fillColor` through `PlaneScratchOp.fillSelection`: the colour's 30/59/11 luma
// rounded to a code on a colour plane (measured against Photoshop), and its Rec. 709 gray on
// a mask or alpha channel), `fill_selection` the MCP twin and the recorded step (authored colour).
//
// Edit ▸ Stroke… (no key equivalent, needs a selection): `StrokeLocation` / `StrokeSettings`
// / `StrokeDialogOptions` (colour not persisted; the well opens on the foreground), `applyStroke`
// computes the band in CANVAS space from the capture's selection (`StrokeSettings.band` →
// `dr_selection_stroke`) and fills it through the same private commit as `applyFill`
// (`commitSelectionPaint`; `OperationCapture.applyingScratch(_:mask:)` on a coverage target),
// `stroke_selection` the MCP twin sharing `fill_selection`'s argument parsing and commit. When
// nothing is selected on a LAYER target, Stroke… outlines the layer's own alpha edge in LAYER
// space, beyond the canvas included (`StrokeSettings.outline` answers a `StrokeOutline`,
// `.selection(coverage)` or `.layerEdge`; `stroke_selection` answers `outline:
// "selection" | "layer_edge"`): `commitLayerEdgeStroke` / the MCP branch call
// `RasterDocument.strokingLayerEdge` (`dr_doc_stroke_layer_edge`, admitted through
// `layerEdgeStrokeScratch`), which grows the layer to cover the band — past the canvas if need be —
// unless Preserve Transparency or a transparency lock keeps its alpha;
// `StrokeSettings.layerHasOutline` (scratch ≠ 0) is the `emptyLayer` check, and the MCP reply
// shares `selectionPaintReply` with the fill commit. A selection's band that reaches past a trimmed
// or offset layer's pixels grows it through `expandingRetouchDestination` first
// (`OperationCapture.paintDestination`; not under Preserve Transparency or a transparency lock),
// and Fill… and `fill_selection` grow it the same way — with nothing selected the
// whole canvas is the fill's reach. The
// retained-source and text-layer gates are the shared ones (`OperationScope.refusal`,
// `applyRasterizingEdit`), covered by the jobs probe's "stroke layer edge" case.

/// Edit ▸ Fill…'s model: what the fill is made of and how it lands.
/// The sheet (`FillSheetController`), the four ⌥⌫ / ⌘⌫ items, the
/// `fill_selection` MCP tool and its recorded Action step all speak these
/// values; the commit is `EditorViewController.applyFill`
/// (EditorViewController+FillStroke.swift) and the pixel work is the core's
/// `dr_doc_fill_selection`. Edit ▸ Stroke… is `StrokeLocation`
/// and `StrokeSettings` below: the selection becomes a band
/// (`dr_selection_stroke`), and the band is filled through the same commit;
/// with nothing selected on a layer the core strokes the layer's own edge in
/// layer space (`dr_doc_stroke_layer_edge`).

/// The Fill dialog's Contents popup, in its menu order (raw values persist in
/// `FillDialogOptions.contents`, so a new case goes last). Content-Aware and
/// History are deliberately absent: Content-Aware Fill has its own command,
/// and filling from a history state is not built (the History Brush tool
/// paints from one). Pattern fills a layer's pixels only — a mask, colour
/// plane, alpha channel or the Quick Mask holds one gray per pixel, so the
/// sheet omits it there (`offered`).
enum FillContents: Int, CaseIterable {
    case foreground, background, color, black, gray50, white, pattern

    var title: String {
        switch self {
        case .foreground: return "Foreground Color"
        case .background: return "Background Color"
        case .color: return "Color…"
        case .black: return "Black"
        case .gray50: return "50% Gray"
        case .white: return "White"
        case .pattern: return "Pattern"
        }
    }

    /// The Contents rows a target takes: every case on a layer, all but
    /// Pattern on a coverage target or in Quick Mask.
    static func offered(coverage: Bool) -> [FillContents] {
        allCases.filter { $0 != .pattern || !coverage }
    }

    /// The colour this choice fills with. The swatches and a picked Color…
    /// are AUTHORED colours and pass through as they are (the commit
    /// converts them once, `colorBytes`); Black, 50% Gray and White are the
    /// DOCUMENT's own constants 0 / 128 / 255, so they are built in `space`
    /// and convert nowhere. nil for Color… with nothing picked, and for
    /// Pattern, which fills with `FillSettings.pattern`'s tile.
    func color(
        foreground: NSColor, background: NSColor, custom: NSColor?, in space: NSColorSpace
    ) -> NSColor? {
        let constant: (CGFloat) -> NSColor = {
            NSColor(colorSpace: space, components: [$0, $0, $0, 1], count: 4)
        }
        switch self {
        case .foreground: return foreground
        case .background: return background
        case .color: return custom
        case .black: return constant(0)
        case .gray50: return constant(128.0 / 255.0)
        case .white: return constant(1)
        case .pattern: return nil
        }
    }
}

/// One fill request, from the sheet or from a ⌥⌫ / ⌘⌫ item.
struct FillSettings {
    var contents: FillContents
    /// The Color… choice; ignored by every other `contents`.
    var customColor: NSColor? = nil
    /// Pattern's tile, resolved when the sheet applied (a built-in's
    /// `{preset}` or the library pattern's custom tile, so the recorded step
    /// never depends on the library); ignored by every other `contents`.
    var pattern: PatternRef? = nil
    var mode: DrBlendMode
    /// 0…100, the sheet's unit. The core takes 0…1; `applyFill` divides.
    var opacity: Double
    /// Keeps the layer's alpha exactly; on a semi-transparent pixel the
    /// colour mixes in at the fill's own opacity × coverage, not
    /// boosted by the kept alpha. Lock Transparency does the
    /// same. Meaningless on a coverage target (a mask, a colour plane, an
    /// alpha channel), where the commit forces it off.
    var preserveTransparency: Bool

    /// Why a fill changed nothing on a layer whose alpha was kept — by
    /// Preserve Transparency or a transparency lock — shared by the alert
    /// and the MCP no-op reply.
    static let transparentNoOp = "Nothing was filled: the layer's transparency is preserved, "
        + "and no pixel in the selected area could change (it is transparent, "
        + "or already the fill colour)."

    /// Why a Hue, Saturation, Color or Luminosity fill is refused on a
    /// mask, colour plane or alpha channel (`DrBlendMode.grayDegenerateModes`).
    static let coverageBlendRefusal = "Hue, Saturation, Color and Luminosity need colour; "
        + "a mask, colour plane, alpha channel or the Quick Mask holds one gray value per pixel. "
        + "Choose another mode, or target the layer's pixels."

    /// Why `fill_selection` refuses a layer, a target or Preserve
    /// Transparency in Quick Mask: it fills the Quick Mask itself.
    static let quickMaskArguments = "In Quick Mask, fill_selection fills the Quick Mask "
        + "coverage (target \"quick_mask\"). Omit layer and preserve_transparency and pass "
        + "no other target, or exit Quick Mask (set_quick_mask) to fill layer pixels, a mask "
        + "or a channel."

    /// Why Pattern contents are refused on a coverage target or the Quick Mask.
    static let patternCoverageRefusal = "Pattern fills a layer's pixels; a mask, colour plane, "
        + "alpha channel or the Quick Mask holds one gray value per pixel. Target the layer, or "
        + "fill with a colour."

    /// Why a Quick Mask fill opened through Fill… was not applied: the Quick
    /// Mask or its coverage changed while the sheet was open.
    static let quickMaskChanged = "The Quick Mask changed while Fill… was open, so nothing "
        + "was filled. Choose Edit ▸ Fill… again to fill the current Quick Mask."
}

/// Edit ▸ Stroke…'s Location radios.
enum StrokeLocation: Int, CaseIterable {
    case inside, center, outside

    var title: String {
        switch self {
        case .inside: return "Inside"
        case .center: return "Center"
        case .outside: return "Outside"
        }
    }

    /// The `stroke_selection` MCP / Action spelling.
    var agentName: String {
        switch self {
        case .inside: return "inside"
        case .center: return "center"
        case .outside: return "outside"
        }
    }

    var dr: DrStrokeLocation {
        switch self {
        case .inside: return DR_STROKE_INSIDE
        case .center: return DR_STROKE_CENTER
        case .outside: return DR_STROKE_OUTSIDE
        }
    }

    init?(agentName: String) {
        guard let match = Self.allCases.first(where: {
            $0.agentName.caseInsensitiveCompare(agentName) == .orderedSame
        }) else { return nil }
        self = match
    }
}

/// What Edit ▸ Stroke… outlines: the captured selection's
/// coverage, or — with nothing selected on a layer — the layer's own alpha
/// edge, which the core strokes in layer space.
enum StrokeOutline: Equatable {
    case selection([UInt8])
    case layerEdge

    /// The `stroke_selection` reply's `outline` spelling.
    var agentName: String {
        switch self {
        case .selection: return "selection"
        case .layerEdge: return "layer_edge"
        }
    }
}

/// One Edit ▸ Stroke… request: the selection's band, filled
/// with one colour through Fill's commit, or with nothing selected the
/// layer's own edge. 8 bits here; 16 bits and 32-bit float through
/// `PrecisionFillStroke`, a float colour plane included.
struct StrokeSettings {
    /// Pixels, `widthRange`.
    var width: Double
    var color: NSColor
    var location: StrokeLocation
    var mode: DrBlendMode
    /// 0…100, the sheet's unit, as in `FillSettings`.
    var opacity: Double
    /// Meaningless on a coverage target, where the commit forces it off.
    var preserveTransparency: Bool

    static let widthRange: ClosedRange<Double> = 1...250

    static let noSelection = "Make a selection before stroking a mask, colour plane or "
        + "alpha channel. With nothing selected, Stroke… outlines a layer's own pixels."

    static let emptyBand = "Nothing to stroke: the band lies entirely outside the canvas "
        + "(an Outside stroke of a selection that covers the whole canvas). "
        + "Choose Inside or Center."

    /// Nothing is selected, so the stroke outlines the layer's own pixels,
    /// and the layer has none.
    static let emptyLayer = "Nothing to stroke: nothing is selected and the layer has no "
        + "pixels to outline."

    static let transparentNoOp = "Nothing was stroked: the layer's transparency is preserved, "
        + "and no pixel under the stroke could change (it is transparent, "
        + "or already the stroke colour)."

    /// Why a stroke with a non-empty band changed nothing when neither
    /// Preserve Transparency nor a transparency lock applies. A band that
    /// reaches past a trimmed or offset layer's pixels grows the layer first
    /// (`OperationCapture.paintDestination`; the layer edge grows in the
    /// core), so the only way
    /// left is a colour, opacity and mode that leave every pixel under the
    /// band as it was.
    static let outsideLayerNoOp = "Nothing was stroked: the stroke's colour, opacity and "
        + "blend mode leave every pixel under it unchanged."

    /// What Stroke… outlines: the captured selection; with
    /// nothing selected on a LAYER target, the layer's own alpha edge (the
    /// mask excluded), stroked in layer space by the core so an oversized
    /// layer's edge beyond the canvas is outlined too; nil otherwise, which
    /// `noSelection` names.
    static func outline(of capture: OperationCapture) -> StrokeOutline? {
        if let selection = capture.selection { return .selection(selection) }
        guard capture.target == .layer else { return nil }
        return .layerEdge
    }

    /// Whether a `.layerEdge` outline has anything to stroke: the layer
    /// holds at least one pixel with alpha > 0 (the core's scratch answers
    /// 0 for none). `emptyLayer` names the other answer.
    static func layerHasOutline(_ capture: OperationCapture) -> Bool {
        capture.doc.layerEdgeStrokeScratch(capture.layerIndex, widthPx: widthRange.lowerBound) != 0
    }

    /// The canvas-sized band `selection` strokes to, or nil when the core
    /// refuses the size, width or location.
    static func band(
        of selection: [UInt8], canvasWidth: Int, canvasHeight: Int, widthPx: Double,
        location: StrokeLocation
    ) -> [UInt8]? {
        var band = selection
        guard RasterSelection.strokeMask(
            &band, width: canvasWidth, height: canvasHeight, widthPx: widthPx,
            location: location.dr)
        else { return nil }
        return band
    }
}

extension OperationCapture {
    /// The document a LAYER Fill… or Stroke… paints: the capture's
    /// own, or — when `mask` (canvas-sized coverage; nil for the whole
    /// canvas, Fill… with nothing selected) reaches a canvas pixel outside
    /// the layer's pixel rect and the layer's alpha is free to change (no
    /// Preserve Transparency, no transparency lock) — the layer grown to its
    /// union with the canvas (`expandingRetouchDestination`), so a fill or an
    /// Outside or Center stroke of a trimmed layer lands in full in the same
    /// undo step. Coverage targets are never grown. A byte document only, as
    /// this commit is; 16 and 32 bits grow in `PrecisionFillStroke`.
    func paintDestination(mask: [UInt8]?, preserve: Bool) throws -> RasterDocument {
        let index = layerIndex
        guard target == .layer, !preserve, doc.transparencyLockOwner(index) == nil,
              let info = doc.layerInfo(index)
        else { return doc }
        let left = info.offsetX, top = info.offsetY
        let right = info.offsetX + info.width, bottom = info.offsetY + info.height
        guard let mask else {
            let covers = left <= 0 && top <= 0 && right >= doc.width && bottom >= doc.height
            return covers ? doc : try doc.expandingRetouchDestination(index)
        }
        guard mask.count == doc.width * doc.height else { return doc }
        var leaves = false
        rows: for y in 0..<doc.height {
            let row = y * doc.width
            if y < top || y >= bottom {
                for x in 0..<doc.width where mask[row + x] != 0 {
                    leaves = true
                    break rows
                }
                continue
            }
            for x in 0..<min(max(left, 0), doc.width) where mask[row + x] != 0 {
                leaves = true
                break rows
            }
            for x in max(right, 0)..<max(doc.width, max(right, 0)) where mask[row + x] != 0 {
                leaves = true
                break rows
            }
        }
        guard leaves else { return doc }
        return try doc.expandingRetouchDestination(index)
    }
}
