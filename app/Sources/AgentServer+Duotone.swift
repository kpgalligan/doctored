import AppKit

// The Duotone document over MCP (#1, `designs/duotone-mode.md`): `get_duotone` reads a Duotone
// document's normalized options, `set_duotone` replaces them (ONE undo step), and `convert_mode`'s
// Duotone routes — `mode: "duotone"` from an 8- or 16-bit Grayscale document or on a Duotone one,
// and `mode: "gray"` on a Duotone document, whose arguments are checked here before the ordinary
// conversion runs (the inks rendered into the gray). The UI path is Image ▸ Mode ▸ Duotone…
// (`EditorViewController+DuotoneMode.swift`'s `convertToDuotoneMode:` → `DuotoneSheetController`
// → `DuotoneEdit`) and Image ▸ Mode ▸ Grayscale (`commitGrayscale` → `CmykModeConversion`). The
// gate (`DuotoneDocument`) has already refused what is unavailable in Duotone. `set_duotone`
// takes `path` (a Duotone Options `.ado` file, exclusive with `inks`/`overprints`: the sheet's
// Load…) and `get_duotone` takes `path` and `overwrite` (the sheet's Save…), declared in
// `AgentPathGuard`.

extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let duotoneHandlers: [String: ToolHandler] = [
        "get_duotone": { $0.getDuotone },
        "set_duotone": { $0.setDuotone },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in
    /// AgentServer+Registry.swift.
    static let duotoneToolFacts: [String: ToolFacts] = [
        "get_duotone": [.nonRecordable, .commonTool],
        "set_duotone": [.commonTool],
    ]

    private func duotoneURL(_ path: String) -> URL {
        URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
    }

    /// The options `inks` and `overprints` name — or, exclusively, the Duotone Options (`.ado`)
    /// file `path` names (phase 1b) — checked by the core (its sentences in-band).
    private func duotoneOptions(_ a: [String: Any]) throws -> DuotoneOptions {
        if let path = stringArg(a, "path") {
            if a["inks"] != nil || a["overprints"] != nil {
                throw ToolError(message: "Pass either path (a Duotone Options .ado file) or inks and "
                    + "overprints, not both.")
            }
            do {
                return try DuotoneOptions(json: RasterPrecisionDocument.readDuotoneADO(at: duotoneURL(path)))
            } catch {
                throw ToolError(message: error.localizedDescription)
            }
        }
        guard let inks = a["inks"] else {
            throw ToolError(message: "Pass inks: 1 to 4 inks, each {name, lab: [L, a, b], curve: 13 "
                + "numbers or null}.")
        }
        var json: [String: Any] = ["inks": inks]
        if let overprints = a["overprints"], !(overprints is NSNull) { json["overprints"] = overprints }
        do {
            return try DuotoneOptions(json: RasterPrecisionDocument.normalizeDuotone(json))
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    /// One Duotone edit and its reply fields.
    private func runDuotoneEdit(_ document: ImageDocument, options: DuotoneOptions) throws -> [String: Any] {
        do {
            let edit = try DuotoneEdit(document, options: options)
            if let refusal = edit.refusal { throw ToolError(message: refusal) }
            if edit.changes {
                let prepared = try edit.compute()
                try edit.publish(prepared, to: document, record: .notACommand)
            }
            return ["ok": true, "changed": edit.changes, "working_memory_plan_bytes": edit.scratch]
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    /// Mirrors Image ▸ Mode ▸ Duotone…'s dialog as it opens on a Duotone document
    /// (`convertToDuotoneMode:`), and with `path` its Save… (phase 1b): the options written as a
    /// Duotone Options (`.ado`) file; an existing file refuses unless `overwrite` is true.
    func getDuotone(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let typed = document.doc?.precision, document.doc?.isDuotone == true else {
            throw ToolError(message: DuotoneDocument.notDuotoneRefusal(tool: "get_duotone"))
        }
        do {
            var reply = try typed.duotoneOptions() ?? [:]
            if let path = stringArg(a, "path") {
                let url = duotoneURL(path)
                if FileManager.default.fileExists(atPath: url.path), boolArg(a, "overwrite") != true {
                    throw ToolError(message: "\(url.path) exists; pass overwrite true to replace it.")
                }
                try RasterPrecisionDocument.writeDuotoneADO(DuotoneOptions(json: reply), to: url)
                reply["path"] = url.path
            }
            // #1 Duotone phase 2a: the Channels row's name, the type's.
            if let doc = document.doc { reply["channel_name"] = DuotoneDocument.channelName(doc) }
            reply["document"] = summary(document)
            return try jsonResult(reply)
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    /// Mirrors Image ▸ Mode ▸ Duotone…'s OK on a Duotone document (`convertToDuotoneMode:`), and
    /// with `path` its Load… then OK (phase 1b): one undo step; the same options answer changed
    /// false.
    func setDuotone(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard document.doc?.isDuotone == true else {
            throw ToolError(message: DuotoneDocument.notDuotoneRefusal(tool: "set_duotone"))
        }
        var reply = try runDuotoneEdit(document, options: duotoneOptions(a))
        reply["duotone"] = document.doc?.precisionInfo?["duotone"] ?? NSNull()
        reply["document"] = summary(document)
        return try jsonResult(reply)
    }

    /// `convert_mode`'s Duotone routes (`AgentServer+CmykMode.swift` asks first): `mode: "duotone"`
    /// mirrors Image ▸ Mode ▸ Duotone…; `mode: "gray"` on a Duotone document has its arguments
    /// checked here, then the ordinary conversion (Image ▸ Mode ▸ Grayscale: the inks' luminance in
    /// the document's own Gray profile). nil hands the call to the ordinary conversion.
    func convertModeDuotone(_ a: [String: Any], mode: ColorModeTarget, document: ImageDocument) throws -> String? {
        let isDuotone = document.doc?.isDuotone == true
        guard mode == .duotone || (mode == .gray && isDuotone) else { return nil }
        let allowed: Set<String> = mode == .duotone ? ["document_id", "mode", "inks", "overprints"]
            : ["document_id", "mode", "intent", "black_point_compensation", "flatten"]
        if mode == .gray, a["gray_space"] != nil {
            throw ToolError(message: "gray_space does not apply to a Duotone document: Grayscale keeps the "
                + "document's own Gray profile.")
        }
        if let key = a.keys.sorted().first(where: { !allowed.contains($0) }) {
            throw ToolError(message: mode == .duotone
                ? "mode duotone takes inks and overprints; \(key) does not apply."
                : "\(key) does not apply to Grayscale from a Duotone document.")
        }
        guard mode == .duotone else { return nil }
        if let doc = document.doc, let reason = DuotoneDocument.sourceRefusal(doc) {
            throw ToolError(message: reason)
        }
        var reply = try runDuotoneEdit(document, options: duotoneOptions(a))
        let gray = document.doc?.precisionInfo?["gray"] as? [String: Any]
        reply["color_mode"] = document.doc?.colorMode.rawValue ?? mode.rawValue
        reply["depth"] = gray?["depth"] ?? NSNull()
        reply["profile"] = gray?["profile"] ?? NSNull()
        reply["duotone"] = document.doc?.precisionInfo?["duotone"] ?? NSNull()
        reply["document"] = summary(document)
        return try jsonResult(reply)
    }
}
