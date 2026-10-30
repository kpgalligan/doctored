import AppKit

// `ToolOptionsStore.load` overlays a saved blob on the family's defaults, so adding a field keeps
// every other saved value and needs no hand-written decoder, while changing an existing field's
// default needs a `droppingStale` entry keyed on a field only newer blobs carry.

// Per-tool option values, persisted across documents and launches so a tool
// comes back with the settings it was last used with — the same memory the
// rail keeps for group members. One Codable value type per tool family; the
// options bar's descriptors read and write these through ToolOptionsStore.
//
// Every stored field is honoured. Crop's Delete cropped pixels is
// read as on while its Fill is Content-Aware Fill, which deletes them.

struct SelectToolOptions: Codable, Equatable {
    /// Combine mode for new selection gestures (0 new, 1 add, 2 subtract,
    /// 3 intersect); the Shift/Option modifiers still override per gesture.
    var modeIndex = 0
    /// Feather radius applied to selections as the gesture commits.
    var feather: Double = 0
    var antiAlias = true
    /// Amounts the overflow popover's morphology fields apply with.
    var growAmount: Double = 8
    var borderWidth: Double = 2
    var smoothRadius: Double = 4
    // Magic Wand.
    var tolerance: Double = 32
    var contiguous = true
    /// The wand's Sample popup: true reads the composite (All layers,
    /// the wand's behaviour before the popup worked), false the active layer
    /// alone. A new key, so an old blob's dead `sampleAllLayers` is ignored.
    var wandSampleAllLayers = true
    /// The wand's Sample Size popup: an index into `wandSampleSizes`,
    /// 0 Point Sample.
    var wandSampleSizeIndex = 0

    /// The Sample Size list, in popup order: Point Sample, then
    /// the 3, 5, 11, 31, 51 and 101 by 101 Average boxes the wand's
    /// reference is the mean of (`selection_region::WAND_SAMPLE_SIZES`).
    static let wandSampleSizes = [1, 3, 5, 11, 31, 51, 101]

    /// The chosen Sample Size, 1 for an index a stale blob put out of range.
    var wandSampleSize: Int {
        Self.wandSampleSizes.indices.contains(wandSampleSizeIndex)
            ? Self.wandSampleSizes[wandSampleSizeIndex] : 1
    }
}

struct CropToolOptions: Codable, Equatable {
    /// Index into EditorViewController.cropRatios.
    var ratioIndex = 0
    var deleteCroppedPixels = false
    /// Index into `CropFill.allCases`: 0 Background (Default).
    var fillIndex = 0
    /// 0 none, 1 thirds.
    var gridIndex = 1
    var snapToGuides = true
}

struct MoveToolOptions: Codable, Equatable {
    /// Whether a Move click activates the entry under the cursor at all.
    /// Off by default: with it on, a click anywhere on the
    /// canvas retargets every later edit, which is surprising until it is
    /// asked for.
    var autoSelect = false
    /// What Auto-Select picks: 0 the LAYER under the cursor, 1 its top-level
    /// GROUP.
    var autoSelectIndex = 0
    /// Show Transform Controls: the box and eight handles around the
    /// moving set, whose press opens Free Transform. Off by default. A new
    /// key, so an old blob's dead `showTransformControls`
    /// (stored true while the checkbox did nothing) is ignored.
    var transformControlsShown = false
    var snapToLayers = true
    var nudgeStep: Double = 1
}

