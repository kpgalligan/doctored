import AppKit

/// Tools offered by the editor UI, shared by EditorViewController (menus,
/// rail, options bar, status) and ImageCanvasView (mouse and key routing).
/// Raw values are a stable order, not a rail position — the rail groups
/// tools into slots (see `railGroups`).
enum EditorTool: Int, CaseIterable {
    case select = 0
    case ellipseSelect
    case lasso
    case wand
    case subject
    case move
    case brush
    case eraser
    case fill
    case gradient
    case text
    case eyedropper
    case crop
    case clone
    case dodge
    // The retouching group: the two healing brushes, the Patch tool and Red
    // Eye. Appended rather than slotted beside `clone` because the raw value
    // is a stable order (a `.rz`-independent but persisted-in-defaults
    // number); the rail puts them where they belong (see `railGroups`).
    case heal
    case spotHeal
    case patch
    case redEye
    case shapeRect
    case shapeEllipse
    case shapeLine
    case zoom
    case hand
    case freehandLasso
    case quickSelect
    case magneticLasso
    // Pins up to ten Info-panel readouts; shares the Eyedropper's slot.
    case colorSampler
    // Measures a distance and an angle; shares the Eyedropper's slot.
    case ruler
    // Crops to a quadrilateral, squaring it up; shares the Crop slot.
    case perspectiveCrop
    // The focus, saturation and history brushes. Appended for the same
    // reason as the healing group; `railGroups` places them.
    case blurBrush
    case sharpenBrush
    case smudge
    case sponge
    case historyBrush
    // Turns a dragged rectangle or lasso into the object inside it.
    // Appended (a stable order); `railGroups` places it beside Quick Selection.
    case objectSelect
    // Draws and edits Bézier paths; its own slot below the shapes.
    case pen
    // Paints over an object and fills the stroke content-aware.
    // Appended (a stable order); `railGroups` places it in the healing slot.
    case remove
    // Moves an outlined region and fills where it was, or extends it.
    // Appended (a stable order); `railGroups` places it beside Patch.
    case contentAwareMove
    // Paints the History Brush's source as stylized strokes; shares its slot.
    case artHistoryBrush
    // The Brush's stroke with an aliased tip; shares the Brush's slot.
    case pencil

    /// The tool whose stroke this one IS: its targets, refusals, blend list,
    /// commit, MCP tool and recording. The Pencil strokes as the Brush (with
    /// `aliasedTip`), so every gate and latch past mouse-down reads this
    /// rather than the tool itself; every other tool is its own.
    var strokeFamily: EditorTool { self == .pencil ? .brush : self }

    /// True for the tool whose stroke uses the Pencil's aliased tip
    /// (`BrushTip.aliased`): hard, every pixel painted fully or not at all.
    var aliasedTip: Bool { self == .pencil }

    /// The status bar's (and tool menu's) name for the tool.
    var displayName: String {
        switch self {
        case .select: return "Rectangle Select"
        case .ellipseSelect: return "Ellipse Select"
        case .lasso: return "Polygonal Lasso"
        case .wand: return "Magic Wand"
        case .subject: return "Subject Select"
        case .move: return "Move"
        case .brush: return "Brush"
        case .eraser: return "Eraser"
        case .fill: return "Fill"
        case .gradient: return "Gradient"
        case .text: return "Text"
        case .eyedropper: return "Eyedropper"
        case .crop: return "Crop"
        case .clone: return "Clone Stamp"
        case .dodge: return "Dodge / Burn"
        case .heal: return "Healing Brush"
        case .spotHeal: return "Spot Healing Brush"
        case .patch: return "Patch"
        case .redEye: return "Red Eye"
        case .shapeRect: return "Rectangle"
        case .shapeEllipse: return "Ellipse"
        case .shapeLine: return "Line"
        case .zoom: return "Zoom"
        case .hand: return "Hand"
        case .freehandLasso: return "Freehand Lasso"
        case .quickSelect: return "Quick Selection"
        case .magneticLasso: return "Magnetic Lasso"
        case .colorSampler: return "Color Sampler"
        case .ruler: return "Ruler"
        case .perspectiveCrop: return "Perspective Crop"
        case .blurBrush: return "Blur"
        case .sharpenBrush: return "Sharpen"
        case .smudge: return "Smudge"
        case .sponge: return "Sponge"
        case .historyBrush: return "History Brush"
        case .objectSelect: return "Object Selection"
        case .pen: return "Pen"
        case .remove: return "Remove"
        case .contentAwareMove: return "Content-Aware Move"
        case .artHistoryBrush: return "Art History Brush"
        case .pencil: return "Pencil"
        }
    }

