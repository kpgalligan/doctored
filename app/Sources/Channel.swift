import AppKit

/// One row of the Channels panel, top to bottom: the colour composite, its
/// three colour planes, the active layer's mask (only while it has one),
/// then every alpha channel the document carries.
enum ChannelRow: Equatable {
    /// "RGB" — the colour composite, and the `.layer` paint target.
    case composite
    /// Red, Green, Blue (never `.alpha`/`.luma`/`.mask`: those have no row).
    case plane(RasterPlane)
    /// "<layer name> Mask", present only while the active layer has one.
    case layerMask
    /// A document channel, by index into `RasterDocument.channelCount`.
    case alpha(Int)
    /// One ink plane of a CMYK document (#1): selecting it shows the plane and targets it for
    /// the Move tool and ⌘T on its selected pixels (`PaintTarget.ink`); painting, filling and
    /// adjusting one plate refuse by name.
    case ink(CmykPlane)

    /// The paint target selecting this row points brush/eraser at.
    var paintTarget: PaintTarget {
        switch self {
        case .composite: return .layer
        case .plane(let plane): return .plane(plane)
        case .layerMask: return .mask
        case .alpha(let index): return .channel(index)
        case .ink(let plane): return .ink(plane)
        }
    }

    /// The row an edit target stands for — `paintTarget`'s inverse, and what
    /// lets a command act on "the selected row" without the panel having to
    /// tell anyone which row that is.
    ///
    /// nil for a target with no row of its own: `.plane(.alpha)` (a
    /// legitimate target reachable from MCP and Load Selection, deliberately
    /// unrepresented in the list), the two read-only planes, and a `.channel`
    /// index past the current list.
    static func forTarget(_ target: PaintTarget, channelCount: Int) -> ChannelRow? {
        switch target {
        case .layer: return .composite
        case .mask: return .layerMask
        case .plane(let plane):
            switch plane {
            case .red, .green, .blue: return .plane(plane)
            case .alpha, .luma, .mask: return nil
            }
        case .channel(let index):
            return (index >= 0 && index < channelCount) ? .alpha(index) : nil
        case .ink(let plane): return .ink(plane)
        }
    }

    /// Where Load Selection reads this row's plane from, given the layer the
    /// panel is showing a mask for. On a CMYK document (#1) the composite row loads the
    /// composite's luminosity and an ink row its plate's stored `1 − ink`.
    func selectionSource(activeLayer: Int, isCmyk: Bool) -> SelectionSource {
        switch self {
        // The composite row loads what the picture covers — its alpha.
        case .composite: return isCmyk ? .compositePlane(.luma) : .compositePlane(.alpha)
        case .plane(let plane): return .compositePlane(plane)
        case .layerMask: return .layerMask(activeLayer)
        case .alpha(let index): return .channel(index)
        case .ink(let plane): return .inkPlate(plane)
        }
    }
}

/// Where Load Selection (and the agent's `load_selection`) reads a plane
/// from. Every case resolves to a canvas-sized coverage plane.
enum SelectionSource: Equatable {
    case channel(Int)
    /// A layer's transparency (its straight alpha, canvas-sized).
    case layerAlpha(Int)
    case layerMask(Int)
    /// Red / green / blue / luma / alpha of the flattened composite.
    case compositePlane(RasterPlane)
    /// One ink plate of a CMYK document's composite (#1), loaded as its stored `1 − ink`.
    case inkPlate(CmykPlane)
}

/// Where Save Selection writes.
enum SelectionDestination: Equatable {
    case newChannel(String)
    case channel(Int)
}

/// Where a LAYER-SPACE plane sits on the canvas: the layer's own rect, plus
/// the canvas it has to be expanded into before `withLayerPlane` (which
/// speaks canvas coordinates) can take it back.
///
/// It exists for the NEIGHBOURHOOD filters. Reading a colour plane
/// canvas-sized puts 0 everywhere outside the layer's rect, and a blur, a
/// sharpen, an emboss or an edge detect then mixes that black into the
/// layer's border — a colour fringe (one plane darkened, the others not)
/// that the same filter on the whole layer never produces. Read at the
/// LAYER's size the filter sees the layer's own pixels at its edge, exactly
/// as the whole-layer path does.
struct PlanePlacement: Equatable {
    let x: Int
    let y: Int
    let width: Int
    let height: Int
    let canvasWidth: Int
    let canvasHeight: Int

