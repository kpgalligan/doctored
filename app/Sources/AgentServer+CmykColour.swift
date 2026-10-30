import AppKit

// The CMYK colour workflow over MCP (#547 phase 5): `separate_color` — the Info panel's CMYK row
// and the Color Picker's gamut alarm — and the CMYK routes `dispatchCmyk` forwards here:
// `assign_profile` / `convert_profile` (Edit ▸ Assign Profile… / Convert to Profile… on a CMYK
// document, `CmykProfileSheetController`; Convert to an RGB profile is `convert_mode`'s path),
// `convert_precision` (Image ▸ Mode ▸ 8/16 Bits/Channel), `histogram` (Levels' and the Info
// panel's histogram per ink) and `render_proof` (what the canvas draws with Proof Colors on).

extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let cmykColourHandlers: [String: ToolHandler] = [
        "separate_color": { $0.separateColor },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let cmykColourToolFacts: [String: ToolFacts] = [
        "separate_color": [.nonRecordable, .commonTool, .cmykTool],
    ]

    /// Mirrors the Info panel's CMYK row ("!" when out of gamut) and the Color Picker's gamut
    /// alarm and in-gamut swatch: one colour through the document's CMYK profile, or the working
    /// CMYK on an RGB document, at Color Settings' intent and BPC.
    func separateColor(_ a: [String: Any]) throws -> String {
        // With no document open the colour separates through the working CMYK; a named
        // `document_id` that does not resolve refuses rather than falling back silently.
        let document = a["document_id"] == nil ? try? target(a) : try target(a)
        let doc = document?.doc
        let cmyk = doc?.isCmyk == true
        var values: [Double]
        var rgbProfile: Data?
        var space = "sRGB"
        if let raw = a["rgb"] as? [Any] {
            values = raw.compactMap { ($0 as? NSNumber)?.doubleValue }
            guard values.count == 3, values.allSatisfy({ (0...255).contains($0) }) else {
                throw ToolError(message: "rgb must be three numbers, each 0–255.")
            }
            values = values.map { $0 / 255 }
            // An RGB document's own numbers are in its profile; a CMYK or Lab Color document's
            // colour is sRGB (a Lab document's profile is its twin, #547 Lab close).
            if !cmyk, doc?.isLab != true, let profile = doc?.iccProfile {
                rgbProfile = profile
                space = doc?.profileName ?? space
            }
        } else if let hex = stringArg(a, "color") {
            guard let color = ProofSetup.color(hex: hex) else {
                throw ToolError(message: "color must be a \"#rrggbb\" string.")
            }
            values = [color.redComponent, color.greenComponent, color.blueComponent].map { Double($0) }
        } else {
            throw ToolError(message: "separate_color needs rgb [r, g, b] (0–255) or color \"#rrggbb\".")
        }
        let separation: CmykSeparation
        do {
            separation = try RasterPrecisionDocument.separate(
                rgb: values, rgbProfile: rgbProfile, document: cmyk ? doc?.precision : nil)
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        let byte = { (v: Double) in Int((min(max(v, 0), 1) * 255).rounded()) }
        let inGamut = separation.inGamut.map(byte)
        return try jsonResult([
            "ink_percent": [separation.ink.c, separation.ink.m, separation.ink.y, separation.ink.k]
                .map { ($0 * 10).rounded() / 10 },
            "out_of_gamut": separation.outOfGamut,
            "delta_e": (separation.deltaE * 100).rounded() / 100,
            "in_gamut": inGamut,
            "in_gamut_hex": String(format: "#%02x%02x%02x", inGamut[0], inGamut[1], inGamut[2]),
            "rgb_space": space,
            "cmyk_profile": separation.profile,
        ])
    }

    /// The CMYK routes of the shared tools (file comment); nil hands the tool on.
    func dispatchCmykColour(_ tool: String, _ a: [String: Any], document: ImageDocument,
                            precision: RasterPrecisionDocument) throws -> String? {
        switch tool {
        case "assign_profile", "convert_profile":
            return try cmykProfileTool(tool, a, document: document, precision: precision)
        case "convert_precision":
            return try cmykConvertPrecision(a, document: document, precision: precision)
        case "histogram":
            return try cmykHistogram(a, document: document, precision: precision)
        case "render_proof":
            return try cmykRenderProof(a, document: document)
        case "render" where a["clipping"] != nil:
            return try cmykClippingRender(a, precision: precision)
        default:
            return nil
        }
    }

    /// `render` with `clipping` on a CMYK document: Levels' and Curves' ⌥ display of the
    /// composite — white where every ink is at paper (highlights), black where any ink is at
    /// 100 % (shadows) — as a PNG, with its plan.
    private func cmykClippingRender(_ a: [String: Any], precision: RasterPrecisionDocument) throws -> String {
        guard let mode = stringArg(a, "clipping"), ["shadows", "highlights"].contains(mode) else {
            throw ToolError(message: "clipping is shadows or highlights.")
        }
        let image: RasterImage
        do {
            let context = try PrecisionWorkContext(source: precision)
            image = try precision.cmykClipDisplay(mode: mode, plates: CmykToneTools.inks,
                                                  workingLimit: context.limit,
                                                  otherRetained: context.otherRetained)
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        let maxSide = min(max(intArg(a, "max_side") ?? 1024, 16), 4096)
        var shown = image
        if max(image.width, image.height) > maxSide {
            let scale = Double(maxSide) / Double(max(image.width, image.height))
            guard let scaled = image.resized(w: max(Int((Double(image.width) * scale).rounded()), 1),
                                             h: max(Int((Double(image.height) * scale).rounded()), 1),
                                             filter: DR_FILTER_NEAREST) else {
                throw ToolError(message: "Could not scale the clipping display")
            }
            shown = scaled
        }
        guard let cgImage = shown.makeCGImage(in: ColorProfile.sRGB),
              let png = NSBitmapImageRep(cgImage: cgImage).representation(using: .png, properties: [:])
        else { throw ToolError(message: "PNG encoding failed") }
        let plan = ((try? precision.descriptor()) ?? [:])["cmyk_clip_display_scratch_bytes"] ?? NSNull()
        let data = try JSONSerialization.data(withJSONObject: [
            "clipping": mode, "width": image.width, "height": image.height,
            "working_memory_plan_bytes": plan,
        ], options: [.sortedKeys])
        return try callResult(content: [
            ["type": "image", "data": png.base64EncodedString(), "mimeType": "image/png"],
            ["type": "text", "text": String(decoding: data, as: UTF8.self)],
        ], isError: false)
    }

    /// The profile `assign_profile` / `convert_profile` name on a CMYK document: `working_cmyk`
    /// (the default), `installed` with name or `file` with path, or for Convert an RGB one
    /// (`srgb`, `display_p3`, an installed or file RGB profile). `rgb` is true for the latter.
    /// Also a Lab Color document's `convert_profile` destination (#547 Lab phase 5b), which
    /// `admitsGray` (Lab phase 6a-ii): an installed or file Gray profile then answers `gray` true.
    func cmykDestination(_ a: [String: Any], admitsGray: Bool = false) throws
        -> (data: Data?, name: String, rgb: Bool, gray: Bool) {
        let choice = (stringArg(a, "profile") ?? "working_cmyk").lowercased()
        let data: Data
        let name: String
        switch choice {
        case "working_cmyk", "working":
            return (nil, WorkingCMYK.installedName ?? "Working CMYK", false, false)
        case "srgb":
            return (RasterProfile.builtin(.sRGB), "sRGB", true, false)
        case "display_p3", "displayp3", "p3":
            return (RasterProfile.builtin(.displayP3), "Display P3", true, false)
        case "installed":
            let wanted = try requiredString(a, "name")
            guard let profile = InstalledProfiles.named(wanted),
                  let loaded = try? Data(contentsOf: profile.url), !loaded.isEmpty else {
                throw ToolError(message: "No installed profile is named \"\(wanted)\". Call "
                    + "list_installed_profiles to see what this machine has.")
            }
            data = loaded
            name = profile.name
        case "file":
            let path = try requiredString(a, "path")
            let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
            guard let loaded = try? Data(contentsOf: url), !loaded.isEmpty else {
                throw ToolError(message: "Could not read a colour profile at \(url.path)")
            }
            data = loaded
            name = RasterProfile.describe(loaded)?.name ?? url.lastPathComponent
        case let other:
            // #547 Lab 6b-ii: a caller that admits Gray (a Lab document's convert_profile)
            // names the Gray choices too.
            if admitsGray {
                throw ToolError(message: "profile is working_cmyk, working_gray, a Gray working "
                    + "space (" + GrayWorkingSpace.allCases.map(\.rawValue).joined(separator: ", ")
                    + "), installed or file (convert_profile also takes srgb or display_p3) "
                    + "(got \"\(other)\").")
            }
            throw ToolError(message: "profile is working_cmyk, installed or file "
                + "(convert_profile also takes srgb or display_p3) (got \"\(other)\").")
        }
        switch RasterProfile.describe(data)?.space {
        case "cmyk": return (data, name, false, false)
        case "rgb": return (data, name, true, false)
        case "gray" where admitsGray: return (data, name, false, true)
        default:
            if admitsGray {
                throw ToolError(message: "“\(name)” is neither a CMYK, an RGB nor a Gray profile.")
            }
            throw ToolError(message: "“\(name)” is neither a CMYK nor an RGB profile.")
        }
    }

    /// Edit ▸ Assign Profile… / Convert to Profile… on a CMYK document (`CmykProfileEdit`), or
    /// Convert to an RGB profile through Image ▸ Mode ▸ RGB Color's conversion.
    private func cmykProfileTool(_ tool: String, _ a: [String: Any], document: ImageDocument,
                                 precision: RasterPrecisionDocument) throws -> String {
        let destination = try cmykDestination(a)
        var options = CmykConversionOptions()
        if let name = stringArg(a, "intent") {
            guard let intent = RenderingIntent(rawValue: name) else {
                throw ToolError(message: "intent must be perceptual, relative, saturation or absolute.")
            }
            options.intent = intent
        }
        if let flag = boolArg(a, "black_point_compensation") { options.blackPointCompensation = flag }
        options.flatten = boolArg(a, "flatten") ?? false
        let previous = precision.cmykInfo()?["profile"] as? String ?? ""
        if destination.rgb {
            guard tool == "convert_profile" else {
                throw ToolError(message: "A CMYK document is assigned a CMYK profile; convert_profile to an "
                    + "RGB profile (or convert_mode rgb) converts it to RGB.")
            }
            do {
                let conversion = try CmykModeConversion(document, target: .rgb, profile: destination.data,
                                                        options: options)
                let prepared = try conversion.compute()
                try conversion.publish(prepared, to: document, record: .notACommand)
            } catch {
                throw ToolError(message: error.localizedDescription)
            }
            return try jsonResult(["ok": true, "changed": true, "color_mode": "rgb",
                                   "profile": document.doc?.profileName ?? destination.name,
                                   "previous_profile": previous, "document": summary(document)])
        }
        let kind: CmykProfileEdit.Kind = tool == "assign_profile" ? .assign : .convert
        do {
            let edit = try CmykProfileEdit(document, kind: kind, profile: destination.data, destination: [:],
                                           options: options)
            guard edit.changes else {
                return try jsonResult(["ok": true, "changed": false, "profile": previous,
                                       "note": "the document is already in “\(previous)”."])
            }
            let prepared = try edit.compute()
            try edit.publish(prepared, to: document, record: .notACommand)
            let info = document.doc?.precision?.cmykInfo() ?? [:]
            return try jsonResult([
                "ok": true, "changed": true, "color_mode": "cmyk",
                "profile": info["profile"] ?? destination.name, "previous_profile": previous,
                "depth": info["depth"] ?? NSNull(), "working_memory_plan_bytes": edit.scratch,
                "note": kind == .assign
                    ? "Ink numbers were not touched, so the picture now looks different."
                    : "Every layer's ink was re-separated through the destination profile.",
                "document": summary(document),
            ])
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    /// Image ▸ Mode ▸ 8/16 Bits/Channel on a CMYK document: mode `cmyk8` or `cmyk16`.
    private func cmykConvertPrecision(_ a: [String: Any], document: ImageDocument,
                                      precision: RasterPrecisionDocument) throws -> String {
        let mode = try requiredString(a, "mode")
        guard let bits = ["cmyk8": 8, "cmyk16": 16][mode] else {
            throw ToolError(message: "A CMYK document converts between mode cmyk8 and cmyk16 (Image ▸ "
                + "Mode ▸ 8/16 Bits/Channel); convert_mode converts it to RGB.")
        }
        do {
            let edit = try CmykProfileEdit(document, kind: .depth(bits), profile: nil, destination: [:],
                                           options: CmykConversionOptions())
            if boolArg(a, "dry_run") == true {
                return try jsonResult(["ok": true, "dry_run": true, "mode": mode, "changed": edit.changes,
                                       "working_memory_plan_bytes": edit.scratch])
            }
            guard edit.changes else {
                return try jsonResult(["ok": true, "changed": false, "mode": mode,
                                       "note": "the document is already \(bits) bits per channel."])
            }
            let prepared = try edit.compute()
            try edit.publish(prepared, to: document, record: .notACommand)
            return try jsonResult(["ok": true, "changed": true, "mode": mode, "color_mode": "cmyk",
                                   "bits_per_channel": bits, "working_memory_plan_bytes": edit.scratch,
                                   "document": summary(document)])
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    /// The histogram per ink and composite (L = 1 − ink); `source`
    /// `scalar` is a mask's, an alpha channel's or one ink plate's raw scalar histogram, the
    /// shared path the 16-bit document answers (`nativeScalarHistogram`).
    private func cmykHistogram(_ a: [String: Any], document: ImageDocument,
                               precision: RasterPrecisionDocument) throws -> String {
        if stringArg(a, "source") == "scalar" { return try nativeScalarHistogram(a, document: document) }
        var request: [String: Any] = ["source": "composite"]
        if let layer = intArg(a, "layer") {
            request = ["source": "layer", "layer": layer]
        }
        let coverage = boolArg(a, "selection") == true
            ? document.canvasSelection?.maskSamples16() ?? [] : []
        let analysis: [String: Any]
        do {
            let context = try PrecisionWorkContext(source: precision)
            analysis = try precision.cmykToneAnalysis(request: request, coverage: coverage,
                                                      workingLimit: context.limit,
                                                      otherRetained: context.otherRetained, control: nil)
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        var plan: Any = NSNull()
        if let answer = try? precision.cmykToneAnalysisPlan(request: request),
           let bytes = answer["working_memory_plan_bytes"] { plan = bytes }
        return try jsonResult([
            "color_mode": "cmyk", "source": request["source"] ?? "composite",
            "bins": 256, "domain": "L = 1 − ink: bin 0 is 100 % ink, bin 255 bare paper",
            "channels": analysis["histogram"] ?? [:],
            "working_memory_plan_bytes": plan,
        ])
    }

    /// `render_proof` on a CMYK document: the proof through a CMYK profile into Display P3.
    private func cmykRenderProof(_ a: [String: Any], document: ImageDocument) throws -> String {
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let editorVC = editor(document)
        var setup = try baseProofSetup(a, existing: editorVC?.proofDisplay.setup, seedWorkingCMYK: true,
                                       cmykDocument: true)
        try applyProofOverrides(a, to: &setup)
        try requireCmykProof(a, setup: setup)
        setup = setup.reconciled(isCmykDocument: true)
        let rendered: (image: RasterImage, space: String, outOfGamut: Int)
        // View ▸ Gamut Warning against the proof profile (`cmyk_proof`).
        let warning = boolArg(a, "gamut_warning") ?? editorVC?.proofDisplay.gamutWarning ?? false
        let setupJSON = setup.jsonText(gamutWarning: warning)
        var plan: Any = NSNull()
        if let typed = doc.precision, let answer = try? typed.cmykProofPlan(profile: setup.profile, setupJSON: setupJSON),
           let bytes = answer["working_memory_plan_bytes"] { plan = bytes }
        do {
            rendered = try CmykProof.render(doc, proof: setup.profile, setupJSON: setupJSON, control: nil)
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        var image = rendered.image
        let (fullWidth, fullHeight) = (image.width, image.height)
        let maxSide = min(max(intArg(a, "max_side") ?? 1024, 64), 4096)
        if max(fullWidth, fullHeight) > maxSide {
            let scale = Double(maxSide) / Double(max(fullWidth, fullHeight))
            guard let scaled = image.resized(w: max(Int((Double(fullWidth) * scale).rounded()), 1),
                                             h: max(Int((Double(fullHeight) * scale).rounded()), 1),
                                             filter: DR_FILTER_BILINEAR) else {
                throw ToolError(message: "Could not scale the proof")
            }
            image = scaled
        }
        guard let tagged = image.makeCGImage(in: ProofRenderer.space(rendered.space, profile: Data())),
              let cgImage = Bitmap.sRGBCopy(of: tagged),
              let png = NSBitmapImageRep(cgImage: cgImage).representation(using: .png, properties: [:])
        else { throw ToolError(message: "PNG encoding failed") }
        let info: [String: Any] = [
            "space": rendered.space, "out_of_gamut": rendered.outOfGamut,
            "width": fullWidth, "height": fullHeight, "profile_name": setup.profileName,
            "intent": setup.intent.rawValue, "black_point_compensation": setup.blackPointCompensation,
            "preserve_numbers": setup.preserveNumbers,
            "plate": setup.plate.map { $0.key as Any } ?? NSNull(),
            "gamut_warning": warning, "source": "cmyk",
            "working_memory_plan_bytes": plan,
        ]
        let data = try JSONSerialization.data(withJSONObject: info, options: [.sortedKeys])
        return try callResult(
            content: [
                ["type": "image", "data": png.base64EncodedString(), "mimeType": "image/png"],
                ["type": "text", "text": String(decoding: data, as: UTF8.self)],
            ], isError: false)
    }

    /// The Info panel's CMYK row for an RGB document's readout (`sample_pixel`): the working
    /// CMYK's ink for `rgb` (0–255 in `document`'s profile) and its "!" verdict; nil when no
    /// working CMYK is set.
    func cmykReadout(rgb: [Int], document: RasterDocument) -> [String: Any]? {
        guard let separation = try? RasterPrecisionDocument.separate(
            rgb: rgb.map { Double($0) / 255 }, rgbProfile: document.iccProfile, document: nil) else { return nil }
        return ["ink_percent": [separation.ink.c, separation.ink.m, separation.ink.y, separation.ink.k]
                    .map { $0.rounded() },
                "out_of_gamut": separation.outOfGamut, "profile": separation.profile]
    }
}