/// Shared by the six paint tools — brush, eraser, clone, dodge and the two
/// healing brushes — each of which keeps its OWN instance, so the eraser's
/// size never follows the brush's. The dodge tool reads `opacity` as its
/// exposure and adds the burn/range pair no other paint tool shows; the
/// healing brushes read `opacity` as the heal's strength and add the
/// aligned/sample pair below.
///
/// Adding a field keeps every other saved value: `ToolOptionsStore.load`
/// overlays a saved blob on the slot's defaults, so the new field alone
/// takes its default. Changing an EXISTING field's default needs a
/// `droppingStale` entry keyed on a field only newer blobs carry, or the
/// old blob's value silently outlives the change.
struct PaintToolOptions: Codable, Equatable {
    var size: Double = 24
    /// 100 = the classic hard round; below that, SoftBrush dab stamping.
    /// A blob without `tipVersion` has its `hardness` (e54f3c9's placeholder
    /// 0) dropped at load (`ToolOptionsStore.paintStale`).
    var hardness: Double = 100
    var opacity: Double = 100
    /// Per-dab deposit; opacity still caps the whole stroke once.
    var flow: Double = 100
    /// The stroke's blend mode as a RAW DrBlendMode value (stable across
    /// releases; 0 = Normal). Honored by brush and clone only.
    var blendIndex = 0
    /// Dab spacing as % of the diameter; 25 is the classic rhythm (and
    /// keeps the smoother hard-path pipeline at hardness 100).
    var spacing: Double = 25
    var angle: Double = 0
    var roundness: Double = 100
    /// Pulled-string stabilization; 0 turns the leash off.
    var smoothing: Double = 20
    var pressureSize = true
    /// Optional for compatibility with existing saved tool-option blobs.
    var pressureOpacity: Bool? = nil
    var airbrush = false
    // Dodge / Burn only.
    var burn = false
    /// 0 shadows, 1 midtones, 2 highlights.
    var rangeIndex = 1
    /// Protect Tones (#547): the stroke moves each pixel's channels together, keeping hue and
    /// saturation. nil or false is off (Doctored keeps every stroke it made before unchanged).
    /// Optional so blobs saved before it keep decoding.
    var protectTones: Bool?
    /// Sponge only: true saturates, nil or false desaturates.
    /// Optional so blobs saved before the tool keep decoding.
    var saturate: Bool?
    /// Sponge only: Vibrance, a checkbox. nil is ON (its
    /// default), so read it as `!= false`; false turns it off.
    var vibrance: Bool?
    /// Blur, Sharpen and Smudge only: Sample All Layers. nil or false
    /// is off (the default); true reads the visible picture under and
    /// including the target and writes the target.
    var sampleAllLayers: Bool?
    /// Spot Healing Brush only: the Type. nil is Content-Aware
    /// (the default and every stroke before the option); Optional so
    /// blobs saved before it keep decoding.
    var spotHealType: SpotHealType?
    /// Art History Brush only: the Style, the Area (px) and the
    /// Tolerance (%). nil is Tight Short, 50 px and 0 %, the
    /// defaults; Optional so blobs saved before the tool keep decoding. Its
    /// Mode is `blendIndex`, limited to `ArtHistoryOptions.modes`.
    var artStyle: ArtHistoryStyle?
    var artArea: Double?
    var artTolerance: Double?
    // Healing Brush only (the two above are Dodge / Burn only the same way).
    /// The sampled source offset set by the first stroke after the source
    /// point was placed PERSISTS across strokes; off, every mouse-down
    /// recomputes it from the source point — today's Clone Stamp rule. While
    /// it is on, the canvas's source marker tracks the pointer rather than
    /// the ⌥-clicked point, because the offset is what persists and the
    /// point being sampled therefore moves (`ImageCanvasView`'s
    /// `cloneSourceMarkerPoint`).
    var aligned = true
    /// WHICH pixels the gesture samples (`RetouchSampling`). Every retouch
    /// tool now asks the CORE for its sampling plate, so this reaches the
    /// core the same way for all of them — unlike the boolean it replaces,
    /// which the Healing Brush honoured Swift-side by choosing a snapshot
    /// while the Spot Healing Brush passed it down.
    ///
    /// `.all` keeps the Clone Stamp, the Healing Brush and the Patch tool
    /// sampling exactly what they sampled before; `ToolOptionsStore` sets
    /// the Spot Healing Brush's own default to `.current`, as its boolean
    /// default was false.
    var sample: RetouchSampling = .all
    /// Drop ADJUSTMENT ENTRIES above the target from the sampled plate.
    /// A provable identity in `.current` and `.currentAndBelow`; the
    /// meaningful control in `.all`.
    var ignoreAdjustments = false
    /// Put the repair on a new layer directly above the target instead of
    /// into its pixels, leaving the original recoverable.
    var outputToNewLayer = false
    // Optional fields preserve older saved tool options without a migration reset.
    var expandDestination: Bool?
    var sourceScale: Double?
    var sourceRotation: Double?
    var sourceFlipX: Bool?
    var sourceFlipY: Bool?
    var sourceOverlay: Bool?
    /// nil in blobs saved before the tip options went live — those carry
    /// the redesign's DISABLED placeholder values (spacing 10), which no
    /// user could have chosen and the pipeline never honored. Decoding nil
    /// normalizes them to the live defaults (see ToolOptionsStore.load);
    /// 1 marks a blob whose tip values are real choices.
    var tipVersion: Int?

