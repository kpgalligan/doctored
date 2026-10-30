import AppKit

extension PrecisionEditorCapture {
    func requestProfilePreview(_ renderer: PreviewRenderer, profile: Data,
                               options: ConversionOptions = .default) throws {
        // The DEFAULT request keeps its own operation, so an 11A/11B preview
        // runs exactly the code it always ran.
        let operation: PrecisionCapturedEdit.Operation = options.isDefault
            ? .convertProfile(profile) : .convertProfileWith(profile, options)
        let edit = try PrecisionCapturedEdit(source, operation: operation)
        renderer.cancelCompute?()
        let reservation = try SelectionWorkBudget.reservePreview(
            bytes: edit.scratchEstimate + facade.projectionScratchEstimate(), outputBytes: 0)
        let previewControl = reservation.control
        renderer.cancelCompute = { previewControl.cancel() }
        // The conversion's own plan and the display preparation each charge
        // this lease, so the worker leaves it out of `otherRetained`.
        renderer.request { reservation.runAsOwnLease { token in
            do {
                let result = try edit.compute(control: token) ?? edit.source
                guard !token.isCancelled else { return nil }
                let prepared = PreparedDocumentEdit(try RasterDocument(precision: result))
                return token.isCancelled ? nil : prepared.preview
            } catch { return nil }
        } }
    }

    /// Report of the edit that LANDED (publish returned true); empty otherwise.
    @discardableResult
    func commitProfile(_ choice: ProfileChoice, convert: Bool,
                       options: ConversionOptions = .default,
                       rasterizing: Bool = false, to document: ImageDocument) throws -> ConversionReport {
        try requireCurrent(document)
        let operation: PrecisionCapturedEdit.Operation
        if convert {
            operation = options.isDefault
                ? .convertProfile(choice.data) : .convertProfileWith(choice.data, options)
        } else {
            operation = rasterizing
                ? .assignProfileRasterizing(choice.data) : .assignProfile(choice.data)
        }
        let name = convert ? "Convert to Profile" : "Assign Profile"
        let result: RasterPrecisionDocument?
        var report = ConversionReport()
        if !facade.artworkIndices.isEmpty, !rasterizing {
            let edited = try facade.artworkProfile(choice.data, convert: convert, options: options)
            result = edited?.document.precision
            report = edited?.report ?? ConversionReport()
        } else {
            // The preflight lists what the conversion bakes; a check that can
            // bake is a cancellable worker wait (`ProfileConversionWork`).
            if convert, options.adjustments == .preserveAppearance {
                report = try ProfileConversionWork.preflight(facade, profile: choice.data,
                    options: options, title: "Checking Profile Conversion")
            }
            result = try PrecisionCapturedEdit(source, operation: operation).run(title: name)
        }
        let landed = try publish(result, to: document, name: name,
            record: .profileCommand(
                convert ? "convert_profile" : "assign_profile", choice,
                options: options, rasterizeSources: rasterizing,
                path: choice.recordedPath, note: "Image ▸ Mode"))
        return landed ? report : ConversionReport()
    }
}
