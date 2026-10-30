import AppKit

/// The spot channel tools (#1 phase 1a-2), mirroring the Channels menu:
///
/// | Handler | UI path it mirrors |
/// |---|---|
/// | `addSpotChannel` | Channels ▸ New Spot Channel… |
/// | `setSpotChannel` | Channels ▸ Spot Channel Options… (and a spot row's menu) |
/// | `mergeSpotChannel` | Channels ▸ Merge Spot Channel (and a spot row's menu) |
/// | `setChannelKind` | Channel Options… ▸ Color Indicates: Spot Color; Spot Channel Options… ▸ Kind: Alpha Channel |
///
/// A spot channel is listed by `list_channels` / `get_document` with its `spot` ink, painted with
/// `target: "channel:<name>"`, and deleted, duplicated and loaded as a selection by the alpha
/// channel tools. Canonical shape: `target(a)` → `channelArgument` → `editStructure` →
/// `jsonResult`; catalog entries are `AgentCatalog+SpotChannels.swift`'s.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let spotChannelsHandlers: [String: ToolHandler] = [
        "add_spot_channel": { $0.addSpotChannel },
        "set_spot_channel": { $0.setSpotChannel },
        "merge_spot_channel": { $0.mergeSpotChannel },
        "set_channel_kind": { $0.setChannelKind },
    ]

    /// Facts for this file's tools; merged in AgentServer+Registry.swift.
    static let spotChannelsToolFacts: [String: ToolFacts] = [
        "add_spot_channel": [.commonTool, .floatTool, .cmykTool, .selectionSensitive],
        "set_spot_channel": [.commonTool, .floatTool, .cmykTool],
        "merge_spot_channel": [.commonTool, .floatTool, .cmykTool],
        "set_channel_kind": [.commonTool, .floatTool, .cmykTool],
    ]

    /// Channels ▸ Merge Spot Channel (#1 1c-1): the document flattened with the spot channel's
    /// ink baked into the colour channels and the channel deleted, one undo step. An alpha channel
    /// and a Duotone document refuse by name.
    func mergeSpotChannel(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let index = try channelArgument(a, doc)
        guard let info = doc.channelInfo(index), info.spot != nil else {
            throw ToolError(message: SpotChannels.notSpotRefusal)
        }
        let plan = try doc.precision?.mergeSpotChannelPlan(index)
        if let refusal = plan?["refusal"] as? String { throw ToolError(message: refusal) }
        let merged = try editStructure(document, "Merge Spot Channel") { try $0.mergingSpotChannel(index) }
        guard merged else { throw ToolError(message: "Merge Spot Channel changed nothing.") }
        var result: [String: Any] = [
            "ok": true, "channel": index, "name": info.name,
            "layers": document.doc?.layerCount ?? 0, "channels": document.doc?.channelCount ?? 0,
        ]
        if let bytes = plan?["working_memory_plan_bytes"] { result["working_memory_plan_bytes"] = bytes }
        return try jsonResult(result)
    }

    /// Channel Options… ▸ Color Indicates: Spot Color (`kind` "spot") and Spot Channel Options… ▸
    /// Kind: Alpha Channel (`kind` "alpha") (#1 1c-1): the channel's kind, its plane inverted so
    /// what showed black becomes full ink, one undo step. A new spot ink defaults to the UI's: the
    /// rubylith colour's Lab and its opacity as the Solidity.
    func setChannelKind(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let index = try channelArgument(a, doc)
        guard let info = doc.channelInfo(index) else {
            throw ToolError(message: "Channel \(index) is out of range.")
        }
        let kind = stringArg(a, "kind")?.lowercased() ?? ""
        guard kind == "spot" || kind == "alpha" else {
            throw ToolError(message: "kind must be \"spot\" or \"alpha\" (got \"\(kind)\")")
        }
        guard (info.spot != nil) != (kind == "spot") else {
            return try structureNoOp(["channel": index, "name": info.name, "kind": kind],
                                     why: "channel \(index) is already \(kind == "spot" ? "a spot" : "an alpha") channel.")
        }
        var ink: SpotInk?
        if kind == "spot" {
            let rubylith = RasterLab.lab(srgb: [Double(info.red) / 255, Double(info.green) / 255,
                                                Double(info.blue) / 255]) ?? SpotChannels.defaultInk.lab
            ink = SpotInk(lab: try Self.spotLab(a) ?? rubylith,
                          solidity: try Self.spotSolidity(a) ?? Int((info.opacity * 100).rounded()))
        }
        let changed = try editStructure(document, kind == "spot" ? "Channel Options" : "Spot Channel Options") {
            try $0.settingChannelKind(index, spot: ink)
        }
        guard changed else { throw ToolError(message: "The channel kind could not be changed.") }
        return try jsonResult([
            "ok": true, "channel": index, "name": info.name, "kind": kind,
            "spot": ink.map { $0.fields as Any } ?? NSNull(),
        ])
    }

    /// New Spot Channel…: name (default the next "Spot Color N"), lab and solidity (default
    /// sRGB red's at 0 %), from "selection" (the default when one exists) or "empty".
    func addSpotChannel(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let selection = editor(document)?.agentSelection
        let from = stringArg(a, "from") ?? (selection == nil ? "empty" : "selection")
        let coverage: CanvasSelection?
        switch from {
        case "empty":
            coverage = nil
        case "selection":
            guard let selection else {
                throw ToolError(
                    message: "Nothing is selected — make a selection first (select_rect, "
                        + "select_magic_wand, select_subject, …), or use from: \"empty\".")
            }
            coverage = selection
        case let other:
            throw ToolError(message: "from must be \"selection\" or \"empty\" (got \"\(other)\")")
        }
        let defaults = SpotChannels.defaultInk
        let ink = SpotInk(lab: try Self.spotLab(a) ?? defaults.lab,
                          solidity: try Self.spotSolidity(a) ?? defaults.solidity)
        let requested = stringArg(a, "name")?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let name = requested.isEmpty ? doc.nextSpotChannelName : requested
        // The budget question New Spot Channel… asks first (`channelBudgetAllowsOneMore`).
        if let reason = doc.channelBudgetRefusal(width: doc.width, height: doc.height, adding: 1) {
            throw ToolError(message: reason)
        }
        let added = try editStructure(document, "New Spot Channel") {
            try $0.addingSpotChannel(name: name, ink: ink, selection: coverage)
        }
        guard added else { throw ToolError(message: "The spot channel could not be added.") }
        var result: [String: Any] = [
            "ok": true, "channel": (document.doc?.channelCount ?? 1) - 1, "name": name,
            "from": from, "channels": document.doc?.channelCount ?? 0,
        ]
        result.merge(ink.fields) { _, new in new }
        return try jsonResult(result)
    }

    /// Spot Channel Options…: name, lab and solidity, each omitted one kept; one undo step.
    func setSpotChannel(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let index = try channelArgument(a, doc)
        guard let info = doc.channelInfo(index), let current = info.spot else {
            throw ToolError(message: SpotChannels.notSpotRefusal)
        }
        let ink = SpotInk(lab: try Self.spotLab(a) ?? current.lab,
                          solidity: try Self.spotSolidity(a) ?? current.solidity)
        let requested = stringArg(a, "name")?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let name: String? = requested.isEmpty || requested == info.name ? nil : requested
        let changed = try editStructure(document, "Spot Channel Options", renderNeutral: true) {
            try $0.settingSpotChannel(index, name: name, ink: ink)
        }
        guard changed else {
            return try structureNoOp(["channel": index, "name": info.name],
                                     why: "channel \(index) already has exactly that name and ink.")
        }
        var result: [String: Any] = ["ok": true, "channel": index, "name": name ?? info.name]
        result.merge(ink.fields) { _, new in new }
        return try jsonResult(result)
    }

    /// The optional `lab` argument: three finite numbers (the core checks their ranges).
    private static func spotLab(_ a: [String: Any]) throws -> [Double]? {
        guard let raw = a["lab"] else { return nil }
        guard let values = raw as? [Any], values.count == 3 else {
            throw ToolError(message: SpotChannels.labSentence)
        }
        let lab = values.compactMap { ($0 as? NSNumber)?.doubleValue ?? ($0 as? String).flatMap(Double.init) }
        guard lab.count == 3, lab.allSatisfy(\.isFinite) else {
            throw ToolError(message: SpotChannels.labSentence)
        }
        return lab
    }

    /// The optional `solidity` argument: an integer percent, 0–100.
    private static func spotSolidity(_ a: [String: Any]) throws -> Int? {
        guard let raw = a["solidity"] else { return nil }
        let value = (raw as? NSNumber)?.doubleValue ?? (raw as? String).flatMap(Double.init)
        guard let value, value.isFinite, value.rounded() == value, (0...100).contains(value) else {
            throw ToolError(message: "solidity must be an integer from 0 to 100")
        }
        return Int(value)
    }
}