    /// A pre-tip blob's placeholders replaced by the live defaults, so an
    /// upgrade renders every stroke exactly as the previous release did
    /// (spacing 10 would otherwise walk soft dabs 2.5x denser than the
    /// classic quarter-diameter rhythm the old pipeline hardcoded).
    var migratedToLiveTip: PaintToolOptions {
        guard tipVersion == nil else { return self }
        var out = self
        out.spacing = 25
        out.tipVersion = 1
        return out
    }
}

/// The Patch tool: which way the drag runs, and where the texture comes
/// from. No tip options — the footprint is an outlined region, not a brush.
struct PatchToolOptions: Codable, Equatable {
    /// 0 Source (the default): the OUTLINED region is healed
    /// from wherever it is dragged to. 1 Destination: the outlined region is
    /// good texture, dragged onto the flaw.
    var directionIndex = 0
    var sample: RetouchSampling = .all
    var ignoreAdjustments = false
}

/// The Content-Aware Move tool: what the drag does with the outlined
/// region, and where the fill and the heal read from. No tip options, as Patch.
struct ContentAwareMoveToolOptions: Codable, Equatable {
    /// 0 Move (the default): the region is filled where it was and its pixels
    /// are healed in where it was dragged. 1 Extend: the pixels are healed in
    /// there and the region stays as it was.
    var modeIndex = 0
    var sample: RetouchSampling = .all
    var ignoreAdjustments = false
}

/// The Red Eye tool's two dials, both percentages.
struct RedEyeToolOptions: Codable, Equatable {
    /// Rejects a red region whose larger side exceeds this fraction of the
    /// drag rectangle's SHORTER side. 100 — permissive — is deliberate: the
    /// gesture the app documents is a tight rectangle over one eye, where
    /// the red iris spans 40–90 % of the short side, so a smaller default
    /// would refuse the tool's own instruction. Turn it down when something
    /// red (a shirt, lipstick) got caught by a sloppy rectangle.
    var pupilSize: Double = 100
    /// How far the corrected pupil darkens after the red is neutralised;
    /// 50 is the default, 0 neutralises only.
    var darken: Double = 50
}

/// The Pen tool's one option.
struct PenToolOptions: Codable, Equatable {
    /// Rubber Band: while a subpath is open, the segment the next click
    /// would add is previewed from its last anchor to the pointer.
    var rubberBand = true
}

/// Content-Aware Fill's sheet values, persisted like a tool's options: the
/// command has no tool of its own to hang them on.
struct ContentAwareFillOptions: Codable, Equatable {
    /// Sampling-ring width in px around the selection (21–512; the core's own
    /// floor is three patch widths and it raises anything below it).
    var ring: Double = 48
    var sample: RetouchSampling = .current
    var ignoreAdjustments = false
    /// The inpaint's RNG seed: the same seed refills identically.
    var seed = 0
}

/// Edit ▸ Fill…'s sheet values, persisted like a tool's options: the
/// command has no tool of its own to hang them on. `contents` is a
/// `FillContents` raw value, `customColor` the Color… choice as `#RRGGBBAA`,
/// `blendMode` a `DrBlendMode` display name, `opacity` 0…100.
struct FillDialogOptions: Codable, Equatable {
    var contents = 0
    var customColor = "#000000FF"
    var blendMode = "Normal"
    var opacity: Double = 100
    var preserveTransparency = false
}

/// Select ▸ Color Range…'s sheet values, persisted like Fill…'s and
/// loaded when the sheet opens, saved on Apply. `range` is a
/// `ColorRangeParameters.range` value, `modeIndex` the Combine popup (0
/// Replace). The samples and their points are not persisted across
/// invocations.
struct ColorRangeDialogOptions: Codable, Equatable {
    var range = "sampled"
    var tolerance: Double = 20
    var softness: Double = 10
    var lower: Double = 0
    var upper: Double = 35
    var invert = false
    var localized = false
    var clusterRange: Double = 100
    var modeIndex = 0
    /// Skin Tones' Fuzziness, default 40.
    var fuzziness: Double = 40
}

/// Edit ▸ Stroke…'s sheet values, persisted like Fill…'s. The
/// colour is NOT persisted: the well opens on the foreground colour.
/// `width` in pixels, `location` a `StrokeLocation` raw value (1 Center),
/// `blendMode` a `DrBlendMode` display name, `opacity` 0…100.
struct StrokeDialogOptions: Codable, Equatable {
    var width: Double = 1
    var location = 1
    var blendMode = "Normal"
    var opacity: Double = 100
    var preserveTransparency = false
}

