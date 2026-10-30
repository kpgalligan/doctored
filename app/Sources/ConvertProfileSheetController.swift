import AppKit

/// Image > Mode > Convert to Profile…: transform every layer's pixels into
/// another space so the picture looks the same.
///
/// The counterpart of Assign, and the reverse trade: Convert changes the
/// pixel NUMBERS so that what the eye sees does not change. Only the layers
/// are converted — masks, alpha channels and layer descriptions are
/// coverage, not colour — and a conversion into a smaller gamut clips, which
/// is the one thing worth previewing here and why this sheet has a live
/// preview at all.
///
/// "What the eye sees does not change" is true of each LAYER and not of
/// every composite, and the note says which this document is: blend math, an
/// adjustment layer's parameters and a Blend If threshold are all evaluated
/// on the numbers being changed, so such a stack visibly moves — correctly,
/// but not silently. `ColorProfileControls.appearanceDependsOnNumbers` is
/// the question.
///
/// Three refusals live in the copy rather than in a disabled menu item,
/// because a beep cannot tell them apart: a document whose own profile is
/// LUT-based cannot be converted FROM, a loaded LUT-based profile cannot be
/// converted INTO, and a destination that describes the space the document
/// is already in has nothing to convert. The first two are real ICC profiles
/// Doctored will happily display, embed and assign — it is only the
/// transform that needs a matrix/TRC model — so the sheet names the reason
/// and points at Assign.
///
/// The third is the one that has to be asked of the CORE. "Is this the space
/// we are in?" is not "are these the same bytes": the "sRGB IEC61966-2.1"
/// blob most cameras and Photoshop embed is 3144 bytes against the built-in
/// sRGB's 2568 and describes exactly the same space, so a byte comparison
/// lights Apply up for a conversion the core then refuses — a dismissed
/// sheet and an unexplained beep. `RasterProfile.describesSameSpace` asks the
/// question the core answers.
final class ConvertProfileSheetController: NSViewController {
    private weak var editor: EditorViewController?
    private weak var canvas: ImageCanvasView?
    /// The handle the preview runs on, captured when the sheet opened: the
    /// PreviewRenderer closure runs on a background queue and must never
    /// reach into live document state (`AdjustmentSheet`'s `baseDoc`
    /// rule). Apply re-runs the conversion on the document's CURRENT handle.
    private let baseDoc: RasterDocument
    private let precisionCapture: PrecisionEditorCapture?
    private let retainedState: EditingState?
    private let capturedRevision: UInt64?
    private let currentProfile: Data
    private let currentName: String
    /// Whether the core can DECODE this document's profile for a conversion:
    /// at 8 bits a matrix/TRC model, or (Phase 11C) a LUT profile with a
    /// readable A2B table; at 16 and 32 bits a matrix/TRC model only. This
    /// is `profileCanDecode`, which asks the typed document at depth.
    private let sourceIsConvertible: Bool

    private let profilePopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let intentPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let bpcBox = NSButton(checkboxWithTitle: "Black point compensation",
                                  target: nil, action: nil)
    private let adjustmentsPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    /// "Rasterize N RAW layers, then convert" — the disclosed way past
    /// the settings a RAW layer's source cannot follow, shown only while the
    /// preflight refuses for that reason (`sourceRefusal`).
    private let rasterizeBox = NSButton(checkboxWithTitle: "", target: nil, action: nil)
    private let note = ColorProfileControls.noteLabel()
    private let policyNote = ColorProfileControls.noteLabel()
    private var applyButton: NSButton!
    private let renderer = PreviewRenderer()

    /// The intent, BPC and adjustment policy the popups name.
    private var options = ConversionOptions.default
    /// The core's preflight refusal for the current selection, or nil.
    private var preflightRefusal: String?
    /// The core's sentence when these settings refuse ONLY because they
    /// cannot keep the document's RAW layers bound: the same request with
    /// `rasterizeSources` passes the preflight. nil otherwise.
    private var sourceRefusal: String?
    /// The note when the user cancels a baking preflight's wait: Apply
    /// stays disabled until a setting changes and the check runs again.
    static let preflightCancelled =
        "The conversion check was cancelled. Change a setting to check again."

