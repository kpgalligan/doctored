import AppKit

// Image ▸ Mode ▸ Duotone… (#1, `designs/duotone-mode.md`): the sheet (`DuotoneSheetController`)
// over an 8- or 16-bit Grayscale document — opened at the last options used this session,
// Monotone Black at first — or over a Duotone document at its own description, then the edit as
// ONE undo step (`DuotoneEdit`, recorded as `convert_mode` with `mode: "duotone"` and the
// options). The menu items' validation is `validateCmykModeItem`'s; the MCP twins are
// `AgentServer+Duotone.swift`'s. The sheet's Preview draws the canvas through its inks
// (`DuotonePreview`, made here; view-only, so it has no MCP twin).
//
// Image ▸ Mode ▸ RGB Color, CMYK Color, Lab Color and Indexed Color convert a Duotone document
// through the render of its inks (`CmykModeConversion`'s Duotone arms, below): RGB is the core's
// render then Convert to Profile (`convertDuotoneToRGB`, then the 8-bit narrowing), and CMYK, Lab
// and Indexed take the rendered RGB document (`duotoneRendered`) through their own routes.
// Grayscale renders the inks into the gray, the inks' luminance in the document's own Gray profile
// (`duotoneToGray`, `commitGrayscale` with no Discard alert, recorded as `convert_mode` `mode:
// "gray"`), and Bitmap… is that rule followed by the Bitmap sheet's conversion (`duotoneToBitmap`,
// composed and unmeasured against Photoshop). A record the render cannot carry (an adjustment
// layer, a layer style) is the plan's `flatten_required`, so the sheet's Flatten box or the Flatten
// alert asks first. One undo step each.

extension EditorViewController {
    @objc func convertToDuotoneMode(_ sender: Any?) {
        guard let document, let doc = document.doc else { NSSound.beep(); return }
        if let reason = DuotoneDocument.sourceRefusal(doc) {
            NSSound.beep()
            document.presentError(DoctoredCoreError(message: reason))
            return
        }
        var options = DuotoneSheetController.lastOptions ?? .monotoneBlack
        if doc.isDuotone {
            do {
                if let json = try doc.precision?.duotoneOptions() { options = try DuotoneOptions(json: json) }
            } catch {
                NSSound.beep()
                document.presentError(error)
                return
            }
        }
        // #1 Duotone phase 2a: the sheet's Preview; nil (no projection) disables its box.
        let preview = try? DuotonePreview(editor: self, document: document)
        presentAsSheet(DuotoneSheetController(
            options: options,
            refusal: { [weak self] in self?.duotoneRefusal($0) },
            commit: { [weak self] in self?.commitDuotone($0) },
            preview: preview))
    }

    /// The core's refusal of `options` on the current document (the sheet's OK asks it first):
    /// malformed options or a source the mode refuses.
    private func duotoneRefusal(_ options: DuotoneOptions) -> String? {
        guard let document else { return "The document closed." }
        do {
            _ = try RasterPrecisionDocument.normalizeDuotone(options.json)
            return try DuotoneEdit(document, options: options).refusal
        } catch {
            return error.localizedDescription
        }
    }

    /// One Duotone edit; unchanged options do nothing.
    private func commitDuotone(_ options: DuotoneOptions) {
        guard let document else { return }
        DuotoneSheetController.lastOptions = options
        do {
            let edit = try DuotoneEdit(document, options: options)
            guard edit.changes else { return }
            let prepared = try edit.compute()
            try edit.publish(prepared, to: document, record: [edit.step()])
        } catch is JobCancelled {
            return
        } catch {
            document.presentError(error)
        }
    }
}

extension CmykModeConversion {
    /// The RGB Color options the core's Duotone → RGB reads: the intent, BPC and Flatten.
    static func duotoneRGBOptions(_ options: CmykConversionOptions) -> [String: Any] {
        var object = rgbOptions(options, float: false)
        object["flatten"] = options.flatten
        return object
    }

