import AppKit

// The stack commands through the app's own actions: File ▸ Load Files
// into Stack… past its open panel, Layer ▸ Auto-Align Layers, each Layer ▸
// Stack Mode item and Layer ▸ Reduce Noise from Stack…'s sheet. The frames are
// PNGs the cases write into the probe's own folder: one value-noise picture,
// and copies of it drawn at an offset.

/// One value-noise picture, larger than any frame by `pad` on every side, so
/// a frame "drawn (dx, dy) off" is a crop of it.
private struct StackNoise {
    static let pad = 48
    let width: Int
    let height: Int
    let pixels: [UInt8]

    init(width: Int, height: Int) {
        let (w, h) = (width + 2 * Self.pad, height + 2 * Self.pad)
        self.width = w
        self.height = h
        /// A lattice value in 0…1 for corner (x, y) of octave `o`, channel `c`.
        func lattice(_ x: Int, _ y: Int, _ o: Int, _ c: Int) -> Double {
            var v = UInt32(truncatingIfNeeded: x &* 374_761_393 &+ y &* 668_265_263
                           &+ (o * 3 + c) &* 1_274_126_177)
            v = (v ^ (v >> 13)) &* 1_103_515_245
            v ^= v >> 16
            return Double(v & 0xFFFF) / 65535
        }
        var out = [UInt8](repeating: 255, count: w * h * 4)
        // Two octaves, 48 px and 16 px cells: smooth enough that a quarter
        // pixel of registration error moves a sample by a few codes, detailed
        // enough to register.
        let octaves: [(cell: Int, weight: Double)] = [(48, 0.6), (16, 0.4)]
        for c in 0..<3 {
            var plane = [Double](repeating: 0, count: w * h)
            for (o, octave) in octaves.enumerated() {
                let cell = octave.cell
                let columns = w / cell + 2
                let rows = h / cell + 2
                var grid = [Double](repeating: 0, count: columns * rows)
                for gy in 0..<rows { for gx in 0..<columns { grid[gy * columns + gx] = lattice(gx, gy, o, c) } }
                for y in 0..<h {
                    let gy = y / cell
                    let fy = Double(y % cell) / Double(cell)
                    let sy = fy * fy * (3 - 2 * fy)
                    for x in 0..<w {
                        let gx = x / cell
                        let fx = Double(x % cell) / Double(cell)
                        let sx = fx * fx * (3 - 2 * fx)
                        let top = grid[gy * columns + gx] * (1 - sx) + grid[gy * columns + gx + 1] * sx
                        let bottom = grid[(gy + 1) * columns + gx] * (1 - sx)
                            + grid[(gy + 1) * columns + gx + 1] * sx
                        plane[y * w + x] += octave.weight * (top * (1 - sy) + bottom * sy)
                    }
                }
            }
            for i in 0..<(w * h) { out[i * 4 + c] = UInt8(30 + (plane[i] * 195).rounded()) }
        }
        pixels = out
    }

    /// A `width` × `height` frame whose content is the picture drawn (dx, dy)
    /// off: frame(x, y) = picture(x − dx, y − dy). `gain` is an exposure: it
    /// scales the picture's LINEAR light (the sRGB curve undone, then put
    /// back), clipped at code 255.
    func frame(width: Int, height: Int, dx: Int = 0, dy: Int = 0, gain: Double = 1) -> [UInt8] {
        var exposed = [UInt8](repeating: 0, count: 256)
        for code in 0..<256 {
            let encoded = Double(code) / 255
            let linear = encoded <= 0.04045 ? encoded / 12.92 : pow((encoded + 0.055) / 1.055, 2.4)
            let lit = min(1, linear * gain)
            let back = lit <= 0.0031308 ? lit * 12.92 : 1.055 * pow(lit, 1 / 2.4) - 0.055
            exposed[code] = gain == 1 ? UInt8(code) : UInt8((back * 255).rounded())
        }
        var out = [UInt8](repeating: 255, count: width * height * 4)
        for y in 0..<height {
            let sy = y - dy + Self.pad
            for x in 0..<width {
                let s = (sy * self.width + (x - dx + Self.pad)) * 4
                let d = (y * width + x) * 4
                for c in 0..<3 { out[d + c] = exposed[Int(pixels[s + c])] }
            }
        }
        return out
    }
}