    /// SF Symbol drawn in the rail and in a group's dropdown. The design's
    /// `highlight.rectangle` and `stamp` don't resolve on macOS 15, hence
    /// the marquee-dash and clone-squares stand-ins.
    var symbol: String {
        switch self {
        case .select: return "rectangle.dashed"
        case .ellipseSelect: return "circle.dashed"
        case .lasso: return "lasso"
        case .wand: return "wand.and.stars"
        case .subject: return "person.and.background.dotted"
        case .move: return "arrow.up.and.down.and.arrow.left.and.right"
        case .brush: return "paintbrush.pointed"
        case .eraser: return "eraser"
        case .fill: return "drop.fill"
        case .gradient: return "circle.lefthalf.filled"
        case .text: return "textformat"
        case .eyedropper: return "eyedropper"
        case .crop: return "crop"
        case .clone: return "square.fill.on.square"
        case .dodge: return "circle.righthalf.filled"
        case .heal: return "bandage"
        case .spotHeal: return "bandage.fill"
        case .patch: return "lasso.badge.sparkles"
        case .redEye: return "eye.trianglebadge.exclamationmark"
        case .shapeRect: return "rectangle"
        case .shapeEllipse: return "circle"
        case .shapeLine: return "line.diagonal"
        case .zoom: return "magnifyingglass"
        case .hand: return "hand.raised"
        case .freehandLasso: return "lasso"
        case .quickSelect: return "paintbrush.pointed.fill"
        case .magneticLasso: return "point.topleft.down.curvedto.point.bottomright.up"
        case .colorSampler: return "eyedropper.halffull"
        case .ruler: return "ruler"
        case .perspectiveCrop: return "perspective"
        case .blurBrush: return "drop"
        case .sharpenBrush: return "triangle"
        case .smudge: return "hand.point.up.left"
        case .sponge: return "circle.hexagongrid"
        case .historyBrush: return "clock.arrow.circlepath"
        case .objectSelect: return "viewfinder"
        case .pen: return "pencil.tip"
        case .remove: return "eraser.line.dashed"
        case .contentAwareMove: return "arrow.up.and.down.and.sparkles"
        case .artHistoryBrush: return "scribble.variable"
        case .pencil: return "pencil"
        }
    }

    /// Drawn in the rail when the symbol is unavailable.
    var fallbackGlyph: String {
        switch self {
        case .select: return "S"
        case .ellipseSelect: return "O"
        case .lasso: return "L"
        case .wand: return "W"
        // S, O, L and W are taken by the rest of this tool's rail group,
        // where a dropdown draws them side by side, so Subject takes the
        // next distinctive letter of its own name.
        case .subject: return "U"
        case .move: return "M"
        case .brush: return "B"
        case .eraser: return "E"
        case .fill: return "K"
        case .gradient: return "G"
        case .text: return "T"
        case .eyedropper: return "I"
        case .crop: return "C"
        case .clone: return "J"
        case .dodge: return "D"
        // H (Hand) and S (Rectangle Select) are taken, so the two healing
        // brushes take the next distinctive letter of their own names —
        // bAndage and spot healiNg — the way Subject took U.
        case .heal: return "A"
        case .spotHeal: return "N"
        case .patch: return "P"
        case .redEye: return "Y"
        case .shapeRect: return "R"
        case .shapeEllipse: return "E"
        case .shapeLine: return "/"
        case .zoom: return "Z"
        case .hand: return "H"
        case .freehandLasso: return "F"
        case .quickSelect: return "Q"
        // L is the Polygonal Lasso's glyph and M the Move tool's, so the
        // Magnetic Lasso takes an unused letter.
        case .magneticLasso: return "X"
        // The Eyedropper's I, the slot's shared key.
        case .colorSampler: return "I"
        case .ruler: return "I"
        case .perspectiveCrop: return "C"
        // Distinct within the toning and focus group, whose dropdown draws
        // them beside Dodge's D: sPonge and sMudge leave S to Sharpen.
        case .blurBrush: return "B"
        case .sharpenBrush: return "S"
        case .smudge: return "M"
        case .sponge: return "P"
        // Y: B, E and J are its group's.
        case .historyBrush: return "Y"
        // oBject: O, S, L, F, X, W, Q and U are its group's.
        case .objectSelect: return "B"
        // Alone in its slot, so Patch's and Sponge's P is free here.
        case .pen: return "P"
        // reMove: R is free in the healing group, whose dropdown draws it
        // beside N, A, P and Y.
        case .remove: return "R"
        // content-aware moVe: M is the Move tool's, and V is free in the
        // healing group.
        case .contentAwareMove: return "V"
        // Art: H is the Hand tool's; A is free in the paint group.
        case .artHistoryBrush: return "A"
        // P is the Pen's and Patch's; peNcil's N is free in the paint group.
        case .pencil: return "N"
        }
    }