    /// A canvas-sized plane with everything OUTSIDE the layer's rect set to
    /// 0 — the clip a wash of a layer plane needs. `layerPlane` reads every
    /// plane canvas-sized with 0 outside the rect, so a plane that is then
    /// INVERTED (a mask rubylith draws where the mask hides) reads 255 out
    /// there and would wash the whole canvas around a small layer. A length
    /// mismatch returns the plane untouched, the `PlaneAlgebra.combine`
    /// convention.
    func clipping(_ plane: [UInt8]) -> [UInt8] {
        guard plane.count == canvasWidth * canvasHeight, canvasWidth > 0, canvasHeight > 0
        else { return plane }
        var out = plane
        for row in 0..<canvasHeight {
            let inRows = row >= y && row < y + height
            for column in 0..<canvasWidth where !inRows || column < x || column >= x + width {
                out[row * canvasWidth + column] = 0
            }
        }
        return out
    }

    /// A CANVAS-sized plane mapped into the layer's own grid — the inverse
    /// of `canvasPlane(from:)`, and the reader a canvas-sized SELECTION mask
    /// goes through before it can gate an op running in layer space.
    ///
    /// A layer pixel that falls outside the canvas reads 0, which is exactly
    /// the core's own rule for the same clip (`doc_select.rs`'s
    /// `mask_coverage`: "a selection never extends beyond the canvas"), so a
    /// plane fill or gradient confined by a selection covers precisely the
    /// pixels the whole-layer path would. nil on a length mismatch, which
    /// the caller turns into a refusal rather than an unconfined edit.
    func layerPlane(from plane: [UInt8]) -> [UInt8]? {
        guard plane.count == canvasWidth * canvasHeight, width > 0, height > 0 else {
            return nil
        }
        var out = [UInt8](repeating: 0, count: width * height)
        for row in 0..<height {
            let cy = y + row
            guard cy >= 0, cy < canvasHeight else { continue }
            for column in 0..<width {
                let cx = x + column
                guard cx >= 0, cx < canvasWidth else { continue }
                out[row * width + column] = plane[cy * canvasWidth + cx]
            }
        }
        return out
    }

    /// A layer-sized plane expanded into a canvas-sized one, 0 outside the
    /// layer's rect — for DRAWING it. A live preview has only the canvas to
    /// show a plane on, and 0 outside the layer is exactly what a
    /// canvas-sized read of that plane shows there anyway.
    ///
    /// It is deliberately NOT how a filtered plane is written back: this
    /// expansion drops every sample outside the canvas, so writing through
    /// it would filter one colour plane only inside the canvas and leave an
    /// oversized layer's off-canvas ring at its old values — a seam the next
    /// Canvas Size or Move reveals. The commit writes the layer-space bytes
    /// with `RasterDocument.applyingLayerPlane` instead. nil on a length
    /// mismatch.
    func canvasPlane(from plane: [UInt8]) -> [UInt8]? {
        guard plane.count == width * height, canvasWidth > 0, canvasHeight > 0 else {
            return nil
        }
        var out = [UInt8](repeating: 0, count: canvasWidth * canvasHeight)
        for row in 0..<height {
            let cy = y + row
            guard cy >= 0, cy < canvasHeight else { continue }
            for column in 0..<width {
                let cx = x + column
                guard cx >= 0, cx < canvasWidth else { continue }
                out[cy * canvasWidth + cx] = plane[row * width + column]
            }
        }
        return out
    }
}

