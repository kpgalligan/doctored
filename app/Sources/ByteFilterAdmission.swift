import AppKit

// All seven destructive filter sheets (Unsharp Mask, High Pass, Reduce Noise, Dust & Scratches, Box
// Blur, Surface Blur, Add Grain) also pass `SliderSheetController`'s `request:` (a
// `ByteFilterRequest` from the same `params`): `ByteFilterAdmission` reserves the 8-bit exact
// preview and admits Apply against the larger of the core's `filterOpScratchBytes` plan and
// `commitStageBytes` (8n + the margined projection: the result and written layer beside the redraw,
// shared with the publication preflight), a refusal shows on the sheet's `tone.preview.status`
// line, and `AgentServer+ByteFilters.swift` (`byteFilterRequest` / `byteFiltered`) is
// `apply_filter`'s one 8-bit route for the seven. Apply then runs as a cancellable job (progress
// panel, Cancel, Esc) through `ByteFilterAdmission.run`, which `apply_filter` keeps. Gaussian Blur
// (`blur`) is one of them too, and every sheet's Apply and Render ▸ Clouds go through `runPrepared`
// (`ImageDocument.applyCapturedByteFilter`): the same job also writes the result into the document
// and prepares its first projection on the worker, so the main thread no longer does either after
// the filter returns.

/// One `dr_image_filter_op` call. A sheet's preview and Apply, and
/// `apply_filter`, build it from the same params, so what is admitted is what runs.
/// `evaluation` is the document whose layer the image is, for an op that
/// follows that document's evaluation policy (`dr_doc_filter_op`: Liquify on a
/// linear-light document); nil is the bare image call.
struct ByteFilterRequest {
    let op: String
    let params: [String: Any]
    var evaluation: RasterDocument? = nil
}

/// The 8-bit `dr_image_filter_op` filters' host admission: Gaussian Blur, Box Blur,
/// Surface Blur, Add Grain, Radial Blur, Clouds, Difference Clouds, Offset,
/// Motion Blur, Lens Blur, Tilt-Shift, Unsharp Mask, High Pass, Dust & Scratches and Reduce Noise are planned by the core (`RasterImage.filterOpScratchBytes`) and
/// admitted against the working limit before the filter allocates, so a filter
/// that does not fit is refused with `RasterResources.admit`'s sentence.
enum ByteFilterAdmission {
    /// Apply and `apply_filter`. Main thread. A plan of 0 (an exact bypass)
    /// admits nothing. Otherwise the filter and then its commit run one after
    /// the other, so the larger of the plan and `commitStageBytes` is
    /// admitted beside the document's own retained bytes, with the default
    /// remedy. Returns the plan (the `WorkerTask` estimate stays the kernel's).
    @discardableResult
    static func admit(_ request: ByteFilterRequest, on image: RasterImage, in document: RasterDocument) throws
        -> UInt64 {
        assert(Thread.isMainThread)
        let plan = try image.filterOpScratchBytes(request.op, params: request.params)
        guard plan > 0 else { return 0 }
        let commit = try commitStageBytes(image, in: document)
        _ = try RasterResources.shared.admit(scratch: max(plan, commit), own: document.ownRetainedBytes())
        return plan
    }

    /// 8·n + document.projectionScratchEstimate(); overflow throws. The 8·n is the n×4 result image and the
    /// n×4 layer it is written into in the updated document, both alive beside the commit's projection.
    /// `capture.source` is the document's own layer, already in `own`, and is not charged again.
    static func commitStageBytes(_ image: RasterImage, in document: RasterDocument) throws -> UInt64 {
        try commitStageBytes(pixels: UInt64(image.width) * UInt64(image.height), in: document)
    }

    /// The one definition of the commit stage, for n = `pixels`: the up-front
    /// gate above passes the filtered image's n, and the 8-bit publication arm
    /// (`checkPublication`) passes the updated document's canvas.
    static func commitStageBytes(pixels: UInt64, in document: RasterDocument) throws -> UInt64 {
        let display = try document.projectionScratchEstimate()
        let (written, multiplied) = pixels.multipliedReportingOverflow(by: 8)
        let (afterFilter, added) = written.addingReportingOverflow(display)
        guard !multiplied, !added else {
            throw DoctoredCoreError(message: "The filter commit estimate overflowed.")
        }
        return afterFilter
    }

    /// Apply and `apply_filter`. Main thread. Admits, then runs `request`
    /// on `image` on the worker under a cancellable control, with the progress
    /// panel, Cancel and Esc (`PrecisionCapturedWork.run`'s pattern). A plan of 0
    /// runs inline. With `waitForSlot`, an anonymous slot holder is waited for
    /// first (`BackgroundWork.waitForAnonymousOccupant`); `apply_filter` passes
    /// false, because `execute`'s worker wait already covered the call. A slot
    /// still held is refused by `WorkerTask`. nil is the core's exact bypass.
    /// Throws admission's sentence, the core's refusal,
    /// `BackgroundWork.busyMessage`, or `JobCancelled`.
    static func run(_ request: ByteFilterRequest, on image: RasterImage, in document: RasterDocument,
                    title: String, waitForSlot: Bool = true) throws -> RasterImage? {
        assert(Thread.isMainThread)
        let plan = try admit(request, on: image, in: document)
        let (op, params, evaluation) = (request.op, request.params, request.evaluation)
        guard plan > 0 else { return try image.filterOp(op, params: params, evaluation: evaluation) }
        if waitForSlot { BackgroundWork.waitForAnonymousOccupant() }
        let control = try RasterJobControl()
        let result = try BackgroundWork.wait(title: title, control: control,
                                             revealAfter: DocumentJobs.panelDelay) {
            try WorkerTask(scratchEstimate: plan) {
                try image.filterOp(op, params: params, control: control, evaluation: evaluation)
            }
        }
        guard !control.isCancelled else { throw JobCancelled() }
        return result
    }

