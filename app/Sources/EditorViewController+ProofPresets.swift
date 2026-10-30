import AppKit

/// Saved proof presets on the editor — the default a window with
/// no setup proofs through, and View ▸ Proof Setup ▸ <preset> / Delete Broken
/// File ▸ <file>.
///
/// Seeding from the default is LAZY: it runs only when the user (⌘Y, ⇧⌘Y,
/// opening Proof Setup) or an MCP call asks for a proof, never in `loadView`
/// or when a window opens. Applying a preset writes VIEW state through
/// `applyProofSetup`, the one setup writer: no undo step, no dirty flag and
/// no Action step.
extension EditorViewController {
    /// The default proof preset resolved into a setup. nil when no default
    /// is set; throws the preset's own sentence when it cannot be resolved.
    func defaultProofSetup() throws -> ProofSetup? {
        guard let preset = try ProofPresetLibrary.defaultPreset() else { return nil }
        return try preset.resolvedSetup()
    }

    /// ⌘Y / ⇧⌘Y with no setup: true when the default preset seeded one and
    /// the toggle continues. With no default the Working CMYK proof seeds it
    /// (#547 phase 5, the default; numbers preserved on a CMYK
    /// document), and only with no working CMYK either does Proof Setup open.
    /// A default that fails opens Proof Setup with the failure as its notice.
    func seedProofSetupFromDefault(_ sender: Any?) -> Bool {
        do {
            guard let setup = try defaultProofSetup() else {
                if let working = ProofSetup.workingCMYK(plate: nil,
                                                        preserveNumbers: document?.doc?.isCmyk == true) {
                    proofDisplay.setup = working
                    return true
                }
                showProofSetup(sender)
                return false
            }
            proofDisplay.setup = setup
            return true
        } catch {
            showProofSetup(sender, notice: error.localizedDescription)
            return false
        }
    }

    /// Resolves `preset` and proofs through it, Proof Colors on and Gamut
    /// Warning as it is. A preset whose profile moved says so in an alert.
    func applyProofPreset(_ preset: ProofPreset) {
        do {
            let setup = try preset.resolvedSetup()
            applyProofSetup(setup, proofColors: true, gamutWarning: proofDisplay.gamutWarning)
        } catch {
            NSSound.beep()
            let alert = NSAlert()
            alert.messageText = "Could Not Apply the Proof Preset"
            alert.informativeText = error.localizedDescription
            alert.runModal()
        }
    }

    /// View ▸ Proof Setup ▸ <preset>: its `representedObject` is the preset.
    @objc func applyProofPresetFromMenu(_ sender: Any?) {
        guard let preset = (sender as? NSMenuItem)?.representedObject as? ProofPreset else {
            NSSound.beep()
            return
        }
        applyProofPreset(preset)
    }

    /// View ▸ Proof Setup ▸ Delete Broken File ▸ <file>: its
    /// `representedObject` is the file's basename. The UI twin of
    /// `delete_proof_preset {file}`.
    @objc func deleteProofPresetFile(_ sender: Any?) {
        guard let file = (sender as? NSMenuItem)?.representedObject as? String else {
            NSSound.beep()
            return
        }
        do {
            try ProofPresetLibrary.delete(file: file)
        } catch {
            NSSound.beep()
            let alert = NSAlert()
            alert.messageText = "Could Not Delete the File"
            alert.informativeText = error.localizedDescription
            alert.runModal()
        }
    }
}
