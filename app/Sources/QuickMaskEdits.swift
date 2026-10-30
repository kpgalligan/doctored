import AppKit

// Gaussian Blur and the Gradient tool in Quick Mask. Quick Mask shows
// the selection's coverage as a paintable plane, so the two edits a mask is
// most often given there now work: Filters ▸ Blur softens the coverage and a
// Gradient drag lays a ramp of coverage over it. Each is one file-clean step
// through `ImageDocument.applyQuickMaskCoverage`, beside the Quick Mask
// stroke and the Quick Mask fill (`QuickMaskFill.swift`, whose shape these
// follow). The UI entries are in `EditorViewController+QuickMaskEdits.swift`
// and the MCP branches in `AgentServer+QuickMaskEdits.swift`.

/// Gaussian Blur over the Quick Mask coverage: the selection's own feather,
/// asked for by sigma. `CanvasSelection.feathered(by:)` takes a radius whose
/// sigma is `0.3·(radius − 1) + 0.8` (the core's mask-feather rule), so the
/// radius for a sigma is `(sigma − 0.8) / 0.3 + 1`, and the feather's
/// 0.5…250 px range is sigma 0.65…75.5.
struct QuickMaskBlur {
    static let sigmaRange = 0.65...75.5
    static let refusal = "In Quick Mask, Gaussian Blur softens the Quick Mask coverage with a "
        + "sigma from 0.65 to 75.5 pixels."

    let sigma: Double

    /// Refuses, by name, a sigma that is not finite or is outside 0.65…75.5.
    init(sigma: Double) throws {
        guard sigma.isFinite, Self.sigmaRange.contains(sigma) else {
            throw DoctoredCoreError(message: Self.refusal + " (got \(sigma))")
        }
        self.sigma = sigma
    }

    /// The feather radius whose Gaussian has this sigma.
    var featherRadius: Double { (sigma - 0.8) / 0.3 + 1 }

    /// The blurred coverage; nil when nothing remains.
    func applying(to selection: CanvasSelection) -> CanvasSelection? {
        selection.feathered(by: featherRadius)
    }
}

/// The Gradient tool's drag over the Quick Mask coverage: each stop's colour
/// enters as its coverage gray (Rec. 709 luma, the coverage-target rule), so
/// white adds to the selection and black removes it, and its alpha is the
/// strength. Every ramp, two-stop or not, takes the stops path here.
///
/// The byte path is the plane gradient's (`PlaneScratchOp.gradient` on an
/// opaque gray scratch, `QuickMaskFill.applying`'s shape); the native path is
/// the core's selection-plane `gradient` on the exact u16 coverage.
struct QuickMaskGradient {
    let ramp: GradientRamp
    let from: CGPoint
    let to: CGPoint

    init(ramp: GradientRamp, from: CGPoint, to: CGPoint) throws {
        guard from != to, [from.x, from.y, to.x, to.y].allSatisfy(\.isFinite) else {
            throw DoctoredCoreError(message: "A gradient needs two different, finite points.")
        }
        self.ramp = ramp
        self.from = from
        self.to = to
    }

    /// The byte coverage after the gradient; nil `coverage` is an empty
    /// Quick Mask. Answers the input unchanged when the core reports no
    /// change.
    func applying(
        to coverage: [UInt8]?, width: Int, height: Int, space: NSColorSpace
    ) throws -> [UInt8] {
        let count = width * height
        let base = coverage ?? [UInt8](repeating: 0, count: count)
        guard width > 0, height > 0, base.count == count else {
            throw DoctoredCoreError(message: "Quick Mask coverage does not match the canvas.")
        }
        guard let stops = ramp.flattened({ color in
            ColorProfile.bytes(color, in: ColorProfile.coverageSpace(space, for: color))
                .flatMap(PlaneAlgebra.coverageColor)
        }) else { throw DoctoredCoreError(message: "Could not convert the gradient's colours.") }
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
        let op = PlaneScratchOp.gradient(
            from: from, to: to, start: [0, 0, 0, 0], end: [0, 0, 0, 0], kind: ramp.style,
            stops: stops, smoothness: ramp.smoothness)
        guard let edited = op.run(scratch, in: nil, mask: nil) else { return base }
        guard let result = edited.layerImage(0)?.lumaPlane(), result.count == count else {
            throw DoctoredCoreError(message: "Could not read the Quick Mask coverage back.")
        }
        return result
    }

