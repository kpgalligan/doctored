import AppKit

/// At 16 bits and at 8 bits the composite scope of `ScopedAnalysisRead`
/// counts the retained projection (same answers, smaller plan), and at 32-bit
/// float the Info panel shows its stop-binned histogram and linear readout for
/// Composite, Active layer and Below active layer, weighting a selection when
/// asked, with the composite counting the retained float projection.

private func storedValue(_ subject: Any, _ label: String) -> Any? {
    Mirror(reflecting: subject).children.first(where: { $0.label == label })?.value
}

private func footnoteText(_ panel: InfoPanelViewController) throws -> String {
    guard let footnote = storedValue(panel, "footnote") as? NSTextField else {
        throw ProbeFailure(description: "Info panel footnote")
    }
    return footnote.stringValue
}

/// Chooses the Info panel's source popup item `tag` the way a click does.
private func chooseInfoSource(_ panel: InfoPanelViewController, tag: Int) throws {
    guard let popup = storedValue(panel, "sourcePopup") as? NSPopUpButton,
          let action = popup.action else {
        throw ProbeFailure(description: "Info panel source popup")
    }
    popup.selectItem(withTag: tag)
    NSApp.sendAction(action, to: popup.target, from: popup)
}

private func histogramBins(_ panel: InfoPanelViewController) throws -> HistogramBins? {
    guard let view = storedValue(panel, "histogramView") as? HistogramView else {
        throw ProbeFailure(description: "Info panel histogram view")
    }
    return view.bins
}

/// The 32-bit float fixture (or the float TIFF at `url`), installed the way
/// `precisionFixture` installs one.
func floatFixture(url: URL = URL(fileURLWithPath: "core/tests/fixtures/float_11f/scene.tiff"))
    throws -> ImageDocument {
    let context = try PrecisionWorkContext()
    let precision = try RasterPrecisionDocument.openPreserving(
        url: url, workingLimit: context.limit, otherRetained: context.otherRetained)
    let source = try RasterDocument(precision: precision)
    try require(source.isFloat, "(c) the fixture opened as a 32-bit float document")
    let document = ImageDocument()
    document.fileType = ImageDocument.nativeTypeIdentifier
    document.installEditingState(EditingState(document: source, selection: SelectionHistoryState(),
        layerSelection: .single(0), paintTarget: .layer, jpegExportQuality: 90, embedColorProfile: true,
        stripMetadata: false, exportBitDepth: 16, exportDither: "none"), markSaved: true)
    document.undoManager?.groupsByEvent = false
    NSDocumentController.shared.addDocument(document)
    document.makeWindowControllers()
    if let window = document.windowControllers.first?.window {
        window.setFrameOrigin(NSPoint(x: -10000, y: -10000))
        window.orderFront(nil)
    }
    document.undoManager?.removeAllActions()
    return document
}

