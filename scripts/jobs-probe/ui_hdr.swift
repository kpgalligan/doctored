import AppKit
import CoreImage
import QuartzCore

// v1.x HDR photos and the EDR canvas, in process: the open choice, Revert's pinned
// rendition, View ▸ HDR Preview's layer, slot image and drawn backing store, and the export
// sheet's HDR rows. The fixtures are authored here by Core Image (two flat patches, 0.5 and 3.0
// of linear light), and the toggle case's document is a uniform 4.0 float raster. main.swift's
// loop runs these cases at `DR_EDR_HEADROOM=4` (every other case at 1), so an SDR test Mac still
// engages HDR Preview.

/// A two-patch HDR scene (0.5 and 3.0 grey, 1.0 = SDR white) as a float CIImage.
private func hdrScene(width: Int = 128, height: Int = 64) throws -> CIImage {
    var data = Data(count: width * height * 16)
    data.withUnsafeMutableBytes { raw in
        let p = raw.bindMemory(to: Float.self)
        for y in 0..<height {
            for x in 0..<width {
                let v: Float = x < width / 2 ? 0.5 : 3.0
                let at = (y * width + x) * 4
                p[at] = v; p[at + 1] = v; p[at + 2] = v; p[at + 3] = 1
            }
        }
    }
    guard let space = CGColorSpace(name: CGColorSpace.extendedLinearSRGB),
          let provider = CGDataProvider(data: data as CFData),
          let image = CGImage(width: width, height: height, bitsPerComponent: 32, bitsPerPixel: 128,
              bytesPerRow: width * 16, space: space,
              bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue
                  | CGBitmapInfo.floatComponents.rawValue | CGBitmapInfo.byteOrder32Little.rawValue),
              provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent),
          let tagged = CGImageCreateCopyWithContentHeadroom(4, image)
    else { throw ProbeFailure(description: "the HDR scene") }
    return CIImage(cgImage: tagged)
}

/// A gain-map HEIC and a PQ HEIC of `hdrScene`, written by Core Image.
private func hdrFixtures(_ tag: String) throws -> (gain: URL, pq: URL) {
    let folder = try uiCaseFolder("hdr")
    let hdr = try hdrScene()
    guard let linear = CGColorSpace(name: CGColorSpace.extendedLinearSRGB),
          let srgb = CGColorSpace(name: CGColorSpace.sRGB),
          let pq = CGColorSpace(name: CGColorSpace.itur_2100_PQ) else {
        throw ProbeFailure(description: "\(tag) colour spaces")
    }
    let context = CIContext(options: [.workingColorSpace: linear])
    let sdr = hdr.applyingFilter("CIToneMapHeadroom",
                                 parameters: ["inputSourceHeadroom": 4.0, "inputTargetHeadroom": 1.0])
    let gain = folder.appendingPathComponent("gain.heic")
    let signal = folder.appendingPathComponent("pq.heic")
    try context.writeHEIFRepresentation(of: sdr, to: gain, format: .RGBA8, colorSpace: srgb,
                                        options: [.hdrImage: hdr, .hdrGainMapAsRGB: true])
    try context.writeHEIF10Representation(of: hdr, to: signal, colorSpace: pq, options: [:])
    try require(HDRStillDecoder.classify(gain) == .gainMap, "\(tag) the gain-map fixture classifies")
    try require(HDRStillDecoder.classify(signal) == .pq, "\(tag) the PQ fixture classifies")
    return (gain, signal)
}

/// The largest RGB of an opened document's composite at (x, y).
private func hdrValue(_ doc: RasterDocument, x: Int, y: Int) throws -> [Double] {
    guard let precision = doc.precision else { throw ProbeFailure(description: "not a typed document") }
    let context = try PrecisionWorkContext(source: precision)
    let image = try precision.render(workingLimit: context.limit, otherRetained: context.otherRetained)
    guard let readout = try FloatReadout.at((x: x, y: y), projection: image, reach: 0) else {
        throw ProbeFailure(description: "no sample at (\(x), \(y))")
    }
    return Array(readout.values.prefix(3))
}

/// Restores the File ▸ Open HDR Photos as HDR preference as it was.
private func withHDRPreference(_ value: HDRPhotoRendition, _ body: () throws -> Void) rethrows {
    let key = HDRPhotoRequest.preferenceKey
    let stored = UserDefaults.standard.object(forKey: key)
    defer {
        if let stored { UserDefaults.standard.set(stored, forKey: key) }
        else { UserDefaults.standard.removeObject(forKey: key) }
    }
    HDRPhotoRequest.preference = value
    try body()
}

