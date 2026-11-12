import Foundation

/// The one check every MCP and Assistant tool argument that names
/// a file or folder passes before the tool runs. `AgentServer.execute` calls
/// `check(tool:arguments:)` first, so the HTTP server, the Assistant, Action
/// replay and Batch all get the same refusals; the four handlers that resolve
/// an export preset's stored folder or profile, and `package_action`'s copy
/// of each step's literal file, call it again on what they resolved.
///
/// A refusal is one sentence naming the location, thrown as a `ToolError`, so
/// it returns in-band (`isError: true`) and a model can choose another path.
///
/// Every path is resolved before it is compared: `~` and relative paths made
/// absolute, `.` and `..` applied, and every existing symlink on the way
/// replaced by its target, so a link into a blocked folder is refused and a
/// path that does not exist yet still resolves through the folders that do.
/// The comparison is case-insensitive, because the default macOS volume is.
///
/// A new tool argument that names a file is declared in `toolPaths`; server
/// start refuses (`catalogFailure`) a catalog argument whose name looks like a
/// path and is declared neither there nor in `nonFilePathArguments`.
struct AgentPathGuard {
    enum Access { case read, write }

    struct BlockedRoot {
        /// Absolute, or `~/…` for the user's home.
        let path: String
        let label: String
    }

    struct PathArgument {
        /// Through dictionaries: `["path"]`, `["settings", "profile_path"]`.
        let keyPath: [String]
        let access: Access
        /// String values that name a built-in rather than a file.
        var builtinNames: Set<String> = []
    }

    /// The sensitive locations no agent tool may read or write, with why.
    static let blockedRoots: [BlockedRoot] = [
        // Doctored's own state: the Assistant's API key (`anthropic_api_key`),
        // every preference library, Actions, recovery and clipboard payloads.
        BlockedRoot(path: "~/Library/Application Support/Doctored",
                    label: "Doctored's Application Support folder"),
        // The same state under its name before the rename, until the first
        // launch moves it (`DataMigration`), and its copy if a move failed.
        BlockedRoot(path: "~/Library/Application Support/Rasterize",
                    label: "the former Rasterize Application Support folder"),
        // The user's login keychains: every saved password and private key.
        BlockedRoot(path: "~/Library/Keychains", label: "the user's keychains"),
        // Developer credentials: SSH and GnuPG private keys, cloud and
        // container credentials, and the token files of package registries.
        BlockedRoot(path: "~/.ssh", label: "SSH keys"),
        BlockedRoot(path: "~/.gnupg", label: "GnuPG keys"),
        BlockedRoot(path: "~/.aws", label: "AWS credentials"),
        BlockedRoot(path: "~/.config/gcloud", label: "Google Cloud credentials"),
        BlockedRoot(path: "~/.kube", label: "Kubernetes credentials"),
        BlockedRoot(path: "~/.docker", label: "Docker credentials"),
        BlockedRoot(path: "~/.netrc", label: "the netrc password file"),
        BlockedRoot(path: "~/.npmrc", label: "npm credentials"),
        BlockedRoot(path: "~/.pypirc", label: "PyPI credentials"),
        BlockedRoot(path: "~/.git-credentials", label: "Git credentials"),
        // Session cookies: a copy is a logged-in session.
        BlockedRoot(path: "~/Library/Cookies", label: "the user's cookies"),
        // Browser profiles: cookies, saved passwords and history.
        BlockedRoot(path: "~/Library/Safari", label: "Safari's profile"),
        BlockedRoot(path: "~/Library/Containers/com.apple.Safari",
                    label: "Safari's container"),
        BlockedRoot(path: "~/Library/Application Support/Google/Chrome",
                    label: "Chrome's profile"),
        BlockedRoot(path: "~/Library/Application Support/Firefox", label: "Firefox's profile"),
        BlockedRoot(path: "~/Library/Application Support/BraveSoftware",
                    label: "Brave's profile"),
        BlockedRoot(path: "~/Library/Application Support/Microsoft Edge",
                    label: "Edge's profile"),
        BlockedRoot(path: "~/Library/Application Support/Arc", label: "Arc's profile"),
        // System configuration (accounts, hosts, sudoers) under both of its
        // names, the system volume, and the system keychains.
        BlockedRoot(path: "/etc", label: "the system configuration folder"),
        BlockedRoot(path: "/private/etc", label: "the system configuration folder"),
        BlockedRoot(path: "/System", label: "the macOS system folder"),
        BlockedRoot(path: "/Library/Keychains", label: "the system keychains"),
    ]

