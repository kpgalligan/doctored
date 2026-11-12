// AppKit save and preview regression probe; verify.py supplies private output paths.
// These checks exercise the real ImageDocument implementation, without AppDelegate.
import AppKit
import ObjectiveC.runtime

@_silgen_name("probe_oversized_layers")
func probeOversizedLayers(_ doc: OpaquePointer)

struct ProbeFailure: Error, CustomStringConvertible {
    let description: String
}
func require(_ ok: @autoclosure () -> Bool, _ message: String) throws {
    if !ok() { throw ProbeFailure(description: message) }
}
guard let outputPath = ProcessInfo.processInfo.environment["DR_SAVE_PROBE_DIR"],
      outputPath.hasPrefix("/") else {
    throw ProbeFailure(description: "Run this probe with verify.py to supply a private output directory.")
}
let folder = URL(fileURLWithPath: outputPath, isDirectory: true)
try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
let sentinel = Data("previous good destination".utf8)
let image = RasterImage.from(rgba: [180, 20, 40, 255], width: 1, height: 1)

autoreleasepool {
    do {
        _ = NSApplication.shared
        AutomationAlerts.installIfEnabled()
        guard let image, let document = ImageDocument.makeUntitled(with: image) else {
            throw ProbeFailure(description: "could not create fixture")
        }
        let source = folder.appendingPathComponent("source.png")
        try image.save(to: source, format: DR_FORMAT_PNG, jpegQuality: 90)
        let originalSource = try Data(contentsOf: source)
        document.fileURL = source
        document.fileType = "public.png"
        document.applyEdit("Describe", record: .notACommand) {
            $0.withLayerMeta(0, "{\"kind\":\"probe\",\"editable\":true}")
        }
        try require(document.doc.layerCount == 1, "fixture must be single-layer")
        try require(document.isDocumentEdited, "fixture is dirty")
        try require(!document.canPreserveState(in: "public.png"), "rich PNG must lose state")
        let before = document.doc
        let modes: [(String, NSDocument.SaveOperationType)] = [
            ("save", .saveOperation), ("save-as", .saveAsOperation),
            ("autosave-elsewhere", .autosaveElsewhereOperation)
        ]
        for (name, operation) in modes {
            let destination = folder.appendingPathComponent("\(name).png")
            try sentinel.write(to: destination)
            var refused = false
            do {
                try document.writeSafely(to: destination, ofType: "public.png", for: operation)
            } catch {
                refused = true
                print("REFUSAL \(name): \(error) / \((error as NSError).userInfo)")
            }
            try require(refused, "safe \(name) must refuse")
            let after = try Data(contentsOf: destination)
            try require(after == sentinel, "safe \(name) changed prior destination")
            try require(document.isDocumentEdited, "safe \(name) cleared dirty flag")
            try require(document.doc === before, "safe \(name) changed document")
            print("PASS safe \(name): refused, destination and dirty state preserved")
            var completed = false
            var callbackError: Error?
            document.save(to: destination, ofType: "public.png", for: operation) { error in
                callbackError = error
                completed = true
            }
            let deadline = Date().addingTimeInterval(15)
            while !completed && Date() < deadline {
                RunLoop.current.run(until: Date().addingTimeInterval(0.01))
            }
            try require(completed && callbackError != nil, "save \(name) callback must refuse")
            let afterSave = try Data(contentsOf: destination)
            try require(afterSave == sentinel && document.isDocumentEdited, "save \(name) changed file or dirty")
            print("PASS save \(name) completion: refusal propagated")
        }
        let native = folder.appendingPathComponent("preserved.rz")
        try document.writeSafely(to: native, ofType: ImageDocument.nativeTypeIdentifier, for: .saveAsOperation)
        let reopened = try RasterDocument.open(url: native)
        try require(reopened.layerMeta(0) == before?.layerMeta(0), "native save must retain description")
        print("PASS native safe write: rich state survives reopen")
        let copy = folder.appendingPathComponent("copy.png")
        var finished = false
        var copyError: Error?
        document.save(to: copy, ofType: "public.png", for: .saveToOperation) { error in
            copyError = error
            finished = true
        }
        let deadline = Date().addingTimeInterval(15)
        while !finished && Date() < deadline {
            RunLoop.current.run(until: Date().addingTimeInterval(0.01))
        }
        try require(finished && copyError == nil, "saveTo failed: \(String(describing: copyError))")
        try require(document.fileURL == source && document.fileType == "public.png", "saveTo retargeted source")
        try require(document.isDocumentEdited && document.doc === before, "saveTo changed dirty or source revision")
        let savedImage = try RasterImage.open(url: copy)
        try require(savedImage.width == 1 && savedImage.height == 1, "copy PNG did not decode")
        let sourceAfterCopy = try Data(contentsOf: source)
        try require(sourceAfterCopy == originalSource, "saveTo overwrote source")
        print("PASS saveTo: PNG written; source URL, bytes, rich state and dirty flag unchanged")
        let exported = folder.appendingPathComponent("export.png")
        guard let model = document.doc else { throw ProbeFailure(description: "missing document") }
        _ = try model.saveImage(try document.currentProjection(), to: exported,
            format: DR_FORMAT_PNG, jpegQuality: 90, embedProfile: true, stripMetadata: false)
        try require(document.isDocumentEdited && document.fileURL == source && document.doc === before,
                    "explicit export changed source")
        print("PASS explicit export: PNG written, source unchanged")

        // Replace only the panel entry point in this throwaway process. This
        // captures delegated Close -> Save routing without opening a user panel.
        let selector = NSSelectorFromString("runModalSavePanelForSaveOperation:delegate:didSaveSelector:contextInfo:")
        guard let method = class_getInstanceMethod(NSDocument.self, selector) else {
            throw ProbeFailure(description: "missing save panel method")
        }
        var seenOperation: UInt = UInt.max
        var seenDelegate: AnyObject?
        var seenContext: UnsafeMutableRawPointer?
        var seenCallback: Selector?
        let interceptor: @convention(block) (AnyObject, UInt, AnyObject?, Selector?, UnsafeMutableRawPointer?) -> Void = {
            _, operation, delegate, callback, context in
            seenOperation = operation
            seenDelegate = delegate
            seenCallback = callback
            seenContext = context
        }
        let replacement = imp_implementationWithBlock(interceptor)
        let original = method_setImplementation(method, replacement)
        defer {
            method_setImplementation(method, original)
            imp_removeBlock(replacement)
        }
        let delegate = NSObject()
        let callback = NSSelectorFromString("probeDocument:didSave:contextInfo:")
        let context = UnsafeMutableRawPointer.allocate(byteCount: 1, alignment: 1)
        defer { context.deallocate() }
        document.save(withDelegate: delegate, didSave: callback, contextInfo: context)
        try require(seenOperation == NSDocument.SaveOperationType.saveAsOperation.rawValue,
                    "delegated save did not route to Save As")
        try require(seenDelegate === delegate && seenCallback == callback && seenContext == context,
                    "delegated save lost delegate/callback/context")
        try require(document.writableTypes(for: .saveAsOperation).first == ImageDocument.nativeTypeIdentifier,
                    "delegated Save As did not lead with native")
        print("PASS delegated Close -> Save: native-leading Save As, callback and context retained")
        // Native validation failure must precede destination replacement too.
        guard let oversized = ImageDocument.makeUntitled(with: image),
              let oversizedModel = oversized.doc,
              let oversizedPointer = oversizedModel.ptr else {
            throw ProbeFailure(description: "oversized fixture")
        }
        probeOversizedLayers(oversizedPointer)
        let tooLargeDestination = folder.appendingPathComponent("oversized-native.rz")
        try sentinel.write(to: tooLargeDestination)
        var capacityRefused = false
        do {
            try oversized.writeSafely(to: tooLargeDestination,
                ofType: ImageDocument.nativeTypeIdentifier, for: .saveAsOperation)
        } catch {
            capacityRefused = true
            try require(error.localizedDescription.contains("layer count"), "wrong native error: \(error)")
            try require(error.localizedDescription.contains(tooLargeDestination.path)
                        && !error.localizedDescription.contains(".doctored-save-"),
                        "native diagnostics must name chosen destination: \(error)")
        }
        let afterCapacity = try Data(contentsOf: tooLargeDestination)
        try require(capacityRefused && afterCapacity == sentinel && oversized.isDocumentEdited,
                    "native capacity refusal changed destination/dirty")
        print("PASS oversized native Save As: prior destination and dirty state preserved")

        guard let failedRender = ImageDocument.makeUntitled(with: image) else {
            throw ProbeFailure(description: "render fixture")
        }
        withUndoGroup(failedRender.undoManager) {
            failedRender.applyEdit("Group", record: .notACommand) { model in
                guard let grouped = try? model.groupLayers([0], name: "Group") else { return nil }
                return try grouped.document.withLayerBlendMode(grouped.group, DR_BLEND_NORMAL)
            }
        }
        guard let previousPreview = failedRender.previewImage else {
            throw ProbeFailure(description: "group must have a valid preview before resize")
        }
        // The qualification argv sets a one-pixel budget. Expand the group
        // contents to two pixels so this revision fails, keeping its prior view.
        withUndoGroup(failedRender.undoManager) {
            failedRender.applyEdit("Resize", record: .notACommand) { try $0.resized(w: 2, h: 1, filter: DR_FILTER_NEAREST) }
        }
        try require(failedRender.projectionError != nil, "expected reduced-budget render failure")
        try require(failedRender.projection == nil, "failed revision exposed stale projection pixels")
        try require(failedRender.previewImage === previousPreview, "failed render replaced last valid preview")
        print("PASS failed render: identical previous CGImage retained; current projection unavailable")
        let renderDestination = folder.appendingPathComponent("failed-render.png")
        try sentinel.write(to: renderDestination)
        var renderRefused = false
        do {
            try failedRender.writeSafely(to: renderDestination, ofType: "public.png", for: .saveToOperation)
        } catch {
            renderRefused = true
            try require(error.localizedDescription.contains("group buffer budget"), "wrong render error: \(error)")
        }
        let afterRender = try Data(contentsOf: renderDestination)
        try require(renderRefused && afterRender == sentinel && failedRender.isDocumentEdited,
                    "render failure changed destination/dirty")
        print("PASS failed-render Save To: prior destination and dirty state preserved")
        failedRender.undoManager?.undo()
        try require(failedRender.doc.width == 1 && failedRender.projectionError == nil
                    && failedRender.projection != nil && failedRender.previewImage != nil,
                    "undo did not restore current projection and clear failure")
        print("PASS undo: previous document size and current projection restored; failure cleared")

        let attributesURL = folder.appendingPathComponent("attributes.rz")
        try sentinel.write(to: attributesURL)
        let creation = Date(timeIntervalSince1970: 1_500_000_000)
        try FileManager.default.setAttributes([.posixPermissions: 0o640, .creationDate: creation],
                                             ofItemAtPath: attributesURL.path)
        let attrsBefore = try FileManager.default.attributesOfItem(atPath: attributesURL.path)
        try document.writeSafely(to: attributesURL,
                                 ofType: ImageDocument.nativeTypeIdentifier, for: .saveAsOperation)
        let attrsAfter = try FileManager.default.attributesOfItem(atPath: attributesURL.path)
        try require((attrsBefore[.posixPermissions] as? NSNumber) == (attrsAfter[.posixPermissions] as? NSNumber),
                    "safe replacement changed permissions")
        try require((attrsBefore[.creationDate] as? Date) == (attrsAfter[.creationDate] as? Date),
                    "safe replacement changed creation date")
        print("PASS native replacement retains POSIX permissions and creation date")
        var nativeCompleted = false
        var nativeError: Error?
        document.save(to: native, ofType: ImageDocument.nativeTypeIdentifier, for: .saveAsOperation) { error in
            nativeCompleted = true
            nativeError = error
        }
        let nativeDeadline = Date().addingTimeInterval(15)
        while !nativeCompleted && Date() < nativeDeadline {
            RunLoop.current.run(until: Date().addingTimeInterval(0.01))
        }
        try require(nativeCompleted && nativeError == nil && !document.isDocumentEdited,
                    "successful native Save As did not clear dirty: \(String(describing: nativeError))")
        try require(document.fileURL == native && document.fileType == ImageDocument.nativeTypeIdentifier,
                    "successful native Save As did not establish native master")
        print("PASS native Save As completion: native master established and dirty cleared")

        // A 32-bit float document whose remembered export depth is 32 (what
        // the Export sheet writes after a default float export): a flat Save
        // To falls back to 16 bits for PNG and refuses JPEG with the 8-bit
        // sentence, leaving the preference and the dirty state alone.
        guard let floatPath = ProcessInfo.processInfo.environment["DR_SAVE_PROBE_FLOAT_FIXTURE"],
              !floatPath.isEmpty else {
            throw ProbeFailure(description: "DR_SAVE_PROBE_FLOAT_FIXTURE is not set")
        }
        let floatURL = URL(fileURLWithPath: floatPath)
        try require(FileManager.default.fileExists(atPath: floatURL.path),
                    "float fixture missing at \(floatURL.path)")
        let floatContext = try PrecisionWorkContext()
        let floatPrecision = try RasterPrecisionDocument.openPreserving(
            url: floatURL, workingLimit: floatContext.limit, otherRetained: floatContext.otherRetained)
        try require(floatPrecision.isFloat, "the float fixture did not open as a float document")
        let floatSource = try RasterDocument(precision: floatPrecision)
        let floatDocument = ImageDocument()
        floatDocument.fileType = ImageDocument.nativeTypeIdentifier
        floatDocument.installEditingState(EditingState(document: floatSource, selection: SelectionHistoryState(),
            layerSelection: .single(0), paintTarget: .layer, jpegExportQuality: 90, embedColorProfile: true,
            stripMetadata: false, exportBitDepth: 32, exportDither: "none"), markSaved: true)
        try require(floatDocument.doc?.isFloat == true, "float fixture is not a 32-bit float document")
        let floatDirty = floatDocument.isDocumentEdited
        let floatPNG = folder.appendingPathComponent("float-save-to.png")
        try floatDocument.writeSafely(to: floatPNG, ofType: "public.png", for: .saveToOperation)
        let floatBytes = try Data(contentsOf: floatPNG)
        try require(floatBytes.count > 26 && floatBytes[24] == 16 && floatBytes[25] == 6,
                    "float Save To PNG must be 16-bit RGBA (IHDR \(floatBytes.count > 26 ? [floatBytes[24], floatBytes[25]] : []))")
        print("PASS float Save To PNG at remembered 32: 16-bit RGBA written")
        let floatJPEG = folder.appendingPathComponent("float-save-to.jpg")
        var jpegMessage: String?
        do {
            try floatDocument.writeSafely(to: floatJPEG, ofType: "public.jpeg", for: .saveToOperation)
        } catch let error as DoctoredCoreError {
            jpegMessage = error.message
        }
        try require(jpegMessage?.contains("requires an explicit 8-bit export choice") == true
                    && jpegMessage?.contains("Choose 8 or 16 bits") == false,
                    "float Save To JPEG must refuse with the 8-bit sentence: \(String(describing: jpegMessage))")
        try require(floatDocument.exportBitDepth == 32, "Save To rewrote the remembered export depth")
        try require(floatDocument.isDocumentEdited == floatDirty, "float Save To changed the dirty state")
        print("PASS float Save To JPEG: 8-bit sentence; remembered depth 32 and dirty state unchanged")

        // A one-layer Grayscale PNG with a guide: File ▸ Save must not write the PNG over itself
        // (a flat file has no guides), and Save routes to the Save As panel, `.doctored` first.
        let grayURL = folder.appendingPathComponent("gray.png")
        guard let grayContext = CGContext(data: nil, width: 4, height: 3, bitsPerComponent: 8,
                                          bytesPerRow: 0, space: CGColorSpaceCreateDeviceGray(),
                                          bitmapInfo: CGImageAlphaInfo.none.rawValue) else {
            throw ProbeFailure(description: "gray fixture context")
        }
        grayContext.setFillColor(gray: 0.5, alpha: 1)
        grayContext.fill(CGRect(x: 0, y: 0, width: 4, height: 3))
        guard let grayImage = grayContext.makeImage(),
              let grayDestination = CGImageDestinationCreateWithURL(grayURL as CFURL, "public.png" as CFString, 1, nil)
        else { throw ProbeFailure(description: "gray fixture image") }
        CGImageDestinationAddImage(grayDestination, grayImage, nil)
        try require(CGImageDestinationFinalize(grayDestination), "gray fixture PNG not written")
        let grayBytes = try Data(contentsOf: grayURL)
        let grayDocument = try ImageDocument(contentsOf: grayURL, ofType: "public.png")
        try require(grayDocument.doc?.isGray == true && grayDocument.doc?.layerCount == 1,
                    "the fixture must open as a one-layer Grayscale document")
        try require(grayDocument.writableTypes(for: .saveOperation).contains("public.png"),
                    "a plain Grayscale PNG must save in place")
        grayDocument.applyGuideEdit("New Guide", record: .addGuide(orientation: "vertical", position: 2)) {
            try $0.addingGuide(.vertical, at: 2)
        }
        try require(grayDocument.doc?.guideCount == 1 && grayDocument.isDocumentEdited, "guide not added")
        var grayRefused = false
        do {
            try grayDocument.writeSafely(to: grayURL, ofType: "public.png", for: .saveOperation)
        } catch {
            grayRefused = true
            try require(error.localizedDescription.contains("guides and ruler origin"),
                        "wrong Grayscale refusal: \(error)")
        }
        try require(grayRefused, "Save of a Grayscale PNG with a guide must refuse to write the PNG")
        let grayAfter = try Data(contentsOf: grayURL)
        try require(grayAfter == grayBytes && grayDocument.isDocumentEdited,
                    "the refused Grayscale save changed the file or the dirty flag")
        seenOperation = UInt.max
        grayDocument.save(withDelegate: delegate, didSave: callback, contextInfo: context)
        try require(seenOperation == NSDocument.SaveOperationType.saveAsOperation.rawValue,
                    "Save of a Grayscale PNG with a guide did not route to Save As")
        try require(grayDocument.writableTypes(for: .saveAsOperation).first == ImageDocument.nativeTypeIdentifier,
                    "the Grayscale Save As did not lead with native")
        print("PASS Grayscale PNG with a guide: Save refuses the PNG and routes to Save As, native first")
        print("ALL SAVE PROBES PASSED")
    } catch {
        print("FAIL \(error)")
        exit(1)
    }
}
