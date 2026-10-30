import AppKit

// Image ▸ Mode ▸ Bitmap… (#547 Bitmap phase 1a): the sheet (`BitmapModeSheetController`), then the
// conversion as ONE undo step (`CmykModeConversion`'s bitmap arms, recorded as `convert_mode` with
// `mode: "bitmap"`, the method and, only when the sheet changed it, the Output resolution). Bitmap
// holds one layer: a document of more (the core plan's `flatten_required`, or
// `ModeFlattenRequired` from inside the job) asks "Flatten layers?" (Flatten / Cancel), as Indexed
// Color does, and Flatten converts again with `flatten: true`. Only an 8-bit Grayscale document
// converts (`BitmapDocument.sourceRefusal`). Image ▸ Mode ▸ Grayscale on a Bitmap document asks
// its Size Ratio (`BitmapToGraySheetController`) and converts back, one undo step, recorded as
// `convert_mode` with `mode: "gray"` and `size_ratio`.

extension EditorViewController {
    @objc func convertToBitmapMode(_ sender: Any?) {
        guard let document, let doc = document.doc else { NSSound.beep(); return }
        if let reason = BitmapDocument.sourceRefusal(doc) {
            NSSound.beep()
            document.presentError(DoctoredCoreError(message: reason))
            return
        }
        presentAsSheet(BitmapModeSheetController(editor: self, document: document))
    }

    /// One Bitmap conversion of the sheet's `options` (the core's form, no Flatten); a
    /// flatten-required refusal asks "Flatten layers?" and, on Flatten, converts again with
    /// Flatten Image first.
    func commitBitmap(document: ImageDocument, options bitmap: [String: Any], flatten: Bool) {
        var options = CmykConversionOptions()
        options.flatten = flatten
        do {
            let conversion = try CmykModeConversion(document, target: .bitmap, profile: nil, options: options,
                                                    bitmapOptions: bitmap)
            let prepared = try conversion.compute()
            try conversion.publish(prepared, to: document, record: [conversion.step()])
        } catch is JobCancelled {
            return
        } catch let required as ModeFlattenRequired where !flatten {
            let alert = NSAlert()
            alert.alertStyle = .warning
            alert.messageText = "Flatten layers?"
            alert.informativeText = required.reason
            alert.addButton(withTitle: "Flatten")
            alert.addButton(withTitle: "Cancel")
            guard alert.runModal() == .alertFirstButtonReturn else { return }
            commitBitmap(document: document, options: bitmap, flatten: true)
        } catch {
            document.presentError(error)
        }
    }

    /// Image ▸ Mode ▸ Grayscale on a Bitmap document: the Size Ratio sheet.
    func presentBitmapToGray() {
        guard let document, document.doc?.isBitmap == true else { NSSound.beep(); return }
        presentAsSheet(BitmapToGraySheetController(editor: self, document: document))
    }

