import AppKit

// The Lab Color depth over MCP (#547 Lab phase 5a): the Lab route `dispatchPrecision` forwards
// here — `convert_precision` mode lab8 / lab16, mirroring Image ▸ Mode ▸ 8/16 Bits/Channel on a
// Lab document (`EditorViewController+CmykProfile.swift`'s `convertToBits` →
// `EditorViewController+LabMode.swift`'s `commitLabDepth`). The edit is `LabDepthEdit`; there is
// no 32-bit Lab. Since Lab phase 5b also `convert_profile`, mirroring Edit ▸
// Convert to Profile… on a Lab document (`EditorViewController+LabProfile.swift`'s
// `commitLabProfileConversion`): Image ▸ Mode's conversion into the named CMYK or RGB profile
// (`CmykModeConversion`), and since Lab phase 6a-ii into a Gray profile (5a-ii's route: each
// pixel's L* through its tone curve). A route, as `AgentServer+GrayColour.swift`:
// `convert_precision`'s and `convert_profile`'s handlers and facts stay
// `PrecisionModeConversion.swift`'s and `AgentServer+Color.swift`'s.

extension AgentServer {
    /// The Lab routes of the shared tools (file comment); nil hands the tool on.
    func dispatchLabColour(_ tool: String, _ a: [String: Any], document: ImageDocument) throws -> String? {
        guard document.doc?.isLab == true else { return nil }
        switch tool {
        case "convert_precision":
            return try labConvertPrecision(a, document: document)
        case "convert_profile": // Phase 5b
            return try labConvertProfile(a, document: document)
        default:
            return nil
        }
    }

    /// Edit ▸ Convert to Profile… on a Lab Color document (#547 Lab phase 5b): `profile`
    /// working_cmyk (the default), srgb, display_p3, installed with name or file with path
    /// (`cmykDestination`), with intent, black_point_compensation and flatten, converts to CMYK
    /// Color or RGB Color in that profile at the document's own depth, one undo step. Since phase
    /// 6a-ii `working_gray`, a Gray working space by name, or an installed or file Gray profile
    /// converts to Grayscale (intent and BPC do not apply).
    private func labConvertProfile(_ a: [String: Any], document: ImageDocument) throws -> String {
        let choice = (stringArg(a, "profile") ?? "").lowercased()
        let graySpace = choice == "working_gray" ? ColorSettings.workingGray.rawValue
            : GrayWorkingSpace(rawValue: choice)?.rawValue
        let destination: (data: Data?, name: String, rgb: Bool, gray: Bool)
        if let space = graySpace.flatMap(GrayWorkingSpace.init(rawValue:)) {
            destination = (nil, space.displayName, false, true)
        } else {
            destination = try cmykDestination(a, admitsGray: true)
        }
        let target: ColorModeTarget = destination.gray ? .gray : destination.rgb ? .rgb : .cmyk
        var options = CmykConversionOptions()
        if let name = stringArg(a, "intent") {
            guard let intent = RenderingIntent(rawValue: name) else {
                throw ToolError(message: "intent must be perceptual, relative, saturation or absolute.")
            }
            options.intent = intent
        }
        if let flag = boolArg(a, "black_point_compensation") { options.blackPointCompensation = flag }
        options.flatten = boolArg(a, "flatten") ?? false
        do {
            let conversion = try CmykModeConversion(document, target: target, profile: destination.data,
                                                    options: options, graySpace: graySpace)
            let prepared = try conversion.compute()
            try conversion.publish(prepared, to: document, record: .notACommand)
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        var result: [String: Any] = ["ok": true, "changed": true, "document": summary(document)]
        if destination.gray {
            result["color_mode"] = "gray"
            let info = document.doc?.precisionInfo?["gray"] as? [String: Any]
            result["profile"] = info?["profile"] ?? destination.name
            result["depth"] = document.doc?.grayDepth ?? NSNull()
        } else if destination.rgb {
            result["color_mode"] = "rgb"
            result["profile"] = document.doc?.profileName ?? destination.name
        } else {
            let info = document.doc?.precision?.cmykInfo() ?? [:]
            result["color_mode"] = "cmyk"
            result["profile"] = info["profile"] ?? destination.name
            result["depth"] = info["depth"] ?? NSNull()
        }
        return try jsonResult(result)
    }

    /// Image ▸ Mode ▸ 8/16 Bits/Channel on a Lab Color document: mode `lab8` or `lab16`.
    private func labConvertPrecision(_ a: [String: Any], document: ImageDocument) throws -> String {
        let mode = try requiredString(a, "mode")
        guard let bits = ["lab8": 8, "lab16": 16][mode] else {
            throw ToolError(message: "A Lab Color document converts between mode lab8 and lab16 "
                + "(Image ▸ Mode ▸ 8/16 Bits/Channel); 32 Bits/Channel is unavailable in Lab Color.")
        }
        do {
            let edit = try LabDepthEdit(document, bits: bits)
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
            return try jsonResult(["ok": true, "changed": true, "mode": mode, "color_mode": "lab",
                                   "bits_per_channel": bits, "working_memory_plan_bytes": edit.scratch,
                                   "document": summary(document)])
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }
}
