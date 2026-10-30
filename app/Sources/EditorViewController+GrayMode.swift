import AppKit

// Image ▸ Mode ▸ Grayscale (#547 phase 1a): "Discard color information?" (Discard /
// Cancel), then the conversion as ONE undo step (`CmykModeConversion`'s gray arms, recorded as
// `convert_mode` with `mode: "gray"`). A record a Grayscale document cannot hold — the core
// plan's `flatten_required` refusal, or `ModeFlattenRequired` from inside the job — asks a second
// alert naming it (Flatten / Cancel), and Flatten converts again with `flatten: true`. The space
// is Color Settings' working Gray; a Duotone document keeps its own Gray profile and asks no
// Discard alert (#547's Duotone close). Phase 5a adds Edit ▸ Assign Profile… / Convert to
// Profile… on a Grayscale document (`GrayProfileSheetController`) and the commit every Gray
// profile edit shares, Image ▸ Mode ▸ 8/16/32 Bits/Channel's included (`GrayProfileEdit`, one
// undo step each).

extension EditorViewController {
    @objc func convertToGrayscaleMode(_ sender: Any?) {
        guard let document, document.doc != nil else { NSSound.beep(); return }
        // #547 Bitmap phase 1a: a Bitmap document asks its Size Ratio instead (no colour to
        // discard).
        if document.doc?.isBitmap == true { presentBitmapToGray(); return }
        // #547 Duotone: Duotone → Grayscale renders the inks into the gray at once (no colour to
        // discard: `CmykModeConversion.duotoneToGray`), with the Flatten alert when it needs one.
        if document.doc?.isDuotone == true {
            commitGrayscale(document: document, flatten: false)
            return
        }
        let discard = NSAlert()
        discard.alertStyle = .warning
        discard.messageText = "Discard color information?"
        discard.informativeText = "Converting to Grayscale keeps each pixel's luminance in the working "
            + "Gray (\(ColorSettings.workingGray.displayName)) and discards its colour."
        discard.addButton(withTitle: "Discard")
        discard.addButton(withTitle: "Cancel")
        guard discard.runModal() == .alertFirstButtonReturn else { return }
        commitGrayscale(document: document, flatten: false)
    }

    /// One Grayscale conversion; a flatten-required refusal asks the Flatten alert and, on
    /// Flatten, converts again with Flatten Image first.
    private func commitGrayscale(document: ImageDocument, flatten: Bool) {
        var options = CmykConversionOptions()
        options.flatten = flatten
        do {
            let space = ColorSettings.workingGray.rawValue
            let conversion = try CmykModeConversion(document, target: .gray, profile: nil,
                                                    options: options, graySpace: space)
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
            commitGrayscale(document: document, flatten: true)
        } catch {
            document.presentError(error)
        }
    }

    /// Assign Profile… / Convert to Profile… on a Grayscale document: its own sheet. False for
    /// every other document.
    func presentGrayProfileSheet(_ kind: GrayProfileEdit.Kind) -> Bool {
        guard let document, document.doc?.isGray == true else { return false }
        presentAsSheet(GrayProfileSheetController(editor: self, document: document, kind: kind))
        return true
    }

    /// The Gray profile edit as one undo step, recorded as its MCP twin.
    func commitGrayProfileEdit(_ kind: GrayProfileEdit.Kind, profile: Data?, space: String?,
                               destination: [String: Any], options: CmykConversionOptions,
                               document: ImageDocument) {
        do {
            let edit = try GrayProfileEdit(document, kind: kind, profile: profile, space: space,
                                           destination: destination, options: options)
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
