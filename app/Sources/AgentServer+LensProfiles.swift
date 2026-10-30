import Foundation

/// Mirrors Filter ▸ Lens Correction's Lens Profile popup (v1-lensfun): `list_lens_profiles` is
/// its list and summary for the document's EXIF camera and lens, and `lensProfileArguments` is
/// `apply_filter`'s `profile` argument ("auto", "none" or a lens id), resolved here to the
/// `profile` object the core's `lens_correction` takes, at every depth.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let lensProfilesHandlers: [String: ToolHandler] = [
        "list_lens_profiles": { $0.listLensProfiles },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let lensProfilesToolFacts: [String: ToolFacts] = [
        "list_lens_profiles": [.nonRecordable, .commonTool, .floatTool],
    ]

    /// The bundled database's answer for the document (Lens Correction's popup and status line):
    /// the camera, the crop factor, the candidates, with `all` every lens on the camera's mount
    /// family, the automatic match and its profile or the reason there is none, the database
    /// credit, the EXIF identity it read and the sheet's one-sentence summary.
    func listLensProfiles(_ a: [String: Any]) throws -> String {
        let unknown = a.keys.filter { !["document_id", "all", "layer"].contains($0) }.sorted()
        guard unknown.isEmpty else {
            throw ToolError(message: "list_lens_profiles does not take \(unknown.joined(separator: ", ")).")
        }
        let document = try target(a)
        let all = boolArg(a, "all") ?? false
        let identity = document.doc.lensIdentity
        let match: LensProfileMatch
        do {
            match = try LensProfileMatch.resolve(identity, .auto, all: all,
                                                 dimensions: try lensTargetSize(a, document))
        } catch let error as ToolError { throw error } catch {
            throw ToolError(message: error.localizedDescription)
        }
        var result = match.answer
        result["identity"] = identity.json
        result["summary"] = match.summary
        return try jsonResult(result)
    }

    /// `apply_filter`'s arguments with a `lens_correction` `profile` resolved to the core's object
    /// (every other call is returned as it is): "none" removes it, "auto" is the document's EXIF
    /// match for the target layer's pixel size, any other string a lens id. A profile object passes
    /// through for the core to validate. An unknown
    /// id, or a choice that resolves to no profile, is an in-band error, never a silent no-op.
    /// `RasterDocument.lensResolvedParameters` (LensProfile.swift) is the same resolution for a
    /// recorded 16/32-bit sheet step, with the sheet's error wording; change the two together.
    func lensProfileArguments(_ a: [String: Any], _ document: ImageDocument) throws -> [String: Any] {
        guard a["filter"] as? String == "lens_correction", let raw = a["profile"],
              !(raw is [String: Any]) else { return a }
        var result = a
        let choice: LensProfileChoice
        do { choice = try LensProfileChoice(argument: raw) } catch {
            throw ToolError(message: error.localizedDescription)
        }
        guard choice != .none else {
            result.removeValue(forKey: "profile")
            return result
        }
        let match: LensProfileMatch
        do {
            match = try LensProfileMatch.resolve(document.doc.lensIdentity, choice,
                                                 dimensions: try lensTargetSize(a, document))
        } catch let error as ToolError { throw error } catch {
            throw ToolError(message: "\(error.localizedDescription). list_lens_profiles lists the lenses to choose from.")
        }
        guard let profile = match.profile else {
            let reason = match.reason ?? "no calibration applies"
            if case .lens(let id) = choice {
                throw ToolError(message: "Lensfun lens \(id) gives no profile for this document (\(reason)); nothing was applied.")
            }
            throw ToolError(message: "No Lensfun profile matches this document (\(reason)); nothing was applied. list_lens_profiles lists lenses to choose from.")
        }
        result["profile"] = profile
        return result
    }

    /// The target layer's pixel size (the frame Auto compares with the EXIF), or the canvas's.
    private func lensTargetSize(_ a: [String: Any], _ document: ImageDocument) throws
        -> (width: Int, height: Int)? {
        guard let doc = document.doc else { return nil }
        let index = try structuralLayerIndex(a, document)
        if let layer = doc.layerInfo(index), layer.width > 0, layer.height > 0 {
            return (layer.width, layer.height)
        }
        return (doc.width, doc.height)
    }
}