    /// A Duotone source's plan toward `target`: the render's refusal and `flatten_required` first,
    /// then the working memory of the render beside the existing route's plan, asked of the source
    /// as it is (RGB, Grayscale and Bitmap are the core's own plans: `convertDuotoneToRGBPlan`,
    /// `convertDuotoneToGrayPlan`, `convertDuotoneToBitmapPlan`).
    static func duotonePlan(source: RasterDocument, typed: RasterPrecisionDocument,
                            target: ColorModeTarget, profile: Data?, options: CmykConversionOptions,
                            indexed: [String: Any], bitmap: [String: Any]) throws -> [String: Any] {
        let route: [String: Any]
        switch target {
        case .rgb:
            return try typed.convertDuotoneToRGBPlan(profile: profile ?? RasterProfile.builtin(.sRGB),
                                                     options: duotoneRGBOptions(options))
        case .gray:
            return try typed.convertDuotoneToGrayPlan(flatten: options.flatten)
        case .bitmap:
            return try typed.convertDuotoneToBitmapPlan(
                options: bitmapCoreOptions(bitmap, flatten: options.flatten))
        case .cmyk:
            route = try typed.convertToCMYKPlan(profile: profile, options: options)
        case .lab:
            route = try labPlan(source: source, typed: typed, options: options)
        case .indexed:
            route = try typed.convertToIndexedPlan(
                options: indexedCoreOptions(indexed, flatten: options.flatten))
        case .duotone:
            throw DoctoredCoreError(message: "Image ▸ Mode ▸ Duotone on a Duotone document is not "
                + "a conversion through its inks.")
        }
        let render = try typed.duotoneRenderPlan(flatten: options.flatten)
        var plan = route
        plan["working_memory_plan_bytes"] = NSNumber(value:
            try PrecisionDocumentDescriptor.integer(render, "working_memory_plan_bytes")
                &+ PrecisionDocumentDescriptor.integer(route, "working_memory_plan_bytes"))
        if let refusal = render["refusal"] as? String {
            plan["refusal"] = refusal
            plan["flatten_required"] = render["flatten_required"] ?? false
        }
        plan["flattens"] = (render["flattens"] as? Bool ?? false) || (route["flattens"] as? Bool ?? false)
        return plan
    }

    /// Image ▸ Mode ▸ Grayscale on a Duotone source inside the job: the core's conversion, the
    /// inks' luminance in the document's own Gray profile (`convertDuotoneToGray`).
    static func duotoneToGray(_ source: RasterDocument, options: CmykConversionOptions,
                              control: RasterJobControl) throws -> RasterPrecisionDocument {
        guard let typed = source.precision else {
            throw DoctoredCoreError(message: "The document is not a Duotone document.")
        }
        let context = try PrecisionWorkContext(source: typed)
        return try control.withChildOperation(from: 0, to: 1) { child in
            try typed.convertDuotoneToGray(flatten: options.flatten, workingLimit: context.limit,
                                           otherRetained: context.otherRetained, control: child)
        }
    }

    /// Image ▸ Mode ▸ Bitmap… on a Duotone source inside the job: the Grayscale rule, then
    /// Bitmap under the sheet's options (`convertDuotoneToBitmap`).
    static func duotoneToBitmap(_ source: RasterDocument, options: CmykConversionOptions,
                                bitmap: [String: Any], control: RasterJobControl) throws
        -> RasterPrecisionDocument {
        guard let typed = source.precision else {
            throw DoctoredCoreError(message: "The document is not a Duotone document.")
        }
        let context = try PrecisionWorkContext(source: typed)
        let core = bitmapCoreOptions(bitmap, flatten: options.flatten)
        return try control.withChildOperation(from: 0, to: 1) { child in
            try typed.convertDuotoneToBitmap(options: core, workingLimit: context.limit,
                                             otherRetained: context.otherRetained, control: child)
        }
    }

    /// The render of a Duotone source through its inks inside the job, from 0 to `end`: the RGB
    /// document CMYK, Lab and Indexed Color convert.
    static func duotoneRendered(_ typed: RasterPrecisionDocument, options: CmykConversionOptions,
                                control: RasterJobControl, to end: Double) throws -> RasterPrecisionDocument {
        let context = try PrecisionWorkContext(source: typed)
        return try control.withChildOperation(from: 0, to: end) { child in
            try typed.duotoneRender(flatten: options.flatten, workingLimit: context.limit,
                                    otherRetained: context.otherRetained, control: child)
        }
    }
}
