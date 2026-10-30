import Foundation

// `ActionAssets` owns the asset table and is the ONLY route from a literal path to a travelling
// `$asset` (`package_action` -> `import_action`); packaging never modifies the library copy and
// recording never rewrites a path.

/// An action's FILE assets: which arguments name a file, how packaging turns
/// a literal path into a `{"$asset": …}` symbol beside a copy of the file,
/// and how an import puts those copies back.
///
/// **The problem.** A step that places an image carries the absolute path the
/// image had when it was recorded. Hand the action to someone else — or move
/// the logo — and the step fails, or worse, opens a different file at the same
/// path. There was no route from "a path" to "a file that travels with the
/// action", which is what made an action un-shareable.
///
/// **The route, and the only one: `package_action` → `import_action`.**
/// Packaging copies each named file into `<slug>.draction/assets/` and
/// rewrites that argument to `{"$asset": "<basename>"}` IN THE BUNDLE — the
/// library copy is never modified, so packaging an action twice produces the
/// same bundle and never mutates what the user has. Importing saves the
/// bundle's `action.json` through `ActionLibrary.save` and copies `assets/*`
/// into that action's own assets folder. Recording stays LITERAL: nothing
/// turns a path into a symbol behind a user's back.
///
/// **Collisions are resolved per SOURCE PATH, not per name.** Two different
/// folders' `logo.png` are two different pictures, so the second becomes
/// `logo-2.png` and its step alone is rewritten to that name; the SAME path
/// used by two steps is one entry both point at.
enum ActionAssets {
    /// tool → the argument keys that name a file on disk.
    ///
    /// `quick_export.destination` is deliberately ABSENT: it is a per-machine
    /// output FOLDER, not an input the action depends on, and copying it into
    /// a bundle would be copying the user's Desktop. The 11A containment rule
    /// says the same thing from the other side — a delivery writes where the
    /// person running it chose.
    static let table: [String: [String]] = [
        "place_image": ["path"],
        "add_live_photo_layer": ["path"],
        // Only when the step actually names a file: `profile: "srgb"` carries
        // a built-in by name and has no path at all.
        "convert_profile": ["path"],
        "assign_profile": ["path"],
        // Phase 12G-2: a recorded paste's landed RZCB payload.
        "paste": ["payload_path"],
    ]

    /// The keys of `step` that name a file, after the per-tool condition.
    static func assetKeys(for step: ActionStep) -> [String] {
        guard let keys = table[step.tool] else { return [] }
        if step.tool == "convert_profile" || step.tool == "assign_profile" {
            guard (step.arguments["profile"] as? String) == "file" else { return [] }
        }
        return keys.filter { step.arguments[$0] != nil }
    }

