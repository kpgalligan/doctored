import Foundation

/// Where Edit ▸ Color Settings' keys live (#547). Without `DR_COLOR_SETTINGS_DIR` they are ordinary
/// `UserDefaults.standard` keys, as before. With it — every owned test instance sets it to a
/// private folder through `scripts/verification_isolation.py`'s `PRESET_DIRECTORIES` — a read
/// takes the launch arguments first (`-ColorWorkingSpace srgb` and a drive's own pins still win),
/// then `<dir>/color-settings.plist`, else unset; a write goes to that file only. So a person's own
/// Color Settings never reach a test instance and a test never writes them.
///
/// The file is read once and cached; `set` and `remove` update the cache and rewrite the file
/// atomically, creating the folder on the first write. A preference, not document state: no undo,
/// no MCP setter (the view-preference exemption, `app/CLAUDE.md`).
enum ColorSettingsStore {
    static let directoryEnvironment = "DR_COLOR_SETTINGS_DIR"
    static let fileName = "color-settings.plist"

    private static let lock = NSLock()
    private static var cache: [String: Any]?

    /// The private file, or nil when the variable is unset or empty.
    private static var fileURL: URL? {
        guard let folder = ProcessInfo.processInfo.environment[directoryEnvironment],
              !folder.isEmpty else { return nil }
        let expanded = (folder as NSString).expandingTildeInPath
        return URL(fileURLWithPath: expanded, isDirectory: true).appendingPathComponent(fileName)
    }

    /// The launch arguments' value for `key` (`-Key value` on the command line).
    private static func argument(_ key: String) -> Any? {
        UserDefaults.standard.volatileDomain(forName: UserDefaults.argumentDomain)[key]
    }

    /// The private file's contents, read once (callers hold `lock`).
    private static func stored(_ url: URL) -> [String: Any] {
        if let cache { return cache }
        var loaded: [String: Any] = [:]
        if let data = try? Data(contentsOf: url),
           let plist = try? PropertyListSerialization.propertyList(from: data, format: nil),
           let dictionary = plist as? [String: Any] {
            loaded = dictionary
        }
        cache = loaded
        return loaded
    }

    /// The raw value for `key` under the private store: arguments, then the file.
    private static func value(_ key: String, in url: URL) -> Any? {
        if let value = argument(key) { return value }
        lock.lock()
        defer { lock.unlock() }
        return stored(url)[key]
    }

    static func string(forKey key: String) -> String? {
        guard let url = fileURL else { return UserDefaults.standard.string(forKey: key) }
        switch value(key, in: url) {
        case let string as String: return string
        case let number as NSNumber: return number.stringValue
        default: return nil
        }
    }

    /// nil when unset; otherwise `UserDefaults.bool(forKey:)`'s reading — a string is true for
    /// "YES" or "true" (any case) or a non-zero integer, so an argv `-ColorBPC NO` reads false.
    static func bool(forKey key: String) -> Bool? {
        guard let url = fileURL else {
            let defaults = UserDefaults.standard
            return defaults.object(forKey: key) == nil ? nil : defaults.bool(forKey: key)
        }
        switch value(key, in: url) {
        case let flag as Bool: return flag
        case let number as NSNumber: return number.boolValue
        case let string as String:
            let lowered = string.lowercased()
            return lowered == "yes" || lowered == "true" || (Int(string).map { $0 != 0 } ?? false)
        default: return nil
        }
    }

    static func set(_ value: Any, forKey key: String) {
        guard let url = fileURL else {
            UserDefaults.standard.set(value, forKey: key)
            return
        }
        lock.lock()
        defer { lock.unlock() }
        var contents = stored(url)
        contents[key] = value
        cache = contents
        write(contents, to: url)
    }

    static func remove(forKey key: String) {
        guard let url = fileURL else {
            UserDefaults.standard.removeObject(forKey: key)
            return
        }
        lock.lock()
        defer { lock.unlock() }
        var contents = stored(url)
        contents.removeValue(forKey: key)
        cache = contents
        write(contents, to: url)
    }

    /// The whole file, atomically; a failure is logged and the cache keeps the value for this run.
    private static func write(_ contents: [String: Any], to url: URL) {
        do {
            try FileManager.default.createDirectory(at: url.deletingLastPathComponent(),
                                                    withIntermediateDirectories: true)
            let data = try PropertyListSerialization.data(fromPropertyList: contents,
                                                          format: .xml, options: 0)
            try data.write(to: url, options: .atomic)
        } catch {
            NSLog("Color Settings could not be written to %@: %@", url.path, String(describing: error))
        }
    }
}