    /// Every folder variable the app reads (app/CLAUDE.md "Verification
    /// isolation"). A set, non-empty one is blocked like Doctored's own
    /// Application Support folder, because it stands in for a part of it;
    /// the app's own code reaches these folders, never a client's path.
    static let environmentFolders: [String] = [
        "DR_ACTIONS_DIR", "DR_RECOVERY_DIR",
        "DR_RAW_PRESETS_DIR", "DR_EXPORT_PRESETS_DIR", "DR_METADATA_POLICIES_DIR",
        "DR_DOCUMENT_PRESETS_DIR", "DR_SHORTCUTS_DIR", "DR_WORKSPACES_DIR",
        "DR_PROOF_PRESETS_DIR", "DR_ADJUSTMENT_PRESETS_DIR",
        "DR_DEVELOP_FILTER_PRESETS_DIR", "DR_CLIPBOARD_PAYLOADS_DIR", "DR_PRINTERS_DIR",
        "DR_MODELS_DIR", "DR_SKIES_DIR", "DR_COLOR_SETTINGS_DIR", "DR_CLOUD_DIR",
        "DR_STYLE_PRESETS_DIR", "DR_HDR_TONING_PRESETS_DIR", "DR_RAW_LOCAL_MASKS_DIR",
        "DR_BRUSH_PRESETS_DIR",
    ]

    /// Every tool argument that names a file or folder, by tool.
    static let toolPaths: [String: [PathArgument]] = {
        var table: [String: [PathArgument]] = [:]
        func add(_ tools: [String], _ keyPath: [String], _ access: Access,
                 builtins: Set<String> = []) {
            for tool in tools {
                table[tool, default: []].append(
                    PathArgument(keyPath: keyPath, access: access, builtinNames: builtins))
            }
        }
        add(["open_document", "inspect_psd", "inspect_vector", "place_image",
             "add_live_photo_layer", "describe_profile", "assign_profile", "convert_profile",
             "set_proof_setup", "render_proof", "read_photoshop_adjustment_preset",
             "load_snapshot", "import_action", "convert_mode", "import_style_presets",
             "set_color_table", "set_duotone", "import_brush_presets"], ["path"], .read)
        add(["load_files_into_stack", "merge_to_hdr", "merge_panorama"], ["paths"], .read)
        add(["contact_sheet_from_sources"], ["files"], .read)
        add(["paste"], ["payload_path"], .read)
        add(["apply_raw_settings"], ["mask_paths"], .read)
        add(["run_batch"], ["input"], .read)
        add(["sky_replacement"], ["sky_path"], .read)
        add(["new_document", "save_document_preset"], ["profile"], .read,
            builtins: ["srgb", "sRGB", "display_p3", "displayP3", "Display P3"])
        add(["add_adjustment_layer", "edit_adjustment_layer", "apply_filter",
             "save_adjustment_preset"], ["params", "file"], .read)
        add(["export_preview", "export_image", "quick_export", "save_export_preset",
             "export_layers", "export_pdf"], ["settings", "profile_path"], .read)
        add(["contact_sheet", "contact_sheet_from_sources"],
            ["output", "settings", "profile_path"], .read)
        add(["save_copy", "export_image", "export_psd", "export_pdf", "save_snapshot",
             "get_raw_local_mask", "export_style_presets", "get_color_table", "get_duotone",
             "export_brush_presets"],
            ["path"], .write)
        add(["copy", "cut", "copy_merged"], ["public_copy_path"], .write)
        add(["quick_export", "save_export_preset", "export_layers", "package_action"],
            ["destination"], .write)
        add(["contact_sheet", "contact_sheet_from_sources"], ["output", "destination"], .write)
        add(["run_batch"], ["output"], .write)
        add(["assisted_workspace_status", "assisted_open_case"], ["workspace"], .write)
        return table
    }()

