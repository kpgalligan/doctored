import AppKit

// View ▸ Proof Setup on a CMYK document and the Working CMYK rows (#547 phase 5): the core's CMYK
// proof request (`ProofSource.cmyk` → `dr_precision_doc_cmyk_proof`, tagged Display P3), the
// Working CMYK and Working Cyan/Magenta/Yellow/Black Plate setups, and `reconciled`, which keeps a
// window's proof state valid across Image ▸ Mode: on a CMYK document a proof profile that is not
// CMYK becomes Working CMYK (keeping the plate), on an RGB one Preserve CMYK Numbers is dropped —
// so a mode change never reaches `proofDidFail` for state the user cannot change. Explicit MCP
// requests are not reconciled: they still refuse in-band.

extension ProofSetup {
    /// True when the proof profile's ICC header names the CMYK colour space (bytes 16–19).
    var isCmykProfile: Bool {
        profile.count >= 20 && profile.subdata(in: 16..<20) == Data("CMYK".utf8)
    }

    /// View ▸ Proof Setup ▸ Working CMYK (`plate` nil) or Working <Plate> Plate, through the
    /// working CMYK profile; nil when none is installed. On a CMYK document Working CMYK
    /// preserves the numbers (`preserveNumbers`).
    static func workingCMYK(plate: CmykPlane?, preserveNumbers: Bool = false) -> ProofSetup? {
        guard let data = WorkingCMYK.installedData else { return nil }
        var setup = ProofSetup(profile: data, profileName: WorkingCMYK.installedName ?? "Working CMYK")
        setup.intent = ColorSettings.intent
        setup.blackPointCompensation = ColorSettings.blackPointCompensation
        setup.plate = plate
        setup.preserveNumbers = preserveNumbers
        return setup
    }

    /// This setup made valid for the window's document (file comment).
    func reconciled(isCmykDocument: Bool) -> ProofSetup {
        if isCmykDocument {
            guard !isCmykProfile else { return self }
            return ProofSetup.workingCMYK(plate: plate, preserveNumbers: true) ?? self
        }
        guard preserveNumbers else { return self }
        var setup = self
        setup.preserveNumbers = false
        return setup
    }

    /// True when this is the Working CMYK setup showing `plate` (the menu rows' checkmarks).
    func isWorking(plate: CmykPlane?) -> Bool {
        guard let data = WorkingCMYK.installedData else { return false }
        return profile == data && self.plate == plate
    }
}

enum CmykProof {
    /// The Proof Setup submenu's Working rows, tag order: Working CMYK, then the four plates.
    static let rows: [(title: String, plate: CmykPlane?)] = [("Working CMYK", nil)]
        + CmykPlane.allCases.map { ("Working \($0.name) Plate", $0) }

    /// The plate a row's tag names (0 = the full proof).
    static func plate(tag: Int) -> CmykPlane? {
        rows.indices.contains(tag) ? rows[tag].plate : nil
    }

    /// The core's CMYK proof of `document`'s composite: RGBA8 in Display P3, `space`
    /// "display-p3". Value-only: it runs on the proof renderer's queue.
    static func render(_ document: RasterDocument, proof: Data, setupJSON: String,
                       control: RasterJobControl?) throws -> (image: RasterImage, space: String, outOfGamut: Int) {
        guard let typed = document.precision, typed.isCmyk else {
            throw DoctoredCoreError(message: "The CMYK proof needs a CMYK document.")
        }
        let context = try PrecisionWorkContext(source: typed)
        let rendered = try typed.cmykProof(profile: proof, setupJSON: setupJSON, workingLimit: context.limit,
                                           otherRetained: context.otherRetained, control: control)
        return (rendered.image, rendered.info["space"] as? String ?? "display-p3",
                (rendered.info["out_of_gamut"] as? NSNumber)?.intValue ?? 0)
    }
}

extension EditorViewController {
    /// View ▸ Proof Setup ▸ Working CMYK / Working <Plate> Plate (tag 0–4): proofs through the
    /// working CMYK, Proof Colors on, as choosing a preset does. View state only.
    @objc func applyWorkingProof(_ sender: Any?) {
        let plate = CmykProof.plate(tag: (sender as? NSMenuItem)?.tag ?? 0)
        let cmyk = document?.doc?.isCmyk == true
        guard let setup = ProofSetup.workingCMYK(plate: plate, preserveNumbers: cmyk) else {
            NSSound.beep()
            return
        }
        applyProofSetup(setup, proofColors: true, gamutWarning: proofDisplay.gamutWarning)
    }
}