/// A fill or a gradient, in CANVAS coordinates, ready to run on the scratch
/// document a plane target became — the ONE place the canvas geometry of a
/// plane edit is mapped into the space that plane lives in.
///
/// A COLOUR plane's scratch document is the layer's own pixel grid (its
/// `PlanePlacement`), so the seed, the endpoints and the canvas-sized
/// selection mask all shift by the layer's offset; an ALPHA CHANNEL is
/// canvas-sized document state (`placement` nil) and everything passes
/// through untouched.
///
/// Layer space, not canvas space, is what makes a plane fill or gradient
/// agree with the same tool on the whole layer: an oversized layer's
/// off-canvas ring is filled with the rest (a canvas-sized write would leave
/// that ONE plane at its old values — a colour seam the next Canvas Size or
/// Move reveals), and a contiguous fill cannot grow through the zeros a
/// canvas-sized read leaves outside the layer's rect.
///
/// Shared by the tools (EditorViewController+PlanePaint) and their MCP
/// mirrors (AgentServer+ChannelTargets), so the two spellings of fill and
/// gradient on a plane can never drift.
enum PlaneScratchOp {
    /// The Fill tool's region grow. The seed is a canvas PIXEL, as both
    /// callers already have it (`Int(point.x)` on the canvas, `x` from MCP).
    case fill(x: Int, y: Int, rgba: [UInt8], tolerance: Int, contiguous: Bool)
    /// `stops` nil is the two-colour gradient; otherwise the stops gradient,
    /// seven floats per stop as `gradientedStops` takes them, and
    /// `start` / `end` are unread; `smoothness` (a fraction, 1 the
    /// Classic ramp) is read with stops only.
    case gradient(
        from: CGPoint, to: CGPoint, start: [UInt8], end: [UInt8], kind: DrGradientKind,
        stops: [Float]? = nil, smoothness: Double = 1)
    /// Edit ▸ Fill… on a coverage target: `rgba` is already the target's
    /// gray (`PlaneAlgebra.fillColor`), `opacity` is 0…1 (the core's
    /// unit). Preserve Transparency has no meaning on a plane.
    case fillSelection(rgba: [UInt8], mode: DrBlendMode, opacity: Double)

    /// Runs on layer 0 of `scratch` — the one-layer document the plane
    /// became — with the geometry and `mask` mapped into `placement`'s
    /// space. nil when the core refuses (a seed outside the plane, a
    /// degenerate gradient, a mask that no longer matches the canvas).
    func run(
        _ scratch: RasterDocument, in placement: PlanePlacement?, mask: [UInt8]?
    ) -> RasterDocument? {
        let local: [UInt8]?
        if let mask = mask, let placement = placement {
            guard let mapped = placement.layerPlane(from: mask) else { return nil }
            local = mapped
        } else {
            local = mask
        }
        let dx = placement?.x ?? 0
        let dy = placement?.y ?? 0
        switch self {
        case .fill(let x, let y, let rgba, let tolerance, let contiguous):
            return scratch.bucketFilled(
                0, x: x - dx, y: y - dy, tolerance: tolerance, rgba: rgba,
                contiguous: contiguous, mask: local)
        case .gradient(let from, let to, let start, let end, let kind, let stops, let smoothness):
            let p0 = CGPoint(x: from.x - CGFloat(dx), y: from.y - CGFloat(dy))
            let p1 = CGPoint(x: to.x - CGFloat(dx), y: to.y - CGFloat(dy))
            if let stops {
                return scratch.gradientedStops(
                    0, from: p0, to: p1, stops: stops, kind: kind, smoothness: smoothness,
                    mask: local)
            }
            return scratch.gradiented(
                0, from: p0, to: p1, start: start, end: end, kind: kind, mask: local)
        case .fillSelection(let rgba, let mode, let opacity):
            return scratch.selectionFilled(
                0, mask: local, rgba: rgba, mode: mode, opacity: opacity,
                preserveTransparency: false)
        }
    }
}

/// Selection algebra on raw planes — THE one implementation of the four
/// per-byte formulas: replace = b, add = max(a, b), subtract = min(a, 255−b),
/// intersect = min(a, b). `CanvasSelection.combine` calls into this instead
/// of restating them, so the rules live in one place; a plane-level entry
/// point exists because a channel is not a `CanvasSelection` (it may
/// legitimately be all-zero, which `CanvasSelection.init?` rejects).
enum PlaneAlgebra {
    /// `a` combined with `b`. A length mismatch returns `a` unchanged — the
    /// operands are always canvas-sized in practice, and refusing loudly
    /// here would turn a caller's stale plane into a crash.
    static func combine<T: FixedWidthInteger & UnsignedInteger>(_ a: [T], _ b: [T], mode: SelectionCombineMode) -> [T] {
        guard mode != .replace else { return b }
        guard a.count == b.count else { return a }
        var out = a
        switch mode {
        case .replace, .add:
            for i in out.indices { out[i] = max(out[i], b[i]) }
        case .subtract:
            for i in out.indices { out[i] = min(out[i], T.max - b[i]) }
        case .intersect:
            for i in out.indices { out[i] = min(out[i], b[i]) }
        }
        return out
    }