struct FillToolOptions: Codable, Equatable {
    /// 0 Foreground, 1 Background.
    var contentsIndex = 0
    var tolerance: Double = 32
    var contiguous = true
    var opacity: Double = 100
    /// A RAW `DrBlendMode` value (0 = Normal), as the paint tools keep it.
    var blendIndex = 0
    /// Grow the region over the composite instead of the active layer alone.
    var sampleAllLayers = false
}

struct GradientToolOptions: Codable, Equatable {
    /// 0 linear, 1 radial, 2 angle, 3 reflected, 4 diamond: an
    /// `DrGradientKind` raw value.
    var typeIndex = 0
    var opacity: Double = 100
    var reverse = false
    /// On, a stop's own opacity (and the swatches' alpha) paints through;
    /// off, every stop paints opaque. A new key, so an old blob takes
    /// the default, which is what the tool did before the checkbox worked.
    var transparency = true
    /// The ramp's stops: authored sRGB `#rrggbb`, position 0…1 and
    /// opacity 0…1, the `GradientFill` stop the layer styles use. nil is
    /// Foreground → Background, the two rail swatches. `GradientRamp`
    /// resolves both forms. An old blob's dead `dither`, `midpoint` and
    /// `methodIndex` are ignored.
    var stops: [GradientStop]? = nil
    /// A RAW `DrBlendMode` value (0 = Normal), as the Fill tool keeps it.
    /// A new key, so an old blob takes Normal, which is what the tool
    /// painted before it had a Mode.
    var blendIndex = 0
    /// The Smoothness of the tool's gradient as a fraction
    /// 0…1 (the Edit… sheet shows percent): 1 is the Classic ramp, 0 the
    /// linear one. A new key, so an old blob takes 1, what the tool painted
    /// before it had one.
    var smoothness: Double = 1
}

struct ShapeToolOptions: Codable, Equatable {
    /// "#RRGGBBAA", or "" for none — ShapeLayerPayload's convention.
    var fill = "#C96F4AFF"
    var stroke = "#E8E2D4FF"
    /// Stops above the hex: the colour is the hex × 2^intensity in
    /// linear light, so a 32-bit float document's shape can be above white.
    /// 0 for an in-range colour. Read and written through the accessors in
    /// `ShapeTool.swift`.
    var fillIntensity: Double = 0
    var strokeIntensity: Double = 0
    var strokeWidth: Double = 2
    var radius: Double = 8
    /// Indices into `ShapeStrokeJoin.allCases` / `ShapeStrokeDash.allCases` /
    /// `ShapeStrokeAlign.allCases` (ShapeStrokeStyle.swift): miter, no dash,
    /// centred — the stroke older builds drew for every shape.
    var strokeJoinIndex = 0
    var strokeDashIndex = 0
    var strokeAlignIndex = 1
    /// The CMYK ink a fill or stroke was PICKED as (#547, the Color Picker's C/M/Y/K on a CMYK
    /// document), kept exactly beside its appearance hex; nil once the colour is set any other
    /// way (`ShapeTool.swift`'s writers). A CMYK commit matches the colour to it.
    var fillInk: CmykInk? = nil
    var strokeInk: CmykInk? = nil
}

/// The text tool's typography. Every field is what `TextStyle` (TextLayer.swift)
/// renders with, so a value here is exactly what the next commit writes into
/// the layer's description; `EditorViewController.currentTextStyle()` reads
/// them and `applyTextOptions` writes a reopened layer's back.
struct TextToolOptions: Codable, Equatable {
    var family = "Helvetica Neue"
    /// NSFontManager weight for the family (0…15: 3 light, 5 regular,
    /// 6 medium, 8 semibold, 9 bold); the popup maps its rows onto these.
    var weight = 5
    var size: Double = 48
    /// Extra spacing between glyphs in px (the `.kern` attribute, so it
    /// changes line breaks); 0 = the font's own spacing, negative tightens.
    var tracking: Double = 0
    /// 0 left, 1 center, 2 right.
    var alignmentIndex = 0
    /// Line height in pt, baseline to baseline: 0 = the font's natural
    /// height, otherwise every line is pinned to exactly this height
    /// (`TextStyle.leading` — the options bar's "Leading … pt" field).
    var leading: Double = 0
    /// Baseline shift in px; positive raises the glyphs (`.baselineOffset`).
    var baselineShift: Double = 0
    var italic = false
    var underline = false
    var strikethrough = false