private func verifyRetainedProjectionReads() throws {
    let deep = try precisionFixture(width: 64, height: 48)
    defer { deep.close() }
    let controller = try editor(deep)
    guard let panel = controller.infoPanel else { throw ProbeFailure(description: "Info panel") }
    _ = panel.view
    try pumpUntil({ deep.retainedPrecisionProjection != nil }, seconds: 10)
    guard let retained = deep.retainedPrecisionProjection, let source = deep.doc else {
        throw ProbeFailure(description: "(a) no retained typed projection")
    }

    // (a) The composite scope reads the retained projection: same answer, smaller plan.
    var mask = [UInt8](repeating: 0, count: source.width * source.height)
    let x0 = (source.width - 20) / 2, y0 = (source.height - 20) / 2
    for y in y0..<(y0 + 20) {
        for x in x0..<(x0 + 20) { mask[y * source.width + x] = 255 }
    }
    let shapes: [(name: String, stride: Int, point: (x: Int, y: Int)?, reach: Int)] = [
        ("histogram stride 1", 1, nil, 0),
        ("histogram stride 3", 3, nil, 0),
        ("sample (5,5) reach 1", 1, (5, 5), 1),
        ("sample (32,24) reach 1", 1, (32, 24), 1),
    ]
    var answered = 0
    for shape in shapes {
        for coverage in [nil, mask] {
            let label = "\(shape.name), coverage \(coverage == nil ? "none" : "20×20")"
            let projected = try ScopedAnalysisRead(source: source, scope: .composite, selection: nil,
                stride: shape.stride, point: shape.point, reach: shape.reach, coverage: coverage,
                projection: retained)
            let rendered = try ScopedAnalysisRead(source: source, scope: .composite, selection: nil,
                stride: shape.stride, point: shape.point, reach: shape.reach, coverage: coverage)
            try require(projected.projection === retained, "(a) \(label): the read holds R")
            try require(projected.scratchEstimate < rendered.scratchEstimate,
                        "(a) \(label): a smaller plan \(projected.scratchEstimate) < \(rendered.scratchEstimate)")
            // The same outcome: equal replies, or (the (5,5) sample lies outside
            // the centred coverage) the same core sentence from both.
            let fast = Result { try projected.read() }
            let slow = Result { try rendered.read() }
            switch (fast, slow) {
            case (.success(let a), .success(let b)):
                try require(NSDictionary(dictionary: a).isEqual(to: b),
                            "(a) \(label): the projection's answer equals the render's")
                answered += 1
            case (.failure(let a), .failure(let b)):
                try require(a.localizedDescription == b.localizedDescription,
                            "(a) \(label): the same refusal: \(a.localizedDescription) | \(b.localizedDescription)")
            default:
                throw ProbeFailure(description: "(a) \(label): outcomes differ: \(fast) | \(slow)")
            }
        }
    }
    try require(answered == 7, "(a) seven of the eight reads answer (the covered-out sample refuses): \(answered)")
    let layer = try ScopedAnalysisRead(source: source, scope: .layer(0), selection: nil,
                                       projection: retained)
    try require(layer.projection == nil, "(a) a layer scope drops the projection")

    // (b) The 16-bit panel still counts the composite, with the 16-bit footnote.
    panel.requestScopedHistogram(scope: .composite)
    try pumpUntil({ panel.histogramFailure == nil && panel.clippingLabel.stringValue.hasPrefix("Clipped") },
                  seconds: 10)
    let footnote = try footnoteText(panel)
    try require(footnote.contains("16-bit authoritative samples"), "(b) the 16-bit footnote: \(footnote)")
}

/// (a8) The 8-bit composite scope reads the retained byte projection —
/// the same answers as a render, with a smaller plan.
private func verifyRetainedByteProjectionReads() throws {
    let byte = try fixture(width: 64, height: 48)
    defer { byte.close() }
    _ = try editor(byte)
    try pumpUntil({ byte.retainedByteProjection != nil }, seconds: 10)
    guard let retained = byte.retainedByteProjection, let source = byte.doc else {
        throw ProbeFailure(description: "(a8) no retained 8-bit projection")
    }
    var mask = [UInt8](repeating: 0, count: source.width * source.height)
    let x0 = (source.width - 20) / 2, y0 = (source.height - 20) / 2
    for y in y0..<(y0 + 20) {
        for x in x0..<(x0 + 20) { mask[y * source.width + x] = 255 }
    }
    let shapes: [(name: String, stride: Int, point: (x: Int, y: Int)?, reach: Int)] = [
        ("histogram stride 1", 1, nil, 0),
        ("histogram stride 3", 3, nil, 0),
        ("sample (5,5) reach 1", 1, (5, 5), 1),
        ("sample (32,24) reach 1", 1, (32, 24), 1),
    ]
    var answered = 0
    var refused = 0
    for shape in shapes {
        for coverage in [nil, mask] {
            let label = "\(shape.name), coverage \(coverage == nil ? "none" : "20×20")"
            let projected = try ScopedAnalysisRead(source: source, scope: .composite, selection: nil,
                stride: shape.stride, point: shape.point, reach: shape.reach, coverage: coverage,
                byteProjection: retained)
            let rendered = try ScopedAnalysisRead(source: source, scope: .composite, selection: nil,
                stride: shape.stride, point: shape.point, reach: shape.reach, coverage: coverage)
            try require(projected.byteProjection === retained, "(a8) \(label): the read holds P")
            try require(projected.projection == nil, "(a8) \(label): no typed projection")
            try require(projected.scratchEstimate < rendered.scratchEstimate,
                        "(a8) \(label): a smaller plan \(projected.scratchEstimate) < \(rendered.scratchEstimate)")
            let fast = Result { try projected.read() }
            let slow = Result { try rendered.read() }
            switch (fast, slow) {
            case (.success(let a), .success(let b)):
                try require(NSDictionary(dictionary: a).isEqual(to: b),
                            "(a8) \(label): the projection's answer equals the render's")
                answered += 1
            case (.failure(let a), .failure(let b)):
                try require(a.localizedDescription == b.localizedDescription,
                            "(a8) \(label): the same refusal: \(a.localizedDescription) | \(b.localizedDescription)")
                refused += 1
            default:
                throw ProbeFailure(description: "(a8) \(label): outcomes differ: \(fast) | \(slow)")
            }
        }
    }
    try require(answered == 7 && refused == 1,
                "(a8) seven of the eight reads answer and one shared refusal: \(answered) / \(refused)")
    let layer = try ScopedAnalysisRead(source: source, scope: .layer(0), selection: nil,
                                       byteProjection: retained)
    try require(layer.byteProjection == nil, "(a8) a layer scope drops the projection")
}