    /// Catalog arguments whose names look like a path and name something
    /// else: the Paths family's `path` is a stored vector path's name.
    static let nonFilePathArguments: [String: Set<String>] = [
        "set_path": ["path"], "rename_path": ["path"], "duplicate_path": ["path"],
        "delete_path": ["path"], "path_to_selection": ["path"], "fill_path": ["path"],
        "stroke_path": ["path"], "set_vector_mask": ["path"], "set_clipping_path": ["path"],
    ]

    /// The argument names `catalogFailure` treats as naming a file.
    private static let pathLikeNames: Set<String> = [
        "path", "paths", "files", "input", "destination", "workspace",
    ]

    static let shared = AgentPathGuard(
        home: NSHomeDirectory(),
        currentDirectory: FileManager.default.currentDirectoryPath,
        environment: ProcessInfo.processInfo.environment,
        actionsDirectory: ActionLibrary.directory.path,
        clipboardPayloadsDirectory: ClipboardPayloadStore.directory.path,
        profileFolders: InstalledProfiles.folders.map(\.path)
            + [InstalledProfiles.printerRoot.path])

    private struct Root {
        let key: String
        let shown: String
        let label: String
    }

    private let home: String
    private let currentDirectory: String
    private let roots: [Root]
    private let actionsKey: String
    private let clipboardKey: String
    private let rawLocalMasksKey: String
    private let profileKeys: [String]

    init(home: String, currentDirectory: String, environment: [String: String],
         actionsDirectory: String, clipboardPayloadsDirectory: String, profileFolders: [String],
         rawLocalMasksDirectory: String = RawLocalMaskStore.directory.path) {
        self.home = home
        self.currentDirectory = currentDirectory
        var roots: [Root] = []
        for root in Self.blockedRoots {
            let resolved = Self.resolve(root.path, home: home, currentDirectory: currentDirectory)
            roots.append(Root(key: Self.key(resolved), shown: root.path, label: root.label))
        }
        for name in Self.environmentFolders {
            guard let value = environment[name], !value.isEmpty else { continue }
            let resolved = Self.resolve(value, home: home, currentDirectory: currentDirectory)
            roots.append(Root(key: Self.key(resolved), shown: Self.shown(resolved, home: home),
                              label: "Doctored's \(name) folder"))
        }
        self.roots = roots
        actionsKey = Self.key(
            Self.resolve(actionsDirectory, home: home, currentDirectory: currentDirectory))
        clipboardKey = Self.key(
            Self.resolve(clipboardPayloadsDirectory, home: home, currentDirectory: currentDirectory))
        rawLocalMasksKey = Self.key(
            Self.resolve(rawLocalMasksDirectory, home: home, currentDirectory: currentDirectory))
        profileKeys = profileFolders.map {
            Self.key(Self.resolve($0, home: home, currentDirectory: currentDirectory))
        }
    }

    // MARK: - Checking

    /// The refusal sentence for `path`, or nil when the tool may use it.
    func refusal(path: String, access: Access, argument: String) -> String? {
        let resolved = Self.resolve(path, home: home, currentDirectory: currentDirectory)
        let key = Self.key(resolved)
        if access == .read, isCarvedOut(key) { return nil }
        if let root = roots.first(where: { Self.isWithin(key, $0.key) }) {
            return "Refused: \(argument) points into \(root.label) (\(root.shown)), "
                + "which agent tools may not read or write."
        }
        guard access == .write else { return nil }
        let components = resolved.split(separator: "/").map(String.init)
        if let bundle = components.firstIndex(where: { $0.lowercased().hasSuffix(".app") }) {
            let shown = Self.shown("/" + components[...bundle].joined(separator: "/"), home: home)
            return "Refused: \(argument) points into an application bundle (\(shown)), "
                + "which agent tools may not write into."
        }
        if Self.isWithin(key, Self.key("/Applications")) {
            return "Refused: \(argument) points into the Applications folder (/Applications), "
                + "which agent tools may not write into."
        }
        return nil
    }