    /// The plane's complement, per byte 255 − v.
    static func inverted(_ a: [UInt8]) -> [UInt8] {
        a.map { 255 - $0 }
    }

    /// A tool colour (straight RGBA bytes) as the opaque gray a PLANE is
    /// filled with: the colour's Rec. 709 LUMA in all three components, its
    /// alpha — which carries the tool's opacity — untouched.
    ///
    /// The luma is READ BACK through the core (a one-pixel image through
    /// `DR_PLANE_LUMA`) rather than recomputed here: the coefficients have
    /// exactly one home (`blend.rs`'s `LUMA_*`), and this is the same
    /// reduction the plane round trip applies to an op's result, so filling
    /// a plane with a colour and filtering it agree on what that colour's
    /// gray is. nil only when the core refuses the probe, which leaves the
    /// caller to refuse rather than fill with a made-up value. Shared by the
    /// fill/gradient tools and their MCP mirrors.
    static func coverageColor(_ rgba: [UInt8]) -> [UInt8]? {
        guard rgba.count == 4,
              let probe = RasterImage.from(
                rgba: [rgba[0], rgba[1], rgba[2], 255], width: 1, height: 1),
              let gray = probe.lumaPlane()?.first
        else { return nil }
        return [gray, gray, gray, rgba[3]]
    }

    /// The gray Edit ▸ Fill… and Stroke… write into an 8-bit coverage
    /// `target` for a tool colour (straight RGBA bytes), its alpha kept. One
    /// colour plane of a layer (Red, Green, Blue or Alpha) takes the colour's
    /// 0.30 / 0.59 / 0.11 luma over its 8-bit codes, rounded to a code (the
    /// core's `dr_plane_ink_u8`, measured against Photoshop by
    /// `scripts/photoshop-probe/plane_gray.py`). A mask or an alpha channel keeps
    /// `coverageColor`'s Rec. 709 gray. nil for a layer target, which is not
    /// filled with a gray, and when the core refuses.
    static func fillColor(_ rgba: [UInt8], target: PaintTarget) -> [UInt8]? {
        switch target {
        case .layer, .ink:
            return nil
        case .mask, .channel:
            return coverageColor(rgba)
        case .plane:
            guard rgba.count == 4,
                  let gray = RasterSelection.planeInk8(
                    red: rgba[0], green: rgba[1], blue: rgba[2])
            else { return nil }
            return [gray, gray, gray, rgba[3]]
        }
    }
}

extension RasterDocument {
    /// The Channels panel's rows for `activeLayer`, in display order. The
    /// mask row appears only while that layer has a mask; the alpha rows
    /// follow the document's own channel order.
    func channelRows(activeLayer: Int) -> [ChannelRow] {
        // #1: a CMYK document lists its composite and its four inks, then (the channels
        // batch) the active layer's mask and its alpha channels, as RGB does. A Grayscale
        // document (#1 phase 1c) lists its one Gray row, then the mask and alpha rows.
        // A Lab Color document (#1 Lab phase 1c) lists its Lab composite and its Lightness, a
        // and b rows (the R, G and B planes of its codes), then the mask and alpha rows.
        var rows: [ChannelRow] = isGray
            ? [.composite]
            : isCmyk
            ? [.composite] + CmykPlane.allCases.map { ChannelRow.ink($0) }
            : [.composite, .plane(.red), .plane(.green), .plane(.blue)]
        if activeLayer >= 0, activeLayer < layerCount, layerHasMask(activeLayer) {
            rows.append(.layerMask)
        }
        rows.append(contentsOf: (0..<channelCount).map { ChannelRow.alpha($0) })
        return rows
    }

