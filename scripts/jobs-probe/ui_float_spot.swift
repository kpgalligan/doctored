import AppKit

// #1: a 32-bit float window's spot channel on the canvas, in process. A shown spot channel's
// overprint is the base the canvas draws instead of the view slot (`ImageCanvasView.draw`), so it
// is built at the window's own SDR view (exposure and candidate tone map), get_view's image is
// that base, and HDR Preview stands down while it shows. Both cases run with the editor's own
// Channels panel loaded (its `loadView` loads it), the branch the app takes. The oracle is the
// IEC 61966-2.1 curve over the uniform value, written here; bounds (1 code, the ink exact) are
// declared before measuring.

/// One pixel's RGB of `image`, drawn 1:1 into an RGBA8 bitmap in its own colour space.
private func floatSpotPixel(_ image: CGImage, x: Int, y: Int) -> [Int]? {
    guard let space = image.colorSpace,
          let context = CGContext(data: nil, width: image.width, height: image.height,
                                  bitsPerComponent: 8, bytesPerRow: image.width * 4, space: space,
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
    else { return nil }
    context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
    guard let data = context.data else { return nil }
    let bytes = data.bindMemory(to: UInt8.self, capacity: image.width * image.height * 4)
    let at = (y * image.width + x) * 4
    return (0..<3).map { Int(bytes[at + $0]) }
}

/// A linear value's 8-bit sRGB code, clipped to 0…1 (the Clip map).
private func floatSpotCode(_ linear: Double) -> Int {
    let v = min(max(linear, 0), 1)
    let encoded = v <= 0.0031308 ? 12.92 * v : 1.055 * pow(v, 1 / 2.4) - 0.055
    return Int((encoded * 255).rounded())
}

/// New Spot Channel "Ink" over the left half: black, Solidity 100, so the ink covers what is
/// under it and the left half draws 0 at any view.
private func floatSpotAdd(_ document: ImageDocument, _ tag: String) throws {
    guard let doc = document.doc else { throw ProbeFailure(description: "\(tag) no document") }
    let mask = (0..<(doc.width * doc.height)).map { $0 % doc.width < doc.width / 2 ? UInt8(255) : 0 }
    let selection = CanvasSelection(shape: .mask(mask), canvasWidth: doc.width, canvasHeight: doc.height)
    document.applyEdit("New Spot Channel", record: .notACommand) {
        try $0.addingSpotChannel(name: "Ink", ink: SpotInk(lab: [0, 0, 0], solidity: 100),
                                 selection: selection)
    }
    try require(document.doc?.spotIndices() == [0], "\(tag) the spot channel is added")
}

/// Every sub-check of a case, so a failing run names each failed bound, not only the first.
private final class FloatSpotChecks {
    let tag: String
    private(set) var failures: [String] = []

    init(_ tag: String) { self.tag = tag }

    func expect(_ ok: Bool, _ what: String) {
        if !ok { failures.append(what) }
    }

    /// Pumps until `predicate` holds or `seconds` pass; a miss is recorded, never thrown.
    func wait(_ predicate: () -> Bool, seconds: Double, _ what: String) {
        expect((try? pumpUntil(predicate, seconds: seconds)) != nil, "\(what) (waited \(seconds) s)")
    }

    /// The left half is the ink (0 exactly), the right half `right` within 1 code.
    func halves(_ image: CGImage?, right: Int, _ what: String) {
        guard let image, let left = floatSpotPixel(image, x: image.width / 4, y: image.height / 2),
              let shown = floatSpotPixel(image, x: image.width * 3 / 4, y: image.height / 2)
        else { return expect(false, "\(what): no base to read") }
        expect(left == [0, 0, 0], "\(what): the ink half is \(left)")
        expect(shown.allSatisfy { abs($0 - right) <= 1 }, "\(what): \(shown) against \(right)")
    }

    func finish() throws {
        guard failures.isEmpty else {
            throw ProbeFailure(description: "\(tag) " + failures.joined(separator: "; "))
        }
    }
}

/// The composite-replacing overprint base the canvas draws, or nil.
private func floatSpotBase(_ controller: EditorViewController) -> CGImage? {
    guard let display = controller.canvas.channelDisplay, display.baseKind == .composite,
          display.replacesComposite else { return nil }
    return display.base
}

/// "float spot view": a uniform 0.25 float window with the spot channel shown. The base's
/// uncovered half is the display code of 0.25 at 0 stops, of 0.5 at +1 stop, and Reinhard's
/// (white 4) of 0.5 under that candidate map; the exposure reports displayed; get_view's image
/// (`canvasDrawnImage`) is the base itself.
func verifyFloatSpotView() throws {
    let checks = FloatSpotChecks("[float spot view]")
    let tag = checks.tag
    let document = try uniformFloat(0.25)
    defer { document.close() }
    let controller = try editor(document)
    try pumpUntil({ document.retainedPrecisionProjection != nil && controller.canvas.image != nil },
                  seconds: 20)
    try require(controller.channelsPanel.isViewLoaded, "\(tag) the Channels panel is loaded")
    try floatSpotAdd(document, tag)
    let plain = floatSpotBase(controller)
    checks.halves(plain, right: floatSpotCode(0.25), "at 0 stops")
    checks.expect(plain != nil && controller.viewState.viewImage === plain,
                  "get_view's image is the drawn base")

    try controller.setViewExposure(1)
    let exposed = floatSpotBase(controller)
    checks.halves(exposed, right: floatSpotCode(0.5), "at +1 stop")
    checks.wait({ controller.viewExposureDisplayed }, seconds: 30, "the exposure is displayed")
    checks.expect(exposed != nil && controller.viewState.viewImage === exposed,
                  "get_view's image is the exposed base")

    controller.setToneMapCandidate(ToneMap(operator: .reinhard, white: 4))
    let u = 0.5
    checks.halves(floatSpotBase(controller), right: floatSpotCode(u * (1 + u / 16) / (1 + u)),
                  "a candidate Reinhard map at +1 stop")

    controller.setToneMapCandidate(nil)
    try controller.setViewExposure(0)
    checks.halves(floatSpotBase(controller), right: floatSpotCode(0.25), "back at 0 stops")
    try checks.finish()
    print("\(tag) PASS 0 stops \(floatSpotCode(0.25)), +1 stop \(floatSpotCode(0.5))")
}

/// "hdr spot view" (DR_EDR_HEADROOM=4): a uniform 4.0 float window shows HDR Preview with no spot
/// channel; adding one stands it down (SDR layer, the base clipped to white beside the ink);
/// hiding the spot row's eye engages it again and showing it stands it down; one undo of the add
/// engages it again.
func verifyHDRSpotView() throws {
    let checks = FloatSpotChecks("[hdr spot view]")
    let tag = checks.tag
    try require(EDRDisplay.potentialHeadroom(of: nil) > 1,
                "\(tag) run with \(EDRDisplay.headroomOverrideVariable)=4")
    let document = try uniformFloat(4)
    defer { document.close() }
    let controller = try editor(document)
    try controller.setHDRPreview(true)
    try pumpUntil({ controller.hdrPreviewDisplayed }, seconds: 30)
    try require(controller.channelsPanel.isViewLoaded, "\(tag) the Channels panel is loaded")

    try floatSpotAdd(document, tag)
    checks.expect(!controller.hdrPreviewEngages, "a shown spot overprint stands HDR Preview down")
    checks.wait({ !controller.canvas.extendedRangeDisplay && !controller.hdrPreviewDisplayed },
                seconds: 30, "the extended range ends and hdr_preview_displayed is false")
    checks.expect(controller.canvas.layer?.contentsFormat == .RGBA8Uint, "the layer is RGBA8")
    checks.halves(floatSpotBase(controller), right: 255, "the SDR base")

    var hidden = document.channelVisibility
    hidden.channels.remove(0)
    hidden.hiddenSpots = [0]
    controller.restoreChannelVisibility(hidden)
    checks.expect(document.doc.map { controller.shownSpotChannels($0).isEmpty } == true,
                  "the spot row's eye is off")
    checks.wait({ controller.hdrPreviewDisplayed }, seconds: 30, "the eye off engages HDR Preview")

    var shown = document.channelVisibility
    shown.channels.insert(0)
    shown.hiddenSpots = []
    controller.restoreChannelVisibility(shown)
    checks.expect(!controller.hdrPreviewEngages, "the eye on stands HDR Preview down again")
    checks.wait({ !controller.canvas.extendedRangeDisplay && !controller.hdrPreviewDisplayed },
                seconds: 30, "the eye on ends the extended range")

    document.undoManager?.undo()
    try require(document.doc?.spotIndices().isEmpty == true, "\(tag) undo removes the spot channel")
    checks.wait({ controller.hdrPreviewDisplayed }, seconds: 30, "undo engages HDR Preview again")
    try checks.finish()
    print("\(tag) PASS")
}
