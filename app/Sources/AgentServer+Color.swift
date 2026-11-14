import AppKit

/// The colour-management and metadata tools, mirroring Image > Mode, Image
/// Size's Resolution field and the Export panel's two checkboxes:
///
/// | Handler | UI path it mirrors |
/// |---|---|
/// | `getColorProfile` | the status bar's profile and ppi segments, and what
///   the Assign/Convert sheets read |
/// | `assignProfile` | Image > Mode > Assign Profile… |
/// | `convertProfile` | Image > Mode > Convert to Profile… |
/// | `setResolution` | Image > Image Size…'s Resolution field with Resample off |
/// | `getMetadata` | no dialog — the packets the Export panel's Strip metadata checkbox governs |
///
/// The static field builders here are also what `get_document` and
/// `save_copy` report with, so the document summary and these tools can
/// never disagree about a profile name.
///
/// Every editing handler is ONE undo step and answers a core refusal in
/// band, as `changed: false` with a note rather than an error: a nil from
/// assign, convert or set_resolution means the document already looks like
/// that, or that one side of the transform is not a matrix/TRC profile — and
/// in none of those cases is retrying the identical call the right move for
/// a model.
///
/// The core answers all of them with the same undifferentiated nil, so each
/// handler asks the questions it CAN answer first — the bytes through
/// `RasterProfile.inspect`, the document through `profileIsConvertible` —
/// and the note that comes back names the real cause. That is the
/// `requireChannelBudget` pattern, for the same reason.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let colorHandlers: [String: ToolHandler] = [
        "get_color_profile": { $0.getColorProfile },
        "assign_profile": { $0.assignProfile },
        "convert_profile": { $0.convertProfile },
        "set_resolution": { $0.setResolution },
        "get_metadata": { $0.getMetadata },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let colorToolFacts: [String: ToolFacts] = [
        "get_color_profile": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
        // A float document relabels (Assign) or converts in linear
        // light with no clamp (Convert); the core's `float_color` refuses
        // what it cannot do by name.
        // #1 phase 5: on a CMYK document Assign/Convert to Profile take a
        // CMYK profile (`AgentServer+CmykColour.swift`'s route).
        "assign_profile": [.typedTool, .floatTool, .cmykTool],
        "convert_profile": [.typedTool, .floatTool, .cmykTool],
        // Phase 12E-1b: a PPI change moves no pixel at all — the typed
        // `set_resolution` shares every sample buffer — so Image Size's
        // resolution half answers at either depth.
        // Phase 12E-1b: the three MERGES composite through the SAME float
        // renderer a projection uses, so nothing new is evaluated; locks and
        // links are per-record bookkeeping; and `set_resolution` moves no
        // pixel at all.
        "set_resolution": [.commonTool, .floatTool],
        "get_metadata": [.nonRecordable, .commonTool, .floatTool],
    ]

    // MARK: - Reading

    /// get_color_profile: the document's colour space, whether Doctored
    /// can convert with it, what the OPEN did, and the print size its ppi
    /// implies.
    func getColorProfile(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        var result = Self.colorProfileFields(doc, document)
        result.merge(Self.resolutionFields(doc)) { _, new in new }
        // #1: a CMYK document's colour is its CMYK profile; the RGB fields describe its view.
        if let precision = doc.precision, precision.isCmyk, let info = precision.cmykInfo() {
            result["color_mode"] = "cmyk"
            result["cmyk_profile"] = info["profile"] ?? NSNull()
            result["cmyk_depth"] = info["depth"] ?? NSNull()
            result["cmyk_profile_bytes"] = precision.cmykProfileData.count
            result["view_profile"] = info["view_profile"] ?? NSNull()
        }
        return try jsonResult(result)
    }

    /// get_metadata: which packets the document carries and how big they
    /// are — presence and size only, never a parsed EXIF dump.
    func getMetadata(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        var result: [String: Any] = [
            "exif": Self.packetField(doc, DR_METADATA_EXIF),
            "xmp": Self.packetField(doc, DR_METADATA_XMP),
            "iptc": Self.packetField(doc, DR_METADATA_IPTC),
            "profile": doc.profileName,
            "on_open": Self.adoptionName(document.profileAdoption),
            "on_open_refusal": document.openProfileRefusal ?? NSNull(),
        ]
        result.merge(Self.resolutionFields(doc)) { _, new in new }
        // Three absent packets mean two different things, and only the
        // document knows which: a JPEG that carried none, or a file whose
        // container this build never walked.
        result.merge(Self.notCapturedField(document)) { _, new in new }
        return try jsonResult(result)
    }

    // MARK: - Editing

    /// assign_profile: RELABELS the document. Not one pixel number changes,
    /// so the picture changes appearance — the distinction Assign and
    /// Convert exist to keep apart.
    func assignProfile(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let profile = try resolveProfile(a)
        // A profile with no matrix/TRC model under a document_v2
        // Exposure/White Balance layer or linear-light evaluation would land
        // a document that cannot render. Thrown first, with the core's own
        // sentence — the note Image ▸ Assign Profile… shows beside that
        // choice, where Apply stays disabled. At 16 bits it is the typed
        // assign plan's refusal, so it precedes the
        // retained-source refusal at both depths.
        if let reason = doc.assignProfileRefusal(profile.data) {
            throw ToolError(message: reason.prefix(1).uppercased() + reason.dropFirst())
        }
        let previous = doc.profileName
        // Asked BEFORE the edit, against the profile the document is leaving:
        // different bytes are not a different space (a camera's 3144-byte
        // HP/IEC sRGB blob against this build's 2568-byte one), and saying
        // the picture now looks different when nothing about it moved is
        // exactly the kind of claim a model acts on.
        let sameSpace = (doc.iccProfile).map {
            ColorProfileControls.describesCurrentSpace(profile.data, current: $0)
        } ?? false
        let artwork = !doc.artworkIndices.isEmpty
        // Phase 11C: the 8A rule is a NAMED refusal rather than a silent
        // detach. `rasterize_sources: true` is the disclosed way through —
        // one result, one undo step.
        let refusal = doc.assignRefusal
        let rasterize = boolArg(a, "rasterize_sources") ?? false
        // The retained-source rule is decided BEFORE the artwork branch —
        // the order `AssignProfileSheetController.applyClicked` uses — so a
        // document holding BOTH artwork and a retained image source still
        // gets the refusal and still honours `rasterize_sources`.
        if let reason = refusal.reason, !rasterize {
            throw ToolError(
                message: reason.prefix(1).uppercased() + reason.dropFirst()
                    + ". Pass rasterize_sources: true to rasterize them and assign in one step.")
        }
        let rasterizing = rasterize && refusal.reason != nil
        let assigned = rasterizing
            ? try editColor(document, "Assign Profile",
                            { try $0.assigningProfileRasterizing(profile.data) })
            : artwork
                ? try performArtworkEdit(document, "Assign Profile", { try $0.artworkProfile(profile.data, convert: false)?.document })
                : try editColor(document, "Assign Profile", { try $0.assigningProfile(profile.data) })
        guard assigned else {
            // Either arm answers nothing only for the profile the document
            // already carries: a locked bound layer throws the core's sentence
            // instead, through `editColor`.
            return try noOpColorResult(
                ["profile": previous], why: "the document is already tagged “\(previous)”.")
        }
        return try jsonResult([
            "ok": true, "changed": true,
            "profile": document.doc?.profileName ?? profile.name,
            "previous_profile": previous,
            "rasterized_sources": rasterizing ? refusal.retainedSources : 0,
            "note": artwork && !rasterizing ? "Ordinary raster codes were relabeled; retained artwork regenerated its continuous authored sRGB colors in the new profile." : sameSpace
                ? "Pixel numbers were not touched, and the new profile describes the space the "
                    + "document was already in, so only the tag changed."
                : "Pixel numbers were not touched, so the picture now looks different.",
        ])
    }

    /// convert_profile: TRANSFORMS every layer's pixels into the new space
    /// so each layer keeps its appearance. Masks, alpha channels and layer
    /// descriptions are coverage, not colour, and are untouched — and so are
    /// layer-style and text-layer colours, which are AUTHORED sRGB and
    /// convert where they are used (a style's at composite time, a text
    /// layer's when it re-renders), so they keep their appearance too.
    ///
    /// The COMPOSITE is a different promise, and the note makes it honestly:
    /// a non-Normal blend mode, an adjustment layer or a blending style
    /// effect is computed from the numbers this changes, so such a document
    /// looks different afterwards.
    func convertProfile(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let profile = try resolveProfile(a)
        let previous = doc.profileName
        // Both halves of the transform are checked separately, so the note
        // names the side that is actually unconvertible — the core's single
        // nil covers three different situations, and "one of them" would send
        // a model guessing which.
        // At 8 and 16 bits a LUT profile with a readable table converts like
        // a matrix/TRC one (the core's relative/no-BPC pipeline); a 32-bit
        // float document needs a matrix/TRC model on both sides
        // (`profileCanDecode`; a source whose curves fail the precision check
        // still converts out). Each side is answered in band before the
        // preflight backstop below.
        guard doc.profileCanDecode else {
            return try noOpColorResult(
                ["profile": previous],
                why: "Doctored cannot convert from “\(previous)” — it carries neither a "
                    + "matrix/TRC model nor a colour table this build can read. "
                    + "assign_profile can still relabel the document.")
        }
        guard doc.isFloat ? profile.matrix : profile.convertible else {
            // A float document cannot be relabelled with a LUT profile
            // either, so its sentence (the core's `float_model_sentence`)
            // points at no Assign.
            if doc.isFloat {
                return try noOpColorResult(
                    ["profile": previous],
                    why: "Doctored cannot convert a 32-bit float document into “\(profile.name)” — "
                        + "it has no matrix/TRC model, and a float document's linear values are "
                        + "defined by its profile's primaries. Choose a matrix/TRC RGB profile.")
            }
            return try noOpColorResult(
                ["profile": previous],
                why: "Doctored cannot convert into “\(profile.name)” — it carries neither a "
                    + "matrix/TRC model nor a colour table this build can read. "
                    + "assign_profile can still relabel the document with it.")
        }
        let options = try ConversionOptions(arguments: a)
        let boundBefore = doc.retainedSourceIndices.count
        // The preflight is the CORE's refusal, asked before anything moves —
        // the same sentence the sheet disables its OK button with.
        do {
            _ = try ProfileConversionWork.preflight(
                doc, profile: profile.data, options: options, title: "Checking Profile Conversion")
        } catch {
            var message = error.localizedDescription
            // When only the bound sources stand in the way, the refusal
            // names the disclosed way through, as assign_profile's does.
            var consenting = options
            consenting.rasterizeSources = true
            if !options.rasterizeSources, boundBefore > 0, !(error is JobCancelled),
               (try? ProfileConversionWork.preflight(
                    doc, profile: profile.data, options: consenting,
                    title: "Checking Profile Conversion")) != nil {
                message += ". Pass rasterize_sources: true to rasterize "
                    + (boundBefore == 1 ? "it" : "them") + " and convert in one step."
            }
            throw ToolError(message: message)
        }
        // What the conversion is worth is a property of this stack, asked
        // before the edit for the same reason the sheet asks it: blend math,
        // an adjustment layer's parameters and a Blend If threshold are
        // evaluated ON the numbers being changed, so a layered composite
        // genuinely moves and "the picture looks the same" would be wrong.
        let appearance = ColorProfileControls.convertAppearanceSentence(doc)
        // #1 Indexed phase 3a: an Indexed document's admitted plan, asked of the source.
        let indexedPlan = IndexedDocument.profilePlanBytes(doc, profile: profile.data, options: options)
        var report = ConversionReport()
        var failure: Error?
        let changed = !doc.artworkIndices.isEmpty
            ? try performArtworkEdit(document, "Convert to Profile", { source in
                guard let result = try source.artworkProfile(profile.data, convert: true, options: options)
                else { return nil }
                report = result.report
                return result.document
            })
            : !doc.isHighDepth && !doc.retainedSourceIndices.isEmpty
                ? try RetainedByteProfileWork.run(in: document, profile: profile.data, options: options)
                    .map { landed in report = landed; return true } ?? false
                : try editColor(document, "Convert to Profile", { source in
                    // The worker's admission, a cancel and the core's
                    // sentence are carried out and answered in band.
                    do {
                        guard let result = try ProfileConversionWork.run(
                            source, profile: profile.data, options: options,
                            title: "Converting Image Profile")
                        else { return nil }
                        report = result.report
                        return result.document
                    } catch {
                        failure = error
                        return nil
                    }
                })
        if let failure { throw ToolError(message: failure.localizedDescription) }
        guard changed
        else {
            return try noOpColorResult(
                ["profile": previous],
                why: "“\(previous)” and “\(profile.name)” already describe the same space.")
        }
        var reply: [String: Any] = [
            "ok": true, "changed": true,
            "profile": document.doc?.profileName ?? profile.name,
            "previous_profile": previous,
            "intent": options.intent.rawValue,
            "black_point_compensation": options.blackPointCompensation,
            "rasterized_sources": options.rasterizeSources
                ? max(0, boundBefore - (document.doc?.retainedSourceIndices.count ?? 0)) : 0,
            "adjustments": report.jsonArray,
            "note": "Every layer's pixels were transformed. " + appearance
                + (report.bakes ? " " + report.notice : ""),
        ]
        if let indexedPlan { reply["working_memory_plan_bytes"] = indexedPlan }
        return try jsonResult(reply)
    }

    /// set_resolution: the print resolution ONLY — no pixel is resampled,
    /// only the print size follows.
    func setResolution(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        guard let x = doubleArg(a, "ppi_x") else {
            throw ToolError(message: "set_resolution requires ppi_x")
        }
        let y = doubleArg(a, "ppi_y") ?? x
        // 1…30000 ppi is the range the core sanitizes to, so refusing outside
        // it is the difference between telling the model its number was wrong
        // and silently clamping 0.5 to 1 and calling that a change.
        guard x.isFinite, y.isFinite, x >= 1, x <= 30000, y >= 1, y <= 30000 else {
            throw ToolError(message: "ppi_x and ppi_y must be between 1 and 30000")
        }
        let previous = doc.resolution
        guard try editColor(
            document, "Image Size", { try $0.settingResolution(x: x, y: y) })
        else {
            return try noOpColorResult(
                Self.resolutionFields(doc),
                why: "the document is already at \(Self.ppi(previous.x)) × "
                    + "\(Self.ppi(previous.y)) ppi.")
        }
        var result: [String: Any] = ["ok": true, "changed": true]
        if let doc = document.doc {
            result.merge(Self.resolutionFields(doc)) { _, new in new }
        }
        result["note"] = "No pixel was resampled — only the print size changed."
        return try jsonResult(result)
    }

    // MARK: - Field builders shared with get_document and save_copy

    /// The document's colour space, for `get_document` and
    /// `get_color_profile` alike.
    static func colorProfileFields(
        _ doc: RasterDocument, _ document: ImageDocument
    ) -> [String: Any] {
        [
            "name": doc.profileName,
            // false means Doctored can DISPLAY and re-embed the profile but
            // has no MATRIX/TRC model for it. Unchanged since before Phase
            // 11C on purpose: the calibrated (`document_v2`) adjustments ask
            // exactly this question. At 16 and 32 bits the typed document
            // answers, adding the high-depth precision checks.
            "convertible": doc.profileIsConvertible,
            // The wider Phase 11C question: can this build turn the
            // document's numbers into PCS colour for a conversion? At 8 and
            // 16 bits true for a LUT profile with a readable A2B table, which
            // `convertible` is false for. At 32 bits a conversion's source
            // must be matrix/TRC. Any matrix/TRC model decodes, so a profile
            // whose curves fail the precision check is true here and false
            // for `convertible`.
            "can_decode": doc.profileCanDecode,
            "on_open": adoptionName(document.profileAdoption),
            // Beside `on_open`: the sentence the Assign Profile sheet shows
            // when the open read the image into sRGB because its profile
            // could not be carried (`ImageDocument.openProfileRefusal`),
            // else null.
            "on_open_refusal": document.openProfileRefusal ?? NSNull(),
        ]
    }

    /// The print resolution and the print size it implies.
    ///
    /// The inch figures are rounded to a thousandth: that is finer than any
    /// printer resolves and it keeps binary-float noise (8.000000000000002)
    /// out of the JSON a model reads back. `max(ppi, 1)` is a division
    /// guard only — the core clamps a stored ppi to at least 1.
    static func resolutionFields(_ doc: RasterDocument) -> [String: Any] {
        let ppi = doc.resolution
        return [
            "ppi_x": ppi.x,
            "ppi_y": ppi.y,
            "print_width_in": round(Double(doc.width) / max(ppi.x, 1) * 1000) / 1000,
            "print_height_in": round(Double(doc.height) / max(ppi.y, 1) * 1000) / 1000,
        ]
    }

    /// The metadata packets the document actually carries. Empty when it
    /// carries none, so `get_document` can omit the key entirely.
    static func metadataFields(_ doc: RasterDocument) -> [String: Any] {
        var fields: [String: Any] = [:]
        for (key, kind) in [
            ("exif", DR_METADATA_EXIF), ("xmp", DR_METADATA_XMP), ("iptc", DR_METADATA_IPTC),
        ] {
            guard let bytes = doc.metadata(kind) else { continue }
            fields[key] = ["present": true, "bytes": bytes.count]
        }
        return fields
    }

    /// `metadata_not_captured: true` when this document's source container
    /// was never walked for packets, and nothing at all otherwise — a key
    /// that is absent on every ordinary document is one a model can read as
    /// a warning rather than as noise.
    ///
    /// It is deliberately NOT folded into `dropped`: that array names what
    /// the DOCUMENT held and the file did not, and we do not know which
    /// packets the source file had, only that we never looked.
    static func notCapturedField(_ document: ImageDocument) -> [String: Any] {
        var fields: [String: Any] = [:]
        if document.metadataNotCaptured { fields["metadata_not_captured"] = true }
        if let report = document.metadataImportReport { fields["metadata_import"] = report }
        return fields
    }

    /// What a finished save wrote and what it did not — the core's own
    /// report, not a guess from a format table. A kind lands in `dropped`
    /// when the format cannot carry it, when it was too large for the
    /// format's segment, or when the call asked for it to be left out.
    ///
    /// `metadata_not_captured` rides along for a platform-decoded document
    /// (HEIC, a Live Photo): its source file's capture data never reached
    /// the document, so no bit can go missing and `dropped` alone would let
    /// the save look lossless. The Export panel's notice says the same thing
    /// in a sentence — `ExportCapabilities.droppedMessage`.
    static func savedFields(
        _ doc: RasterDocument, _ document: ImageDocument, report: RasterSaveReport
    ) -> [String: Any] {
        // Only what the document HOLDS is reported, so a file that never had
        // an EXIF block is never described as having lost one. A profile and
        // a resolution are always held — a document has both, always — so
        // those two rows are unconditional and land in `dropped` when the
        // format cannot carry them or the call asked for them to be left out.
        let kinds: [(name: String, bit: UInt32, held: Bool)] = [
            ("color_profile", DR_CARRIES_PROFILE, true),
            ("exif", DR_CARRIES_EXIF, doc.metadata(DR_METADATA_EXIF) != nil),
            ("xmp", DR_CARRIES_XMP, doc.metadata(DR_METADATA_XMP) != nil),
            ("iptc", DR_CARRIES_IPTC, doc.metadata(DR_METADATA_IPTC) != nil),
            ("resolution", DR_CARRIES_RESOLUTION, true),
        ]
        var wrote: [String] = []
        var dropped: [String] = []
        for kind in kinds where kind.held {
            if report.carries(kind.bit) {
                wrote.append(kind.name)
            } else {
                dropped.append(kind.name)
            }
        }
        var fields: [String: Any] = ["wrote": wrote, "dropped": dropped]
        fields.merge(notCapturedField(document)) { _, new in new }
        return fields
    }

    // MARK: - Shared pieces

    /// One colour edit, off the event path like every agent edit, with the
    /// core's refusal handed back rather than thrown. Returns false when the
    /// op answered nil — nothing would change — and rethrows anything else,
    /// so a real failure still reaches the model as an error. (The channel
    /// tools use `editStructure`, the same wrapper for the same reason.)
    private func editColor(
        _ document: ImageDocument, _ actionName: String,
        _ transform: (RasterDocument) throws -> RasterDocument?
    ) throws -> Bool {
        var refused = false
        do {
            try performGroupedEdit(document, actionName) { current in
                let out = try transform(current)
                if out == nil { refused = true }
                return out
            }
        } catch let error as ToolError {
            guard refused else { throw error }
            return false
        }
        return true
    }

    /// The in-band "nothing changed" answer: no undo step opened and no byte
    /// moved, and saying so plainly is what lets a model carry on instead of
    /// retrying the identical call.
    private func noOpColorResult(_ fields: [String: Any], why: String) throws -> String {
        try jsonResult(
            fields.merging([
                "ok": true, "changed": false,
                "note": "Nothing changed: \(why) No undo step was added.",
            ]) { _, shared in shared })
    }

    /// A profile the caller named, already known to be one the core will
    /// accept: its bytes, the name to report, and whether Doctored can
    /// transform INTO it (false for a LUT-based profile, which can still be
    /// assigned).
    private struct ResolvedProfile {
        let data: Data
        let name: String
        /// Whether the core can convert INTO it: a matrix/TRC model, or
        /// (Phase 11C) a LUT profile with a readable B2A table.
        let convertible: Bool
        /// Whether it is an RGB matrix/TRC model — the 32-bit float
        /// conversion destination rule (`info.kind == .rgbMatrix`).
        let matrix: Bool
        /// The absolute path, when the profile came from one.
        let path: String?
    }

    /// The `profile` argument resolved to bytes: one of the two built-ins
    /// this build writes, or an `.icc`/`.icm` file on disk.
    ///
    /// The bytes are inspected HERE, before the core sees them, so a refusal
    /// can say *why* in the tool's own words (a path, a profile class) —
    /// the core's own sentence names only the argument. Everything this
    /// checks is something `dr_doc_assign_profile` also refuses. The
    /// profile-dependent refusals come after it — the matrix/TRC refusal
    /// (`assignProfileRefusal`) and the retained-source one — each thrown by
    /// the caller with the core's sentence, and the core throws the same
    /// sentences itself, so the ONLY reason for a nil from assign is "the
    /// document already carries these exact bytes" — which is what the
    /// caller is then free to report.
    private func resolveProfile(_ a: [String: Any]) throws -> ResolvedProfile {
        let choice = (stringArg(a, "profile") ?? "srgb").lowercased()
        let data: Data
        var source: String?
        switch choice {
        case "srgb":
            data = RasterProfile.builtin(.sRGB)
        case "display_p3", "displayp3", "p3":
            data = RasterProfile.builtin(.displayP3)
        case "file":
            let path = try requiredString(a, "path")
            let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
            guard let loaded = try? Data(contentsOf: url), !loaded.isEmpty else {
                throw ToolError(message: "Could not read a colour profile at \(url.path)")
            }
            data = loaded
            source = url.path
        case "installed":
            // Phase 11C: a profile named out of the system's own folders.
            // Resolved to a PATH here, so the recorded step and every
            // downstream call carry a value rather than a name that could
            // resolve differently on another machine.
            let wanted = try requiredString(a, "name")
            guard let profile = InstalledProfiles.named(wanted) else {
                throw ToolError(
                    message: "No installed profile is named \"\(wanted)\". Call "
                        + "list_installed_profiles to see what this machine has.")
            }
            guard let loaded = try? Data(contentsOf: profile.url), !loaded.isEmpty else {
                throw ToolError(message: "Could not read \(profile.path)")
            }
            data = loaded
            source = profile.path
        case let other:
            throw ToolError(
                message: "profile must be srgb, display_p3, file or installed "
                    + "(got \"\(other)\")")
        }
        // 16 MiB is the DRDC writer's per-blob cap, which the core enforces on
        // assign too so a document can never hold a profile the format would
        // refuse to save. Checking it here is what keeps the caller's "already
        // tagged that" note honest: without it a 20 MiB file would come back
        // as a bare nil and be reported as a no-op.
        let maxProfileBytes = 16 * 1024 * 1024
        guard data.count <= maxProfileBytes else {
            throw ToolError(
                message: "That colour profile is \(data.count) bytes — over the 16 MiB a "
                    + "Doctored document can carry.")
        }
        let info = RasterProfile.inspect(data)
        let predicate = info.kind.refusalPredicate ?? ""
        switch info.kind {
        case .notICC:
            throw ToolError(message: "That file " + predicate + ".")
        case .notRGB:
            throw ToolError(
                message: "“\(info.name)” " + predicate + " — Doctored documents are RGB, "
                    + "so Gray, CMYK and Lab profiles cannot be assigned.")
        case .notImageProfile:
            throw ToolError(
                message: "“\(info.name)” " + predicate + " — so there is no way to interpret a "
                    + "document's pixels in it.")
        case .rgbUnconvertible:
            return ResolvedProfile(
                data: data, name: info.name, convertible: false, matrix: false, path: source)
        case .rgbMatrix, .rgbLut:
            return ResolvedProfile(
                data: data, name: info.name, convertible: true,
                matrix: info.kind == .rgbMatrix, path: source)
        }
    }

    /// One metadata packet's presence and size.
    private static func packetField(_ doc: RasterDocument, _ kind: DrMetadataKind) -> [String: Any]
    {
        guard let bytes = doc.metadata(kind) else { return ["present": false, "bytes": 0] }
        return ["present": true, "bytes": bytes.count]
    }

    /// What the OPEN did, as the catalog's three words.
    private static func adoptionName(_ outcome: RasterAdoptOutcome) -> String {
        switch outcome {
        case .unchanged: return "unchanged"
        case .converted: return "converted"
        case .keptUnconvertible: return "kept"
        }
    }

    /// A ppi value for prose, without a trailing `.0` on the common case.
    ///
    /// Four decimals is the whole story: the core quantizes a stored ppi to
    /// four (the same `q4` the global light uses), so nothing finer exists to
    /// report — and trailing zeros are trimmed so a 300 ppi document reads
    /// "300", not "300.0000", in a sentence a model is meant to act on.
    private static func ppi(_ value: Double) -> String {
        if value == value.rounded() { return "\(Int(value))" }
        var text = String(format: "%.4f", value)
        while text.hasSuffix("0") { text.removeLast() }
        if text.hasSuffix(".") { text.removeLast() }
        return text
    }
}
