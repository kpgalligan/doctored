import AppKit

/// The Phase 11C output-colour tools:
///
/// | Handler | UI path it mirrors |
/// |---|---|
/// | `getProofSetup` | View ▸ Proof Setup ▸ Custom… (reading it) |
/// | `setProofSetup` | View ▸ Proof Setup ▸ Custom… / <preset>, View ▸ Proof Colors, View ▸ Gamut Warning |
/// | `renderProof` | what the canvas draws with Proof Colors on (`exact` at 16 bits: the full-depth composite) |
/// | `describeProfile` | the Proof Setup and Convert sheets' profile inspection |
/// | `listInstalledProfiles` | the profile popups' Installed section |
///
/// **`set_proof_setup` writes no document.** A proof setup is per-window VIEW
/// state: no undo step, no dirty flag, no history entry and no Action step —
/// the same exemption the rulers, the grid and the snap toggles take. It
/// therefore does NOT go through `performGroupedEdit`, and the drive checks
/// that `get_document`, `get_color_profile`, the dirty flag, an export's
/// bytes and the current selection all survive it untouched.
///
/// A Lab Color document (#547 Lab phase 5b) proofs its Display P3 view: the
/// display codes in the view's profile (`proofSourceProfile`), and with
/// `exact` the u16 view (`LabDocument.exactProofImage`); `space` answers
/// display-p3 (`LabDocument.reportedProofSpace`). A Duotone document refuses
/// the proof (`DuotoneDocument.disabledTools`) but for a `set_proof_setup`
/// that only turns proofing off (`DuotoneDocument.turnsProofingOff`).
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let softProofHandlers: [String: ToolHandler] = [
        "get_proof_setup": { $0.getProofSetup },
        "set_proof_setup": { $0.setProofSetup },
        "render_proof": { $0.renderProof },
        "describe_profile": { $0.describeProfile },
        "list_installed_profiles": { $0.listInstalledProfiles },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let softProofToolFacts: [String: ToolFacts] = [
        // Phase 11C: the proof setup is per-window VIEW state — no undo
        // step, no dirty flag, nothing a `save_copy` would carry — so it
        // takes the same exemption the ruler and grid preferences do, and
        // the other three only read.
        // Soft proofing — View ▸ Proof Setup…, Proof Colors and Gamut
        // Warning. 16 bits proof the projection's display codes (or the full
        // composite with `exact`); float stays refused through
        // `FloatDocument.tools`. `describe_profile` and
        // `list_installed_profiles` read no document (`documentFreeTools`).
        // #547 phase 5: a CMYK document proofs through a CMYK profile, with
        // Preserve CMYK Numbers and the plates (`render_proof`'s CMYK route
        // is `AgentServer+CmykColour.swift`).
        "get_proof_setup": [.nonRecordable, .commonTool, .cmykTool],
        "set_proof_setup": [.nonRecordable, .commonTool, .cmykTool],
        "render_proof": [.nonRecordable, .commonTool, .cmykTool],
        "describe_profile": [.nonRecordable, .documentFree],
        "list_installed_profiles": [.nonRecordable, .documentFree],
    ]

    /// get_proof_setup: what this window is proofing through, if anything.
    func getProofSetup(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let editorVC = editor(document) else {
            throw ToolError(message: "That document has no editor window.")
        }
        var result = editorVC.proofDisplay.json
        // The preset a window with no setup proofs through.
        let defaultName = (try? ProofPresetLibrary.defaultPresetName()).flatMap { $0 }
        result["default_preset"] = defaultName as Any? ?? NSNull()
        return try jsonResult(result)
    }

    /// set_proof_setup: choose the medium and the two toggles. Every key is
    /// optional; the ones that are absent keep their current values. A
    /// `preset` turns Proof Colors on unless `proof_colors` says
    /// otherwise, as choosing one in View ▸ Proof Setup does.
    func setProofSetup(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let editorVC = editor(document) else {
            throw ToolError(message: "That document has no editor window.")
        }
        var display = editorVC.proofDisplay
        let cmyk = document.doc?.isCmyk == true
        // #547 phase 5: with no setup and no default preset, turning the
        // warning or the proof on seeds Working CMYK, as ⌘Y and ⇧⌘Y do.
        let seeds = boolArg(a, "gamut_warning") == true || boolArg(a, "proof_colors") == true
        var setup = try baseProofSetup(a, existing: display.setup,
                                       seedWorkingCMYK: seeds, cmykDocument: cmyk)
        try applyProofOverrides(a, to: &setup)
        if cmyk {
            try requireCmykProof(a, setup: setup)
            setup = setup.reconciled(isCmykDocument: true)
        } else if setup.preserveNumbers {
            throw ToolError(message: "Preserve Numbers needs a CMYK document.")
        }
        display.setup = setup
        if let flag = boolArg(a, "proof_colors") {
            display.proofColors = flag
        } else if stringArg(a, "preset") != nil {
            display.proofColors = true
        }
        if let flag = boolArg(a, "gamut_warning") {
            display.gamutWarning = flag
            if flag { display.proofColors = true }
        }
        editorVC.applyProofSetup(
            setup, proofColors: display.proofColors, gamutWarning: display.gamutWarning)
        var result = editorVC.proofDisplay.json
        result["ok"] = true
        result["note"] = "View state only — the master, the renders and every export are "
            + "untouched, and there is no undo step."
        return try jsonResult(result)
    }

    /// render_proof: the proofed picture, as a PNG, plus how many pixels the
    /// gamut rule flagged. Reads the current setup and lets the call override
    /// any part of it for this render only.
    func renderProof(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let editorVC = editor(document)
        // The setup: the window's (or a preset, or the default preset), with
        // this call's overrides on top.
        var setup = try baseProofSetup(a, existing: editorVC?.proofDisplay.setup)
        try applyProofOverrides(a, to: &setup)
        let warning = boolArg(a, "gamut_warning") ?? editorVC?.proofDisplay.gamutWarning ?? false
        let wantMask = boolArg(a, "mask") ?? false

        let profile = doc.proofSourceProfile
        let setupJSON = setup.jsonText(gamutWarning: warning)
        if setup.preserveNumbers {
            throw ToolError(message: "Preserve Numbers needs a CMYK document.")
        }
        var rendered: (image: RasterImage, space: String, outOfGamut: Int)
        var maskCount = 0
        // What was proofed: the composite at 8 bits (the retained
        // projection when there is one; a render otherwise) and with
        // `exact`, the 16-bit projection's 8-bit display codes — what the
        // canvas shows — otherwise. A float document never reaches here.
        var source = "composite"
        if doc.isHighDepth {
            let projection = try document.currentPrecisionProjection()
            if boolArg(a, "exact") == true {
                let exact = try exactProof(projection, lab: doc.isLab,
                                           proof: setup.profile, setupJSON: setupJSON,
                                           wantMask: wantMask)
                rendered = exact.rendered
                maskCount = exact.maskCount
            } else {
                source = "display"
                let display = try ProofRenderer.displayCodes(of: projection, in: doc)
                let context = try PrecisionWorkContext(byteImage: display)
                rendered = try display.proofRender(
                    documentProfile: profile, proof: setup.profile, setup: setupJSON,
                    workingLimit: context.limit, otherRetained: context.otherRetained)
                if wantMask {
                    maskCount = try display.proofGamutMask(
                        documentProfile: profile, proof: setup.profile, setup: setupJSON,
                        workingLimit: context.limit, otherRetained: context.otherRetained).count
                }
            }
        } else {
            let flat: RasterImage
            do { flat = try document.retainedByteProjection ?? doc.render() } catch {
                throw ToolError(message: error.localizedDescription)
            }
            let context = try PrecisionWorkContext(byteImage: flat)
            rendered = try flat.proofRender(
                documentProfile: profile, proof: setup.profile, setup: setupJSON,
                workingLimit: context.limit, otherRetained: context.otherRetained)
            if wantMask {
                maskCount = try flat.proofGamutMask(
                    documentProfile: profile, proof: setup.profile, setup: setupJSON,
                    workingLimit: context.limit, otherRetained: context.otherRetained).count
            }
        }

        rendered.space = LabDocument.reportedProofSpace(rendered.space, isLab: doc.isLab)
        var image = rendered.image
        let fullWidth = image.width
        let fullHeight = image.height
        let maxSide = min(max(intArg(a, "max_side") ?? 1024, 64), 4096)
        if max(fullWidth, fullHeight) > maxSide {
            let scale = Double(maxSide) / Double(max(fullWidth, fullHeight))
            let w = max(Int((Double(fullWidth) * scale).rounded()), 1)
            let h = max(Int((Double(fullHeight) * scale).rounded()), 1)
            guard let scaled = image.resized(w: w, h: h, filter: DR_FILTER_BILINEAR) else {
                throw ToolError(message: "Could not scale the proof")
            }
            image = scaled
        }
        // Tagged with the space the CORE reported, then converted to sRGB for
        // the reader, exactly as `render` does.
        let space = ProofRenderer.space(rendered.space, profile: profile)
        guard let tagged = image.makeCGImage(in: space),
              let cgImage = Bitmap.sRGBCopy(of: tagged),
              let png = NSBitmapImageRep(cgImage: cgImage)
                .representation(using: .png, properties: [:])
        else { throw ToolError(message: "PNG encoding failed") }
        var info: [String: Any] = [
            "space": rendered.space,
            "out_of_gamut": rendered.outOfGamut,
            "width": fullWidth,
            "height": fullHeight,
            "profile_name": setup.profileName,
            "intent": setup.intent.rawValue,
            "black_point_compensation": setup.blackPointCompensation,
            "gamut_warning": warning,
            "source": source,
        ]
        if wantMask { info["mask_out_of_gamut"] = maskCount }
        let data = try JSONSerialization.data(withJSONObject: info, options: [.sortedKeys])
        return try callResult(
            content: [
                ["type": "image", "data": png.base64EncodedString(), "mimeType": "image/png"],
                ["type": "text", "text": String(decoding: data, as: UTF8.self)],
            ], isError: false)
    }

    /// `render_proof exact:true` at 16 bits: the FULL 16-bit composite
    /// through the typed proof, on the background worker with progress and
    /// Cancel — about 17 s at 24 MP, and `mask` adds a second pass. A Lab
    /// projection (`lab`) proofs its u16 Display P3 view, made first.
    private func exactProof(_ projection: RasterPrecisionImage, lab: Bool, proof: Data,
                            setupJSON: String, wantMask: Bool)
        throws -> (rendered: (image: RasterImage, space: String, outOfGamut: Int), maskCount: Int)
    {
        let control = try RasterJobControl()
        try control.beginCompoundOperation()
        let context = try PrecisionWorkContext(image: projection)
        let viewed = lab
        let task = try WorkerTask {
            () throws -> (rendered: (image: RasterImage, space: String, outOfGamut: Int),
                          maskCount: Int) in
            // #547 Lab phase 5b: the view, admitted with the projection's own context.
            let source: RasterPrecisionImage
            if lab {
                source = try control.withChildOperation(from: 0, to: 0.2) {
                    try LabDocument.exactProofImage(of: projection, context: context, control: $0)
                }
            } else {
                source = projection
            }
            let viewContext = viewed ? try PrecisionWorkContext(image: source) : context
            let limit = viewContext.limit, otherRetained = viewContext.otherRetained
            let start = viewed ? 0.2 : 0, split = viewed ? 0.6 : 0.5
            let rendered = try control.withChildOperation(from: start, to: wantMask ? split : 1) {
                try source.proofRender(
                    proof: proof, setup: setupJSON,
                    workingLimit: limit, otherRetained: otherRetained, control: $0)
            }
            var maskCount = 0
            if wantMask {
                maskCount = try control.withChildOperation(from: split, to: 1) {
                    try source.proofGamutMask(
                        proof: proof, setup: setupJSON,
                        workingLimit: limit, otherRetained: otherRetained, control: $0).count
                }
            }
            return (rendered, maskCount)
        }
        do {
            return try BackgroundWork.wait(task, title: "Rendering Exact Proof", control: control)
        } catch {
            if control.isCancelled { throw JobCancelled() }
            throw error
        }
    }

    /// describe_profile: everything the core can say about a profile file.
    func describeProfile(_ a: [String: Any]) throws -> String {
        let path = try requiredString(a, "path")
        let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
        guard let bytes = try? Data(contentsOf: url), !bytes.isEmpty else {
            throw ToolError(message: "Could not read a colour profile at \(url.path)")
        }
        var result = try RasterProfile.describeJSON(bytes)
        result["path"] = url.path
        result["bytes"] = bytes.count
        return try jsonResult(result)
    }

    /// list_installed_profiles: the machine's own ColorSync, Adobe and
    /// printer-driver (`/Library/Printers`) profiles, filtered by device space.
    func listInstalledProfiles(_ a: [String: Any]) throws -> String {
        var kinds: [String] = []
        if let list = a["kinds"] as? [String] { kinds = list }
        else if let single = stringArg(a, "kinds") { kinds = [single] }
        let profiles = InstalledProfiles.matching(kinds: kinds)
        return try jsonResult([
            "count": profiles.count,
            "profiles": profiles.map { profile in
                [
                    "name": profile.name,
                    "path": profile.path,
                    "space": profile.description.space,
                    "class": profile.description.profileClass,
                    "model": profile.description.model,
                    "can_decode": profile.description.canDecode,
                    "can_encode": profile.description.canEncode,
                    "proofable": profile.description.proofable,
                    "high_depth_destination": profile.description.highDepthDestination,
                ]
            },
        ])
    }

    /// The setup a call starts from, in order: a saved `preset` (refused
    /// beside `profile`), `profile` applied onto the existing setup, the
    /// existing setup, then the default proof preset.
    func baseProofSetup(_ a: [String: Any], existing: ProofSetup?, seedWorkingCMYK: Bool = false,
                        cmykDocument: Bool = false) throws -> ProofSetup {
        if let preset = try proofPresetSetup(a) { return preset }
        if let resolved = try resolveProofProfile(a) {
            var setup = existing
                ?? ProofSetup(profile: resolved.data, profileName: resolved.name,
                              path: resolved.path)
            setup.setProfile(resolved.data, name: resolved.name, path: resolved.path)
            return setup
        }
        if let existing { return existing }
        let fallback: ProofSetup?
        do {
            fallback = try ProofPresetLibrary.defaultPreset()?.resolvedSetup()
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        if fallback == nil, seedWorkingCMYK,
           let working = ProofSetup.workingCMYK(plate: nil, preserveNumbers: cmykDocument) {
            return working
        }
        guard let fallback else {
            throw ToolError(
                message: "No proof profile is set. Pass preset with a saved proof preset's "
                    + "name, profile:\"working_cmyk\", profile:\"installed\" with a name, or "
                    + "profile:\"file\" with a path.")
        }
        return fallback
    }

    /// The explicit simulation fields — intent, black point compensation,
    /// the two simulate flags and the warning colour — on top of `setup`.
    func applyProofOverrides(_ a: [String: Any], to setup: inout ProofSetup) throws {
        if let raw = stringArg(a, "intent") {
            guard let intent = RenderingIntent(rawValue: raw) else {
                throw ToolError(
                    message: "intent must be perceptual, relative, saturation or absolute "
                        + "(got \"\(raw)\")")
            }
            setup.intent = intent
        }
        if let flag = boolArg(a, "black_point_compensation") {
            setup.blackPointCompensation = flag
        }
        if let flag = boolArg(a, "simulate_paper_white") { setup.simulatePaperWhite = flag }
        if let flag = boolArg(a, "simulate_ink_black") { setup.simulateInkBlack = flag }
        if let hex = stringArg(a, "gamut_color") {
            guard let color = ProofSetup.color(hex: hex) else {
                throw ToolError(message: "gamut_color must be a \"#RRGGBB\" string.")
            }
            setup.gamutColor = color
        }
        // #547 phase 5: Preserve CMYK Numbers and one plate of the proof device.
        if let flag = boolArg(a, "preserve_numbers") { setup.preserveNumbers = flag }
        if a["plate"] is NSNull || stringArg(a, "plate") == "none" {
            setup.plate = nil
        } else if let name = stringArg(a, "plate") {
            guard let plate = CmykPlane(key: name) else {
                throw ToolError(message: "plate must be cyan, magenta, yellow, black or none.")
            }
            setup.plate = plate
        }
    }

    /// A CMYK document's EXPLICIT proof request that still refuses in-band (#547 phase 5): a
    /// named proof profile or preset that is not CMYK. A window's carried state is reconciled
    /// instead (`ProofSetup.reconciled`). Gamut Warning is admitted since the remaining-gaps
    /// batch (`cmyk_proof`).
    func requireCmykProof(_ a: [String: Any], setup: ProofSetup) throws {
        let named = stringArg(a, "profile") != nil || stringArg(a, "preset") != nil
        if named && !setup.isCmykProfile {
            throw ToolError(message: "the proof profile \"\(setup.profileName)\" is not CMYK; a CMYK "
                + "document proofs through a CMYK profile")
        }
    }

    /// The `profile`/`path`/`name` triple every tool above accepts, resolved
    /// to BYTES. nil when the call named no profile at all.
    private func resolveProofProfile(_ a: [String: Any])
        throws -> (data: Data, name: String, path: String?)?
    {
        guard let choice = stringArg(a, "profile")?.lowercased() else { return nil }
        switch choice {
        case "working_cmyk":
            // #547 phase 5: Edit ▸ Color Settings' working CMYK.
            guard let data = WorkingCMYK.installedData else {
                throw ToolError(message: "No working CMYK profile is installed.")
            }
            return (data, WorkingCMYK.installedName ?? "Working CMYK", nil)
        case "file":
            let path = try requiredString(a, "path")
            let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
            guard let bytes = try? Data(contentsOf: url), !bytes.isEmpty else {
                throw ToolError(message: "Could not read a colour profile at \(url.path)")
            }
            let description = try RasterProfile.describeJSON(bytes)
            try requireProofable(description, named: url.lastPathComponent)
            return (bytes, description["name"] as? String ?? url.lastPathComponent, url.path)
        case "installed":
            let wanted = try requiredString(a, "name")
            guard let profile = InstalledProfiles.named(wanted) else {
                throw ToolError(
                    message: "No installed profile is named \"\(wanted)\". Call "
                        + "list_installed_profiles to see what this machine has.")
            }
            guard let bytes = try? Data(contentsOf: profile.url), !bytes.isEmpty else {
                throw ToolError(message: "Could not read \(profile.path)")
            }
            guard profile.description.proofable else {
                throw ToolError(
                    message: "“\(profile.name)” cannot be proofed through — "
                        + (profile.description.proofRefusal
                            ?? "it needs both a device-to-PCS and a PCS-to-device table."))
            }
            return (bytes, profile.name, profile.path)
        case let other:
            throw ToolError(
                message: "profile must be working_cmyk, file or installed (got \"\(other)\")")
        }
    }

    private func requireProofable(_ description: [String: Any], named: String) throws {
        guard description["proofable"] as? Bool == true else {
            let reason = description["proof_refusal"] as? String
                ?? "it needs both a device-to-PCS and a PCS-to-device table."
            throw ToolError(
                message: "“\(description["name"] as? String ?? named)” cannot be proofed "
                    + "through — \(reason)")
        }
    }
}
