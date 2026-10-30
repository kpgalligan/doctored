import AppKit

// The Grayscale colour workflow over MCP (#547 phase 5a): the Gray routes `dispatchPrecision`
// forwards here — `assign_profile` / `convert_profile` (Edit ▸ Assign Profile… / Convert to
// Profile… on a Grayscale document, `GrayProfileSheetController`) and `convert_precision`
// (Image ▸ Mode ▸ 8/16/32 Bits/Channel, `EditorViewController+CmykProfile.swift`'s
// `convertToBits` and `PrecisionModeConversion.swift`'s `convertToFloatPrecision`). The edit is
// `GrayProfileEdit`; 32 → 8/16 is `hdr_toning`'s.

extension AgentServer {
    /// The Gray routes of the shared tools (file comment); nil hands the tool on.
    func dispatchGrayColour(_ tool: String, _ a: [String: Any], document: ImageDocument) throws -> String? {
        guard document.doc?.isGray == true else { return nil }
        switch tool {
        case "assign_profile", "convert_profile":
            return try grayProfileTool(tool, a, document: document)
        case "convert_precision":
            return try grayConvertPrecision(a, document: document)
        default:
            return nil
        }
    }

    /// The profile `assign_profile` / `convert_profile` name on a Grayscale document:
    /// `working_gray` (the default: Edit ▸ Color Settings' working Gray), a Gray working space by
    /// its wire name, `installed` with name, `file` with path, or `srgb` / `display_p3`, which the
    /// core refuses by name (a Grayscale document takes Gray profiles only).
    private func grayDestination(_ a: [String: Any]) throws -> (data: Data?, space: String?, name: String) {
        let choice = (stringArg(a, "profile") ?? "working_gray").lowercased()
        switch choice {
        case "working_gray", "working":
            let working = ColorSettings.workingGray
            return (nil, working.rawValue, working.displayName)
        case "srgb":
            return (RasterProfile.builtin(.sRGB), nil, "sRGB")
        case "display_p3", "displayp3", "p3":
            return (RasterProfile.builtin(.displayP3), nil, "Display P3")
        case "installed":
            let wanted = try requiredString(a, "name")
            guard let profile = InstalledProfiles.named(wanted),
                  let loaded = try? Data(contentsOf: profile.url), !loaded.isEmpty else {
                throw ToolError(message: "No installed profile is named \"\(wanted)\". Call "
                    + "list_installed_profiles to see what this machine has.")
            }
            return (loaded, nil, profile.name)
        case "file":
            let path = try requiredString(a, "path")
            let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
            guard let loaded = try? Data(contentsOf: url), !loaded.isEmpty else {
                throw ToolError(message: "Could not read a colour profile at \(url.path)")
            }
            return (loaded, nil, RasterProfile.describe(loaded)?.name ?? url.lastPathComponent)
        case let other:
            if let space = GrayWorkingSpace(rawValue: other) {
                return (nil, space.rawValue, space.displayName)
            }
            let spaces = GrayWorkingSpace.allCases.map(\.rawValue).joined(separator: ", ")
            throw ToolError(message: "On a Grayscale document profile is working_gray, a Gray space ("
                + spaces + "), installed or file (got \"\(other)\").")
        }
    }

    /// Edit ▸ Assign Profile… / Convert to Profile… on a Grayscale document (`GrayProfileEdit`).
    private func grayProfileTool(_ tool: String, _ a: [String: Any], document: ImageDocument) throws -> String {
        let destination = try grayDestination(a)
        var options = CmykConversionOptions()
        if let name = stringArg(a, "intent") {
            guard let intent = RenderingIntent(rawValue: name) else {
                throw ToolError(message: "intent must be perceptual, relative, saturation or absolute.")
            }
            options.intent = intent
        }
        if let flag = boolArg(a, "black_point_compensation") { options.blackPointCompensation = flag }
        let previous = document.doc?.profileName ?? ""
        let kind: GrayProfileEdit.Kind = tool == "assign_profile" ? .assign : .convert
        func mode(_ fields: [String: Any]) -> [String: Any] {
            var out = fields
            out["color_mode"] = document.doc?.colorMode.rawValue ?? "gray"
            return out
        }
        do {
            let edit = try GrayProfileEdit(document, kind: kind, profile: destination.data,
                                           space: destination.space, destination: [:], options: options)
            guard edit.changes else {
                return try jsonResult(mode(["ok": true, "changed": false, "profile": previous,
                                            "note": "the document is already in “\(previous)”."]))
            }
            let prepared = try edit.compute()
            try edit.publish(prepared, to: document, record: .notACommand)
            return try jsonResult(mode([
                "ok": true, "changed": true,
                "profile": document.doc?.profileName ?? destination.name, "previous_profile": previous,
                "depth": document.doc?.grayDepth ?? NSNull(), "working_memory_plan_bytes": edit.scratch,
                "note": kind == .assign
                    ? "The gray values were not touched, so the picture now looks different."
                    : "Every layer's gray values were re-encoded through the destination profile.",
                "document": summary(document),
            ]))
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    /// Image ▸ Mode ▸ 8/16/32 Bits/Channel on a Grayscale document: mode `gray8`, `gray16` or
    /// `gray32`; from 32 bits the core refuses (HDR Toning converts it).
    private func grayConvertPrecision(_ a: [String: Any], document: ImageDocument) throws -> String {
        let mode = try requiredString(a, "mode")
        guard let bits = ["gray8": 8, "gray16": 16, "gray32": 32][mode] else {
            throw ToolError(message: "A Grayscale document converts between mode gray8, gray16 and gray32 "
                + "(Image ▸ Mode ▸ 8/16/32 Bits/Channel); convert_mode converts it to RGB.")
        }
        do {
            let edit = try GrayProfileEdit(document, kind: .depth(bits), profile: nil, space: nil,
                                           destination: [:], options: CmykConversionOptions())
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
            return try jsonResult(["ok": true, "changed": true, "mode": mode, "color_mode": "gray",
                                   "bits_per_channel": bits, "working_memory_plan_bytes": edit.scratch,
                                   "document": summary(document)])
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }
}