    /// The image a row shows (a panel thumbnail, or the canvas base). The
    /// composite rows read the CACHED projection the caller passes in —
    /// this path must never call `flattened()`, which allocates 16 bytes per
    /// canvas pixel and re-runs the whole compositor. Pass nil only when
    /// there is no projection yet, in which case the composite rows answer
    /// nil and the panel draws an empty well. `maxSide` 0 means full size.
    ///
    /// Every row's bytes are THIS document's numbers — the composite row is
    /// the picture, a plane row is that picture's channel replicated into
    /// R=G=B — so a caller drawing the result tags it with the document's
    /// space, not sRGB (`ChannelsPanelViewController`'s thumbnail well).
    ///
    /// On a Lab Color document a plane row is extracted from `labCodes`, the raw-code thumbnail
    /// the panel fitted ONCE per reload (`labCodeThumbnail`, #1 Lab phase 1c): no row reads the
    /// full projection, and none reads the Display P3 view.
    func planeImage(
        for row: ChannelRow, composite: RasterImage?, activeLayer: Int, maxSide: Int,
        labCodes: RasterImage? = nil
    ) -> RasterImage? {
        if case .ink(let plane) = row {
            guard let precision, let context = try? PrecisionWorkContext(source: precision) else { return nil }
            return try? precision.cmykPlaneView(plane, maxSide: maxSide, workingLimit: context.limit,
                                                otherRetained: context.otherRetained)
        }
        if isHighDepth {
            switch row {
            case .ink: return nil
            case .composite: return composite.flatMap { Self.fitted($0, maxSide: maxSide) }
            case .plane(let plane) where isLab: return labCodes?.planeImage(plane.dr, maxSide: 0)
            case .plane(let plane): return composite?.planeImage(plane.dr, maxSide: maxSide)
            case .layerMask: return nativePlaneThumbnail(target: "mask", index: activeLayer, maxSide: maxSide)
            case .alpha(let index):
                return spotThumbnail(index, nativePlaneThumbnail(target: "channel", index: index, maxSide: maxSide))
            }
        }
        switch row {
        case .composite:
            // The RGB row is the picture itself, in colour:
            // the only row that is not a plane.
            guard let composite = composite else { return nil }
            return Self.fitted(composite, maxSide: maxSide)
        case .plane(let plane):
            return composite?.planeImage(plane.dr, maxSide: maxSide)
        case .layerMask:
            return layerPlaneImage(activeLayer, DR_PLANE_MASK, maxSide: maxSide)
        case .alpha(let index):
            // A spot channel's row is ink dark (#1, `spotThumbnail`).
            return spotThumbnail(index, channelImage(index, maxSide: maxSide))
        case .ink:
            return nil
        }
    }

    /// Aspect-fit downscale for the one row whose source is already an
    /// image rather than a plane (the core does the same arithmetic behind
    /// every `max_side` parameter; this is its Swift twin for a handle the
    /// host already holds). Never upscales; `maxSide` 0 keeps full size.
    ///
    /// Internal because the Channels panel fits the projection ONCE per
    /// reload and hands the small image back in as `composite`: a plane row
    /// then extracts its plane from a 44-pixel thumbnail instead of from the
    /// full canvas, which is the difference between a free reload and
    /// seconds of main thread on a large document.
    static func fitted(_ image: RasterImage, maxSide: Int) -> RasterImage? {
        let longest = max(image.width, image.height)
        guard maxSide > 0, longest > maxSide else { return image }
        let scale = Double(maxSide) / Double(longest)
        let w = max(Int((Double(image.width) * scale).rounded()), 1)
        let h = max(Int((Double(image.height) * scale).rounded()), 1)
        return image.resized(w: w, h: h, filter: DR_FILTER_BILINEAR)
    }

    /// The scratch-document round trip behind plane FILL and plane GRADIENT:
    /// a grayscale plane image becomes a one-layer document, an EXISTING
    /// document op runs on its layer 0, and the result's luma comes back as
    /// the plane — with no second region grow and no second gradient
    /// anywhere.
    ///
    /// The scratch document is the size of the image handed in, which is the
    /// TARGET's own space: a colour plane's layer grid, an alpha channel's
    /// canvas. `PlaneScratchOp` maps the canvas geometry and the canvas-sized
    /// selection mask into that space, so this needs to know nothing about
    /// either. Shared by the tools (EditorViewController+PlanePaint) and
    /// their MCP mirrors (AgentServer+ChannelTargets).
    static func planeThroughScratch(
        _ source: RasterImage, _ op: (RasterDocument) -> RasterDocument?
    ) -> [UInt8]? {
        guard let scratch = RasterDocument.from(image: source), let edited = op(scratch),
              let result = edited.layerImage(0)
        else { return nil }
        return result.lumaPlane()
    }

