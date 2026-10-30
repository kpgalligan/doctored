import AppKit

// The Evaluation sheet follows `ConvertProfileSheetController`'s rules exactly — the preview runs
// on the handle captured at init, Apply re-runs the switch on the document's CURRENT handle — and
// its readout is `RasterDocument.compare` computed on the render queue and handed back through a
// lock-guarded mailbox.

/// Image ▸ Evaluation… — choosing how this document's projection evaluates
/// its stack (`DocumentEvaluation.swift`).
///
/// The sheet exists because the choice is consequential and reversible only by
/// choosing again: linear light changes what every blend, opacity and mask
/// means, and it REFUSES the operators this phase has not migrated. So the
/// card carries three things a menu toggle could not: the live preview of the
/// candidate projection, a measured readout of how far it sits from the one on
/// screen, and — when the document cannot take the policy — the core's own
/// sentence naming the layer and the operator.
///
/// The preview follows `ConvertProfileSheetController`'s rules exactly: the
/// handle it renders is captured at init (`baseDoc` / `precisionCapture`),
/// because `PreviewRenderer`'s closure runs on a background queue and must
/// never reach into live document state, while Apply re-runs the switch on the
/// document's CURRENT handle. The readout is computed in that same closure —
/// comparing the two projections is the expensive part, and doing it anywhere
/// else would either block the main thread or compare two different handles —
/// and handed back through a lock-guarded mailbox that `onRender` drains.
final class EvaluationSheetController: NSViewController {
    private weak var editor: EditorViewController?
    private weak var canvas: ImageCanvasView?
    /// The handle the preview runs on, captured when the sheet opened.
    private let baseDoc: RasterDocument
    /// `baseDoc`'s retained 8-bit projection, captured with it: the readout's
    /// `before` is what the canvas shows, not a second composite. nil
    /// at high depth or when none is retained, and the preview flattens.
    /// `retainedPrecisionBefore` is the typed twin: the retained typed
    /// projection of `capture.facade`, nil at 8 bits or when none is
    /// retained, and the typed preview renders "before" again.
    private let retainedBefore: RasterImage?
    private let retainedPrecisionBefore: RasterPrecisionImage?
    private let precisionCapture: PrecisionEditorCapture?
    private let currentProfile: Data
    private let current: EvaluationPolicy

    private let policyPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let note = ColorProfileControls.noteLabel()
    private let readout = ColorProfileControls.noteLabel()
    private var applyButton: NSButton!
    private let renderer = PreviewRenderer()

    /// What the popup names.
    private var choice: EvaluationPolicy
    /// The core's refusal for `choice` on this document, or nil.
    private var refusal: String?

    /// The measured difference between the candidate projection and the one
    /// the canvas is showing. Written on the render queue, read on the main
    /// thread in `onRender`, so it is behind a lock rather than a bare field.
    private let mailboxLock = NSLock()
    private var mailbox: ProjectionDifference?

    /// nil when there is no document, or when its profile cannot be read — a
    /// document always has one, so that means the read failed and the sheet
    /// has nothing truthful to say. The caller beeps.
    init?(editor: EditorViewController) {
        guard let doc = editor.document?.doc, let profile = doc.iccProfile, !profile.isEmpty
        else { return nil }
        self.editor = editor
        self.canvas = editor.canvas
        self.baseDoc = doc
        self.retainedBefore = doc.isHighDepth ? nil : editor.document?.retainedByteProjection
        self.retainedPrecisionBefore = doc.isHighDepth ? editor.document?.retainedPrecisionProjection : nil
        self.currentProfile = profile
        self.current = doc.evaluation
        self.choice = doc.evaluation
        precisionCapture = doc.isHighDepth
            ? editor.document.flatMap { try? PrecisionEditorCapture($0) } : nil
        super.init(nibName: nil, bundle: nil)
        renderer.onRender = { [weak self] cgImage in
            guard let self else { return }
            if let capture = self.precisionCapture, let document = self.editor?.document,
               (try? capture.requireCurrent(document)) == nil {
                self.canvas?.previewImage = nil
                return
            }
            self.canvas?.previewImage = cgImage
            self.mailboxLock.lock()
            let measured = self.mailbox
            self.mailboxLock.unlock()
            self.showReadout(measured)
        }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("EvaluationSheetController does not support NSCoder")
    }

    override func loadView() {
        for policy in EvaluationPolicy.allCases {
            policyPopup.addItem(withTitle: policy.displayName)
            policyPopup.lastItem?.representedObject = policy.rawValue
        }
        policyPopup.selectItem(at: EvaluationPolicy.allCases.firstIndex(of: current) ?? 0)
        policyPopup.font = DS.sans(13)
        policyPopup.target = self
        policyPopup.action = #selector(policyChanged(_:))

        let grid = NSGridView(views: [[fieldLabel("Evaluate in:"), policyPopup]])
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106

        let content = NSStackView(views: [grid, note, readout])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 12

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        refreshRefusal()
        syncControls()
        view = makeSheetView(
            title: "Evaluation",
            hint: "Linear light decodes each layer through this document's colour profile, "
                + "composites in linear light, and re-encodes once at the output. Blends, "
                + "opacity and masks all change meaning; the picture on screen behind this "
                + "sheet is the result.",
            content: content,
            buttonRow: makeButtonRow(
                cancel: cancelButton, apply: applyButton,
                leading: [sheetFootnote("one undo step")]))
    }