    /// The core's adjustment report for the current selection.
    private var report = ConversionReport()

    /// The profile the popup names, and what the core makes of it. The kind
    /// and the same-space answer are stored rather than recomputed: each
    /// costs a parse of the blob, and they only ever change when the
    /// selection does.
    private var choice: ProfileChoice
    private var choiceKind: RasterProfileKind
    /// Whether converting into `choice` would change nothing — the core's
    /// own question, asked through `RasterProfile.describesSameSpace`.
    private var choiceIsCurrentSpace: Bool

    /// nil when there is no document, or when its profile cannot be read —
    /// a document always has one, so that means the read failed and the
    /// sheet has nothing truthful to convert from. The caller beeps.
    init?(editor: EditorViewController) {
        guard let doc = editor.document?.doc, let profile = doc.iccProfile, !profile.isEmpty
        else { return nil }
        self.editor = editor
        self.canvas = editor.canvas
        self.baseDoc = doc
        retainedState = doc.retainedSourceIndices.isEmpty ? nil : editor.document?.captureEditingState()
        capturedRevision = editor.document?.revision
        precisionCapture = doc.isHighDepth ? editor.document.flatMap { try? PrecisionEditorCapture($0) } : nil
        self.currentProfile = profile
        self.currentName = doc.profileName
        self.sourceIsConvertible = doc.profileCanDecode
        // Opens on the document's own profile, so the sheet starts on the
        // identity: Apply is disabled until a different space is picked, and
        // the canvas needs no first preview because it is already showing
        // exactly what that selection would produce.
        let initial =
            WorkingSpace.allCases.first { $0.profileData == profile }
            .map { ProfileChoice.builtin($0) } ?? ProfileChoice.file(profile, doc.profileName, nil)
        self.choice = initial
        // The opening selection IS the document's profile, and the core has
        // already classified that one through `profileCanDecode` (at depth
        // exactly the matrix/TRC question), so the blob need not be parsed
        // again here — and neither does the same-space question, whose
        // answer for a profile against itself is yes.
        self.choiceKind = doc.profileCanDecode ? .rgbMatrix : .rgbUnconvertible
        self.choiceIsCurrentSpace = true
        super.init(nibName: nil, bundle: nil)
        renderer.onRender = { [weak self] cgImage in
            if let self, let capture = self.precisionCapture, let document = self.editor?.document,
               (try? capture.requireCurrent(document)) == nil { self.canvas?.previewImage = nil; return }
            self?.canvas?.previewImage = cgImage
        }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("ConvertProfileSheetController does not support NSCoder")
    }

    override func loadView() {
        ColorProfileControls.fillProfiles(
            profilePopup, current: currentProfile, currentName: currentName,
            installed: baseDoc.isHighDepth
                ? InstalledProfiles.highDepthDestinations : InstalledProfiles.exportDestinations)
        profilePopup.target = self
        profilePopup.action = #selector(profileChanged(_:))

        for intent in RenderingIntent.allCases {
            intentPopup.addItem(withTitle: intent.displayName)
            intentPopup.lastItem?.tag = Int(intent.code)
        }
        intentPopup.selectItem(withTag: Int(options.intent.code))
        intentPopup.font = DS.sans(13)
        intentPopup.target = self
        intentPopup.action = #selector(optionsChanged(_:))

        bpcBox.font = DS.sans(13)
        bpcBox.state = options.blackPointCompensation ? .on : .off
        bpcBox.target = self
        bpcBox.action = #selector(optionsChanged(_:))

        for (index, policy) in AdjustmentPolicy.allCases.enumerated() {
            adjustmentsPopup.addItem(withTitle: policy.displayName)
            adjustmentsPopup.lastItem?.tag = index
        }
        adjustmentsPopup.selectItem(withTag: 0)
        adjustmentsPopup.font = DS.sans(13)
        adjustmentsPopup.target = self
        adjustmentsPopup.action = #selector(optionsChanged(_:))

        rasterizeBox.font = DS.sans(13)
        rasterizeBox.title = AssignProfileSheetController.rasterizeTitle(
            baseDoc.retainedSourceIndices.count, then: "convert")
        rasterizeBox.target = self
        rasterizeBox.action = #selector(optionsChanged(_:))

        let grid = NSGridView(views: [
            [fieldLabel("Destination:"), profilePopup],
            [fieldLabel("Intent:"), intentPopup],
            [NSGridCell.emptyContentView, bpcBox],
            [fieldLabel("Adjustment layers:"), adjustmentsPopup],
            [NSGridCell.emptyContentView, rasterizeBox],
        ])
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106

        let content = NSStackView(views: [grid, note, policyNote])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 12

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        refreshPreflight()
        syncControls()
        view = makeSheetView(
            title: "Convert to profile",
            hint: "Convert transforms every layer's pixels into another space, so each layer "
                + "keeps its appearance while its numbers change. Layer masks, alpha channels "
                + "and layer descriptions are coverage, not colour, and are left exactly as "
                + "they are.",
            content: content,
            buttonRow: makeButtonRow(
                cancel: cancelButton, apply: applyButton,
                leading: [sheetFootnote("one undo step")]))
    }

