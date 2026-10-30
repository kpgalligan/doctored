import AppKit

private func previewPixelsEqual(_ a: RasterImage, _ b: RasterImage) -> Bool {
    guard a.width == b.width, a.height == b.height else { return false }
    for y in 0..<a.height { for x in 0..<a.width {
        guard let lhs = a.pixelRGBA(x: x, y: y), let rhs = b.pixelRGBA(x: x, y: y),
              lhs.r == rhs.r, lhs.g == rhs.g, lhs.b == rhs.b, lhs.a == rhs.a else { return false }
    } }
    return true
}

private func requireCurrentMaskPreview(_ controller: EditorViewController,
                                       source: RasterDocument) throws {
    guard let state = controller.transformSession?.corePreview else {
        throw ProbeFailure(description: "Masked Free Transform did not select the core preview path")
    }
    try pumpUntil({ !state.isPending && state.projection != nil }, seconds: 15)
    guard let session = controller.transformSession, let preview = state.projection else {
        throw ProbeFailure(description: state.status)
    }
    let work = try TransformWork(source: source, layers: session.layers,
        quad: session.transform.hasCornerOffsets ? session.transform.warpedQuad(of: session.sourceRect) : nil,
        matrix: session.transform.matrix, sampler: session.sampler, droppingDescriptions: [])
    guard let result = work.transformedDocument() else {
        throw ProbeFailure(description: "Reference transform refused")
    }
    let expected = try result.render()
    try require(previewPixelsEqual(preview, expected), "Current preview equals actual core transform/composite byte for byte")
    try require(controller.canvas.transformPreview?.usesCoreComposite == true
                && controller.canvas.transformPreview?.composite === state.image,
                "Actual canvas draws the qualified core projection")
}