    /// The editor action that selects this tool — sent up the responder
    /// chain by the rail and its dropdowns, the same selector the Tools
    /// menu uses.
    var action: Selector {
        switch self {
        case .select: return #selector(EditorViewController.selectSelectTool(_:))
        case .ellipseSelect: return #selector(EditorViewController.selectEllipseTool(_:))
        case .lasso: return #selector(EditorViewController.selectLassoTool(_:))
        case .wand: return #selector(EditorViewController.selectWandTool(_:))
        case .subject: return #selector(EditorViewController.selectSubjectTool(_:))
        case .move: return #selector(EditorViewController.selectMoveTool(_:))
        case .brush: return #selector(EditorViewController.selectBrushTool(_:))
        case .eraser: return #selector(EditorViewController.selectEraserTool(_:))
        case .fill: return #selector(EditorViewController.selectFillTool(_:))
        case .gradient: return #selector(EditorViewController.selectGradientTool(_:))
        case .text: return #selector(EditorViewController.selectTextTool(_:))
        case .eyedropper: return #selector(EditorViewController.selectEyedropperTool(_:))
        case .crop: return #selector(EditorViewController.selectCropTool(_:))
        case .clone: return #selector(EditorViewController.selectCloneTool(_:))
        case .dodge: return #selector(EditorViewController.selectDodgeTool(_:))
        case .heal: return #selector(EditorViewController.selectHealTool(_:))
        case .spotHeal: return #selector(EditorViewController.selectSpotHealTool(_:))
        case .patch: return #selector(EditorViewController.selectPatchTool(_:))
        case .redEye: return #selector(EditorViewController.selectRedEyeTool(_:))
        case .shapeRect: return #selector(EditorViewController.selectShapeRectTool(_:))
        case .shapeEllipse: return #selector(EditorViewController.selectShapeEllipseTool(_:))
        case .shapeLine: return #selector(EditorViewController.selectShapeLineTool(_:))
        case .zoom: return #selector(EditorViewController.selectZoomTool(_:))
        case .hand: return #selector(EditorViewController.selectHandTool(_:))
        case .freehandLasso: return #selector(EditorViewController.selectFreehandLassoTool(_:))
        case .quickSelect: return #selector(EditorViewController.selectQuickSelectTool(_:))
        case .magneticLasso: return #selector(EditorViewController.selectMagneticLassoTool(_:))
        case .colorSampler: return #selector(EditorViewController.selectColorSamplerTool(_:))
        case .ruler: return #selector(EditorViewController.selectRulerTool(_:))
        case .perspectiveCrop: return #selector(EditorViewController.selectPerspectiveCropTool(_:))
        case .blurBrush: return #selector(EditorViewController.selectBlurBrushTool(_:))
        case .sharpenBrush: return #selector(EditorViewController.selectSharpenBrushTool(_:))
        case .smudge: return #selector(EditorViewController.selectSmudgeTool(_:))
        case .sponge: return #selector(EditorViewController.selectSpongeTool(_:))
        case .historyBrush: return #selector(EditorViewController.selectHistoryBrushTool(_:))
        case .objectSelect: return #selector(EditorViewController.selectObjectSelectTool(_:))
        case .pen: return #selector(EditorViewController.selectPenTool(_:))
        case .remove: return #selector(EditorViewController.selectRemoveTool(_:))
        case .contentAwareMove:
            return #selector(EditorViewController.selectContentAwareMoveTool(_:))
        case .artHistoryBrush: return #selector(EditorViewController.selectArtHistoryBrushTool(_:))
        case .pencil: return #selector(EditorViewController.selectPencilTool(_:))
        }
    }

