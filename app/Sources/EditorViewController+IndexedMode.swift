import AppKit

// Image ▸ Mode ▸ Indexed Color… (#1 Indexed phase 1a): the sheet (`IndexedModeSheetController`),
// then the conversion as ONE undo step (`CmykModeConversion`'s indexed arms, recorded as
// `convert_mode` with `mode: "indexed"` and the sheet's options). Indexed Color holds one layer:
// a document of more (the core plan's `flatten_required`, or `ModeFlattenRequired`
// from inside the job) asks "Flatten layers?" (Flatten / Cancel), and Flatten converts again with
// `flatten: true`. An 8-bit RGB source is promoted inside the job (Grayscale's RGB stage,
// `rgbStage`); an 8-bit Grayscale one indexes its twin's R = G = B. Image ▸ Mode ▸ RGB Color on an
// Indexed document converts at once, with no sheet: the RGB document its storage is, narrowed to
// 8 bits as the other 8-bit modes are (`CmykModeConversion`'s RGB arm). Since phase 1c the sheet
// offers Custom (its table edited in `ColorTableSheetController`) and Previous (the last
// conversion's palette, `IndexedPalettes`), sent to the core as `palette: "custom"` with a
// `table`, and CMYK Color… and Lab Color convert an Indexed document directly: the palette expanded
// exactly (`indexedExpanded`), then the existing RGB → CMYK and RGB → Lab routes, one undo step.

extension EditorViewController {
    @objc func convertToIndexedMode(_ sender: Any?) {
        guard let document, let doc = document.doc else { NSSound.beep(); return }
        if let reason = IndexedDocument.sourceRefusal(doc) {
            NSSound.beep()
            document.presentError(DoctoredCoreError(message: reason))
            return
        }
        presentAsSheet(IndexedModeSheetController(editor: self, document: document))
    }

    /// One Indexed Color conversion of the sheet's `options` (the core's form, no Flatten); a
    /// flatten-required refusal asks "Flatten layers?" and, on Flatten, converts again with
    /// Flatten Image first.
    func commitIndexed(document: ImageDocument, options indexed: [String: Any], flatten: Bool) {
        var options = CmykConversionOptions()
        options.flatten = flatten
        do {
            let conversion = try CmykModeConversion(document, target: .indexed, profile: nil, options: options,
                                                    indexedOptions: indexed)
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
            commitIndexed(document: document, options: indexed, flatten: true)
        } catch {
            document.presentError(error)
        }
    }

    /// Image ▸ Mode ▸ RGB Color on an Indexed document: at once, one undo step.
    func convertIndexedToRGBMode() {
        guard let document, document.doc?.isIndexed == true else { NSSound.beep(); return }
        do {
            let conversion = try CmykModeConversion(document, target: .rgb, profile: nil,
                                                    options: CmykConversionOptions())
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
    /// The core's options (`indexed_convert`): the sheet's, plus Flatten.
    static func indexedCoreOptions(_ options: [String: Any], flatten: Bool) -> [String: Any] {
        var object = options
        object["flatten"] = flatten
        if object["palette"] == nil { object["palette"] = "uniform" }
        return object
    }

    /// The RGB document an Indexed source's CMYK and Lab conversions start from (#1 Indexed phase
    /// 1c): its storage, every sample shared (`convertIndexedToRGB`, exact); any other typed
    /// document as it is. Only an Indexed document answers an index at the origin: a test of the
    /// mode that reads no descriptor.
    static func indexedExpanded(_ typed: RasterPrecisionDocument) throws -> RasterPrecisionDocument {
        typed.indexedIndex(x: 0, y: 0) != nil ? try typed.convertIndexedToRGB() : typed
    }

    /// Indexed → RGB's plan: the conversion itself shares every sample (`without_indexed`), so the
    /// working memory is the 8-bit narrowing and the result's display preparation, charged as the
    /// core's conversion term (`indexed_convert::TO_INDEXED_BYTES_PER_PIXEL`, 41 B a pixel), which
    /// `scripts/indexed-admission/measure.py` `to-rgb-from-indexed` holds.
    static func indexedToRGBPlan(_ source: RasterDocument) -> [String: Any] {
        let canvas = UInt64(source.width) &* UInt64(source.height)
        return ["working_memory_plan_bytes": NSNumber(value: canvas &* 41), "refusal": NSNull(),
                "flatten_required": false, "flattens": false]
    }

    /// The Indexed Color arm inside the job: the source's RGB stage (`rgbStage`: an 8-bit RGB
    /// source promoted or flattened, an 8-bit Grayscale one as it is), then the core's plan and
    /// conversion. A document of more than one plain layer ends the job with
    /// `ModeFlattenRequired`.
    static func indexed(_ source: RasterDocument, options: CmykConversionOptions, indexed: [String: Any],
                        control: RasterJobControl) throws -> RasterPrecisionDocument {
        let core = indexedCoreOptions(indexed, flatten: options.flatten)
        let (rgb, start) = try rgbStage(source, options: options, control: control)
        let plan = try rgb.convertToIndexedPlan(options: core)
        if let refusal = plan["refusal"] as? String {
            if plan["flatten_required"] as? Bool == true { throw ModeFlattenRequired(reason: refusal) }
            throw DoctoredCoreError(message: refusal)
        }
        let context = try PrecisionWorkContext(source: rgb)
        return try control.withChildOperation(from: start, to: 1) { child in
            try rgb.convertToIndexed(options: core, workingLimit: context.limit,
                                     otherRetained: context.otherRetained, control: child)
        }
    }

    /// The recorded step's Indexed arguments (`convert_mode`'s names): the core's options with the
    /// matte's codes as `matte_rgb`.
    static func indexedStepArguments(_ options: [String: Any]) -> [String: Any] {
        var arguments: [String: Any] = [:]
        for (key, value) in options where key != "flatten" {
            arguments[key == "matte" ? "matte_rgb" : key] = value
        }
        return arguments
    }
}