    /// The options bar's ranges for the numeric typography — the ONE home
    /// the bar's fields, the agent's `weight` / `tracking` / `leading` /
    /// `baseline_shift` arguments and the restore of a reopened layer's
    /// description all read, so agent and UI accept the same values and the
    /// store never holds one the bar cannot express.
    static let weightRange = 0...15
    static let trackingRange: ClosedRange<Double> = -100...100
    static let leadingRange: ClosedRange<Double> = 0...1000
    static let baselineShiftRange: ClosedRange<Double> = -100...100

    /// The typography clamped into those ranges, a non-finite number
    /// falling back to its default. `applyTextOptions` stores a reopened
    /// layer's description through this: `TextLayerPayload.decode` accepts
    /// any finite tracking, leading or shift, and a `.rz` from elsewhere
    /// can carry one the bar cannot express — left in the store it would
    /// type every NEW session with it (a tracking of a million lays each
    /// glyph on its own line and pushes the commit past the pixel cap)
    /// until the field was reset by hand. `currentTextStyle` reads the
    /// store through it too, so a hand-edited defaults blob is bounded the
    /// same way. The reopened SESSION itself still previews the layer's own
    /// values (`canvas.textStyle`), so what it shows is what the layer
    /// renders and an unchanged ⌘Return registers no edit.
    func clampingTypography() -> TextToolOptions {
        var clamped = self
        clamped.weight = min(max(weight, Self.weightRange.lowerBound), Self.weightRange.upperBound)
        clamped.tracking = Self.clamp(tracking, to: Self.trackingRange, default: 0)
        clamped.leading = Self.clamp(leading, to: Self.leadingRange, default: 0)
        clamped.baselineShift = Self.clamp(baselineShift, to: Self.baselineShiftRange, default: 0)
        return clamped
    }

    private static func clamp(
        _ value: Double, to range: ClosedRange<Double>, default fallback: Double
    ) -> Double {
        guard value.isFinite else { return fallback }
        return min(max(value, range.lowerBound), range.upperBound)
    }
}

struct SampleToolOptions: Codable, Equatable {
    /// 0 point, 1 3×3, 2 5×5.
    var sampleSizeIndex = 0
    /// 0 current layer, 1 all layers.
    var fromIndex = 1
    var showRing = true
    var copyOnPick = false
}

/// The view chrome's preferences: rulers, guides, the grid and snapping.
///
/// None of this is document state, and the split is deliberate. The guide
/// LIST and the ruler ORIGIN live in the core and in `.rz`, because they are
/// per-document geometry that must survive save/reopen and undo. Everything
/// here describes how this USER works — which unit the rulers speak, whether
/// snapping is on, what a guide is coloured — and this store "follows the
/// user, not the document" (see the class comment below), so putting any of
/// it in a file would make a document arrive and silently change the app's
/// behaviour. LOCK GUIDES is one app-wide boolean rather than a per-guide
/// byte for the same reason.
///
/// Adding a field keeps every other saved value: `ToolOptionsStore.load`
/// overlays a saved blob on the family's defaults, so the new field alone
/// takes its default. Changing an EXISTING field's default needs a
/// `droppingStale` entry keyed on a field only newer blobs carry.
struct ViewToolOptions: Codable, Equatable {
    var scrubbyZoom = false
    /// On by default from this phase: the rulers are the chrome the app was
    /// most missed for. Blobs saved before this phase lack `rulerUnitIndex`,
    /// so `load` drops their `rulers` (`ToolOptionsStore.viewStale`) and they
    /// take this default.
    var rulers = true
    var guides = true
    /// 0 auto, 1 on, 2 off.
    var pixelGridIndex = 0
    /// Index into `CanvasUnit.allCases` — the unit BOTH rulers label in and
    /// the grid's spacing is authored in.
    var rulerUnitIndex = 0
    /// Lock Guides: the mouse alone. New Guide…, Clear Guides and every MCP
    /// mutator still work, because the lock exists to stop an accidental
    /// drag, not to freeze the document.
    var guidesLocked = false
    /// Index into `GuideColor.allCases`.
    var guideColorIndex = 0
    var showGrid = false
    /// In the RULER's unit, so 100 with the default Pixels unit is a 100 px
    /// grid.
    var gridSpacing: Double = 100
    var gridSubdivisions = 4
    /// View ▸ Snap, the master switch that outranks the two per-tool
    /// checkboxes (`crop.snapToGuides`, `move.snapToLayers`).
    var snapEnabled = true
    /// `SnapTarget`'s raw value — View ▸ Snap To.
    var snapTargets = SnapTarget.all.rawValue
    /// The right panel's width in points, dragged by `PanelDividerView` and
    /// carried by a saved workspace layout. App-wide like the rest of this
    /// family: a panel width follows the user, not the document.
    var panelWidth: Double = 304
    /// View ▸ Show Color Samplers: the canvas markers.
    var colorSamplers = true
    /// The Info panel's Color Samplers readout (`ColorSamplerReadoutMode`).
    var colorSamplerReadoutIndex = 0
    /// View ▸ Show Clipping: the canvas clipping overlay.
    var showClipping = false
}

