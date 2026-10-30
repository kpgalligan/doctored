import AppKit

private struct ScopeProbeError: Error, CustomStringConvertible {
    let description: String
}

private func scopeRequire(_ condition: @autoclosure () -> Bool, _ message: String) throws {
    if !condition() { throw ScopeProbeError(description: message) }
}

private func scopePixels(_ image: RasterImage?) throws -> [UInt8] {
    guard let image else { throw ScopeProbeError(description: "missing target image") }
    var bytes: [UInt8] = []
    for y in 0..<image.height {
        for x in 0..<image.width {
            guard let p = image.sample(x: x, y: y, reach: 0) else {
                throw ScopeProbeError(description: "cannot sample fixture")
            }
            bytes += [p.r, p.g, p.b, p.a]
        }
    }
    return bytes
}

private func scopeDisplayBytes(_ image: CGImage?) throws -> Data {
    guard let data = image?.dataProvider?.data else {
        throw ScopeProbeError(description: "missing preview pixels")
    }
    return data as Data
}

private func scopeBlend(_ old: UInt8, _ new: UInt8, _ coverage: UInt8) -> UInt8 {
    UInt8((Int(old) * (255 - Int(coverage)) + Int(new) * Int(coverage) + 127) / 255)
}

/// The document/UI routing half of Phase 2, paired with the numerical Rust
/// scope tests and the built-app MCP/Actions workflow. No application windows
/// or modal dialogs are opened by this helper.
func verifyOperationScope() throws {
    let rgba: [UInt8] = [20, 40, 60, 255, 70, 90, 110, 255,
                         120, 140, 160, 255, 170, 190, 210, 255]
    let scalar: [UInt8] = [32, 96, 160, 224]
    let coverage: [UInt8] = [0, 128, 255, 0]
    guard let image = RasterImage.from(rgba: rgba, width: 4, height: 1),
          let initial = RasterDocument.from(image: image),
          let masked = initial.addingLayerMask(0, kind: DR_MASK_FROM_SELECTION, selection: scalar),
          let base = try masked.addingChannel(name: "Scope", plane: scalar, width: 4, height: 1),
          let selection = CanvasSelection(shape: .mask(coverage), canvasWidth: 4, canvasHeight: 1)
    else { throw ScopeProbeError(description: "cannot create operation-scope fixture") }

    for target: PaintTarget in [.layer, .mask, .plane(.red), .plane(.alpha), .channel(0)] {
        guard let document = ImageDocument.makeUntitled(with: image) else {
            throw ScopeProbeError(description: "cannot create scope document")
        }
        document.doc = base
        document.planeEditTarget = target
        document.setCanvasSelection(selection)
        while let undo = document.undoManager, undo.groupingLevel > 0 { undo.endUndoGrouping() }
        document.undoManager?.removeAllActions()
        guard let capture = document.captureOperation(),
              let preview = capture.preview({ $0.inverted() })
        else { throw ScopeProbeError(description: "cannot capture/preview \(target)") }
        let previewBytes = try scopeDisplayBytes(preview)
        document.undoManager?.beginUndoGrouping()
        document.applyToActiveLayer("Invert", record: .filter("invert", target: capture.agentTarget)) {
            $0.inverted()
        }
        document.undoManager?.endUndoGrouping()
        while let undo = document.undoManager, undo.groupingLevel > 0 { undo.endUndoGrouping() }
        guard let result = document.doc else { throw ScopeProbeError(description: "lost scope document") }
        try scopeRequire(result !== base, "\(target): filter did not commit")
        try scopeRequire(document.isDocumentEdited, "\(target): filter did not dirty")
        try scopeRequire(ActionRecorder.shared.lastRepeatable?.arguments["target"] as? String
                            == capture.agentTarget, "\(target): Actions recorded the wrong target")
        let raw = try scopePixels(result.layerImage(0))
        switch target {
        case .layer:
            for i in 0..<4 {
                for c in 0..<3 {
                    let old = rgba[i * 4 + c]
                    try scopeRequire(raw[i * 4 + c] == scopeBlend(old, 255 - old, coverage[i]),
                                     "layer filter wrote outside coverage or blended it twice")
                }
                try scopeRequire(raw[i * 4 + 3] == 255, "layer filter changed alpha")
            }
        case .plane(let plane):
            let changed = plane == .alpha ? 3 : 0
            for i in 0..<4 {
                for c in 0..<4 {
                    let old = rgba[i * 4 + c]
                    // The existing alpha-plane contract clears RGB when
                    // selected alpha changes to zero; other writes preserve it.
                    let alpha = scopeBlend(255, 0, coverage[i])
                    let expected: UInt8 = c == changed
                        ? scopeBlend(old, 255 - old, coverage[i])
                        : (plane == .alpha && alpha == 0 && c < 3 ? 0 : old)
                    try scopeRequire(raw[i * 4 + c] == expected, "plane filter rewrote another plane")
                }
            }
        case .mask, .channel:
            try scopeRequire(raw == rgba, "scalar filter rewrote the photograph")
            let actual = target == .mask ? result.layerMaskCoverage(0) : result.channelPlane(0)
            let expected = zip(scalar, coverage).map { scopeBlend($0.0, 255 - $0.0, $0.1) }
            try scopeRequire(actual == expected, "scalar filter wrote wrong selected coverage")
        case .ink:
            // A CMYK ink plate (#547) is never among this RGB probe's targets.
            throw ScopeProbeError(description: "\(target): an ink target is not an RGB scope")
        }
        let committedDisplay: CGImage?
        switch capture.previewBase {
        case .composite: committedDisplay = result.flattened()?.makeCGImage(in: result.colorSpace)
        case .plane(let plane):
            committedDisplay = result.compositePlaneImage(plane.dr, maxSide: 0)?
                .makeCGImage(in: ColorProfile.sRGB)
        case .channel(let index):
            committedDisplay = result.channelImage(index, maxSide: 0)?.makeCGImage(in: ColorProfile.sRGB)
        case .layerMask, .nothing:
            committedDisplay = nil
        case .ink:
            // A CMYK ink plane's preview; this RGB document never has one.
            throw ScopeProbeError(description: "\(target): an RGB document previewed an ink plane")
        }
        let committedBytes = try scopeDisplayBytes(committedDisplay)
        try scopeRequire(previewBytes == committedBytes, "\(target): preview and commit differ")
        document.undoManager?.undo()
        try scopeRequire(document.doc === base, "\(target): undo did not restore source")
        try scopeRequire(document.canvasSelection?.maskBytes() == coverage,
                         "\(target): filter undo lost selection")
        document.undoManager?.redo()
        try scopeRequire(document.doc === result, "\(target): redo did not restore result")
    }
    print("PASS operation scope: layer/mask/color/alpha/channel coverage, preview, Actions target, undo/redo")

    guard let document = ImageDocument.makeUntitled(with: image) else {
        throw ScopeProbeError(description: "cannot create stale-capture document")
    }
    document.doc = base
    document.setCanvasSelection(selection)
    guard let capture = document.captureOperation() else {
        throw ScopeProbeError(description: "cannot capture stale-check fixture")
    }
    try scopeRequire(capture.isCurrent(in: document), "new capture is stale")
    document.setCanvasSelection(nil)
    try scopeRequire(!capture.isCurrent(in: document), "selection change did not invalidate sheet commit")
    document.setCanvasSelection(selection)
    document.planeEditTarget = .mask
    try scopeRequire(!capture.isCurrent(in: document), "target change did not invalidate sheet commit")
    document.planeEditTarget = .layer
    document.activeLayerIndex = 1
    try scopeRequire(!capture.isCurrent(in: document), "active layer change did not invalidate sheet commit")
    document.activeLayerIndex = 0
    document.setQuickMaskActive(true)
    try scopeRequire(document.captureOperation() == nil && !capture.isCurrent(in: document),
                     "Quick Mask unexpectedly permits destructive filtering")
    document.setQuickMaskActive(false)
    document.doc = base.clone()
    try scopeRequire(!capture.isCurrent(in: document), "document revision change did not invalidate sheet commit")
    print("PASS stale scope: layer/target/selection/revision changes refuse; Quick Mask filters refuse")

    let description = "{\"type\":\"text\",\"v\":999}"
    guard let described = base.withLayerMeta(0, description),
          let maskedDocument = ImageDocument.makeUntitled(with: image)
    else { throw ScopeProbeError(description: "cannot create described-mask fixture") }
    maskedDocument.doc = described
    maskedDocument.planeEditTarget = .mask
    maskedDocument.applyToActiveLayer("Invert Mask", record: .filter("invert", target: "mask")) {
        $0.inverted()
    }
    try scopeRequire(maskedDocument.doc?.layerMeta(0) == description,
                     "mask filtering rasterized the layer description")
    let preservedPixels = try scopePixels(maskedDocument.doc?.layerImage(0))
    try scopeRequire(preservedPixels == rgba, "mask filtering changed described pixels")
    try scopeRequire(maskedDocument.doc?.layerMaskCoverage(0) == scalar.map { 255 - $0 },
                     "described layer mask filter did not commit")
    print("PASS described layer mask: filters preserve pixels and editable metadata without rasterizing")

    for target: PaintTarget in [.mask, .plane(.red), .channel(0)] {
        guard let capture = OperationCapture(doc: base, layerIndex: 0, target: target, selection: coverage),
              let filled = capture.applyingScratch(.fill(x: 1, y: 0, rgba: [255, 255, 255, 255],
                                                        tolerance: 255, contiguous: false)),
              let gradient = capture.applyingScratch(.gradient(
                from: .zero, to: CGPoint(x: 3, y: 0), start: [255, 255, 255, 255],
                end: [0, 0, 0, 255], kind: DR_GRADIENT_LINEAR))
        else { throw ScopeProbeError(description: "scalar fill/gradient refused \(target)") }
        for (name, output) in [("fill", filled), ("gradient", gradient)] {
            guard let read = OperationCapture(doc: output, layerIndex: 0, target: target, selection: nil)
            else { throw ScopeProbeError(description: "missing scalar result") }
            let old = try scopePixels(capture.source)
            let new = try scopePixels(read.source)
            try scopeRequire(Array(new[0..<4]) == Array(old[0..<4])
                             && Array(new[12..<16]) == Array(old[12..<16]),
                             "\(target) \(name): unselected samples changed")
            if target == .mask || target.isChannel {
                let pixels = try scopePixels(output.layerImage(0))
                try scopeRequire(pixels == rgba, "\(target) \(name): layer pixels changed")
            }
            if name == "fill" {
                try scopeRequire(new[4] == scopeBlend(old[4], 255, coverage[1]),
                                 "\(target) fill: feathered coverage applied twice")
            }
        }
    }
    print("PASS scalar fill/gradient: selected writes on mask/plane/channel preserve unselected pixels")

    guard let pixelLocked = try? base.withLayerLocks(0, DR_LOCK_PIXELS.rawValue),
          let allLocked = try? base.withLayerLocks(0, DR_LOCK_ALL.rawValue),
          let group = try? initial.groupLayers([0], name: "Scope Group"),
          let groupMasked = group.document.addingLayerMask(
            group.group, kind: DR_MASK_FROM_SELECTION, selection: scalar),
          let moved = try? groupMasked.withLayerOffset(group.group, 1, 0),
          let groupCapture = OperationCapture(
            doc: moved, layerIndex: group.group, target: .mask, selection: [0, 255, 0, 0]),
          let groupResult = groupCapture.applying({ $0.inverted() }),
          let groupStorage = groupResult.layerMaskStorage(group.group)
    else { throw ScopeProbeError(description: "cannot create lock/group scope fixtures") }
    try scopeRequire(OperationScope.refusalReason(.filter, target: .mask, doc: pixelLocked,
                                                 layerIndex: 0) == nil,
                     "Pixels lock incorrectly blocks the mask")
    try scopeRequire(OperationScope.refusalReason(.filter, target: .mask, doc: allLocked,
                                                 layerIndex: 0) != nil,
                     "Lock All fails to block mask edits")
    try scopeRequire(OperationScope.refusalReason(.pixelOnly, target: .mask, doc: base,
                                                 layerIndex: 0) != nil,
                     "unsupported pixel-only operation falls through the mask")
    try scopeRequire(OperationScope.refusalReason(.layerGeometry, target: .layer,
                                                 doc: moved, layerIndex: group.group) == nil,
                     "whole-group transforms were disabled by the pixel-target policy")
    try scopeRequire(groupCapture.placement?.x == 1 && groupStorage.offsetX == 1,
                     "group-mask capture or commit baked its stored offset")
    let groupBytes = try scopePixels(groupStorage.image)
    try scopeRequire(groupBytes[0] == 255 - scalar[0] && groupBytes[4] == scalar[1]
                     && groupBytes[12] == scalar[3], "group mask selected write used wrong coordinates")
    print("PASS scope policy: mask locks, unsupported operations and translated group-mask storage")
}