    // MARK: - State

    private var canApply: Bool { choice != current && refusal == nil }

    /// Re-asks the CORE what this choice would do. Cheap — the switch moves no
    /// pixels — and the only authority on the refusal.
    private func refreshRefusal() {
        refusal = nil
        guard choice != current else { return }
        do {
            _ = try baseDoc.withEvaluation(choice)
        } catch {
            refusal = error.localizedDescription
        }
    }

    private var noteText: String {
        if let refusal { return refusal }
        if choice == current {
            return "This document already evaluates in \(choice.displayName.lowercased()). "
                + choice.summary
        }
        return choice.summary
    }

    private func showReadout(_ difference: ProjectionDifference?) {
        readout.stringValue = difference.map { "Against the current projection: \($0.summary)" }
            ?? ""
        readout.isHidden = readout.stringValue.isEmpty
    }

    private func syncControls() {
        note.stringValue = noteText
        applyButton.isEnabled = canApply
        if !canApply { showReadout(nil) }
    }

    // MARK: - Preview

    /// The document as the switch would leave it, drawn through the space its
    /// numbers belong to — which does not change here, so any visible
    /// difference is the policy's and nothing else's.
    private func requestPreview() {
        mailboxLock.lock()
        mailbox = nil
        mailboxLock.unlock()
        guard canApply else {
            // Cleared THROUGH the renderer so a render already in flight
            // cannot land after this.
            renderer.request { nil }
            return
        }
        if let precisionCapture {
            requestTypedPreview(precisionCapture)
            return
        }
        let source = baseDoc
        let retained = retainedBefore
        let policy = choice
        let space = ColorProfile.space(for: currentProfile)
        let lock = mailboxLock
        renderer.request { [weak self] in
            guard let candidate = try? source.withEvaluation(policy) ?? source,
                  let after = candidate.flattened() else { return nil }
            if let before = retained ?? source.flattened(),
               let measured = try? RasterDocument.compare(before, after) {
                lock.lock()
                self?.mailbox = measured
                lock.unlock()
            }
            return after.makeCGImage(in: space)
        }
    }

    /// The typed twin, under the shared preview reservation — the shape
    /// `PrecisionEditorCapture.requestProfilePreview` uses.
    private func requestTypedPreview(_ capture: PrecisionEditorCapture) {
        let source = capture.source
        let facade = capture.facade
        let retained = retainedPrecisionBefore
        let policy = choice
        let lock = mailboxLock
        do {
            // The candidate projection, plus the one on screen when none is
            // retained, so the readout can be measured on the SAME queue that
            // made them.
            let estimate = try facade.projectionScratchEstimate()
            // Each display preparation charges its own projection, so the
            // worker runs the reservation as its own lease. The first
            // projection stays live while the on-screen one admits, so when
            // none is retained it is a payload lease every admission counts.
            let first = retained == nil
                ? try PrecisionEditPayload.admitted(bytes: estimate) : nil
            let reservation = try SelectionWorkBudget.reservePreview(
                bytes: estimate + 1, outputBytes: 0)
            renderer.cancelCompute?()
            let control = reservation.control
            renderer.cancelCompute = { control.cancel() }
            renderer.request { [weak self] in
                withExtendedLifetime(first) { reservation.runAsOwnLease { token in
                    do {
                        guard let switched = try source.settingEvaluation(policy.code)
                        else { return nil }
                        guard !token.isCancelled else { return nil }
                        let prepared = PreparedDocumentEdit(try RasterDocument(precision: switched))
                        if let after = prepared.precisionProjection,
                           let before = retained ?? PreparedDocumentEdit(facade).precisionProjection,
                           let measured = try? RasterDocument.compare(before, after) {
                            lock.lock()
                            self?.mailbox = measured
                            lock.unlock()
                        }
                        return token.isCancelled ? nil : prepared.preview
                    } catch { return nil }
                } }
            }
        } catch { canvas?.previewImage = nil }
    }

    // MARK: - Actions

    @objc private func policyChanged(_ sender: Any?) {
        let raw = policyPopup.selectedItem?.representedObject as? String
        choice = raw.flatMap(EvaluationPolicy.init(rawValue:)) ?? current
        refreshRefusal()
        syncControls()
        requestPreview()
    }

    @objc private func applyClicked(_ sender: Any?) {
        guard canApply else {
            NSSound.beep()
            return
        }
        let policy = choice
        renderer.cancel()
        canvas?.previewImage = nil
        dismiss(self)
        // The commit lives in the editor's extension, not here: the sheet
        // chooses, `commitEvaluation` re-runs the switch on the document's
        // current handle as one undo step.
        editor?.commitEvaluation(policy)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        renderer.cancel()
        canvas?.previewImage = nil
        dismiss(self)
    }
}