    /// The Ruler's Straighten Layer on a Bitmap document (#547 Bitmap phase 3): the straightened
    /// crop of `BitmapDocument.straightenWindow`, as the Crop tool commits a turned box
    /// (`commitCropSession`'s angle branch, `RasterDocument.straightenedCrop`), as ONE "Straighten
    /// Layer" step recorded as the `straighten_layers` step, whose MCP twin runs `crop` with the
    /// same window and angle on a Bitmap document.
    /// True when the straightened crop committed, so the caller clears the ruler only then.
    @discardableResult
    func straightenBitmapWithRuler(_ line: RulerMeasurement) -> Bool {
        guard let document, let doc = document.doc,
              let window = BitmapDocument.straightenWindow(line, width: doc.width, height: doc.height)
        else {
            NSSound.beep()
            return false
        }
        let rect = window.rect
        let angle = window.angle
        let sampler = transformSampler
        let deleteCropped = ToolOptionsStore.shared.crop.deleteCroppedPixels
        let record: [ActionStep] = .step(
            "straighten_layers",
            ["x0": Double(line.start.x), "y0": Double(line.start.y),
             "x1": Double(line.end.x), "y1": Double(line.end.y),
             "sampler": Self.samplerAgentName(sampler)],
            note: "Ruler tool: Straighten Layer")
        defer { document.preparedEdit = nil }
        document.applyEdit(
            "Straighten Layer", record: record, changesCanvasGeometry: true,
            resultingSelection: document.selectionCarried(
                by: CropSession.canvasMap(window: rect, angle: angle),
                width: Int(rect.width), height: Int(rect.height))
        ) { doc in
            let center = CGPoint(x: rect.midX, y: rect.midY)
            let matrix = CGAffineTransform(translationX: center.x, y: center.y)
                .rotated(by: CGFloat(-angle) * .pi / 180)
                .translatedBy(x: -center.x, y: -center.y)
            guard let prepared = try doc.straightenedCrop(
                matrix, sampler: sampler,
                x: Int(rect.minX), y: Int(rect.minY), w: Int(rect.width), h: Int(rect.height),
                described: [], deleteCropped: deleteCropped, background: nil) else { return nil }
            document.preparedEdit = prepared
            return prepared.document
        }
        return document.doc !== doc
    }

    /// Bitmap → Grayscale at `sizeRatio`, one undo step.
    func commitBitmapToGray(document: ImageDocument, sizeRatio: Int) {
        do {
            let conversion = try CmykModeConversion(document, target: .gray, profile: nil,
                                                    options: CmykConversionOptions(),
                                                    bitmapOptions: ["size_ratio": sizeRatio])
            let prepared = try conversion.compute()
            try conversion.publish(prepared, to: document, record: [conversion.step()])
        } catch is JobCancelled {
            return
        } catch {
            document.presentError(error)
        }
    }
}

extension CmykModeConversion {
    /// The core's options (`bitmap_convert`): the sheet's, plus Flatten.
    static func bitmapCoreOptions(_ options: [String: Any], flatten: Bool) -> [String: Any] {
        var object = options
        object["flatten"] = flatten
        return object
    }

    /// The Bitmap arm inside the job: the core's plan and conversion of the 8-bit Grayscale
    /// source. A document of more than one plain layer ends the job with `ModeFlattenRequired`.
    static func bitmap(_ source: RasterDocument, options: CmykConversionOptions, bitmap: [String: Any],
                       control: RasterJobControl) throws -> RasterPrecisionDocument {
        guard let typed = source.precision else {
            throw DoctoredCoreError(message: BitmapDocument.sourceRefusal(source)
                ?? "Bitmap converts an 8-bit Grayscale document.")
        }
        let core = bitmapCoreOptions(bitmap, flatten: options.flatten)
        let plan = try typed.convertToBitmapPlan(options: core)
        if let refusal = plan["refusal"] as? String {
            if plan["flatten_required"] as? Bool == true { throw ModeFlattenRequired(reason: refusal) }
            throw DoctoredCoreError(message: refusal)
        }
        let context = try PrecisionWorkContext(source: typed)
        return try control.withChildOperation(from: 0, to: 1) { child in
            try typed.convertToBitmap(options: core, workingLimit: context.limit,
                                      otherRetained: context.otherRetained, control: child)
        }
    }

    /// The Bitmap → Grayscale arm inside the job, at the options' `size_ratio`.
    static func bitmapToGray(_ source: RasterDocument, bitmap: [String: Any],
                             control: RasterJobControl) throws -> RasterPrecisionDocument {
        guard let typed = source.precision else {
            throw DoctoredCoreError(message: "The document is not a Bitmap document.")
        }
        let context = try PrecisionWorkContext(source: typed)
        return try control.withChildOperation(from: 0, to: 1) { child in
            try typed.convertBitmapToGray(options: bitmap, workingLimit: context.limit,
                                          otherRetained: context.otherRetained, control: child)
        }
    }
}