    /// The canvas cursor while the tool is at rest. The move and hand
    /// tools' open hands close mid-drag; the canvas swaps that state in
    /// itself.
    var cursor: NSCursor {
        switch self {
        case .select, .ellipseSelect, .lasso, .freehandLasso, .magneticLasso, .quickSelect, .wand, .subject,
            .fill, .gradient, .brush,
            .eraser, .eyedropper, .colorSampler, .crop, .clone, .dodge, .heal, .spotHeal, .patch,
            .redEye, .shapeRect, .shapeEllipse, .shapeLine, .ruler, .perspectiveCrop, .blurBrush,
            .sharpenBrush, .smudge, .sponge, .historyBrush, .objectSelect, .pen, .remove,
            .contentAwareMove:
            return .crosshair
        case .artHistoryBrush, .pencil: return .crosshair
        case .move, .hand:
            return .openHand
        case .text:
            return .iBeam
        case .zoom:
            return Self.zoomCursor
        }
    }

    /// AppKit ships no magnifier cursor; one is built once from the zoom
    /// tool's own symbol, falling back to the crosshair if the symbol
    /// somehow fails to render.
    private static let zoomCursor: NSCursor = {
        guard let icon = NSImage(
            systemSymbolName: "plus.magnifyingglass", accessibilityDescription: "Zoom")?
            .withSymbolConfiguration(NSImage.SymbolConfiguration(pointSize: 15, weight: .regular))
        else { return .crosshair }
        let tinted = icon.tinted(with: .black)
        return NSCursor(image: tinted, hotSpot: NSPoint(x: 6, y: 6))
    }()

    /// The bare key that selects the tool: the single source for both
    /// directions of the mapping.
    /// A key shared by several tools (M, L, W, G, J, U) cycles them in rail
    /// order on repeated presses (see `tool(forKey:current:last:)`).
    var keyCharacter: String {
        switch self {
        case .select, .ellipseSelect: return "m"
        case .lasso, .freehandLasso, .magneticLasso: return "l"
        case .wand, .quickSelect, .objectSelect, .subject: return "w"
        case .move: return "v"
        // The Brush and the Pencil share B and cycle in rail order.
        case .brush, .pencil: return "b"
        case .eraser: return "e"
        // The G slot: Paint Bucket and Gradient share it and cycle.
        case .fill, .gradient: return "g"
        case .text: return "t"
        // Bare I cycles the pair.
        case .eyedropper, .colorSampler, .ruler: return "i"
        case .crop, .perspectiveCrop: return "c"
        case .clone: return "s"
        // The toning slot: O cycles Dodge / Burn and Sponge.
        case .dodge, .sponge: return "o"
        // R is free here and cycles the three focus tools in rail order.
        case .blurBrush, .sharpenBrush, .smudge: return "r"
        case .historyBrush: return "y"
        case .artHistoryBrush: return "y"
        // The healing slot shares one key; repeated presses cycle
        // its tools, exactly as U cycles the shape tools.
        case .heal, .spotHeal, .patch, .redEye, .remove, .contentAwareMove: return "j"
        case .shapeRect, .shapeEllipse, .shapeLine: return "u"
        case .pen: return "p"
        case .zoom: return "z"
        case .hand: return "h"
        }
    }

    /// The tool a bare key selects (an unmodified,
    /// lowercased character from keyDown); nil when the character selects
    /// no tool — non-tool bare keys (brush-size brackets, Quick Mask's Q)
    /// stay in the canvas's keyDown. A key shared by several tools (the
    /// shape group's U) maps to the first; cycling in rail order is
    /// `tool(forKey:current:last:)`'s business.
    init?(keyCharacter: String) {
        guard let tool = Self.allCases.first(where: { $0.keyCharacter == keyCharacter }) else {
            return nil
        }
        self = tool
    }

    /// The tool a bare press of `key` resolves to: the key's non-planned
    /// peers in rail order, skipping any `available` refuses. On a peer, the
    /// next peer (wrapping); otherwise the peer last used for the key, else
    /// the first. Nil when the key has no available peer.
    static func tool(
        forKey key: String, current: EditorTool, last: EditorTool?,
        available: (EditorTool) -> Bool = { _ in true }
    ) -> EditorTool? {
        let peers = railGroups.flatMap { $0 }.filter {
            $0.keyCharacter == key && !$0.planned && available($0)
        }
        guard let first = peers.first else { return nil }
        if let index = peers.firstIndex(of: current) {
            return peers[(index + 1) % peers.count]
        }
        if let last, peers.contains(last) { return last }
        return first
    }