    /// Where layer `idx` sits on this canvas — its rect and the canvas
    /// around it — with no pixels read. The cheap half of
    /// `layerSpacePlaneImage`, for callers that only need to clip a
    /// canvas-sized plane to the layer (`PlanePlacement.clipping`). nil for
    /// an out-of-range index.
    func layerPlacement(_ idx: Int) -> PlanePlacement? {
        guard let info = layerInfo(idx) else { return nil }
        return PlanePlacement(
            x: info.offsetX, y: info.offsetY, width: info.width, height: info.height,
            canvasWidth: width, canvasHeight: height)
    }

    /// One colour plane of layer `idx` at the LAYER's own size, as an opaque
    /// grayscale image, plus where it sits on the canvas — the reader every
    /// FILTER and adjustment on a colour plane uses (`PlanePlacement` says
    /// why it is not the canvas-sized `layerPlaneImage`).
    ///
    /// The plane comes out of the layer's own pixels through the same
    /// `dr_image_plane_image` reader the panel's thumbnails use, so there is
    /// no second plane extractor. nil for an out-of-range layer or a plane
    /// the core will not read.
    func layerSpacePlaneImage(
        _ idx: Int, _ plane: DrPlane
    ) -> (image: RasterImage, placement: PlanePlacement)? {
        guard let info = layerInfo(idx),
              let image = layerImage(idx)?.planeImage(plane, maxSide: 0)
        else { return nil }
        return (
            image,
            PlanePlacement(
                x: info.offsetX, y: info.offsetY, width: image.width, height: image.height,
                canvasWidth: width, canvasHeight: height)
        )
    }

    /// The canvas-sized plane behind a Load Selection source, or nil when
    /// the source is gone (a deleted channel, a layer index that shifted, a
    /// mask that was removed). Every `SelectionSource` carries its own
    /// index, so this needs no active layer of its own — the caller resolves
    /// "the active layer" when it BUILDS the source.
    func selectionPlane(for source: SelectionSource) -> [UInt8]? {
        switch source {
        case .channel(let index):
            return channelPlane(index)
        case .layerAlpha(let idx):
            return layerPlane(idx, DR_PLANE_ALPHA)
        case .layerMask(let idx):
            return layerPlane(idx, DR_PLANE_MASK)
        case .compositePlane(let plane):
            // The luminosity loads in Color Settings' Gray working space
            // (`core/src/gray_working.rs`); every other luma reader stays Rec. 709.
            if plane == .luma { return compositeGrayPlane(ColorSettings.workingGray) }
            return compositePlane(plane.dr)
        case .inkPlate:
            // A CMYK document is high depth: its plates load through the native path
            // (`PrecisionScalarOperations.canvasPlane`), never this byte one.
            return nil
        }
    }

    /// The next free default channel name ("Alpha 1", "Alpha 2", …) — the
    /// lowest number no existing channel already uses, so deleting the
    /// middle of a run reuses that number rather than counting past it.
    var nextChannelName: String {
        let taken = Set((0..<channelCount).compactMap { channelInfo($0)?.name })
        var n = 1
        while taken.contains("Alpha \(n)") { n += 1 }
        return "Alpha \(n)"
    }

    /// The first channel with this name, or nil. Names are not unique (the
    /// core does not enforce it), so "first" is the rule everywhere.
    func channelIndex(named name: String) -> Int? {
        (0..<channelCount).first { channelInfo($0)?.name == name }
    }

    /// The channel carrying this stable id (`ChannelInfo.id`), or nil once it
    /// is gone. 0 — the core's "no such channel" — is never found.
    ///
    /// Every insert, delete and undo renumbers the list, so anything holding
    /// a channel across a document change — a paint target, an eye, an open
    /// field editor or dialog — comes back through here instead of trusting
    /// the index it captured.
    func channelIndex(forID id: UInt64) -> Int? {
        guard id != 0 else { return nil }
        return (0..<channelCount).first { channelID($0) == id }
    }
}