    /// The exact u16 coverage after the gradient; nil `coverage` is an empty
    /// Quick Mask.
    func applyingNative(
        to coverage: [UInt16]?, width: Int, height: Int, space: NSColorSpace
    ) throws -> [UInt16] {
        let count = width * height
        let base = coverage ?? [UInt16](repeating: 0, count: count)
        guard width > 0, height > 0, base.count == count else {
            throw DoctoredCoreError(message: "Quick Mask coverage does not match the canvas.")
        }
        let stops = try zip(ramp.stops, ramp.midpoints).map { stop, mid -> [String: Any] in
            let ink = try PrecisionInk(
                color: stop.color, space: ColorProfile.coverageSpace(space, for: stop.color))
            let gray = Double(try PrecisionScalarOperations.ink(ink)) / 65535
            var entry: [String: Any] = [
                "position": stop.position, "color": [gray, gray, gray, ink.rgba[3]],
            ]
            entry.merge(mid.keys) { _, new in new }
            return entry
        }
        var operation: [String: Any] = [
            "kind": "gradient", "from": [Double(from.x), Double(from.y)],
            "to": [Double(to.x), Double(to.y)], "style": ramp.shapeName,
            "stops": stops, "opacity": 1.0,
        ]
        if ramp.smoothness < 1 { operation["smoothness"] = ramp.smoothness }
        _ = try RasterResources.shared.admit(scratch: UInt64(count) * 4 + 4096)
        return try NativeSelectionWork(
            width: width, height: height,
            operation: operation,
            input: base
        ).run(title: "Gradient")
    }
}

extension ImageDocument {
    /// Filters ▸ Blur in Quick Mask and `apply_filter filter: "blur"` there:
    /// the Quick Mask coverage Gaussian-blurred as ONE file-clean "Gaussian
    /// Blur" step. False when nothing changed (an empty Quick Mask, or
    /// coverage the blur leaves as it is).
    @discardableResult
    func applyQuickMaskBlur(sigma: Double, record: [ActionStep]) throws -> Bool {
        let blur = try QuickMaskBlur(sigma: sigma)
        guard quickMaskActive else {
            throw DoctoredCoreError(message: "Quick Mask is not active; there is no Quick Mask to blur.")
        }
        guard let selection = canvasSelection else { return false }
        return try applyQuickMaskCoverage(
            { _ in blur.applying(to: selection) }, actionName: "Gaussian Blur", record: record)
    }

    /// The Gradient tool's drag in Quick Mask and `gradient` there: the ramp
    /// laid over the Quick Mask coverage as ONE file-clean "Gradient" step.
    /// Native coverage (a 16-bit or 32-bit float document, or u16 samples on
    /// a byte one) takes the exact path. False when nothing changed.
    @discardableResult
    func applyQuickMaskGradient(
        _ ramp: GradientRamp, from: CGPoint, to: CGPoint, record: [ActionStep]
    ) throws -> Bool {
        let gradient = try QuickMaskGradient(ramp: ramp, from: from, to: to)
        guard quickMaskActive else {
            throw DoctoredCoreError(
                message: "Quick Mask is not active; there is no Quick Mask to paint a gradient on.")
        }
        let space = nsColorSpace
        return try applyQuickMaskCoverage({ doc in
            let width = doc.width, height = doc.height
            if doc.isHighDepth || self.canvasSelection?.hasNativeCoverage == true {
                _ = try RasterResources.shared.admit(scratch: UInt64(width * height) * 2 + 4096)
                let samples = try gradient.applyingNative(
                    to: self.canvasSelection?.maskSamples16(), width: width, height: height,
                    space: space)
                return CanvasSelection(
                    shape: .mask16(samples), canvasWidth: width, canvasHeight: height)
            }
            let bytes = try gradient.applying(
                to: self.canvasSelection?.maskBytes(), width: width, height: height, space: space)
            return CanvasSelection(shape: .mask(bytes), canvasWidth: width, canvasHeight: height)
        }, actionName: "Gradient", record: record)
    }
}
