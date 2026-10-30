import Foundation

/// The one-time move of the app's data from its name before the rename.
///
/// Rasterize kept its state in `~/Library/Application Support/Rasterize` and
/// its preferences in the `com.kgalligan.Rasterize` defaults domain; Doctored
/// keeps them in `…/Doctored` and `com.kgalligan.Doctored`. The first
/// ordinary launch moves the folder (one `moveItem`, the same volume, so the
/// API key, every preset library, Actions, recovery and the model store go
/// across together) and copies the old domain's keys into the new one once,
/// behind `markerKey`. A library folder a later feature rename renamed moves
/// the same way, once, unless its new folder already exists (`moveRenamedFolder`
/// below). It never deletes the old domain, and it never runs in
/// an automation launch, so no test, drive or gate step can touch the
/// owner's real folders or domains: they call `migrate` with scratch
/// folders and suite names only.
enum DataMigration {
    struct Outcome: Equatable {
        var movedFolder: Bool
        var copiedDefaults: Bool
    }

    /// Set in the new domain once its keys have been copied.
    static let markerKey = "DoctoredMigratedFromRasterize"

    /// True for a launch a script or probe owns: `DR_BACKGROUND` or
    /// `DR_AUTOMATION_ALERTS` set, or an agent socket other than the default.
    /// Any non-empty variable with the pre-rename `RZ_` prefix counts too: the
    /// app reads none of them, but a script written before the rename still
    /// sets them, and it must never migrate the owner's folder.
    static func isAutomationLaunch(_ environment: [String: String],
                                   defaultSocketPath: String?) -> Bool {
        if environment.contains(where: { $0.key.hasPrefix("RZ_") && !$0.value.isEmpty }) {
            return true
        }
        if environment["DR_BACKGROUND"]?.isEmpty == false { return true }
        if environment["DR_AUTOMATION_ALERTS"]?.isEmpty == false { return true }
        if let socket = environment[AgentServer.socketEnvironmentKey], !socket.isEmpty,
           socket != defaultSocketPath {
            return true
        }
        return false
    }

    /// Moves `oldFolder` to `newFolder` when only the old one holds data,
    /// then copies `oldDomain`'s keys `newDomain` lacks, once. A failure is
    /// logged and leaves both folders as they were; nothing is thrown.
    @discardableResult
    static func migrate(oldFolder: URL, newFolder: URL, oldDomain: String, newDomain: String,
                        fileManager: FileManager = .default) -> Outcome {
        var outcome = Outcome(movedFolder: false, copiedDefaults: false)
        if fileManager.fileExists(atPath: oldFolder.path), isAbsent(newFolder, fileManager) {
            do {
                if fileManager.fileExists(atPath: newFolder.path) {
                    try fileManager.removeItem(at: newFolder)
                }
                try fileManager.moveItem(at: oldFolder, to: newFolder)
                outcome.movedFolder = true
            } catch {
                NSLog("Doctored data migration: could not move %@ to %@: %@",
                      oldFolder.path, newFolder.path, error.localizedDescription)
            }
        }
        let defaults = UserDefaults.standard
        var current = defaults.persistentDomain(forName: newDomain) ?? [:]
        if current[markerKey] == nil {
            for (key, value) in defaults.persistentDomain(forName: oldDomain) ?? [:]
            where current[key] == nil {
                current[key] = value
            }
            current[markerKey] = true
            defaults.setPersistentDomain(current, forName: newDomain)
            outcome.copiedDefaults = true
        }
        return outcome
    }

    /// Moves a library folder a feature rename renamed (2026-10-29: `Camera Raw Filter Presets`
    /// → `Develop Filter Presets`, `LegacyNames`) when `oldFolder` exists and `newFolder` does
    /// not exist at all: any existing new folder, even an empty one, wins and nothing is
    /// touched. True when it moved; a failure is logged and returns false.
    @discardableResult
    static func moveRenamedFolder(from oldFolder: URL, to newFolder: URL,
                                  fileManager: FileManager = .default) -> Bool {
        guard fileManager.fileExists(atPath: oldFolder.path),
              !fileManager.fileExists(atPath: newFolder.path) else { return false }
        do {
            try fileManager.moveItem(at: oldFolder, to: newFolder)
            return true
        } catch {
            NSLog("Doctored data migration: could not move %@ to %@: %@",
                  oldFolder.path, newFolder.path, error.localizedDescription)
            return false
        }
    }

    /// The new folder counts as absent when it does not exist, is empty, or
    /// holds only what the MCP helper makes before it launches the app
    /// (`mcp-proxy/src/launch.rs`): `Agent/` with nothing but
    /// `agent.launch.lock` and `agent.sock` in it. Anything else is data.
    private static func isAbsent(_ folder: URL, _ fileManager: FileManager) -> Bool {
        guard fileManager.fileExists(atPath: folder.path) else { return true }
        guard let entries = try? fileManager.contentsOfDirectory(atPath: folder.path) else {
            return false
        }
        if entries.isEmpty { return true }
        guard entries == ["Agent"] else { return false }
        let agent = folder.appendingPathComponent("Agent", isDirectory: true)
        guard let inside = try? fileManager.contentsOfDirectory(atPath: agent.path) else {
            return false
        }
        return Set(inside).isSubset(of: ["agent.launch.lock", "agent.sock"])
    }

    /// The launch-time call (`main.swift`): skipped for an automation launch.
    static func runAtLaunch() {
        if isAutomationLaunch(ProcessInfo.processInfo.environment,
                              defaultSocketPath: AgentServer.defaultSocketPath) {
            return
        }
        guard let support = FileManager.default.urls(
            for: .applicationSupportDirectory, in: .userDomainMask).first else { return }
        let doctored = support.appendingPathComponent("Doctored", isDirectory: true)
        migrate(oldFolder: support.appendingPathComponent("Rasterize", isDirectory: true),
                newFolder: doctored,
                oldDomain: "com.kgalligan.Rasterize", newDomain: "com.kgalligan.Doctored")
        moveRenamedFolder(
            from: doctored.appendingPathComponent("Camera Raw Filter Presets", isDirectory: true),
            to: doctored.appendingPathComponent("Develop Filter Presets", isDirectory: true))
    }
}