    func check(_ path: String, access: Access, argument: String) throws {
        if let message = refusal(path: path, access: access, argument: argument) {
            throw AgentServer.ToolError(message: message)
        }
    }

    /// Every declared path argument of `tool` in `arguments`. A value of
    /// another type is left for the handler to refuse.
    func check(tool: String, arguments: [String: Any]) throws {
        guard let declared = Self.toolPaths[tool] else { return }
        for entry in declared {
            var value: Any? = arguments
            for key in entry.keyPath {
                value = (value as? [String: Any])?[key]
            }
            let name = entry.keyPath.joined(separator: ".")
            if let path = value as? String {
                guard !path.isEmpty, !entry.builtinNames.contains(path) else { continue }
                try check(path, access: entry.access, argument: name)
            } else if let paths = value as? [Any] {
                for (index, element) in paths.enumerated() {
                    guard let path = element as? String, !path.isEmpty,
                          !entry.builtinNames.contains(path) else { continue }
                    try check(path, access: entry.access, argument: "\(name)[\(index)]")
                }
            }
        }
    }

    /// A resolved export preset: its destination folder is written and its
    /// profile file is read. Re-checked at use, because the library copy may
    /// predate this check or have been written by hand.
    func check(exportPreset preset: ExportPreset, argument: String) throws {
        try check(preset.destination, access: .write, argument: "\(argument)'s destination")
        if let path = preset.settings.profile.path {
            try check(path, access: .read, argument: "\(argument)'s settings.profile_path")
        }
    }

    /// The reads the app itself names by path and a blocked folder holds:
    /// (1) a file in an action's `<slug>.assets/` folder, which Action replay
    /// substitutes for a `$asset` symbol; (2) a `.drcb` (or legacy) payload directly in
    /// the clipboard payloads folder, which a recorded saved-payload paste
    /// names; (3) a file under an installed-profile folder, which
    /// `list_installed_profiles` reports and drives pass to the colour tools;
    /// (4) a `.drmask` (or legacy) directly in the RAW local mask store, which a recorded
    /// Masking sync's `apply_raw_settings` names in `mask_paths`.
    private func isCarvedOut(_ key: String) -> Bool {
        if key.hasPrefix(actionsKey + "/") {
            let rest = key.dropFirst(actionsKey.count + 1).split(separator: "/")
            if rest.count >= 2, rest[0].count > ".assets".count, rest[0].hasSuffix(".assets") {
                return true
            }
        }
        if key.hasPrefix(clipboardKey + "/") {
            let rest = key.dropFirst(clipboardKey.count + 1)
            if !rest.contains("/"), SideFileExtension.clipboardPayload.isFileName(rest) {
                return true
            }
        }
        if key.hasPrefix(rawLocalMasksKey + "/") {
            let rest = key.dropFirst(rawLocalMasksKey.count + 1)
            if !rest.contains("/"), SideFileExtension.rawLocalMask.isFileName(rest) {
                return true
            }
        }
        return profileKeys.contains { key.hasPrefix($0 + "/") }
    }

    // MARK: - Resolution

    /// The path made absolute, with `.`/`..` applied and every existing
    /// symlink replaced by its target (at most 40 of them, above macOS's
    /// own limit of 32 per lookup), so a dangling link and a tail that does not exist resolve too.
    static func resolve(_ path: String, home: String, currentDirectory: String) -> String {
        var absolute = path
        if absolute == "~" || absolute.hasPrefix("~/") {
            absolute = home + absolute.dropFirst()
        } else if absolute.hasPrefix("~") {
            absolute = (absolute as NSString).expandingTildeInPath
        }
        if !absolute.hasPrefix("/") { absolute = currentDirectory + "/" + absolute }
        var pending = Array(absolute.split(separator: "/").map(String.init).reversed())
        var resolved: [String] = []
        var hops = 0
        while let component = pending.popLast() {
            if component == "." { continue }
            if component == ".." {
                _ = resolved.popLast()
                continue
            }
            let candidate = "/" + (resolved + [component]).joined(separator: "/")
            if hops < 40,
               let target = try? FileManager.default.destinationOfSymbolicLink(atPath: candidate) {
                hops += 1
                if target.hasPrefix("/") { resolved = [] }
                pending.append(contentsOf: target.split(separator: "/").map(String.init).reversed())
                continue
            }
            resolved.append(component)
        }
        return "/" + resolved.joined(separator: "/")
    }

