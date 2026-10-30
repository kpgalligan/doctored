import AppKit

// In Quick Mask Fill…, its twin and the four chords fill the whole Quick Mask coverage
// (`QuickMaskFill`: bytes through `PlaneScratchOp.fillSelection`, native u16 through the plane
// `blend`) as one file-clean "Fill" step through `ImageDocument.applyQuickMaskFill`
// (`applySelectionState`), at every depth
// (`validatePrecisionItem`,
// `DocumentPrecision.refusal` and `RetainedImagePolicy.refusal` admit it); the UI records
// `fill_selection` with `target: "quick_mask"`, which refuses by name outside Quick Mask; an MCP
// `fill_selection` with no `target` is recorded pinned to `"quick_mask"` in Quick Mask and
// `"layer"` outside it (`ActionRecorder.pinningModeDefaults`), and `stroke_selection` still
// refuses there.

/// Edit ▸ Fill… and the four ⌥⌫ / ⌘⌫ fills in Quick Mask: ONE colour
/// through a blend mode at an opacity over the whole Quick Mask coverage.
/// The colour becomes its coverage gray (Rec. 709 luma, the coverage-target
/// rule), so white adds to the selection and black removes it.
///
/// The byte path is Fill…'s alpha-channel path (`PlaneScratchOp.fillSelection`
/// on an opaque gray scratch); the native path is Apply Image's plane blend
/// (`precision_plane_math::blend`) on the exact u16 coverage. The commit is
/// `ImageDocument.applyQuickMaskFill`, beside `applyQuickMaskStroke`.
struct QuickMaskFill {
    let color: NSColor
    let mode: DrBlendMode
    /// 0…1, the core's unit.
    let opacity: Double

    /// Refuses the four HSL modes by name (`FillSettings.coverageBlendRefusal`)
    /// and a non-finite opacity; a finite opacity is clamped to 0…1.
    init(color: NSColor, mode: DrBlendMode, opacity: Double) throws {
        guard !DrBlendMode.degeneratesOnGray(mode) else {
            throw DoctoredCoreError(message: FillSettings.coverageBlendRefusal)
        }
        guard opacity.isFinite else {
            throw DoctoredCoreError(message: "The fill opacity must be a finite number from 0 to 1.")
        }
        self.color = color
        self.mode = mode
        self.opacity = min(max(opacity, 0), 1)
    }

    /// The byte coverage after the fill; nil `coverage` is an empty Quick
    /// Mask. Answers the input unchanged when the core reports no change.
    func applying(
        to coverage: [UInt8]?, width: Int, height: Int, space: NSColorSpace
    ) throws -> [UInt8] {
        let count = width * height
        let base = coverage ?? [UInt8](repeating: 0, count: count)
        guard width > 0, height > 0, base.count == count else {
            throw DoctoredCoreError(message: "Quick Mask coverage does not match the canvas.")
        }
        guard let bytes = ColorProfile.bytes(color, in: ColorProfile.coverageSpace(space, for: color)),
              let gray = PlaneAlgebra.coverageColor(bytes)
        else { throw DoctoredCoreError(message: "Could not convert the fill colour.") }
        var rgba = [UInt8](repeating: 255, count: count * 4)
        for index in 0..<count {
            let value = base[index]
            rgba[index * 4] = value
            rgba[index * 4 + 1] = value
            rgba[index * 4 + 2] = value
        }
        guard let image = RasterImage.from(rgba: rgba, width: width, height: height),
              let scratch = RasterDocument.from(image: image)
        else { throw DoctoredCoreError(message: "Could not read the Quick Mask coverage.") }
        let op = PlaneScratchOp.fillSelection(rgba: gray, mode: mode, opacity: opacity)
        guard let edited = op.run(scratch, in: nil, mask: nil) else { return base }
        guard let result = edited.layerImage(0)?.lumaPlane(), result.count == count else {
            throw DoctoredCoreError(message: "Could not read the filled Quick Mask coverage.")
        }
        return result
    }

    /// The exact u16 coverage after the fill; nil `coverage` is an empty
    /// Quick Mask. A fill whose effective opacity is 0 answers the input.
    func applyingNative(
        to coverage: [UInt16]?, width: Int, height: Int, space: NSColorSpace
    ) throws -> [UInt16] {
        let count = width * height
        let base = coverage ?? [UInt16](repeating: 0, count: count)
        guard width > 0, height > 0, base.count == count else {
            throw DoctoredCoreError(message: "Quick Mask coverage does not match the canvas.")
        }
        let ink = try PrecisionInk(color: color, space: ColorProfile.coverageSpace(space, for: color))
        let gray = try PrecisionScalarOperations.ink(ink)
        let effective = opacity * ink.rgba[3]
        guard effective > 0 else { return base }
        _ = try RasterResources.shared.admit(scratch: UInt64(count) * 4 + 4096)
        let source = [UInt16](repeating: gray, count: count)
        return try NativeSelectionWork(
            width: width, height: height,
            operation: ["kind": "blend", "channels": 1, "mode": Int(mode.rawValue),
                        "opacity": effective],
            input: base + source
        ).run(title: "Fill")
    }
}