/// Cross-tool state that is nobody's option in particular: the rail's two
/// color swatches ("#RRGGBBAA", TextLayer's hex convention).
struct SharedToolState: Codable, Equatable {
    /// Setting either swatch clears its Lab ink (#547 Lab phase 2e): `setPaintColor` /
    /// `setBackgroundColor` write it, so a colour set any other way paints as its colour.
    var foreground = "#000000FF" { didSet { foregroundLab = nil } }
    var background = "#FFFFFFFF" { didSet { backgroundLab = nil } }
    /// #547 phase 2: the ink a CMYK document's Color Picker or eyedropper set for each swatch,
    /// kept exactly (the swatch's sRGB is its display); nil once the colour is set any other way
    /// (`setPaintColor`/`setBackgroundColor` clear it), when the colour converts through the
    /// document's profile instead.
    var foregroundInk: CmykInk? = nil
    var backgroundInk: CmykInk? = nil
    /// #547 Lab phase 2e: the L*a*b* the Eyedropper picked on a Lab document for each swatch,
    /// kept exactly (`LabInk`; the swatch's sRGB is its clipped display); nil once the swatch's
    /// colour is set any other way.
    var foregroundLab: LabInk? = nil
    var backgroundLab: LabInk? = nil
}

/// The persistence point: one property per family (paint tools get one
/// each), loaded from UserDefaults at startup and written back on every
/// change. Shared across editor windows deliberately — tool options follow
/// the user, not the document.
final class ToolOptionsStore {
    static let shared = ToolOptionsStore()

    var select: SelectToolOptions { didSet { save(select, "select") } }
    var crop: CropToolOptions { didSet { save(crop, "crop") } }
    var move: MoveToolOptions { didSet { save(move, "move") } }
    var brush: PaintToolOptions { didSet { save(brush, "brush") } }
    var eraser: PaintToolOptions { didSet { save(eraser, "eraser") } }
    var clone: PaintToolOptions { didSet { save(clone, "clone") } }
    var dodge: PaintToolOptions { didSet { save(dodge, "dodge") } }
    var heal: PaintToolOptions { didSet { save(heal, "heal") } }
    var spotHeal: PaintToolOptions { didSet { save(spotHeal, "spotHeal") } }
    // `opacity` is the Strength of the first four and the History
    // Brush's Opacity.
    var blurBrush: PaintToolOptions { didSet { save(blurBrush, "blurBrush") } }
    var sharpenBrush: PaintToolOptions { didSet { save(sharpenBrush, "sharpenBrush") } }
    var smudge: PaintToolOptions { didSet { save(smudge, "smudge") } }
    var sponge: PaintToolOptions { didSet { save(sponge, "sponge") } }
    var historyBrush: PaintToolOptions { didSet { save(historyBrush, "historyBrush") } }
    /// `opacity` is the Art History Brush's Opacity.
    var artHistoryBrush: PaintToolOptions { didSet { save(artHistoryBrush, "artHistoryBrush") } }
    /// The Remove tool reads `size`, `sample` (All Layers or Current
    /// Layer) and `outputToNewLayer` alone; `setPaintOptions` keeps the rest
    /// at their defaults.
    var remove: PaintToolOptions { didSet { save(remove, "remove") } }
    /// The Remove tool's Mode (Model or Content-Aware); `RemoveModel.swift`.
    var removeTool: RemoveToolOptions { didSet { save(removeTool, "removeTool") } }
    var patch: PatchToolOptions { didSet { save(patch, "patch") } }
    var contentAwareMove: ContentAwareMoveToolOptions {
        didSet { save(contentAwareMove, "contentAwareMove") }
    }
    var redEye: RedEyeToolOptions { didSet { save(redEye, "redEye") } }
    var pen: PenToolOptions { didSet { save(pen, "pen") } }
    var contentAwareFill: ContentAwareFillOptions {
        didSet { save(contentAwareFill, "contentAwareFill") }
    }
    var fillDialog: FillDialogOptions { didSet { save(fillDialog, "fillDialog") } }
    var strokeDialog: StrokeDialogOptions { didSet { save(strokeDialog, "strokeDialog") } }
    var colorRangeDialog: ColorRangeDialogOptions {
        didSet { save(colorRangeDialog, "colorRangeDialog") }
    }
    var fill: FillToolOptions { didSet { save(fill, "fill") } }
    var gradient: GradientToolOptions { didSet { save(gradient, "gradient") } }
    var shape: ShapeToolOptions { didSet { save(shape, "shape") } }
    var text: TextToolOptions { didSet { save(text, "text") } }
    var sample: SampleToolOptions { didSet { save(sample, "sample") } }
    var view: ViewToolOptions { didSet { save(view, "view") } }
    var sharedState: SharedToolState { didSet { save(sharedState, "shared") } }