/// Writes one frame as `<folder>/<name>.png`.
func uiStackFrame(_ folder: URL, _ name: String, width: Int, height: Int, dx: Int = 0, dy: Int = 0,
                  gain: Double = 1) throws -> URL {
    let picture = StackNoise(width: width, height: height)
    let url = folder.appendingPathComponent(name + ".png")
    try uiWritePNG(picture.frame(width: width, height: height, dx: dx, dy: dy, gain: gain),
                   width: width, height: height, to: url)
    return url
}

/// A `width` × `height` frame of a 1024 × 768 value-noise picture enlarged
/// `width / 1024` times, drawn (dx, dy) canvas pixels off, as
/// `<folder>/<name>.png`: a camera-sized frame at the cost of a small picture.
/// Both offsets are whole canvas pixels, so two such frames differ by exactly
/// the shift.
private func uiEnlargedStackFrame(_ folder: URL, _ name: String, width: Int, height: Int,
                                  dx: Int, dy: Int) throws -> URL {
    let picture = StackNoise(width: 1024, height: 768)
    let scale = width / 1024
    let bitmap = CGImageAlphaInfo.premultipliedLast.rawValue
    guard let space = CGColorSpace(name: CGColorSpace.sRGB),
          let provider = CGDataProvider(data: Data(picture.pixels) as CFData),
          let image = CGImage(
            width: picture.width, height: picture.height, bitsPerComponent: 8, bitsPerPixel: 32,
            bytesPerRow: picture.width * 4, space: space, bitmapInfo: CGBitmapInfo(rawValue: bitmap),
            provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent),
          let context = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8,
                                  bytesPerRow: width * 4, space: space, bitmapInfo: bitmap) else {
        throw ProbeFailure(description: "enlarged frame \(name): no bitmap")
    }
    context.interpolationQuality = .high
    // CoreGraphics' origin is bottom-left: the picture's top-left lands at
    // (dx − pad, dy − pad) canvas pixels, y down.
    let pad = StackNoise.pad * scale
    let drawnWidth = picture.width * scale, drawnHeight = picture.height * scale
    context.draw(image, in: CGRect(x: dx - pad, y: height - drawnHeight - (dy - pad),
                                   width: drawnWidth, height: drawnHeight))
    guard let data = context.data else {
        throw ProbeFailure(description: "enlarged frame \(name): no pixels")
    }
    let rgba = Array(UnsafeBufferPointer(
        start: data.assumingMemoryBound(to: UInt8.self), count: width * height * 4))
    let url = folder.appendingPathComponent(name + ".png")
    try uiWritePNG(rgba, width: width, height: height, to: url)
    return url
}

/// A new, empty folder for one case's files under the probe's own directory.
func uiCaseFolder(_ name: String) throws -> URL {
    let folder = directory.appendingPathComponent("\(name)-\(UUID().uuidString.prefix(8))")
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    return folder
}

/// The documents open now that were not in `before`.
func uiNewDocuments(since before: [NSDocument]) -> [ImageDocument] {
    NSDocumentController.shared.documents
        .filter { candidate in !before.contains { $0 === candidate } }
        .compactMap { $0 as? ImageDocument }
}

/// Puts a document an app action opened where the fixtures live: off screen,
/// explicit undo groups, clean.
func uiSettle(_ document: ImageDocument) {
    document.windowControllers.first?.window?.setFrameOrigin(NSPoint(x: -10000, y: -10000))
    document.undoManager?.groupsByEvent = false
    document.updateChangeCount(.changeCleared)
    document.undoManager?.removeAllActions()
}

/// The frames as the layers of one document, through `ImageDocument.openStack`
/// (what Load Files into Stack runs), bottom-first in the order given.
private func stackDocument(_ urls: [URL], _ tag: String) throws -> ImageDocument {
    let document = try ImageDocument.openStack(urls, name: nil).document
    uiSettle(document)
    guard document.doc?.layerCount == urls.count else {
        document.close()
        throw ProbeFailure(description: "\(tag) the stack fixture has \(urls.count) layers")
    }
    return document
}