    /// The smallest brush this tool can do anything with, in px — the floor
    /// its Size field, its `[` key and its agent mirror all clamp to.
    ///
    /// One for every paint tool but the two healing brushes, and three for
    /// those, because `doc_heal`'s rule makes anything smaller inert: the
    /// solve keeps the destination at every covered pixel that touches an
    /// uncovered one, so the healed footprint is always the brush eroded by a
    /// pixel and a footprint one or two pixels across is entirely boundary.
    /// Confirmed against the built app: `heal_stroke` at size 1 and 2 changes
    /// nothing while 3 and 4 heal. Left un-floored, a retoucher stepping the
    /// brush down with `[` to chase a thin scratch watched the clone land for
    /// the whole drag and then vanish with a beep.
    var minimumBrushSize: Double {
        switch self {
        case .heal, .spotHeal:
            return 3
        case .brush, .eraser, .clone, .dodge, .select, .ellipseSelect, .lasso, .wand,
            .subject, .move, .fill, .gradient, .text, .eyedropper, .crop, .patch, .redEye,
            .shapeRect, .shapeEllipse, .shapeLine, .zoom, .hand, .freehandLasso, .quickSelect,
            .magneticLasso, .colorSampler, .ruler, .perspectiveCrop, .blurBrush, .sharpenBrush,
            .smudge, .sponge, .historyBrush, .objectSelect, .pen, .remove, .contentAwareMove:
            return 1
        case .artHistoryBrush, .pencil: return 1
        }
    }

    /// The largest brush this tool takes, in px — the ceiling its Size field,
    /// its `]` key, the canvas sync and its agent mirror all clamp to.
    ///
    /// 5,000 for Brush, Eraser, Clone Stamp and Dodge/Burn, whose cost is per
    /// dab and is bounded by the stroke caps (`NativeBrushStroke.maximumWork`,
    /// `maximumDabs`). 200 for the two healing brushes: their solve runs over
    /// the whole covered footprint and its ring, so its cost grows with the
    /// brush's area, not per dab. 200 also for Blur, Sharpen, Smudge, Sponge
    /// and History Brush, which keep the cap they shipped with (their
    /// agent mirrors clamp to it in `retouchBrushAmounts`); Smudge's stroke is
    /// bounded by its own `NativeBrushStroke.maximumSmudgeWork` as well.
    /// 1,000 for Remove: its stroke is a Content-Aware Fill
    /// hole, whose own caps bound the cost. Every other tool reads no brush
    /// size and answers 200, the value the shared clamp had.
    var maximumBrushSize: Double {
        switch self {
        case .brush, .eraser, .clone, .dodge, .pencil:
            return 5000
        case .remove:
            return 1000
        case .heal, .spotHeal, .blurBrush, .sharpenBrush, .smudge, .sponge, .historyBrush,
            .select, .ellipseSelect, .lasso, .wand, .subject, .move, .fill,
            .gradient, .text, .eyedropper, .crop, .patch, .redEye, .shapeRect, .shapeEllipse,
            .shapeLine, .zoom, .hand, .freehandLasso, .quickSelect, .magneticLasso,
            .colorSampler, .ruler, .perspectiveCrop, .objectSelect, .pen, .contentAwareMove:
            return 200
        case .artHistoryBrush: return 200
        }
    }

    /// True for the tools whose stroke is a BRUSH TIP: the ones the bare
    /// `[` / `]` keys resize, whose options bar carries the tip cluster, and
    /// whose paint is clipped to the active layer's extent. Lives here rather
    /// than as a `||` chain repeated in the canvas — a per-tool fact belongs
    /// on the enum. Exhaustive on purpose: a new tool must answer it.
    ///
    /// Remove answers true for the hover ring, `[` / `]` and the size
    /// sync alone: its drag is `RemoveSession`'s, never the stroke pipeline's,
    /// and its round tip has no hardness, angle or roundness.
    var usesBrushTip: Bool {
        switch self {
        case .brush, .eraser, .clone, .dodge, .heal, .spotHeal, .blurBrush, .sharpenBrush,
            .smudge, .sponge, .historyBrush, .remove:
            return true
        case .artHistoryBrush, .pencil: return true
        case .select, .ellipseSelect, .lasso, .wand, .subject, .move, .fill, .gradient,
            .text, .eyedropper, .crop, .patch, .redEye, .shapeRect, .shapeEllipse,
            .shapeLine, .zoom, .hand, .freehandLasso, .quickSelect, .magneticLasso,
            .colorSampler, .ruler, .perspectiveCrop, .objectSelect, .pen, .contentAwareMove:
            return false
        }
    }