    /// The paint-family options for a paint tool; nil for the rest.
    ///
    /// Size is floored at the tool's own minimum on the way out AND on the way
    /// in (`EditorTool.minimumBrushSize` — three for the healing brushes,
    /// whose solve can do nothing with a footprint two pixels across) and capped
    /// at its maximum (`EditorTool.maximumBrushSize`). Both
    /// directions, because this is the one read every caller shares — the
    /// options bar, the canvas sync and the `[` key's read-modify-write — and
    /// a blob saved by an earlier build can hold anything.
    func paintOptions(for tool: EditorTool) -> PaintToolOptions? {
        var options: PaintToolOptions
        switch tool {
        case .brush: options = brush
        case .eraser: options = eraser
        case .clone: options = clone
        case .dodge: options = dodge
        case .heal: options = heal
        case .spotHeal: options = spotHeal
        case .blurBrush: options = blurBrush
        case .sharpenBrush: options = sharpenBrush
        case .smudge: options = smudge
        case .sponge: options = sponge
        case .historyBrush: options = historyBrush
        case .artHistoryBrush: options = artHistoryBrush
        case .remove: options = remove
        default: return nil
        }
        options.size = min(max(options.size, tool.minimumBrushSize), tool.maximumBrushSize)
        return options
    }

    func setPaintOptions(_ options: PaintToolOptions, for tool: EditorTool) {
        var options = options
        options.size = min(max(options.size, tool.minimumBrushSize), tool.maximumBrushSize)
        switch tool {
        case .brush: brush = options
        case .eraser: eraser = options
        case .clone: clone = options
        case .dodge: dodge = options
        case .heal: heal = options
        case .spotHeal: spotHeal = options
        case .blurBrush: blurBrush = options
        case .sharpenBrush: sharpenBrush = options
        case .smudge: smudge = options
        case .sponge: sponge = options
        case .historyBrush: historyBrush = options
        case .artHistoryBrush: artHistoryBrush = options
        case .remove:
            // Size, Sample and the output are the whole tool. The tip keys
            // (`{` `}` `,` `.`) reach here through the shared paint path, and
            // a stored hardness, angle or roundness would bend a hover ring
            // whose stroke is always a hard round, so they are not kept.
            var kept = remove
            kept.size = options.size
            kept.sample = options.sample
            kept.outputToNewLayer = options.outputToNewLayer
            remove = kept
        default: break
        }
    }