/// "hdr open choice": a gain map opens float with a value above 1 at its
/// highlight, `.sdr` opens its 8-bit SDR base with the disclosure, and a PQ
/// still refuses `.sdr` by its sentence.
func verifyHDROpenChoice() throws {
    let tag = "[hdr open choice]"
    let (gain, signal) = try hdrFixtures(tag)
    let previous = HDRPhotoRequest.mode
    defer { HDRPhotoRequest.mode = previous }

    HDRPhotoRequest.mode = .hdr
    let opened = try ImageDocument.openDocument(at: gain)
    try require(opened.doc.isFloat && opened.hdrRendition == .hdr, "\(tag) the gain map opens float")
    let highlight = try hdrValue(opened.doc, x: 100, y: 32)
    try require(highlight.allSatisfy { $0 > 1 }, "\(tag) the highlight is above white: \(highlight)")
    let record = opened.metadataReport?["hdr_open"] as? [String: Any]
    try require(record?["rendition"] as? String == "hdr" && record?["kind"] as? String == "gain_map",
                "\(tag) hdr_open: \(record ?? [:])")

    HDRPhotoRequest.mode = .sdr
    let base = try ImageDocument.openDocument(at: gain)
    try require(base.doc.precision == nil && base.hdrRendition == .sdr,
                "\(tag) .sdr opens the 8-bit SDR base")
    let warnings = (base.metadataReport?["warnings"] as? [String]) ?? []
    try require(warnings.contains(HDRPhotoRequest.sdrBaseDisclosure), "\(tag) the SDR base is disclosed")

    do {
        _ = try ImageDocument.openDocument(at: signal)
        throw ProbeFailure(description: "\(tag) PQ opened as SDR")
    } catch let error as ProbeFailure {
        throw error
    } catch {
        try require(error.localizedDescription == HDRPhotoRequest.noSDRRendition(.pq),
                    "\(tag) the PQ refusal: \(error.localizedDescription)")
    }
    HDRPhotoRequest.mode = .hdr
    let float = try ImageDocument.openDocument(at: signal)
    try require(float.doc.isFloat, "\(tag) PQ opens float as HDR")
    let pqHighlight = try hdrValue(float.doc, x: 100, y: 32)
    try require(pqHighlight.allSatisfy { abs($0 - 3) < 0.05 }, "\(tag) PQ's 3.0: \(pqHighlight)")
    print("\(tag) PASS gain map highlight \(highlight), PQ highlight \(pqHighlight)")
}

/// "hdr revert keeps rendition": Revert reopens the rendition the document
/// was opened with, whatever the preference says now.
func verifyHDRRevertKeepsRendition() throws {
    let tag = "[hdr revert keeps rendition]"
    let (gain, _) = try hdrFixtures(tag)
    let previous = HDRPhotoRequest.mode
    defer { HDRPhotoRequest.mode = previous }
    let controller = NSDocumentController.shared
    let type = try controller.typeForContents(of: gain)

    for (opened, preference) in [(HDRPhotoRendition.sdr, HDRPhotoRendition.hdr), (.hdr, .sdr)] {
        HDRPhotoRequest.mode = opened
        guard let document = try controller.makeDocument(withContentsOf: gain, ofType: type)
            as? ImageDocument else { throw ProbeFailure(description: "\(tag) open") }
        defer { document.close() }
        HDRPhotoRequest.mode = nil
        try require(document.hdrRendition == opened, "\(tag) opened \(opened)")
        try withHDRPreference(preference) {
            try document.revert(toContentsOf: gain, ofType: type)
        }
        try require(document.hdrRendition == opened, "\(tag) \(opened) after revert")
        try require((document.doc?.isFloat ?? false) == (opened == .hdr),
                    "\(tag) opened \(opened) reverts at its own depth with the preference \(preference)")
    }
    print("\(tag) PASS")
}