    // MARK: - State

    /// False when the document's profile cannot be converted from, when the
    /// selected profile cannot be converted into, and when the selection
    /// describes the space the document is already in — the three cases the
    /// core answers with the same bare nil, and which the note tells apart.
    private var canApply: Bool {
        guard sourceIsConvertible, choiceIsDestination else { return false }
        guard preflightRefusal == nil else { return false }
        // A non-default intent, BPC or the adjustment policy is a real
        // change even when the two spaces agree, so the same-space refusal
        // applies only to the default request.
        return !choiceIsCurrentSpace || !options.isDefault
    }

    /// Whether the selection can be converted INTO. At 16 and 32 bits the
    /// typed conversion needs a matrix/TRC model on both sides, so a LUT
    /// picked through "Load from file…" refuses here rather than in the core.
    private var choiceIsDestination: Bool {
        baseDoc.isHighDepth ? choiceKind == .rgbMatrix : choiceKind.canBeDestination
    }

    /// The line under the popup. It carries the refusals in full, because it
    /// is the one label in the card that grows to fit a sentence and the one
    /// place a user can find out why Apply is grey.
    private var noteText: String {
        guard sourceIsConvertible else {
            if baseDoc.isHighDepth {
                return "Doctored cannot convert a high-depth document from “\(currentName)” — at 16 "
                    + "and 32 bits both profiles must be matrix/TRC. Assign Profile can still "
                    + "relabel the document."
            }
            return "Doctored cannot convert from “\(currentName)” — it is not a matrix/TRC "
                + "profile. Assign Profile can still relabel the document."
        }
        guard choiceIsDestination else {
            // A float document cannot be relabelled with a LUT profile
            // either, so this is the core's `float_model_sentence`, which
            // points at no Assign.
            if baseDoc.isFloat {
                return "Doctored cannot convert a 32-bit float document into “\(choice.name)” — it "
                    + "has no matrix/TRC model, and a float document's linear values are defined "
                    + "by its profile's primaries. Choose a matrix/TRC RGB profile."
            }
            if baseDoc.isHighDepth {
                return "Doctored cannot convert a high-depth document into “\(choice.name)” — at 16 "
                    + "and 32 bits both profiles must be matrix/TRC. Assign Profile can still "
                    + "relabel the document."
            }
            return "Doctored cannot convert into “\(choice.name)” — it carries neither a "
                + "matrix/TRC model nor a colour table this build can read. Assign Profile "
                + "can still relabel the document."
        }
        if let preflightRefusal {
            guard sourceRefusal != nil else { return preflightRefusal }
            // The core's own sentence, verbatim, then the choice below it.
            return preflightRefusal.prefix(1).uppercased() + preflightRefusal.dropFirst()
                + ". Rasterizing keeps each RAW layer's current pixels and drops its camera "
                + "original, which is why it is a separate choice."
        }
        if baseDoc.isFloat {
            return "Converts every layer's linear values between the two profiles' primaries with "
                + "no clamp, so a colour outside the destination gamut keeps a negative component. "
                + "Live adjustment parameters stay unchanged, so their evaluated appearance may "
                + "change. The preview shows that result."
        }
        if baseDoc.isHighDepth {
            return "Converts stored 16-bit RGB through the document profiles and preserves alpha. Destination gamut may clip. "
                + "Live adjustment parameters stay unchanged, so their evaluated appearance may change. The preview shows that result."
        }
        guard !choiceIsCurrentSpace || !options.isDefault else {
            // Two spellings of one refusal: the same profile, and a
            // different profile for the same space. Naming both sides in the
            // second case is the whole point — the two names are often
            // identical, and a user who picked a profile off disk deserves
            // to be told that it lands where the document already is.
            return choice.data == currentProfile
                ? "This document is already in “\(currentName)”, so there is nothing to convert."
                : "“\(choice.name)” and “\(currentName)” already describe the same colour "
                    + "space, so there is nothing to convert."
        }
        // What "the picture looks the same" is actually worth is a property
        // of THIS stack, not of the conversion: blend math, an adjustment's
        // parameters and a Blend If threshold are evaluated on the numbers
        // being changed, so a layered composite genuinely moves. The sheet
        // says which case the document is in rather than promising the
        // pleasant one — the live preview behind it is showing the truth
        // either way.
        return "Every layer's pixels move from “\(currentName)” to “\(choice.name)”. "
            + ColorProfileControls.convertAppearanceSentence(baseDoc)
            + " Layer-style and text colours are authored in sRGB and convert where they are "
            + "used rather than here, so their own colours are unaffected."
    }