    private init() {
        select = Self.load("select", default: SelectToolOptions())
        crop = Self.load("crop", default: CropToolOptions())
        move = Self.load("move", default: MoveToolOptions())
        brush = Self.load("brush", default: PaintToolOptions(), droppingStale: Self.paintStale)
            .migratedToLiveTip
        eraser = Self.load("eraser", default: PaintToolOptions(), droppingStale: Self.paintStale)
            .migratedToLiveTip
        clone = Self.load("clone", default: PaintToolOptions(), droppingStale: Self.paintStale)
            .migratedToLiveTip
        dodge = Self.load(
            "dodge", default: PaintToolOptions(opacity: 50), droppingStale: Self.paintStale)
            .migratedToLiveTip
        heal = Self.load("heal", default: PaintToolOptions(), droppingStale: Self.paintStale)
            .migratedToLiveTip
        // Spot healing generates its own source from the layer it is used on;
        // sampling the composite there would inpaint from pixels the layer
        // does not own (§0.5), so its default is off — the one difference.
        spotHeal = Self.load(
            "spotHeal", default: PaintToolOptions(sample: .current),
            droppingStale: Self.paintStale)
            .migratedToLiveTip
        // Strength 50, as Dodge / Burn's Exposure; the History Brush paints
        // the state back at full opacity.
        blurBrush = Self.retouchBrush("blurBrush", strength: 50)
        sharpenBrush = Self.retouchBrush("sharpenBrush", strength: 50)
        smudge = Self.retouchBrush("smudge", strength: 50)
        sponge = Self.retouchBrush("sponge", strength: 50)
        historyBrush = Self.retouchBrush("historyBrush", strength: 100)
        artHistoryBrush = Self.retouchBrush("artHistoryBrush", strength: 100)
        // A 40 px brush, and the layer's own pixels as Content-Aware Fill
        // samples them by default.
        remove = Self.load(
            "remove", default: PaintToolOptions(size: 40, sample: .current),
            droppingStale: Self.paintStale)
            .migratedToLiveTip
        removeTool = Self.load("removeTool", default: RemoveToolOptions())
        patch = Self.load("patch", default: PatchToolOptions())
        contentAwareMove = Self.load("contentAwareMove", default: ContentAwareMoveToolOptions())
        redEye = Self.load("redEye", default: RedEyeToolOptions())
        pen = Self.load("pen", default: PenToolOptions())
        contentAwareFill = Self.load("contentAwareFill", default: ContentAwareFillOptions())
        fillDialog = Self.load("fillDialog", default: FillDialogOptions())
        strokeDialog = Self.load("strokeDialog", default: StrokeDialogOptions())
        colorRangeDialog = Self.load("colorRangeDialog", default: ColorRangeDialogOptions())
        fill = Self.load("fill", default: FillToolOptions())
        gradient = Self.load("gradient", default: GradientToolOptions())
        shape = Self.load("shape", default: ShapeToolOptions())
        text = Self.load("text", default: TextToolOptions())
        sample = Self.load("sample", default: SampleToolOptions())
        view = Self.load("view", default: ViewToolOptions(), droppingStale: Self.viewStale)
        sharedState = Self.load("shared", default: SharedToolState())
    }

    /// One of the retouch brushes' paint slots, loaded as the older paint tools are.
    private static func retouchBrush(_ name: String, strength: Double) -> PaintToolOptions {
        load(name, default: PaintToolOptions(opacity: strength), droppingStale: paintStale)
            .migratedToLiveTip
    }

    private static func key(_ name: String) -> String { "toolOptions.\(name)" }

    /// key to drop → marker key; the marker's ABSENCE marks a blob older
    /// than the key's default change.
    private static let viewStale = ["rulers": "rulerUnitIndex"]
    private static let paintStale = ["hardness": "tipVersion"]

    /// The saved blob overlaid on `fallback`: a key the blob carries keeps
    /// its saved value and a key it lacks (a field added since) takes the
    /// default, so adding a field never resets the rest. A key in `stale` is
    /// dropped first when the blob lacks its marker key — a blob saved before
    /// that key's default changed. A missing blob, an unparsable one or a
    /// field whose type changed falls back to `fallback` whole.
    private static func load<T: Codable>(
        _ name: String, default fallback: T, droppingStale stale: [String: String] = [:]
    ) -> T {
        guard let data = UserDefaults.standard.data(forKey: key(name)),
              var saved = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              let baseData = try? JSONEncoder().encode(fallback),
              let base = (try? JSONSerialization.jsonObject(with: baseData)) as? [String: Any]
        else { return fallback }
        for (staleKey, marker) in stale where saved[marker] == nil {
            saved.removeValue(forKey: staleKey)
        }
        guard let merged = try? JSONSerialization.data(withJSONObject: overlay(saved, on: base)),
              let value = try? JSONDecoder().decode(T.self, from: merged)
        else { return fallback }
        return value
    }

    /// `saved`'s values replace `base`'s, recursing where both are objects.
    private static func overlay(_ saved: [String: Any], on base: [String: Any]) -> [String: Any] {
        var out = base
        for (key, value) in saved {
            if let inner = value as? [String: Any], let baseInner = base[key] as? [String: Any] {
                out[key] = overlay(inner, on: baseInner)
            } else {
                out[key] = value
            }
        }
        return out
    }

    private func save<T: Encodable>(_ value: T, _ name: String) {
        guard let data = try? JSONEncoder().encode(value) else { return }
        UserDefaults.standard.set(data, forKey: Self.key(name))
    }
}