/// A uniform float document of `value` in sRGB, installed the way `floatFixture` installs one
/// (also `ui_float_spot.swift`'s).
func uniformFloat(_ value: Float, width: Int = 64, height: Int = 48) throws -> ImageDocument {
    var bytes = Data(count: width * height * 16)
    bytes.withUnsafeMutableBytes { raw in
        let p = raw.bindMemory(to: Float.self)
        for i in 0..<(width * height) {
            p[i * 4] = value; p[i * 4 + 1] = value; p[i * 4 + 2] = value; p[i * 4 + 3] = 1
        }
    }
    let icc = RasterProfile.builtin(.sRGB)
    let source = try RasterDocument.fromHostHDRPixels(bytes: bytes, width: width, height: height,
        rowBytes: width * 16, layout: "rgba32f_le_opaque", orientation: 1, transfer: "linear",
        primaries: nil, profile: icc)
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

/// The largest RGB binary16 sample of a premultipliedLast 16-bit float image.
private func halfPeak(_ image: CGImage) -> Float? {
    guard image.bitsPerComponent == 16, image.bitmapInfo.contains(.floatComponents),
          let data = image.dataProvider?.data as Data? else { return nil }
    return data.withUnsafeBytes { raw -> Float in
        let halves = raw.bindMemory(to: UInt16.self)
        var peak = -Float.infinity
        for i in 0..<(halves.count / 4) {
            for c in 0..<3 { peak = max(peak, Float(Float16(bitPattern: halves[i * 4 + c]))) }
        }
        return peak
    }
}

/// "hdr preview toggle": on a uniform 4.0 float document, HDR Preview makes
/// the canvas layer EDR (16-bit float contents), lands a 16-bit float
/// extended-range slot image holding 4.0, and the drawn backing store is
/// float and extended with a sample above 1; +1 stop doubles the peak; off
/// restores RGBA8, the window's colour space and an SDR slot; Info's value
/// never changes.
func verifyHDRPreviewToggle() throws {
    let tag = "[hdr preview toggle]"
    try require(EDRDisplay.potentialHeadroom(of: nil) > 1,
                "\(tag) run with \(EDRDisplay.headroomOverrideVariable)=4")
    let document = try uniformFloat(4)
    defer {
        ExtendedDrawRecord.recording = false
        document.close()
    }
    let controller = try editor(document)
    let info = try FloatReadout.at((x: 10, y: 10), document: document)?.values
    try controller.setHDRPreview(false)
    try pumpUntil({ !controller.canvas.extendedRangeDisplay }, seconds: 20)
    let prior = controller.view.window?.colorSpace
    try controller.setHDRPreview(true)
    try pumpUntil({ controller.hdrPreviewDisplayed }, seconds: 30)
    guard let layer = controller.canvas.layer else { throw ProbeFailure(description: "\(tag) no layer") }
    try require(layer.wantsExtendedDynamicRangeContent && layer.contentsFormat == .RGBA16Float,
                "\(tag) the layer is EDR-configured")
    guard let slot = controller.canvas.proofImage else { throw ProbeFailure(description: "\(tag) no slot image") }
    try require(slot.bitsPerComponent == 16 && slot.bitmapInfo.contains(.floatComponents)
                && (slot.colorSpace.map { CGColorSpaceUsesExtendedRange($0) } ?? false),
                "\(tag) the slot is a 16-bit float extended-range image")
    let slotPeak = halfPeak(slot) ?? 0
    try require(slotPeak > 1 && abs(slotPeak - 4) < 0.01, "\(tag) the slot holds 4.0: \(slotPeak)")
    try require(controller.view.window?.colorSpace?.cgColorSpace?.name == CGColorSpace.extendedDisplayP3,
                "\(tag) the window draws in extended Display P3")
    ExtendedDrawRecord.recording = true
    controller.canvas.lastExtendedDraw = nil
    controller.canvas.needsDisplay = true
    controller.canvas.display()
    guard let listed = controller.canvas.lastExtendedDraw else {
        throw ProbeFailure(description: "\(tag) draw(_:) recorded nothing")
    }
    // AppKit records a layer-backed view's draw(_:) as a DISPLAY LIST
    // (`kCGContextTypeDisplayList`: no bitmap, no bytes) that Core Animation
    // rasterizes into the RGBA16Float backing store later, so the draw itself
    // has no pixels to read. What the layer draws is read by rendering it
    // into a float, extended-range bitmap.
    let bounds = controller.canvas.layer?.bounds.size ?? .zero
    let width = max(1, Int(bounds.width)), height = max(1, Int(bounds.height))
    guard let space = CGColorSpace(name: CGColorSpace.extendedLinearSRGB),
          let bitmap = CGContext(data: nil, width: width, height: height, bitsPerComponent: 16,
              bytesPerRow: width * 8, space: space,
              bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.floatComponents.rawValue
                  | CGBitmapInfo.byteOrder16Little.rawValue) else {
        throw ProbeFailure(description: "\(tag) a float bitmap")
    }
    controller.canvas.lastExtendedDraw = nil
    layer.render(in: bitmap)
    let rendered = bitmap.makeImage().flatMap(halfPeak) ?? 0
    guard let drawn = controller.canvas.lastExtendedDraw else {
        throw ProbeFailure(description: "\(tag) rendering the layer did not run draw(_:)")
    }
    try require(listed.bitsPerComponent == 0 && listed.drawnPeak == nil,
                "\(tag) the backing draw is AppKit's display list: \(listed)")
    try require(drawn.bitsPerComponent == 16 && drawn.floatComponents && drawn.extendedRange,
                "\(tag) draw(_:) draws into a float extended-range bitmap: \(drawn)")
    try require(rendered > 1 && abs(rendered - 4) < 0.05,
                "\(tag) the drawn bitmap holds 4.0: \(rendered)")

    try controller.setViewExposure(1)
    try pumpUntil({ controller.hdrPreviewDisplayed && controller.viewExposure.shown?.exposureStops == 1 },
                  seconds: 30)
    let doubled = controller.extendedContentPeak ?? 0
    try require(abs(doubled - 2 * slotPeak) < 0.02, "\(tag) +1 stop doubles the peak: \(doubled)")

    try controller.setHDRPreview(false)
    try pumpUntil({ !controller.canvas.extendedRangeDisplay
                    && controller.viewExposure.shown?.extendedRange == false }, seconds: 30)
    try require(layer.contentsFormat == .RGBA8Uint && !layer.wantsExtendedDynamicRangeContent,
                "\(tag) off restores RGBA8")
    try require(controller.view.window?.colorSpace == prior, "\(tag) off restores the window's space")
    if let sdr = controller.canvas.proofImage {
        try require(sdr.bitsPerComponent == 8 && !sdr.bitmapInfo.contains(.floatComponents),
                    "\(tag) the slot is SDR again")
    }
    try controller.setViewExposure(0)
    let after = try FloatReadout.at((x: 10, y: 10), document: document)?.values
    try require(info == after && info?.first == 4, "\(tag) Info reads the document value: \(String(describing: after))")
    try require(!document.isDocumentEdited && document.undoManager?.canUndo != true,
                "\(tag) HDR Preview is view state")
    print("\(tag) PASS slot \(slotPeak), backing draw \(listed), layer render \(drawn) holding \(rendered), +1 stop \(doubled)")
}

/// "hdr export rows": a float document's export sheet offers the HDR
/// encoding at 32 bits for HEIC only; a gain-map and a PQ HEIC encode. At
/// AVIF the row is disabled with the refusal as its tooltip, the status line
/// says it at once with no preview encode, and Save stays off; back at HEIC
/// the row and Save return.
func verifyHDRExportRows() throws {
    let tag = "[hdr export rows]"
    let document = try uniformFloat(2)
    defer { document.close() }
    let controller = try editor(document)
    controller.showExportWorkspace(nil)
    let opened = {
        (controller.presentedViewControllers ?? []).compactMap { $0 as? ExportWorkspaceSheetController }.last
    }
    try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 30)
    guard let sheet = opened() else { throw ProbeFailure(description: "\(tag) no Export sheet") }
    let format = try uiControl(sheet.view, id: "export.format", as: NSPopUpButton.self)
    let hdr = try uiControl(sheet.view, id: "export.hdrEncoding", as: NSPopUpButton.self)
    let save = try uiControl(sheet.view, id: "export.save", as: NSButton.self)
    let status = {
        uiViews(sheet.view, as: NSTextField.self).map(\.stringValue)
    }
    try require(!hdr.isEnabled && hdr.toolTip == nil, "\(tag) the HDR row is off for TIFF")
    format.selectItem(withTitle: ExportFormat.heic.displayName)
    try uiSend(format, "\(tag) HEIC")
    try require(hdr.isEnabled && hdr.titleOfSelectedItem == "Gain map" && hdr.toolTip == nil,
                "\(tag) HEIC at 32 bits offers the HDR row")
    try pumpUntil({ save.isEnabled }, seconds: 60)
    hdr.selectItem(withTitle: "PQ")
    try uiSend(hdr, "\(tag) PQ")
    try pumpUntil({ save.isEnabled }, seconds: 60)
    format.selectItem(withTitle: ExportFormat.avif.displayName)
    try uiSend(format, "\(tag) AVIF")
    try require(!hdr.isEnabled, "\(tag) the HDR row is off for AVIF")
    try require(hdr.toolTip == HostImageEncoder.hdrAVIFUnavailable,
                "\(tag) the AVIF row's tooltip is the refusal: \(String(describing: hdr.toolTip))")
    try require(status().contains { $0.contains("no HDR AVIF encoder") },
                "\(tag) the status line names the refusal: \(status())")
    try require(!save.isEnabled, "\(tag) an HDR AVIF cannot be exported")
    format.selectItem(withTitle: ExportFormat.heic.displayName)
    try uiSend(format, "\(tag) HEIC again")
    try require(hdr.isEnabled && hdr.toolTip == nil, "\(tag) HEIC offers the HDR row again")
    try pumpUntil({ save.isEnabled }, seconds: 60)
    try floatViewCancel(sheet, tag)
    print("\(tag) PASS")
}

/// The sheet's Cancel, by its key equivalent.
private func floatViewCancel(_ sheet: ExportWorkspaceSheetController, _ tag: String) throws {
    guard let button = buttons(sheet.view).first(where: { $0.keyEquivalent == "\u{1b}" }) else {
        throw ProbeFailure(description: "\(tag) Cancel is missing")
    }
    try uiSend(button, "\(tag) Cancel")
    try pumpUntil({ sheet.view.window?.isVisible != true }, seconds: 30)
}