    /// The sheets' Apply and Render ▸ Clouds. Main thread. Admits as
    /// `run` does and waits for an anonymous slot holder, then one job runs
    /// `request` on the capture's source (child 0…0.8), writes the result
    /// through the capture's selection and prepares the updated document's
    /// first projection (child 0.8…1), all on the worker (`TransformWork.wait`'s
    /// structure), with the progress panel, Cancel and Esc.
    ///
    /// nil = nothing to publish: plan 0 (exact bypass; nothing ran), or the
    /// write changed no covered sample. Throws admission's sentence, the
    /// core's refusal, `BackgroundWork.busyMessage`, `JobCancelled`, or the
    /// prepared edit's `error`.
    static func runPrepared(_ request: ByteFilterRequest, capture: OperationCapture,
                            title: String) throws -> PreparedDocumentEdit? {
        assert(Thread.isMainThread)
        let plan = try admit(request, on: capture.source, in: capture.doc)
        guard plan > 0 else { return nil }
        BackgroundWork.waitForAnonymousOccupant()
        let (op, params, evaluation) = (request.op, request.params, request.evaluation)
        let control = try RasterJobControl()
        let prepared = try BackgroundWork.wait(title: title, control: control,
                                               revealAfter: DocumentJobs.panelDelay) {
            try WorkerTask(scratchEstimate: plan) { () -> PreparedDocumentEdit? in
                try control.beginCompoundOperation()
                let filtered = try control.withChildOperation(from: 0, to: 0.8) { child in
                    try capture.source.filterOp(op, params: params, control: child, evaluation: evaluation)
                }
                // A nil write is unchanged bytes (or a lock): the quiet path.
                guard let filtered,
                      let updated = capture.writing(filtered, selection: capture.selection)
                else { return nil }
                return try control.withChildOperation(from: 0.8, to: 1) { child in
                    PreparedDocumentEdit(updated, scratch: BackgroundWork.scratch, control: child)
                }
            }
        }
        guard !control.isCancelled else { throw JobCancelled() }
        if let message = prepared?.error { throw DoctoredCoreError(message: message) }
        return prepared
    }

    /// The sheet's Apply: true when the core plans no scratch for
    /// `request` — an exact bypass, which runs inline and needs no worker slot.
    static func isExactBypass(_ request: ByteFilterRequest, on image: RasterImage) -> Bool {
        (try? image.filterOpScratchBytes(request.op, params: request.params)) == 0
    }

    /// The exact preview. Main thread. The preview runs the filter and then
    /// writes and displays its result, one after the other, so it reserves the
    /// larger of the plan and `commitStageBytes`: the n×4 result, the written
    /// layer, then the document's projection.
    static func reservePreview(_ request: ByteFilterRequest, capture: OperationCapture) throws
        -> SelectionWorkBudget.Reservation {
        let image = capture.source
        let plan = try image.filterOpScratchBytes(request.op, params: request.params)
        let afterFilter = try commitStageBytes(image, in: capture.doc)
        return try SelectionWorkBudget.reservePreview(bytes: max(plan, afterFilter), outputBytes: 0)
    }
}

extension ImageDocument {
    /// `applyCapturedOperation` for a `dr_image_filter_op` request: the
    /// filter, its write and its first projection run as one worker job
    /// (`ByteFilterAdmission.runPrepared`), and the prepared edit is published
    /// as it is. A nil capture beeps. A nil job result records nothing and
    /// shows nothing, as an exact bypass and a write that changes no covered
    /// sample always have. A described layer still drops its description on
    /// main (`applyRasterizingEdit`), so its published handle is not the
    /// prepared one and it renders on main as before.
    ///
    /// `onLateBusy` is a sheet's way back: when the job is
    /// refused because the worker slot was taken after the sheet's own slot
    /// wait — behind the rasterize prompt — the refusal is not presented.
    /// The edit ends as refused (the ordinary beep), and the handler runs
    /// afterwards so the sheet can come back with its values. Without a
    /// handler (Render ▸ Clouds) the refusal is presented as before.
    func applyCapturedByteFilter(_ capture: OperationCapture?, _ actionName: String,
                                 record: [ActionStep], request: ByteFilterRequest,
                                 onLateBusy: (() -> Void)? = nil) {
        guard let capture else { NSSound.beep(); return }
        defer { preparedEdit = nil }
        var lateBusy = false
        applyCapturedRewrite(capture, actionName, record: record) {
            do {
                let prepared = try ByteFilterAdmission.runPrepared(request, capture: capture, title: actionName)
                preparedEdit = prepared
                return prepared?.document
            } catch where onLateBusy != nil && BackgroundWork.isBusyRefusal(error) {
                lateBusy = true
                return nil
            }
        }
        if lateBusy { onLateBusy?() }
    }
}