    /// Re-asks the CORE what this selection would do, and is the only
    /// authority on the refusal, so it runs on every change of the
    /// destination, the intent, BPC or the policy. Usually cheap (no pixels
    /// move); a preserve-appearance check that can bake renders per
    /// adjustment layer at 8 and 16 bits, so `ProfileConversionWork.preflight`
    /// runs that one as a cancellable worker wait while this sheet stays up;
    /// a cancel leaves Apply disabled until a setting changes.
    private func refreshPreflight() {
        preflightRefusal = nil
        sourceRefusal = nil
        report = ConversionReport()
        options.rasterizeSources = false
        guard sourceIsConvertible, choiceIsDestination else { return }
        do {
            report = try ProfileConversionWork.preflight(
                baseDoc, profile: choice.data, options: options,
                title: "Checking Profile Conversion")
        } catch is JobCancelled {
            preflightRefusal = Self.preflightCancelled
        } catch {
            let refusal = error.localizedDescription
            // When the same request passes once it may rasterize the
            // bound layers, they are all that stands in the way, and the
            // checkbox offers exactly that.
            var consenting = options
            consenting.rasterizeSources = true
            guard !baseDoc.retainedSourceIndices.isEmpty,
                  let allowed = try? ProfileConversionWork.preflight(
                      baseDoc, profile: choice.data, options: consenting,
                      title: "Checking Profile Conversion")
            else {
                preflightRefusal = refusal
                return
            }
            sourceRefusal = refusal
            if rasterizeBox.state == .on {
                options = consenting
                report = allowed
            } else {
                preflightRefusal = refusal
            }
        }
    }

    private func syncControls() {
        rasterizeBox.isHidden = sourceRefusal == nil
        note.stringValue = noteText
        policyNote.stringValue = preflightRefusal == nil ? report.notice : ""
        policyNote.isHidden = policyNote.stringValue.isEmpty
        applyButton.isEnabled = canApply
    }

    // MARK: - Preview