    /// True for a name a `$asset` symbol may carry: ONE path component, no
    /// separator, no `..`, no leading dot. It is what keeps a bundle from
    /// naming anything outside its own assets folder — the same containment
    /// rule `doc_export::expand_name_pattern` enforces on an output pattern.
    static func isValidFileName(_ name: String) -> Bool {
        guard !name.isEmpty, !name.hasPrefix("."), name != "..",
              !name.contains("/"), !name.contains(":"),
              !name.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) })
        else { return false }
        return (name as NSString).lastPathComponent == name
    }

    /// The bundle folder one action packages into.
    static func bundleName(for action: Action) -> String {
        ActionLibrary.slug(for: action.name) + "." + SideFileExtension.actionBundle.current
    }

    // MARK: - Diagnosis

    /// The assets this action references that are not on disk, as the full
    /// paths a reader has to go and look at. Empty when everything resolves —
    /// including for an action that references none.
    ///
    /// `list_actions {name}` reports it, and the Actions window greys Play on
    /// it, so a broken bundle is visible BEFORE a run changes anything.
    static func missing(in action: Action) -> [String] {
        guard let folder = ActionLibrary.assetsDirectory(forActionNamed: action.name) else {
            // An action with no file in the library (an unsaved recording)
            // has no assets folder; a `$asset` in it is unresolvable and says
            // so at run time, naming the action.
            return referencedNames(in: action).map { "\(action.name)/assets/\($0)" }
        }
        var out: [String] = []
        for name in referencedNames(in: action) {
            let url = folder.appendingPathComponent(name)
            if !FileManager.default.fileExists(atPath: url.path) { out.append(url.path) }
        }
        return out
    }

    /// Every `{"$asset": …}` name this action references, de-duplicated and
    /// sorted.
    static func referencedNames(in action: Action) -> [String] {
        var names: Set<String> = []
        for step in action.steps { collect(step.arguments, into: &names) }
        return names.sorted()
    }

    private static func collect(_ value: Any, into names: inout Set<String>) {
        if ActionSymbol.looksLikeSymbol(value) {
            if case .success(.asset(let file)) = ActionSymbol.parse(value) { names.insert(file) }
            return
        }
        if let object = value as? [String: Any] {
            for element in object.values { collect(element, into: &names) }
            return
        }
        if let array = value as? [Any] {
            for element in array { collect(element, into: &names) }
        }
    }

    // MARK: - Packaging

    /// One failure type for both directions, so the two tools and the two
    /// window buttons report the same sentence.
    struct Failure: LocalizedError {
        let message: String
        var errorDescription: String? { message }

        init(_ message: String) {
            self.message = message
        }
    }

    /// Writes `<destination>/<slug>.draction/` holding `action.json` and an
    /// `assets/` folder, and answers the bundle's URL and the asset file names
    /// it wrote, in the order they were added.
    ///
    /// Nothing partial is left behind: every source file is resolved and
    /// checked BEFORE a directory is created, so a missing path refuses with
    /// that path in the message and no bundle appears.
    static func package(
        _ action: Action, to destination: URL, overwrite: Bool
    ) throws -> (url: URL, assets: [String]) {
        // 1. Resolve every referenced file, refusing before anything is
        //    created. `sources` is keyed by the RESOLVED source path, which
        //    is what makes "the same file twice is one entry" true.
        var order: [String] = []
        var byPath: [String: String] = [:]
        var used: Set<String> = []
        var rewritten = action
        let library = ActionLibrary.assetsDirectory(forActionNamed: action.name)
        for index in rewritten.steps.indices {
            let step = rewritten.steps[index]
            for key in assetKeys(for: step) {
                let source: URL
                switch step.arguments[key] {
                case let literal as String where !literal.isEmpty:
                    source = URL(fileURLWithPath: (literal as NSString).expandingTildeInPath)
                case let symbol as [String: Any]:
                    guard case .success(.asset(let file)) = ActionSymbol.parse(symbol),
                          let library = library
                    else {
                        throw Failure(
                            "step \(index + 1): its \(key) is neither a path nor an asset this "
                                + "action's library folder holds")
                    }
                    source = library.appendingPathComponent(file)
                default:
                    continue
                }
                guard FileManager.default.fileExists(atPath: source.path) else {
                    throw Failure(
                        "step \(index + 1) needs \(source.path), which is not there — package "
                            + "the action once every file it uses exists")
                }
                let name: String
                if let existing = byPath[source.path] {
                    name = existing
                } else {
                    name = freeName(for: source.lastPathComponent, used: used)
                    used.insert(name.lowercased())
                    byPath[source.path] = name
                    order.append(name)
                }
                rewritten.steps[index].arguments[key] = ["$asset": name]
            }
        }

        // 2. Create, refusing an occupied name unless told otherwise.
        let bundle = destination.appendingPathComponent(bundleName(for: action), isDirectory: true)
        let manager = FileManager.default
        if manager.fileExists(atPath: bundle.path) {
            guard overwrite else {
                throw Failure(
                    "\(bundle.path) already exists — pass overwrite: true to replace it, or "
                        + "choose another folder.")
            }
            try remove(bundle)
        }
        let assets = bundle.appendingPathComponent("assets", isDirectory: true)
        do {
            try manager.createDirectory(at: assets, withIntermediateDirectories: true)
            for (path, name) in byPath {
                try manager.copyItem(
                    at: URL(fileURLWithPath: path), to: assets.appendingPathComponent(name))
            }
            try rewritten.encoded().write(
                to: bundle.appendingPathComponent("action.json"), options: [.atomic])
            try DurableFile.syncTree(bundle)
        } catch {
            // A half-written bundle is worse than none: it looks importable.
            try? manager.removeItem(at: bundle)
            throw Failure("could not write \(bundle.lastPathComponent): \(error.localizedDescription)")
        }
        DurableFile.persistDirectory(destination)
        return (url: bundle, assets: order)
    }

    /// `<stem>-<n>.<ext>` at the first free n ≥ 2, when two DIFFERENT sources
    /// share a basename. Compared case-insensitively for the reason
    /// `BatchRun.claimed` is: the common macOS volume is.
    private static func freeName(for basename: String, used: Set<String>) -> String {
        guard used.contains(basename.lowercased()) else { return basename }
        let stem = (basename as NSString).deletingPathExtension
        let ext = (basename as NSString).pathExtension
        var suffix = 2
        while true {
            let candidate = ext.isEmpty ? "\(stem)-\(suffix)" : "\(stem)-\(suffix).\(ext)"
            if !used.contains(candidate.lowercased()) { return candidate }
            suffix += 1
        }
    }

    // MARK: - Importing

    /// Reads `<path>` (a `.draction` folder), saves its action into the
    /// library and copies its assets beside it. Answers the name it landed
    /// under and the asset files it installed.
    ///
    /// `name` renames on the way in; `overwrite` replaces both the JSON and
    /// the assets folder of a same-named action. Without it a name that is
    /// taken refuses — the same rule `save_action` follows, and for the same
    /// reason: an import must not quietly destroy an action someone built.
    static func importBundle(
        at path: URL, name override: String?, overwrite: Bool
    ) throws -> (name: String, assets: [String]) {
        let manager = FileManager.default
        var isDirectory: ObjCBool = false
        guard manager.fileExists(atPath: path.path, isDirectory: &isDirectory),
              isDirectory.boolValue
        else {
            let bundle = SideFileExtension.actionBundle
            throw Failure("\(path.path) is not a folder — an action bundle is a .\(bundle.current) "
                + "folder (or a legacy .\(bundle.legacy) folder)")
        }
        let json = path.appendingPathComponent("action.json")
        guard let data = try? Data(contentsOf: json) else {
            throw Failure("\(json.path) could not be read")
        }
        var action: Action
        switch Action.decode(data) {
        // The decode refusals surface VERBATIM: a bundle whose action names
        // an unknown tool has to say so by step number, exactly as the same
        // file would in the library.
        case .success(let decoded): action = decoded
        case .failure(let error): throw Failure("\(json.lastPathComponent): \(error.message)")
        }
        if let override = override {
            let trimmed = override.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !trimmed.isEmpty else { throw Failure("an action needs a name") }
            action.name = trimmed
        }
        if !overwrite, ActionLibrary.summary(named: action.name) != nil {
            throw Failure(
                "An action named \"\(action.name)\" already exists — pass overwrite: true to "
                    + "replace it, or import it under another name.")
        }
        do {
            try ActionLibrary.save(action)
        } catch let error as ActionLibrary.Failure {
            throw Failure(error.message)
        }
        guard let folder = ActionLibrary.assetsDirectory(forActionNamed: action.name) else {
            throw Failure("the imported action could not be found in the library")
        }
        // The assets folder is REPLACED, never merged: a stale file from a
        // previous import of a different bundle would be an asset the action
        // no longer references and nothing would ever remove it.
        try remove(folder)
        var installed: [String] = []
        let source = path.appendingPathComponent("assets", isDirectory: true)
        let entries =
            (try? manager.contentsOfDirectory(at: source, includingPropertiesForKeys: nil))
            ?? []
        if !entries.isEmpty {
            do {
                try manager.createDirectory(at: folder, withIntermediateDirectories: true)
                for entry in entries {
                    let file = entry.lastPathComponent
                    guard isValidFileName(file) else { continue }
                    try manager.copyItem(at: entry, to: folder.appendingPathComponent(file))
                    installed.append(file)
                }
            } catch {
                throw Failure("could not install the bundle's assets: \(error.localizedDescription)")
            }
        }
        return (name: action.name, assets: installed.sorted())
    }

    /// Removes a path that may not be there, turning only a REAL failure into
    /// one. `removeItem` throws for "no such file", which is the normal case
    /// at both call sites.
    private static func remove(_ url: URL) throws {
        guard FileManager.default.fileExists(atPath: url.path) else { return }
        do {
            try FileManager.default.removeItem(at: url)
        } catch {
            throw Failure("could not replace \(url.lastPathComponent): \(error.localizedDescription)")
        }
    }
}