/// Straight RGB of layer `layer` at canvas pixel (x, y).
private func stackLayerPixel(_ doc: RasterDocument?, _ layer: Int, _ x: Int, _ y: Int) throws -> [Int] {
    guard let doc, let image = doc.layerCanvasImage(layer), x >= 0, y >= 0, x < image.width,
          y < image.height,
          let rgb = image.withPlateBytes({ bytes -> [Int] in
              let p = (y * image.width + x) * 4
              return (0..<4).map { Int(bytes[p + $0]) }
          }) else {
        throw ProbeFailure(description: "no pixel of layer \(layer) at (\(x), \(y))")
    }
    return rgb
}

private let stackBoth = LayerSelection(primary: 1, others: [0])

/// "load files into stack": the action past its open panel.
func verifyLoadFilesIntoStack() throws {
    let tag = "[load files into stack]"
    let folder = try uiCaseFolder("ui-load-stack")
    let (width, height) = (96, 64)
    // Given out of order: the action sorts by natural file name.
    let ten = try uiStackFrame(folder, "frame 10", width: width, height: height, dx: 4)
    let two = try uiStackFrame(folder, "frame 2", width: width, height: height, dx: 2)
    let one = try uiStackFrame(folder, "frame 1", width: width, height: height)
    let delegate = AppDelegate()
    var opened: [ImageDocument] = []
    defer { for document in opened { document.close() } }

    var before = NSDocumentController.shared.documents
    var alerts = AutomationAlerts.recorded.count
    delegate.loadFilesIntoStack(urls: [ten, two, one])
    var made = uiNewDocuments(since: before)
    opened += made
    try require(made.count == 1, "\(tag) one new document: \(made.count); \(uiAlerts())")
    made.forEach(uiSettle)
    guard let stack = made.first?.doc else { throw ProbeFailure(description: "\(tag) no stack") }
    let names = (0..<stack.layerCount).map { stack.layerInfo($0)?.name ?? "-" }
    try require(names == ["frame 1", "frame 2", "frame 10"],
                "\(tag) three layers bottom-first in natural name order: \(names)")
    try require(stack.width == width && stack.height == height && !stack.isHighDepth,
                "\(tag) an 8-bit \(width) × \(height) stack: \(stack.width) × \(stack.height)")
    try require(AutomationAlerts.recorded.count == alerts,
                "\(tag) three like files load with no alert: \(uiAlerts())")

    // A file of another size: the canvas is the largest width and height, and
    // the only alert is the notice the load itself returns, if it has one.
    let tall = try uiStackFrame(folder, "tall", width: height, height: width)
    let notice = try StackLoad.build([one, tall]).notice
    before = NSDocumentController.shared.documents
    alerts = AutomationAlerts.recorded.count
    delegate.loadFilesIntoStack(urls: [tall, one])
    made = uiNewDocuments(since: before)
    opened += made
    made.forEach(uiSettle)
    guard made.count == 1, let mixed = made.first?.doc else {
        throw ProbeFailure(description: "\(tag) the mixed-size load opened \(made.count) documents: "
            + uiAlerts())
    }
    try require(mixed.width == width && mixed.height == width && mixed.layerCount == 2,
                "\(tag) the canvas is \(width) × \(width): \(mixed.width) × \(mixed.height)")
    let raised = Array(AutomationAlerts.recorded.dropFirst(alerts))
    if let notice {
        try require(raised.count == 1 && raised[0].messageText == "Load Files into Stack"
                    && raised[0].informativeText == notice,
                    "\(tag) exactly the load's notice is shown: \(uiAlerts())")
    } else {
        try require(raised.isEmpty, "\(tag) a load with no notice shows no alert: \(uiAlerts())")
    }

    // A file that is no image: one alert, no document.
    let broken = folder.appendingPathComponent("broken.png")
    try Data("not an image".utf8).write(to: broken)
    before = NSDocumentController.shared.documents
    alerts = AutomationAlerts.recorded.count
    delegate.loadFilesIntoStack(urls: [one, broken])
    made = uiNewDocuments(since: before)
    opened += made
    try require(made.isEmpty, "\(tag) an unreadable file opens no document")
    let refusal = Array(AutomationAlerts.recorded.dropFirst(alerts))
    try require(refusal.count == 1
                && refusal[0].messageText == "Could Not Load the Files into a Stack"
                && refusal[0].informativeText.contains("broken.png"),
                "\(tag) one alert naming the file: \(uiAlerts())")
    print("PASS: load files into stack — the action past its open panel loads \"frame 10\", "
        + "\"frame 2\", \"frame 1\" as one 8-bit document with the layers frame 1, 2, 10 "
        + "bottom-first and no alert; a file of another size grows the canvas and shows "
        + (notice == nil ? "no alert" : "exactly the load's notice")
        + "; a file that is no image shows one \"Could Not Load the Files into a Stack\" alert "
        + "and opens nothing")
}