    /// The comparison form: canonical Unicode, lower case.
    private static func key(_ path: String) -> String {
        path.precomposedStringWithCanonicalMapping.lowercased()
    }

    private static func isWithin(_ key: String, _ root: String) -> Bool {
        key == root || key.hasPrefix(root + "/")
    }

    /// `path` with the home prefix shown as `~`.
    private static func shown(_ path: String, home: String) -> String {
        let resolvedHome = resolve(home, home: home, currentDirectory: "/")
        for prefix in [home, resolvedHome] where !prefix.isEmpty && prefix != "/" {
            if path == prefix { return "~" }
            if path.hasPrefix(prefix + "/") { return "~" + path.dropFirst(prefix.count) }
        }
        return path
    }

    // MARK: - Start check

    /// nil when every catalog argument named like a path (`path`, `paths`,
    /// `files`, `input`, `destination`, `workspace`, `…_path`, at any depth
    /// of `properties`/`items`) is declared in `toolPaths` at the same key
    /// path or in `nonFilePathArguments`, every `toolPaths` tool has a
    /// handler, and each declaration's first key is a property of its tool
    /// in the catalog (when the catalog has the tool); else a message.
    static func catalogFailure(_ catalog: String) -> String? {
        guard
            let tools = (try? JSONSerialization.jsonObject(with: Data(catalog.utf8)))
                as? [[String: Any]]
        else { return "the tool catalog did not parse as an array of tool objects" }
        var undeclared: [String] = []
        var stale: [String] = []
        var schemas: [String: [String: Any]] = [:]
        for tool in tools {
            guard let name = tool["name"] as? String else { continue }
            let schema = tool["inputSchema"] as? [String: Any] ?? [:]
            schemas[name] = schema
            let declared = Set((toolPaths[name] ?? []).map { $0.keyPath })
            let exempt = nonFilePathArguments[name] ?? []
            for keyPath in pathLikeKeyPaths(in: schema, prefix: []) {
                if declared.contains(keyPath) { continue }
                if keyPath.count == 1, exempt.contains(keyPath[0]) { continue }
                undeclared.append("\(name).\(keyPath.joined(separator: "."))")
            }
        }
        for (tool, entries) in toolPaths {
            guard AgentServer.handlers[tool] != nil else {
                stale.append("\(tool) (no handler)")
                continue
            }
            guard let schema = schemas[tool] else { continue }
            let properties = schema["properties"] as? [String: Any] ?? [:]
            for entry in entries where properties[entry.keyPath[0]] == nil {
                stale.append("\(tool).\(entry.keyPath.joined(separator: "."))")
            }
        }
        var clauses: [String] = []
        if !undeclared.isEmpty {
            clauses.append("path arguments not declared in AgentPathGuard.toolPaths: "
                + undeclared.sorted().joined(separator: ", "))
        }
        if !stale.isEmpty {
            clauses.append("AgentPathGuard.toolPaths entries the catalog does not have: "
                + stale.sorted().joined(separator: ", "))
        }
        return clauses.isEmpty ? nil : clauses.joined(separator: "; ")
    }

    private static func pathLikeKeyPaths(in schema: [String: Any], prefix: [String])
        -> [[String]] {
        var found: [[String]] = []
        if let properties = schema["properties"] as? [String: Any] {
            for (name, value) in properties {
                let keyPath = prefix + [name]
                if pathLikeNames.contains(name) || name.hasSuffix("_path") {
                    found.append(keyPath)
                }
                if let child = value as? [String: Any] {
                    found += pathLikeKeyPaths(in: child, prefix: keyPath)
                }
            }
        }
        if let items = schema["items"] as? [String: Any] {
            found += pathLikeKeyPaths(in: items, prefix: prefix)
        }
        return found
    }
}
