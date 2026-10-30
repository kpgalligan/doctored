import AppKit

// Image ▸ Mode ▸ Lab Color (#1 Lab phase 1): the conversion at once (no "Discard
// color information?" — Lab keeps every colour), as ONE undo step (`CmykModeConversion`'s lab
// arms, recorded as `convert_mode` with `mode: "lab"`). A record a Lab document cannot hold —
// the core plan's `flatten_required` refusal, or `ModeFlattenRequired` from inside the job — asks
// "Flatten image?" naming it (Flatten / Cancel), and Flatten converts again with `flatten: true`.
// An RGB source converts relative colorimetric from its profile; an 8-bit one is promoted inside
// the job and converted at depth 8; a Grayscale one at its declared depth; a CMYK one through 16-bit
// ProPhoto RGB (`rgbStage`) at the CMYK depth. RGB Color back is the RGB arm of
// `CmykModeConversion` (`convertLabToRGB`), and since Lab phase 5a CMYK Color its CMYK arm (the
// core's 16-bit ProPhoto stage, this rule reversed). Since Lab phase 5a-ii Grayscale is
// `CmykModeConversion`'s gray arm (the core's `lab_gray`). 8/16 Bits/Channel on a Lab document is
// `commitLabDepth` (`LabDepthEdit`).

extension EditorViewController {
    @objc func convertToLabMode(_ sender: Any?) {
        guard let document, document.doc != nil else { NSSound.beep(); return }
        commitLab(document: document, flatten: false)
    }

    /// One Lab Color conversion; a flatten-required refusal asks the Flatten alert and, on
    /// Flatten, converts again with Flatten Image first.
    private func commitLab(document: ImageDocument, flatten: Bool) {
        var options = CmykConversionOptions()
        options.flatten = flatten
        do {
            let conversion = try CmykModeConversion(document, target: .lab, profile: nil, options: options)
            let prepared = try conversion.compute()
            try conversion.publish(prepared, to: document, record: [conversion.step()])
        } catch is JobCancelled {
            return
        } catch let required as ModeFlattenRequired where !flatten {
            let alert = NSAlert()
            alert.alertStyle = .warning
            alert.messageText = "Flatten image?"
            alert.informativeText = required.reason
            alert.addButton(withTitle: "Flatten")
            alert.addButton(withTitle: "Cancel")
            guard alert.runModal() == .alertFirstButtonReturn else { return }
            commitLab(document: document, flatten: true)
        } catch {
            document.presentError(error)
        }
    }

    /// Image ▸ Mode ▸ 8/16 Bits/Channel on a Lab document (#1 Lab phase 5a, `LabDepthEdit`) as
    /// one undo step, recorded as `convert_precision`; the declared depth beeps.
    func commitLabDepth(_ bits: Int, document: ImageDocument) {
        do {
            let edit = try LabDepthEdit(document, bits: bits)
            guard edit.changes else { NSSound.beep(); return }
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
    /// The Lab Color options the core reads (`lab_convert`): Flatten and the depth.
    static func labOptions(flatten: Bool, eightBit: Bool) -> [String: Any] {
        ["flatten": flatten, "depth": eightBit ? 8 : 16]
    }

    /// The core's Lab Color plan for a typed source; a CMYK source's is its RGB stage's plan, its
    /// working memory the larger of it and the Lab stage's own figure, which the core plans over
    /// the CMYK leaves' pixels (`lab_convert`), as CMYK → Grayscale's.
    static func labPlan(source: RasterDocument, typed: RasterPrecisionDocument,
                        options: CmykConversionOptions) throws -> [String: Any] {
        let lab = labOptions(flatten: options.flatten, eightBit: declaredDepth(source) == 8)
        guard typed.isCmyk else { return try typed.convertToLabPlan(options: lab) }
        var stage = rgbStageOptions
        stage.flatten = options.flatten
        var plan = try typed.convertCMYKToRGBPlan(profile: RasterProfile.builtin(.proPhotoRGB),
                                                  options: stage)
        let labStage = try typed.convertToLabPlan(options: lab)
        plan["working_memory_plan_bytes"] = max(
            try PrecisionDocumentDescriptor.integer(plan, "working_memory_plan_bytes"),
            try PrecisionDocumentDescriptor.integer(labStage, "working_memory_plan_bytes"))
        return plan
    }

    /// The Lab Color arm inside the job: the source's RGB stage (`rgbStage`), then the core's plan
    /// and conversion at the source's depth. A record the mode cannot hold ends the job with
    /// `ModeFlattenRequired`.
    static func lab(_ source: RasterDocument, options: CmykConversionOptions, eightBit: Bool,
                    control: RasterJobControl) throws -> RasterPrecisionDocument {
        let lab = labOptions(flatten: options.flatten, eightBit: eightBit)
        let (rgb, start) = try rgbStage(source, options: options, control: control)
        let plan = try rgb.convertToLabPlan(options: lab)
        if let refusal = plan["refusal"] as? String {
            if plan["flatten_required"] as? Bool == true { throw ModeFlattenRequired(reason: refusal) }
            throw DoctoredCoreError(message: refusal)
        }
        let context = try PrecisionWorkContext(source: rgb)
        return try control.withChildOperation(from: start, to: 1) { child in
            try rgb.convertToLab(options: lab, workingLimit: context.limit,
                                 otherRetained: context.otherRetained, control: child)
        }
    }
}