    /// True for the tools whose selection is DRAGGED and may therefore begin on the
    /// pasteboard around the image: the press is clamped to the nearest image
    /// edge. Click tools (polygonal lasso, wand, subject) stay inert there. Exhaustive on
    /// purpose: a new tool must answer it.
    var acceptsMarginPress: Bool {
        switch self {
        case .select, .ellipseSelect, .freehandLasso, .quickSelect, .objectSelect:
            return true
        case .brush, .eraser, .clone, .dodge, .heal, .spotHeal, .lasso, .wand, .subject,
            .move, .fill, .gradient, .text, .eyedropper, .crop, .patch, .redEye, .shapeRect,
            .shapeEllipse, .shapeLine, .zoom, .hand, .magneticLasso, .colorSampler, .ruler,
            .perspectiveCrop, .blurBrush, .sharpenBrush, .smudge, .sponge, .historyBrush, .pen,
            .remove, .contentAwareMove:
            return false
        case .artHistoryBrush, .pencil: return false
        }
    }

    /// True for the tools whose stroke is read as COVERAGE alone: the canvas
    /// draws their overlay in white ink, and the controller hands its alpha to
    /// an op that reworks the layer's own pixels (Dodge / Burn, Blur, Sharpen, Smudge, Sponge
    /// and History Brush) rather than compositing a colour. Exhaustive on purpose.
    var strokeIsCoverage: Bool {
        switch self {
        case .dodge, .blurBrush, .sharpenBrush, .smudge, .sponge, .historyBrush:
            return true
        case .artHistoryBrush: return true
        case .brush, .eraser, .clone, .heal, .spotHeal, .select, .ellipseSelect, .lasso, .wand,
            .subject, .move, .fill, .gradient, .text, .eyedropper, .crop, .patch, .redEye,
            .shapeRect, .shapeEllipse, .shapeLine, .zoom, .hand, .freehandLasso, .quickSelect,
            .magneticLasso, .colorSampler, .ruler, .perspectiveCrop, .objectSelect, .pen, .remove,
            .contentAwareMove, .pencil:
            return false
        }
    }

    /// True while a tool's behavior hasn't landed: it stays visible in the
    /// rail and menus (with a "Soon" affordance) but validation disables it
    /// everywhere. The availability gate future tools arrive behind.
    var planned: Bool { false }

    // MARK: - Rail grouping

    /// The rail's slots, top to bottom. A group with more than one member is
    /// ONE slot: it draws whichever member is current and offers the rest
    /// under a corner-triangle dropdown, so related tools cost one slot no
    /// matter how many join them. Every tool appears exactly once (the rail
    /// asserts it); grouping is the rail's business alone — menus, keys,
    /// the canvas and the agent all address tools directly.
    static let railGroups: [[EditorTool]] = [
        [.select, .ellipseSelect, .lasso, .freehandLasso, .magneticLasso, .wand, .quickSelect,
         .objectSelect, .subject],
        [.crop, .perspectiveCrop],
        [.move],
        [.brush, .pencil, .eraser, .clone, .historyBrush, .artHistoryBrush],
        // The healing slot, immediately below the paint group.
        [.spotHeal, .remove, .heal, .patch, .contentAwareMove, .redEye],
        // The toning and focus tools in one slot.
        [.dodge, .sponge, .blurBrush, .sharpenBrush, .smudge],
        [.fill],
        [.gradient],
        [.shapeRect, .shapeEllipse, .shapeLine],
        [.pen],
        [.text],
        [.eyedropper, .colorSampler, .ruler],
        [.zoom, .hand],
    ]

    /// Index into `railGroups` of the slot this tool lives in.
    var railGroupIndex: Int {
        Self.railGroups.firstIndex { $0.contains(self) } ?? 0
    }

    // MARK: - Stable identity

    /// The tool's name in a saved workspace layout: the CASE name, never the
    /// raw value and never `displayName`. A raw value is a stable order but
    /// reads as a number in a file nobody can then check by eye, and a display
    /// name is UI text that a rename would orphan.
    var identifier: String { String(describing: self) }

    /// The tool `identifier` names, or nil. Exhaustive by construction: it
    /// searches `allCases`, so a new case is answerable the day it is added.
    static func named(_ id: String) -> EditorTool? {
        allCases.first { $0.identifier == id }
    }
}