/// "auto-align action": Edit ▸ Auto-Align Layers… (Auto) on two frames of one picture.
func verifyAutoAlignAction() throws {
    let tag = "[auto-align action]"
    let folder = try uiCaseFolder("ui-auto-align")
    let (width, height) = (800, 600)
    let reference = try uiStackFrame(folder, "reference", width: width, height: height)
    let shifted = try uiStackFrame(folder, "shifted", width: width, height: height, dx: 24, dy: -16)
    let document = try stackDocument([reference, shifted], tag)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    guard let base = document.doc else { throw ProbeFailure(description: "\(tag) no document") }
    let points = [(400, 300), (200, 150), (600, 150), (200, 450), (600, 450)]
    /// The largest RGB difference between the two layers at each point.
    func differences(_ doc: RasterDocument?) throws -> [Int] {
        try points.map { point in
            let a = try stackLayerPixel(doc, 0, point.0, point.1)
            let b = try stackLayerPixel(doc, 1, point.0, point.1)
            return (0..<3).map { abs(a[$0] - b[$0]) }.max() ?? 0
        }
    }
    let apart = try differences(base)
    try require(apart.filter { $0 > 8 }.count >= 3,
                "\(tag) the shifted layer differs from the reference before: \(apart)")

    controller.setSelectedLayers(.single(1))
    try require(!uiValidates(controller, "autoAlignLayers:"),
                "\(tag) Auto-Align Layers does not validate with one layer selected")
    controller.setSelectedLayers(stackBoth)
    try require(uiValidates(controller, "autoAlignLayers:"),
                "\(tag) Auto-Align Layers validates with two layers selected")
    let entries = document.history.entries.count
    let alerts = AutomationAlerts.recorded.count
    let steps = uiRecorded { controller.commitAutoAlign(registration: .auto) }
    try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")
    guard let aligned = document.doc, aligned !== base else {
        throw ProbeFailure(description: "\(tag) Auto-Align Layers changed nothing")
    }
    try require(document.history.entries.count == entries + 1
                && document.history.entries.last?.name == AutoAlign.title
                && document.undoManager?.undoActionName == AutoAlign.title,
                "\(tag) one \"\(AutoAlign.title)\" entry: \(document.history.entries.map(\.name))")
    try require(steps.count == 1 && steps[0].tool == "auto_align_layers",
                "\(tag) one recorded auto_align_layers step: \(steps.map(\.tool))")
    let together = try differences(aligned)
    try require(together.allSatisfy { $0 <= 8 },
                "\(tag) the aligned layer matches the reference within 8 codes at five interior "
                + "points: \(together) (before: \(apart))")
    // The union canvas: the moved layer reaches 24 and 16 px past the old
    // canvas (2 px of resampling slack), and no layer reaches past the new one.
    try require(aligned.layerCount == 2 && abs(aligned.width - (width + 24)) <= 2
                && abs(aligned.height - (height + 16)) <= 2 && aligned.revealAllRect() == nil,
                "\(tag) the layer count is unchanged and the canvas grew to the union: "
                + "\(aligned.width) × \(aligned.height)")
    document.undoManager?.undo()
    try require(document.doc === base && document.undoManager?.canUndo != true,
                "\(tag) one undo restores the document")

    // Identical layers: nothing to move, so no step.
    let twin = try stackDocument([reference, reference], tag)
    defer { twin.close() }
    let twinController = try editor(twin)
    twinController.setSelectedLayers(stackBoth)
    let twinBase = twin.doc
    let twinEntries = twin.history.entries.count
    let none = uiRecorded { twinController.commitAutoAlign(registration: .auto) }
    try require(twin.doc === twinBase && twin.history.entries.count == twinEntries
                && twin.undoManager?.canUndo != true && none.isEmpty,
                "\(tag) identical layers: nothing changes and nothing is recorded: "
                + "\(none.map(\.tool)); same handle \(twin.doc === twinBase); entries "
                + "\(twinEntries) -> \(twin.history.entries.count); "
                + "undo \(twin.undoManager?.canUndo == true)")
    try require(AutomationAlerts.recorded.count == alerts,
                "\(tag) identical layers show no alert: \(uiAlerts())")

    // A camera-sized frame a few pixels off: the shift is 1.5 and 1 analysis
    // frame pixels, below the agreement score's resolution, and must still
    // align rather than give way to the identity (`AutoAlign.identityRadius`).
    let (bigWidth, bigHeight, bigDx, bigDy) = (4096, 3072, 3, -2)
    let bigReference = try uiEnlargedStackFrame(folder, "big reference", width: bigWidth,
                                                height: bigHeight, dx: 0, dy: 0)
    let bigShifted = try uiEnlargedStackFrame(folder, "big shifted", width: bigWidth,
                                              height: bigHeight, dx: bigDx, dy: bigDy)
    let big = try stackDocument([bigReference, bigShifted], tag)
    defer { big.close() }
    guard let bigBase = big.doc, bigBase.width == bigWidth, bigBase.height == bigHeight else {
        throw ProbeFailure(description: "\(tag) a \(bigWidth) × \(bigHeight) stack")
    }
    // The oracle: the floating layer's points belong (−dx, −dy) off in the
    // reference, so every corner of the canvas moves by exactly that.
    let analysis = AutoAlign.analysis(canvasWidth: bigWidth, canvasHeight: bigHeight)
    guard let referenceFrame = try AutoAlign.analysisFrame(
              of: bigBase, layer: 0, scale: analysis.scale, space: bigBase.colorSpace),
          let floatingFrame = try AutoAlign.analysisFrame(
              of: bigBase, layer: 1, scale: analysis.scale, space: bigBase.colorSpace),
          let (measured, _) = AutoAlign.register(floating: floatingFrame, reference: referenceFrame,
                                                 registration: .auto),
          let bigQuad = measured.rescaled(fromAnalysisScale: analysis.scale)
              .quad(of: CGRect(x: 0, y: 0, width: bigWidth, height: bigHeight)) else {
        throw ProbeFailure(description: "\(tag) the (\(bigDx), \(bigDy)) shift at \(bigWidth) × "
            + "\(bigHeight) registers")
    }
    let corners = Homography.corners(of: CGRect(x: 0, y: 0, width: bigWidth, height: bigHeight))
    let misses = zip(bigQuad, corners).map {
        hypot($0.x - ($1.x - CGFloat(bigDx)), $0.y - ($1.y - CGFloat(bigDy)))
    }
    try require(misses.allSatisfy { $0 <= 1 },
                "\(tag) the (\(bigDx), \(bigDy)) shift at \(bigWidth) × \(bigHeight) registers within "
                + "1 px at every corner: \(misses)")
    let bigController = try editor(big)
    bigController.setSelectedLayers(stackBoth)
    let bigEntries = big.history.entries.count
    let bigSteps = uiRecorded { bigController.commitAutoAlign(registration: .auto) }
    try require(big.doc !== bigBase && big.history.entries.count == bigEntries + 1
                && bigSteps.count == 1,
                "\(tag) the (\(bigDx), \(bigDy)) shift at \(bigWidth) × \(bigHeight) aligns as one "
                + "entry and one step: same handle \(big.doc === bigBase); entries \(bigEntries) -> "
                + "\(big.history.entries.count); steps \(bigSteps.map(\.tool))")
    try require(AutomationAlerts.recorded.count == alerts,
                "\(tag) the camera-sized frame shows no alert: \(uiAlerts())")
    print("PASS: auto-align action — Auto-Align Layers validates with two raster layers selected "
        + "and not with one; on a layer drawn (24, -16) off its reference it is one "
        + "\"\(AutoAlign.title)\" entry and one recorded auto_align_layers step, the layers then "
        + "agree within 8 codes at five interior points (\(together); \(apart) before), the "
        + "canvas grows to the union (\(aligned.width) × \(aligned.height)), one undo "
        + "restores; on identical layers nothing changes and nothing is recorded; a "
        + "(\(bigDx), \(bigDy)) shift at \(bigWidth) × \(bigHeight) registers within 1 px at every "
        + "corner (\(misses.map { String(format: "%.2f", Double($0)) })) and aligns as one "
        + "entry")
}

