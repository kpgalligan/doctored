import AppKit

/// `package_action` and `import_action` — the only route from "an action that
/// names files on this machine" to "an action somebody else can run".
///
/// They mirror the Actions window's Package… and Import… buttons, and neither
/// does any work of its own: `ActionAssets` owns the asset table, the
/// basename-collision rule, the copy and the rewrite, so the two buttons and
/// the two tools cannot drift.
///
/// **Packaging never modifies the library copy.** It writes a bundle whose
/// `action.json` has each file argument rewritten to `{"$asset": …}` beside a
/// copy of the file; the action in the library still carries its literal
/// paths. That is what makes packaging repeatable and what keeps a user's own
/// action working after they have shared it.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let actionAssetsHandlers: [String: ToolHandler] = [
        "package_action": { $0.packageAction },
        "import_action": { $0.importAction },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let actionAssetsToolFacts: [String: ToolFacts] = [
        // Phase 12B: a batch plays actions (recording one would nest a
        // replay), and the other four read or write the action library —
        // none of them changes a picture.
        // None of these handlers resolves a document either — New
        // Document, the clipboard read and New from Clipboard, the action
        // library and recorder, and the batch and bundle tools (a batch opens
        // each file at its own depth and plays through this same gate).
        // Phase 12B's batch and bundle tools read no document, so they are
        // in `documentFreeTools`. `reorder_layers` is a TYPED tool
        // since 12E-1 (`typedTools` below): the core owns the typed reorder.
        // Phase 12C: a colour LABEL is metadata with a typed implementation
        // of its own (the `layer_label` edit), and `find_layers` only reads —
        // neither touches pixel storage. `new_document` and the three
        // document-preset tools read no document, so they are in
        // `documentFreeTools`.
        // Phase 12B's batch and bundle tools read no document, so they are
        // in `DocumentPrecision.documentFreeTools`. A float document's
        // own admitted set still gates every step a batch plays.
        "package_action": [.nonRecordable, .documentFree],
        "import_action": [.nonRecordable, .documentFree],
    ]

    /// Mirrors the Actions window's Package… button.
    func packageAction(_ a: [String: Any]) throws -> String {
        guard let name = stringArg(a, "name") else {
            throw ToolError(message: "Missing required argument: name")
        }
        guard let destination = stringArg(a, "destination"), !destination.isEmpty else {
            throw ToolError(
                message: "Missing required argument: destination — the FOLDER the "
                    + "<slug>.draction bundle is written into.")
        }
        let action: Action
        switch ActionLibrary.entry(named: name) {
        case .ok(let decoded)?: action = decoded
        case .broken(let file, let reason)?: throw ToolError(message: "\(file): \(reason)")
        case nil:
            throw ToolError(message: "No action named \"\(name)\". Call list_actions.")
        }
        let folder = URL(
            fileURLWithPath: (destination as NSString).expandingTildeInPath, isDirectory: true)
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: folder.path, isDirectory: &isDirectory),
              isDirectory.boolValue
        else {
            throw ToolError(message: "\(folder.path) is not a folder.")
        }
        for (index, step) in action.steps.enumerated() {
            for key in ActionAssets.assetKeys(for: step) {
                guard let path = step.arguments[key] as? String else { continue }
                try AgentPathGuard.shared.check(
                    path, access: .read, argument: "steps[\(index)].\(key)")
            }
        }
        do {
            let bundle = try ActionAssets.package(
                action, to: folder, overwrite: boolArg(a, "overwrite") ?? false)
            return try jsonResult([
                "ok": true, "name": action.name, "path": bundle.url.path,
                "assets": bundle.assets,
            ])
        } catch let error as ActionAssets.Failure {
            throw ToolError(message: error.message)
        }
    }

    /// Mirrors the Actions window's Import… button.
    func importAction(_ a: [String: Any]) throws -> String {
        guard let path = stringArg(a, "path"), !path.isEmpty else {
            throw ToolError(
                message: "Missing required argument: path — the <slug>.draction FOLDER to "
                    + "import.")
        }
        let bundle = URL(
            fileURLWithPath: (path as NSString).expandingTildeInPath, isDirectory: true)
        do {
            let imported = try ActionAssets.importBundle(
                at: bundle, name: stringArg(a, "name"),
                overwrite: boolArg(a, "overwrite") ?? false)
            var result: [String: Any] = [
                "ok": true, "name": imported.name, "assets": imported.assets,
                "slug": ActionLibrary.slug(for: imported.name),
            ]
            // Said here rather than discovered at the first run: a bundle
            // whose `assets/` was edited by hand can install an action that
            // references a file it did not bring.
            if let action = ActionLibrary.action(named: imported.name) {
                let missing = ActionAssets.missing(in: action)
                if !missing.isEmpty { result["missing_assets"] = missing }
            }
            return try jsonResult(result)
        } catch let error as ActionAssets.Failure {
            throw ToolError(message: error.message)
        }
    }
}