private func verifyFloatPanel() throws {
    let float = try floatFixture()
    defer { float.close() }
    let controller = try editor(float)
    guard let panel = controller.infoPanel else { throw ProbeFailure(description: "Info panel") }
    _ = panel.view
    try pumpUntil({ float.retainedPrecisionProjection != nil }, seconds: 10)

    // (c) The composite: stop-binned, with below-zero and above-white markers.
    panel.requestScopedHistogram(scope: .composite)
    try pumpUntil({ panel.histogramFailure != nil || panel.clippingLabel.stringValue.hasPrefix("Below zero:") },
                  seconds: 10)
    try require(panel.histogramFailure == nil,
                "(c) the composite histogram reads: \(panel.histogramFailure ?? "")")
    let bins = try histogramBins(panel)
    try require((bins?.exactShadow ?? 0) > 0 && (bins?.exactHighlight ?? 0) > 0,
                "(c) below-zero and above-white counts: \(bins?.exactShadow ?? -1) \(bins?.exactHighlight ?? -1)")
    var footnote = try footnoteText(panel)
    try require(footnote.contains("32-bit float") && footnote.contains("weights selection coverage")
                    && !footnote.contains("16-bit authoritative samples"),
                "(c) the float footnote: \(footnote)")
    panel.setPrecisionCursor((1, 1))
    try require(panel.readoutFailure == nil && panel.valueLabels[.rgb]?.stringValue != "—",
                "(c) the composite readout reads: \(panel.readoutFailure ?? "nil")")

    // (d) Active layer: the histogram arrives and the scoped cursor reads.
    try chooseInfoSource(panel, tag: 1)
    try require(panel.selectedAnalysisScope == .layer(0), "(d) the scope is layer 0")
    panel.binsArrived(nil)
    panel.requestScopedHistogram(scope: panel.selectedAnalysisScope)
    try pumpUntil({ panel.histogramFailure != nil || panel.clippingLabel.stringValue.hasPrefix("Below zero:") },
                  seconds: 10)
    try require(panel.histogramFailure == nil,
                "(d) the Active layer histogram reads: \(panel.histogramFailure ?? "")")
    try require(panel.scopedCursorRequired, "(d) Active layer requires the scoped cursor")
    panel.valueLabels[.rgb]?.stringValue = "—"
    panel.setPrecisionCursor((1, 1))
    try pumpUntil({ panel.readoutFailure != nil || panel.valueLabels[.rgb]?.stringValue != "—" },
                  seconds: 10)
    try require(panel.readoutFailure == nil && panel.valueLabels[.rgb]?.stringValue != "—",
                "(d) the Active layer readout reads: \(panel.readoutFailure ?? "nil")")

    // (e) Below active layer with the active layer at 0: nothing below, no failure.
    try chooseInfoSource(panel, tag: 2)
    try require(panel.selectedAnalysisScope == .backdrop(below: 0), "(e) the scope is backdrop below 0")
    panel.requestScopedHistogram(scope: panel.selectedAnalysisScope)
    try pumpUntil({ panel.histogramFailure != nil || panel.clippingLabel.stringValue == "No pixels to count" },
                  seconds: 10)
    try require(panel.histogramFailure == nil && panel.clippingLabel.stringValue == "No pixels to count",
                "(e) Below active layer counts nothing: \(panel.histogramFailure ?? panel.clippingLabel.stringValue)")

    // (f) Composite with a weighted 8×8 selection: the scoped cursor weights it.
    try chooseInfoSource(panel, tag: 0)
    _ = try select(float, side: 8)
    panel.selectionWeightCheckbox.state = .on
    // Showing the Info tab is the one path that installs the panel's
    // selection provider (`EditorViewController.showInfoTab`).
    controller.showInfoTab()
    try require(panel.selectionProvider?() != nil, "(f) the panel reads the canvas selection")
    try require(panel.scopedCursorRequired, "(f) a weighted selection requires the scoped cursor")
    panel.valueLabels[.rgb]?.stringValue = "—"
    panel.setPrecisionCursor((24, 16))
    try pumpUntil({ panel.readoutFailure != nil || panel.valueLabels[.rgb]?.stringValue != "—" },
                  seconds: 10)
    footnote = try footnoteText(panel)
    try require(panel.readoutFailure == nil && panel.valueLabels[.rgb]?.stringValue != "—",
                "(f) the selected centre reads: \(footnote)")
    panel.setPrecisionCursor((1, 1))
    try pumpUntil({ panel.readoutFailure != nil }, seconds: 10)
    try require(panel.readoutFailure?.contains("no positively weighted pixels") == true,
                "(f) outside the selection names the refusal: \(panel.readoutFailure ?? "nil")")
    panel.selectionWeightCheckbox.state = .off

    // (g) The float composite counts the retained projection with the render's answers.
    guard let retained = float.retainedPrecisionProjection, let source = float.doc else {
        throw ProbeFailure(description: "(g) no retained float projection")
    }
    var mask = [UInt8](repeating: 0, count: source.width * source.height)
    let x0 = (source.width - 20) / 2, y0 = (source.height - 20) / 2
    for y in y0..<(y0 + 20) {
        for x in x0..<(x0 + 20) { mask[y * source.width + x] = 255 }
    }
    let shapes: [(name: String, point: (x: Int, y: Int)?, reach: Int)] = [
        ("histogram", nil, 0),
        ("sample (24,16) reach 1", (24, 16), 1),
    ]
    for shape in shapes {
        for coverage in [nil, mask] {
            let label = "\(shape.name), coverage \(coverage == nil ? "none" : "20×20")"
            let projected = try ScopedAnalysisRead(source: source, scope: .composite, selection: nil,
                point: shape.point, reach: shape.reach, coverage: coverage, projection: retained)
            let rendered = try ScopedAnalysisRead(source: source, scope: .composite, selection: nil,
                point: shape.point, reach: shape.reach, coverage: coverage)
            try require(projected.projection === retained, "(g) \(label): the read holds R")
            let fast = try projected.read()
            let slow = try rendered.read()
            try require(fast["storage"] as? String == "rgba32_float", "(g) \(label): the float schema")
            // Since 4dbba7db the document's sample adds `center_display_codes`;
            // a bare projection carries no display policy, so its answer is the
            // document's without that key (`precision_scoped_source.rs`).
            var document = slow
            let codes = document.removeValue(forKey: "center_display_codes")
            try require(fast["center_display_codes"] == nil && (shape.point == nil) == (codes == nil),
                        "(g) \(label): only the document's sample carries center_display_codes")
            try require(NSDictionary(dictionary: fast).isEqual(to: document),
                        "(g) \(label): the projection's answer equals the render's")
        }
    }
}

