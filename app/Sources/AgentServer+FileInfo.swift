import AppKit

/// The File Info tools (`FileInfo.swift`):
///
/// | Handler | UI path it mirrors |
/// |---|---|
/// | `getFileInfo` | the File ▸ File Info… sheet as it opens |
/// | `setFileInfo` | File ▸ File Info… ▸ OK |
///
/// `get_file_info` reads and records nothing. `set_file_info` IS a step: the
/// fields are per-document, undoable and `.rz`-persisted, the test
/// `AgentServer+Guides.swift`'s header states. A key the call omits keeps the
/// document's own value, so a recorded step that names one field changes one
/// field on whatever document it is replayed on.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let fileInfoHandlers: [String: ToolHandler] = [
        "get_file_info": { $0.getFileInfo },
        "set_file_info": { $0.setFileInfo },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let fileInfoToolFacts: [String: ToolFacts] = [
        // The fields live in the metadata packets, which every depth carries
        // and no pixel depends on, so both tools answer at 8, 16 and 32 bits.
        "get_file_info": [.nonRecordable, .commonTool, .floatTool],
        "set_file_info": [.commonTool, .floatTool],
    ]

    /// get_file_info: the four fields, always all four.
    func getFileInfo(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        return try jsonResult(doc.fileInfo.json)
    }

    /// File ▸ File Info… ▸ OK. One undo step; a call that changes nothing is
    /// answered in band as `changed: false`, and the core's refusal (a field
    /// past its limit) comes back as its own sentence.
    func setFileInfo(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let keys = ["creator", "copyright", "description", "keywords"]
        guard keys.contains(where: { a[$0] != nil }) else {
            throw ToolError(
                message: "set_file_info requires at least one of creator, copyright, "
                    + "description or keywords")
        }
        // An omitted key keeps the document's own value.
        var wanted = doc.fileInfo
        if let list = try Self.fileInfoList(a, "creator") { wanted.creator = list }
        if let list = try Self.fileInfoList(a, "keywords") { wanted.keywords = list }
        if let text = try Self.fileInfoText(a, "copyright") { wanted.copyright = text }
        if let text = try Self.fileInfoText(a, "description") { wanted.description = text }
        // The core answers nil for "the metadata already says this"; that is
        // not a failure, so it is told apart from `performGroupedEdit`'s
        // generic one and answered in band.
        var unchanged = false
        do {
            try performGroupedEdit(document, "File Info") { source in
                let out = try source.settingFileInfo(wanted)
                if out == nil { unchanged = true }
                return out
            }
        } catch let error as ToolError {
            guard unchanged else { throw error }
        }
        var result: [String: Any] = [
            "ok": true, "changed": !unchanged,
            "file_info": (document.doc ?? doc).fileInfo.json,
        ]
        if unchanged {
            result["note"] = "Nothing changed: the document already carries these values. "
                + "No undo step was added."
        }
        return try jsonResult(result)
    }

    /// A list argument: an array of strings, or one string as a one-entry
    /// list. nil when the key is absent.
    private static func fileInfoList(_ a: [String: Any], _ key: String) throws -> [String]? {
        guard let value = a[key] else { return nil }
        if let list = value as? [String] { return list }
        if let one = value as? String { return [one] }
        throw ToolError(message: "\(key) must be an array of strings, or one string")
    }

    /// A text argument. nil when the key is absent.
    private static func fileInfoText(_ a: [String: Any], _ key: String) throws -> String? {
        guard let value = a[key] else { return nil }
        guard let text = value as? String else {
            throw ToolError(message: "\(key) must be a string")
        }
        return text
    }
}