/// "stack mode items": each Layer ▸ Stack Mode item by its tag.
func verifyStackModeItems() throws {
    let tag = "[stack mode items]"
    let folder = try uiCaseFolder("ui-stack-mode")
    let (width, height) = (160, 120)
    let first = try uiStackFrame(folder, "first", width: width, height: height)
    let second = try uiStackFrame(folder, "second", width: width, height: height, dx: 40, dy: 30)
    let document = try stackDocument([first, second], tag)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    guard let base = document.doc else { throw ProbeFailure(description: "\(tag) no document") }
    controller.setSelectedLayers(stackBoth)
    let alerts = AutomationAlerts.recorded.count
    let action = #selector(EditorViewController.stackMode(_:))
    for (position, mode) in StackMode.allCases.enumerated() {
        let item = NSMenuItem(title: mode.title, action: action, keyEquivalent: "")
        item.tag = position
        controller.setSelectedLayers(stackBoth)
        try require(uiValidates(controller, "stackMode:", tag: position),
                    "\(tag) \(mode.title) validates with two layers selected")
        // The cursor, not the count: an undone mode's entry stays retained
        // as redo until the next edit replaces it.
        let cursor = document.history.cursor
        let steps = uiRecorded { controller.stackMode(item) }
        guard let stacked = document.doc, stacked !== base, stacked.layerCount == 3 else {
            throw ProbeFailure(description: "\(tag) \(mode.title) added no layer: \(uiAlerts())")
        }
        try require(stacked.layerInfo(2)?.name == mode.title && document.activeLayerIndex == 2,
                    "\(tag) the new layer \"\(mode.title)\" is on top and active: "
                    + "\(stacked.layerInfo(2)?.name ?? "-"), active \(document.activeLayerIndex)")
        try require(stacked.layerInfo(0)?.visible == false && stacked.layerInfo(1)?.visible == false
                    && stacked.layerInfo(2)?.visible == true,
                    "\(tag) \(mode.title) hides the layers it read")
        try require(document.history.cursor == cursor + 1
                    && document.history.entries.count == cursor + 2,
                    "\(tag) \(mode.title) is one entry: \(document.history.entries.map(\.name))")
        try require(steps.count == 1 && steps[0].tool == "stack_mode"
                    && steps[0].arguments["mode"] as? String == mode.rawValue,
                    "\(tag) one recorded stack_mode step carrying \(mode.rawValue): "
                    + "\(steps.map { "\($0.tool) \($0.arguments)" })")
        document.undoManager?.undo()
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "\(tag) \(mode.title): one undo restores the document")
    }
    // A tag past the list: nothing.
    let stray = NSMenuItem(title: "", action: action, keyEquivalent: "")
    stray.tag = StackMode.allCases.count
    controller.setSelectedLayers(stackBoth)
    let none = uiRecorded { controller.stackMode(stray) }
    try require(document.doc === base && document.undoManager?.canUndo != true && none.isEmpty,
                "\(tag) a tag out of range changes nothing")
    try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")
    print("PASS: stack mode items — each of the \(StackMode.allCases.count) Stack Mode items, by "
        + "its tag, adds a layer named after the mode on top, active, with the two layers it read "
        + "hidden, as one entry, one recorded stack_mode step carrying the mode and one undo; a "
        + "tag out of range changes nothing")
}

