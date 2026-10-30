import AppKit

// The Crop tool's commit of a box past the canvas with Fill set to Generative Expand or
// Content-Aware Fill (CropFill.swift holds the shared run). Every refusal, the
// Generative Expand sheet and its consent card come first, with the 16-bit box's preview gesture
// untouched, so a refusal or a Cancel leaves the box live. Only right before the run starts is the
// gesture cancelled; from then on every outcome but `completed` keeps the box and re-arms it at
// 16 bits (`rearmPrecisionCropSession`). Mirrored for the agent by `crop`'s `fill`.
extension EditorViewController {
    /// The Fill popup changed: Delete cropped pixels follows it (Content-Aware deletes them),
    /// and so does a 16-bit extended box's preview.
    func cropFillChanged() {
        optionsBar.refreshValues()
        if let session = cropSession, session.precisionGesture != nil {
            refreshPrecisionCropPreview(session)
        }
    }

    /// Return / double-click on an extended box whose Fill is not Background (Default).
    func commitCropFill(_ request: CropFillRequest, fill: CropFill) {
        guard let document, let doc = document.doc else { return }
        guard DocumentJobs.runningUserJob(on: document) == nil else {
            NSSound.beep()
            return
        }
        switch fill {
        case .background:
            return
        case .generativeExpand:
            guard validateGenerativeExpandItem() else {
                if let reason = GenerativeFillRun.refusal(doc, feature: .expand) {
                    document.presentError(DoctoredCoreError(message: reason))
                } else {
                    NSSound.beep()
                }
                return
            }
            presentAsSheet(GenerativeFillSheetController(feature: .expand) { [weak self] settings in
                self?.runCropGenerativeExpand(settings, request)
            })
        case .contentAware:
            guard doc.hasQualifyingBackground() else {
                document.presentError(DoctoredCoreError(message: CropFillRun.contentAwareScope))
                return
            }
            if let reason = DocumentPrecision.refusal(
                tool: "content_aware_fill", arguments: ContentAwareFillSettings().arguments(layer: 0),
                document: document) {
                document.presentError(DoctoredCoreError(message: reason))
                return
            }
            let record: [ActionStep] = .crop(
                request.rect, angle: request.angle, sampler: Self.samplerAgentName(request.sampler),
                deleteCropped: true, backgroundColor: backgroundColor, fill: .contentAware)
            startCropFill {
                try CropFillRun.startContentAware(request, document: document, record: record,
                                                  showProgress: true).job
            }
        }
    }

    /// The sheet's Generate: the gate (its consent card's Cancel sends nothing and keeps the
    /// box), then the run.
    private func runCropGenerativeExpand(_ settings: GenerativeFillSettings, _ request: CropFillRequest) {
        guard let document else { return }
        do {
            try GenerativeFillRun.gate(CloudProviders.provider(settings.provider), agent: false)
        } catch is JobCancelled {
            return
        } catch {
            document.presentError(error)
            return
        }
        startCropFill {
            try CropFillRun.startGenerativeExpand(settings, request, document: document,
                                                  showProgress: true).job
        }
    }

    /// Cancels the box's 16-bit gesture, starts the run and settles its outcome: `completed`
    /// reopens the box over the new canvas; anything else keeps the box, re-armed at 16 bits,
    /// after presenting a failure or a stale document. A box the user has since closed (another
    /// tool) is neither reopened nor re-armed.
    private func startCropFill(_ start: () throws -> DocumentJob) {
        guard let document, let session = cropSession else { return }
        let precision = session.precisionGesture != nil
        cropSession?.precisionGesture?.cancel()
        cropSession?.precisionGesture = nil
        do {
            let job = try start()
            let prior = job.completion
            job.completion = { [weak self] finished in
                prior?(finished)
                self?.cropFillFinished(finished, session: session, precision: precision)
            }
        } catch is JobCancelled {
            if precision { rearmPrecisionCropSession(session, document: document) }
        } catch {
            document.presentError(error)
            if precision { rearmPrecisionCropSession(session, document: document) }
        }
    }

    private func cropFillFinished(_ job: DocumentJob, session: CropSession, precision: Bool) {
        let open = cropSession != nil
        if job.state == "completed" {
            presentGenerativeShortfall(job)
            guard open else { return }
            beginCropSession()
            optionsBar.refreshValues()
            return
        }
        if ["failed", "stale"].contains(job.state), let message = job.message {
            document?.presentError(DoctoredCoreError(message: message))
        }
        if precision, open, let document { rearmPrecisionCropSession(session, document: document) }
    }
}