func verifyMaskTransformPreview() throws {
    let document = try fixture(width: 90, height: 60)
    defer { document.close() }
    let controller = try editor(document)
    let coverage = (0..<90 * 60).map { UInt8($0 % 90 < 38 ? 255 : 0) }
    guard let masked = document.doc?.addingLayerMask(0, kind: DR_MASK_FROM_SELECTION, selection: coverage),
          let moved = try masked.transformingMask(0,
            matrix: CGAffineTransform(a: 0.7, b: 0, c: 0, d: 1.3, tx: 11, ty: -4), filter: DR_FILTER_BILINEAR),
          let source = try moved.settingMaskProperties(0, linked: false, density: 0.65, feather: 2) else {
        throw ProbeFailure(description: "Independent unlinked mask fixture")
    }
    document.doc = source
    document.updateChangeCount(.changeCleared)
    document.undoManager?.removeAllActions()
    controller.freeTransform(nil)
    controller.transformDegrees = 13
    controller.transformScaleXPercent = 80
    try requireCurrentMaskPreview(controller, source: source)
    try require(document.doc === source && document.undoManager?.canUndo != true,
                "Independent-mask preview leaves native source and undo unchanged")
    controller.transformDegrees = 17
    controller.transformDegrees = 23
    try requireCurrentMaskPreview(controller, source: source)
    try require(controller.transformSession?.transform.degrees == 23,
                "Coalesced preview publishes the latest geometry")
    controller.transformSession?.transform.cornerOffsets[1] = CGVector(dx: -8, dy: 4)
    controller.refreshTransformPreview()
    try requireCurrentMaskPreview(controller, source: source)
    controller.transformDegrees = 31
    controller.canvas.onTransformCancel?()
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 }, seconds: 15)
    try require(controller.transformSession == nil && controller.canvas.transformPreview == nil
                && document.doc === source, "Cancel drops queued/in-flight preview without a late canvas replacement")

    controller.freeTransform(nil)
    controller.transformDegrees = 9
    try requireCurrentMaskPreview(controller, source: source)
    controller.commitPendingTransform()
    try require(!controller.isTransforming && document.doc !== source, "Masked final transform commits and closes the session")
    document.undoManager?.undo()
    try require(document.doc === source && document.undoManager?.canUndo != true,
                "Masked transform has one undo restoring independent properties and raw coverage")
    print("PASS: actual independent/unlinked density+feather Free Transform preview, latest generation, perspective, cancel and one undo")

    let grouped = try fixture(width: 70, height: 50)
    defer { grouped.close() }
    let groupController = try editor(grouped)
    guard let group = try? grouped.doc?.groupLayers([0], name: "Masked Group"),
          let groupMask = group.document.addingLayerMask(group.group, kind: DR_MASK_REVEAL_ALL),
          let shifted = try groupMask.transformingMask(group.group,
            matrix: CGAffineTransform(translationX: 18, y: 0), filter: DR_FILTER_NEAREST),
          let groupSource = try shifted.settingMaskProperties(group.group, linked: false, density: 0.7, feather: 1) else {
        throw ProbeFailure(description: "Independent group mask fixture")
    }
    grouped.doc = groupSource
    grouped.updateChangeCount(.changeCleared)
    grouped.undoManager?.removeAllActions()
    groupController.setActiveLayer(group.group)
    groupController.freeTransform(nil)
    groupController.transformDegrees = -11
    try requireCurrentMaskPreview(groupController, source: groupSource)
    groupController.canvas.onTransformCancel?()
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 }, seconds: 15)
    print("PASS: grouped transform previews its fixed independent group mask through the real compositor")

    guard let pixel = RasterImage.from(rgba: [40, 80, 120, 255], width: 1, height: 1),
          let tiny = masked.withLayerPixels(0, pixel),
          let unlinked = try tiny.settingMaskProperties(0, linked: false, density: 1, feather: 0) else {
        throw ProbeFailure(description: "Independent allocation fixture")
    }
    let matrix = CGAffineTransform(scaleX: 2, y: 1)
    let linkedWork = try TransformWork(source: tiny, layers: [0], quad: nil, matrix: matrix,
        sampler: DR_FILTER_BILINEAR, droppingDescriptions: [])
    let unlinkedWork = try TransformWork(source: unlinked, layers: [0], quad: nil, matrix: matrix,
        sampler: DR_FILTER_BILINEAR, droppingDescriptions: [])
    let linkedBytes = try linkedWork.scratchEstimate()
    let unlinkedBytes = try unlinkedWork.scratchEstimate()
    // Phase 12F-2: admission comes from the core's geometry plan
    // (`doc_source_geometry.rs` `geometry_edit_plan`), which charges a
    // transformed mask destination at 6 bytes per sample.
    try require(linkedBytes - unlinkedBytes == 6 * UInt64(180 * 60),
                "Admission measures the independent destination mask, not the 2x1 layer: \(linkedBytes) - \(unlinkedBytes)")
    let quad = [CGPoint(x: 0, y: 0), CGPoint(x: 1, y: 0), CGPoint(x: 0.8, y: 1), CGPoint(x: 0, y: 1)]
    let perspective = try TransformWork(source: tiny, layers: [0], quad: quad, matrix: .identity,
        sampler: DR_FILTER_BILINEAR, droppingDescriptions: [])
    let perspectiveBytes = try perspective.scratchEstimate()
    // The core plans the EXACT projective mask destination rather than a
    // 100 MB scalar maximum, so the figure is a few MiB of plan bookkeeping.
    try require(perspectiveBytes >= 4 * 1024 * 1024 && perspectiveBytes < 8 * 1024 * 1024,
                "Perspective reserves the planned mask destination without multiplying it by RGBA/projection scratch: \(perspectiveBytes)")
    print("PASS: independent linked mask admission and bounded planned perspective mask reservation")

    // Group width/height describe the union of descendants, not allocated
    // pixels. A legal 1x1 canvas can therefore have a union whose area cannot
    // be multiplied as Int, while its independently stored mask is one byte.
    let distant = try fixture(width: 1, height: 1)
    defer { distant.close() }
    let minimum = Int(Int32.min), maximum = Int(Int32.max) - 1
    guard let duplicate = try distant.doc?.duplicatingLayer(0),
          let first = try duplicate.withLayerOffset(0, minimum, minimum),
          let second = try first.withLayerOffset(1, maximum, maximum),
          let distantGroup = try? second.groupLayers([0, 1], name: "Wide Group Bounds"),
          let distantMask = distantGroup.document.addingLayerMask(distantGroup.group, kind: DR_MASK_REVEAL_ALL),
          let info = distantMask.layerInfo(distantGroup.group) else {
        throw ProbeFailure(description: "Extreme legal group-bounds fixture")
    }
    try require(info.width > Int(Int32.max) && info.height > Int(Int32.max),
                "Fixture actually exercises group union dimensions beyond Int32")
    distant.doc = distantMask
    distant.updateChangeCount(.changeCleared)
    distant.undoManager?.removeAllActions()
    let capture = try MaskEditCapture(distant, layer: distantGroup.group)
    let estimate = try MaskEdit.transform(CGAffineTransform(translationX: 1, y: 0), DR_FILTER_NEAREST)
        .scratchEstimate(for: capture)
    try require(estimate > 0 && estimate < 20 * 1024 * 1024,
                "A group mask reserves its canvas-sized plane without overflowing or charging the sparse child union")
    print("PASS: extreme Int32 child placement keeps independent group-mask admission finite and ordinary")
}