/// Two histograms' plotted counts and mass, compared exactly.
private func sameBins(_ a: HistogramBins?, _ b: HistogramBins?) -> Bool {
    guard let a, let b else { return false }
    return a.red == b.red && a.green == b.green && a.blue == b.blue && a.luma == b.luma
        && a.total == b.total
}

/// `Histogram.loadForSheet`'s delivered bins for `mode`, pumped until they land.
private func sheetBins(_ document: ImageDocument, mode: AdjustmentSheetMode,
                       label: String) throws -> HistogramBins? {
    var delivered = false
    var bins: HistogramBins?
    var failure: String?
    Histogram.loadForSheet(document, mode: mode, destructiveLayer: nil, then: { result in
        bins = result
        delivered = true
    }, onFailure: { message in
        failure = message
        delivered = true
    })
    try pumpUntil({ delivered }, seconds: 10)
    try require(failure == nil, "(s) \(label): the sheet histogram failed: \(failure ?? "")")
    try require(bins != nil, "(s) \(label): the sheet histogram delivered bins")
    return bins
}

/// (s) A New Adjustment Layer sheet over the whole stack plans the
/// composite and counts the retained projection — at 8 bits a stale carried
/// projection (a pixel edit falsely declared render-neutral) is what the
/// sheet shows, not a fresh render; at 16 bits the same bins as the retained
/// typed projection.
private func verifySheetHistogramReadsRetained() throws {
    let byte = try fixture(width: 64, height: 48)
    defer { byte.close() }
    _ = try editor(byte)
    try pumpUntil({ byte.retainedByteProjection != nil }, seconds: 10)
    let created = Histogram.sheetPlan(byte, mode: .create(selection: nil), destructiveLayer: nil)
    try require(created.source == .composite, "(s8) .create over the whole stack plans the composite: "
                + "\(created.source)")
    let destructive = Histogram.sheetPlan(byte, mode: .destructive, destructiveLayer: nil)
    try require(destructive.source == .layer(0), "(s8) .destructive still plans layer 0: "
                + "\(destructive.source)")
    withUndoGroup(byte.undoManager) {
        byte.applyEdit("Probe Stale Carry", record: .notACommand, renderNeutral: true) { doc in
            doc.layerImage(0)?.inverted().flatMap { doc.withLayerPixels(0, $0) }
        }
    }
    guard let stale = byte.doc, let carried = byte.retainedByteProjection else {
        throw ProbeFailure(description: "(s8) no retained 8-bit projection after the stale carry")
    }
    let sheet = try sheetBins(byte, mode: .create(selection: nil), label: "8-bit")
    let retained = ScopedAnalysisRead.bins(try ScopedAnalysisRead(source: stale, scope: .composite,
        selection: nil, byteProjection: carried).read())
    let rendered = ScopedAnalysisRead.bins(try ScopedAnalysisRead(source: stale, scope: .composite,
        selection: nil).read())
    try require(sameBins(sheet, retained), "(s8) the sheet counts the retained projection")
    try require(!sameBins(sheet, rendered), "(s8) the sheet is not a fresh render (the stale carry differs)")

    let deep = try precisionFixture(width: 64, height: 48)
    defer { deep.close() }
    _ = try editor(deep)
    try pumpUntil({ deep.retainedPrecisionProjection != nil }, seconds: 10)
    guard let projection = deep.retainedPrecisionProjection, let source = deep.doc else {
        throw ProbeFailure(description: "(s16) no retained typed projection")
    }
    let typedPlan = Histogram.sheetPlan(deep, mode: .create(selection: nil), destructiveLayer: nil)
    try require(typedPlan.source == .composite, "(s16) .create over the whole stack plans the composite: "
                + "\(typedPlan.source)")
    let typedSheet = try sheetBins(deep, mode: .create(selection: nil), label: "16-bit")
    let typedRetained = ScopedAnalysisRead.bins(try ScopedAnalysisRead(source: source,
        scope: .composite, selection: nil, projection: projection).read())
    try require(sameBins(typedSheet, typedRetained), "(s16) the sheet counts the retained projection")
    print("INFO: info analysis [sheet histogram] 8-bit .create reads the carried projection; "
        + "16-bit .create equals the retained typed projection")
}

func verifyInfoAnalysis() throws {
    try verifyRetainedProjectionReads()
    try verifyRetainedByteProjectionReads()
    try verifyFloatPanel()
    try verifySheetHistogramReadsRetained()
    print("PASS: info analysis — the 16-bit and 8-bit composite scopes read the retained "
        + "projection with the render's answers and a smaller plan; the 32-bit float Info panel "
        + "shows its stop-binned histogram and linear readout for Composite, Active layer and "
        + "Below active layer and weights a selection, its composite counting the retained "
        + "float projection with the render's answers; a New Adjustment Layer sheet over the "
        + "whole stack counts the retained projection at 8 and 16 bits")
}
