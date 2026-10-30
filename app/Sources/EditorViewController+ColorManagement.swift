import AppKit

/// Image > Mode: the two profile commands, plus the validation the frozen
/// editor delegates to them. The third item, Working Space, is an app-wide
/// PREFERENCE and lives on `AppDelegate` — it must stay usable with no
/// document open, which is exactly when a document-scoped responder does
/// not exist.
///
/// The sheets choose; this file commits and validates. `AssignProfile…`
/// commits inside its own dialog (one `applyEdit`, no preview to unwind),
/// while Convert hands its choice back here — the
/// `EditorViewController+ChannelMath.applyImage` shape — so the conversion
/// re-runs on the document's CURRENT handle rather than on the one the
/// sheet previewed.
extension EditorViewController {
    /// Image > Mode > Assign Profile… — RELABELS the document: the pixel
    /// numbers do not change, so the picture changes appearance.
    @objc func assignProfile(_ sender: Any?) {
        // #1 phase 5: a CMYK document assigns a CMYK profile (CmykProfileSheetController).
        if presentCmykProfileSheet(.assign) { return }
        // #1 phase 5a: a Grayscale document assigns a Gray profile (GrayProfileSheetController).
        if presentGrayProfileSheet(.assign) { return }
        guard let document = document, let sheet = AssignProfileSheetController(document: document)
        else {
            NSSound.beep()
            return
        }
        presentAsSheet(sheet)
    }

    /// Image > Mode > Convert to Profile… — TRANSFORMS every layer's pixels
    /// so the picture looks the same in the new space.
    @objc func convertToProfile(_ sender: Any?) {
        // #1 Lab phase 5b: a Lab Color document converts through Image ▸ Mode's conversion.
        if presentLabProfileSheet() { return }
        if presentCmykProfileSheet(.convert) { return }
        if presentGrayProfileSheet(.convert) { return }
        guard let sheet = ConvertProfileSheetController(editor: self) else {
            NSSound.beep()
            return
        }
        presentAsSheet(sheet)
    }

    /// Commits `convertingToProfile` as ONE undo step.
    ///
    /// The conversion re-runs against the document's CURRENT handle inside
    /// the transform rather than the handle the sheet previewed, so an edit
    /// that slipped in while the dialog was open survives — the rule every
    /// live-preview sheet follows.
    ///
    /// It goes through `applyEdit`, NOT `applyRasterizingEdit`, and that is
    /// deliberate: a profile conversion is a whole-document
    /// reinterpretation, like a rotate, not a destructive local edit. A text
    /// or shape layer's meta still describes its pixels afterwards, because
    /// re-rendering it authors its sRGB colour into the document's space —
    /// the same move the conversion just made — so there is nothing for the
    /// user to be asked about. The agent's `convert_profile` takes the same
    /// path (`performGroupedEdit`, not `performPixelEdit`), so the two
    /// cannot drift.
    ///
    /// An 8-bit conversion runs on the bounded worker with admission, progress
    /// and Cancel (`ProfileConversionWork`); a refusal or a cancel is
    /// presented like the core's own refusal, with no undo step.
    ///
    /// A nil result — the profiles describe the same space, or one of them
    /// is not a matrix/TRC profile — beeps inside `applyEdit` and registers
    /// no undo step. The sheet disables Apply for all three cases first and
    /// says which one it is, asking the core the same same-space question
    /// this call would (`RasterProfile.describesSameSpace`) rather than
    /// comparing profile bytes, which would leave Apply live for a different
    /// blob describing the document's own space.
    func commitProfileConversion(_ choice: ProfileChoice,
                                 options: ConversionOptions = .default) {
        guard let document = document else {
            NSSound.beep()
            return
        }
        let bytes = choice.data
        let record: [ActionStep] = .profileCommand(
            "convert_profile", choice, options: options, path: choice.recordedPath,
            note: "Image ▸ Convert to Profile…")
        if let source = document.doc, !source.artworkIndices.isEmpty {
            let landed = document.applyArtworkProfile(
                bytes, convert: true, options: options, record: record)
            if landed.bakes { presentConversionNotice(landed, document: document) }
            return
        }
        if let source = document.doc, !source.isHighDepth, !source.retainedSourceIndices.isEmpty {
            do {
                // nil without a throw is the identity (the same space under
                // non-default options): beep, as `applyEdit` does.
                guard let landed = try RetainedByteProfileWork.run(
                    in: document, profile: bytes, options: options, record: record)
                else {
                    NSSound.beep()
                    return
                }
                if landed.bakes { presentConversionNotice(landed, document: document) }
            } catch { document.presentError(error) }
            return
        }
        // One `applyEdit`, and the REPORT is shown once afterwards: a
        // preserve-appearance conversion that baked a layer must say which
        // ones, and a keep-parameters one says nothing new.
        // The core's refusal is a SENTENCE (a nested or clipped adjustment,
        // one over transparent pixels, a retained source these settings
        // cannot convert). Swallowing it would leave the document unchanged
        // with nothing but a beep, so it is carried out of the non-throwing
        // transform and presented.
        var landed = ConversionReport()
        var refusal: Error?
        document.applyEdit("Convert to Profile", record: record) { source in
            do {
                guard let result = try ProfileConversionWork.run(
                    source, profile: bytes, options: options, title: "Converting Image Profile")
                else { return nil }
                landed = result.report
                return result.document
            } catch {
                refusal = error
                return nil
            }
        }
        if let refusal {
            document.presentError(refusal)
            return
        }
        if landed.bakes {
            presentConversionNotice(landed, document: document)
        }
    }

    /// The one notice a conversion shows: which adjustment layers were baked
    /// and why, in the CORE's own words.
    func presentConversionNotice(_ report: ConversionReport, document: ImageDocument) {
        let alert = NSAlert()
        alert.messageText = "Adjustment layers were baked"
        alert.informativeText = report.notice + "\n\n"
            + report.adjustments.filter(\.isBaked).map(\.reason).joined(separator: "\n")
        if let window = view.window {
            alert.beginSheetModal(for: window)
        } else {
            alert.runModal()
        }
    }

    /// Enablement for the two items above.
    ///
    /// **Convert to Profile stays ENABLED for a document whose profile
    /// cannot be converted from.** A LUT-based (A2B/B2A) profile is a real
    /// profile Doctored displays, embeds and can relabel — only the
    /// transform is unavailable — and a greyed-out menu item is the one form
    /// of refusal that cannot say so or point at Assign. The sheet opens
    /// with Apply disabled and the reason spelled out instead, which is the
    /// four-outcome policy's own answer for this document.
    func validateColorItem(_: NSValidatedUserInterfaceItem) -> Bool { true }
}