/// The Reduce Noise sheet `showReduceNoiseFromStack(_:)` just presented.
private func stackNoiseSheet(_ controller: EditorViewController,
                             _ tag: String) throws -> ReduceNoiseStackSheetController {
    let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    controller.showReduceNoiseFromStack(nil)
    let opened = {
        (controller.presentedViewControllers ?? [])
            .compactMap { $0 as? ReduceNoiseStackSheetController }
            .last { !known.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 30)
    guard let sheet = opened() else {
        throw ProbeFailure(description: "\(tag) no Reduce Noise from Stack sheet")
    }
    return sheet
}

/// "reduce noise stack sheet": the Mean / Median sheet and its commit.
func verifyReduceNoiseStackSheet() throws {
    let tag = "[reduce noise stack sheet]"
    let folder = try uiCaseFolder("ui-reduce-noise")
    let (width, height) = (800, 600)
    let first = try uiStackFrame(folder, "first", width: width, height: height)
    let second = try uiStackFrame(folder, "second", width: width, height: height, dx: 6, dy: -4)
    let document = try stackDocument([first, second], tag)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    guard let base = document.doc else { throw ProbeFailure(description: "\(tag) no document") }
    controller.setSelectedLayers(stackBoth)
    try require(uiValidates(controller, "showReduceNoiseFromStack:"),
                "\(tag) Reduce Noise from Stack… validates with two layers selected")
    let entries = document.history.entries.count
    let alerts = AutomationAlerts.recorded.count

    let cancelled = try uiRecorded {
        let sheet = try stackNoiseSheet(controller, tag)
        let median = try uiControl(sheet.view, id: "reduce-noise-median", as: NSButton.self)
        let mean = try uiControl(sheet.view, id: "reduce-noise-mean", as: NSButton.self)
        try require(median.state == .on && mean.state == .off,
                    "\(tag) Median is the preselected choice")
        guard let cancel = buttons(sheet.view).first(where: { $0.keyEquivalent == "\u{1b}" }) else {
            throw ProbeFailure(description: "\(tag) the sheet has no Cancel")
        }
        try uiSend(cancel, "\(tag) Cancel")
        try pumpUntil({ sheet.view.window?.isVisible != true }, seconds: 30)
    }
    try require(document.doc === base && document.history.entries.count == entries
                && document.undoManager?.canUndo != true && cancelled.isEmpty,
                "\(tag) Cancel changes nothing")

    let steps = try uiRecorded {
        let sheet = try stackNoiseSheet(controller, tag)
        let mean = try uiControl(sheet.view, id: "reduce-noise-mean", as: NSButton.self)
        mean.performClick(nil)
        let median = try uiControl(sheet.view, id: "reduce-noise-median", as: NSButton.self)
        try require(mean.state == .on && median.state == .off,
                    "\(tag) clicking Mean selects it alone")
        let apply = try uiControl(sheet.view, id: "reduce-noise-apply", as: NSButton.self)
        try uiSend(apply, "\(tag) Apply")
        try pumpUntil({ sheet.view.window?.isVisible != true && !BackgroundWork.occupied },
                      seconds: 60)
    }
    try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")
    guard let reduced = document.doc, reduced !== base, reduced.layerCount == 3 else {
        throw ProbeFailure(description: "\(tag) Apply added no layer")
    }
    try require(reduced.layerInfo(2)?.name == StackModes.noiseLayerName
                && document.activeLayerIndex == 2,
                "\(tag) the new layer is \"\(StackModes.noiseLayerName)\", on top and active: "
                + "\(reduced.layerInfo(2)?.name ?? "-")")
    try require(document.history.entries.count == entries + 1,
                "\(tag) one entry: \(document.history.entries.map(\.name))")
    try require(steps.count == 1 && steps[0].tool == "reduce_noise_from_stack"
                && steps[0].arguments["mode"] as? String == StackMode.mean.rawValue,
                "\(tag) one recorded reduce_noise_from_stack step with mode mean: "
                + "\(steps.map { "\($0.tool) \($0.arguments)" })")
    document.undoManager?.undo()
    try require(document.doc === base && document.undoManager?.canUndo != true,
                "\(tag) one undo restores the document")
    print("PASS: reduce noise stack sheet — the sheet opens on Median; Cancel changes nothing; "
        + "Mean and Apply add \"\(StackModes.noiseLayerName)\" on top as one entry, one recorded "
        + "reduce_noise_from_stack step with mode mean and one undo, with no alert")
}