    /// The document as the conversion would leave it, tagged with the
    /// DESTINATION profile so the canvas draws it through the space its new
    /// numbers belong to — which is the whole point: a correct conversion
    /// looks unchanged, and only the clipped colours move.
    ///
    /// A nil result clears the preview and puts the real document back on
    /// screen, which is the honest picture of "this would change nothing".
    private func requestPreview() {
        if let precisionCapture {
            do {
                try precisionCapture.requestProfilePreview(
                    renderer, profile: choice.data, options: options)
            } catch { canvas?.previewImage = nil }
            return
        }
        guard canApply else {
            // Cleared THROUGH the renderer, not by assigning the canvas
            // directly: a render already in flight still delivers its
            // result, and only a request queued behind it is guaranteed to
            // land last.
            renderer.request { nil }
            return
        }
        let bytes = choice.data
        let source = baseDoc
        let options = self.options
        if retainedState != nil {
            do {
                let scratch = try ProfileConversionWork.scratch(source, profile: bytes, options: options)
                let reservation = try SelectionWorkBudget.reservePreview(
                    bytes: scratch + source.projectionScratchEstimate(), outputBytes: 0)
                renderer.cancelCompute?()
                renderer.cancelCompute = { reservation.control.cancel() }
                // The conversion's own plan and the display preparation each
                // charge this lease, so the worker leaves it out of
                // `otherRetained`.
                renderer.request { reservation.runAsOwnLease { token in
                    do {
                        guard token.setScratchLimit(scratch) else { return nil }
                        let result = try source.convertingToProfile(
                            bytes, options: options, control: token)?.document
                        guard let converted = result, !token.isCancelled else { return nil }
                        let preview = PreparedDocumentEdit(converted).preview
                        return token.isCancelled ? nil : preview
                    } catch { return nil }
                } }
            } catch { canvas?.previewImage = nil }
            return
        }
        renderer.request {
            guard let converted = try? source.convertingToProfile(bytes, options: options)
            else { return nil }
            return converted.document.flattened()?
                .makeCGImage(in: ColorProfile.space(for: bytes))
        }
    }

    // MARK: - Actions

    @objc private func profileChanged(_ sender: Any?) {
        ColorProfileControls.resolveSelection(
            profilePopup, presenter: self, previous: choice
        ) { [weak self] picked in
            guard let self = self else { return }
            self.choice = picked
            self.choiceKind = picked.kind
            self.choiceIsCurrentSpace = self.baseDoc.isHighDepth ? picked.data == self.currentProfile
                : ColorProfileControls.describesCurrentSpace(picked.data, current: self.currentProfile)
            self.refreshPreflight()
            self.syncControls()
            self.requestPreview()
        }
    }

    /// The intent, BPC, policy and rasterize controls share one action: each
    /// one changes what the conversion would do, so each one re-asks the
    /// preflight (which sets `options.rasterizeSources` from the checkbox).
    @objc private func optionsChanged(_ sender: Any?) {
        options.intent = RenderingIntent.allCases
            .first { Int($0.code) == intentPopup.selectedTag() } ?? .relative
        options.blackPointCompensation = bpcBox.state == .on
        let policies = AdjustmentPolicy.allCases
        options.adjustments = policies.indices.contains(adjustmentsPopup.selectedTag())
            ? policies[adjustmentsPopup.selectedTag()] : .keep
        refreshPreflight()
        syncControls()
        requestPreview()
    }

    @objc private func applyClicked(_ sender: Any?) {
        if let retainedState, let document = editor?.document,
           document.revision != capturedRevision || !document.isCurrentEditingState(retainedState) {
            document.presentError(DoctoredCoreError(message: "The document changed after this profile preview was opened. Reopen Convert to Profile."))
            return
        }
        guard canApply else {
            NSSound.beep()
            return
        }
        let choice = self.choice
        let options = self.options
        renderer.cancel()
        canvas?.previewImage = nil
        dismiss(self)
        if let precisionCapture, let document = editor?.document {
            do {
                let landed = try precisionCapture.commitProfile(
                    choice, convert: true, options: options, to: document)
                if landed.bakes { editor?.presentConversionNotice(landed, document: document) }
            } catch is JobCancelled {
                return
            } catch { document.presentError(error) }
            return
        }
        // The commit lives in the editor's extension, not here: the sheet
        // chooses, `commitProfileConversion` re-runs the conversion on the
        // document's current handle as one undo step.
        editor?.commitProfileConversion(choice, options: options)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        renderer.cancel()
        canvas?.previewImage = nil
        dismiss(self)
    }
}
