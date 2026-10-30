import Foundation

/// The four side files Doctored writes beside a document — snapshot packages,
/// action bundles, clipboard payloads and RAW local masks — and their two
/// extensions each. A writer that picks a name uses `current`; a writer given
/// an explicit path and every reader accept `current` or `legacy`, because a
/// file written before the rename (R2) carries the old extension and must keep
/// opening. `legacy` is the only Swift spelling of the four old extensions.
enum SideFileExtension: CaseIterable {
    case snapshot, actionBundle, clipboardPayload, rawLocalMask

    /// The extension new files are written with.
    var current: String {
        switch self {
        case .snapshot: return "drsnapshot"
        case .actionBundle: return "draction"
        case .clipboardPayload: return "drcb"
        case .rawLocalMask: return "drmask"
        }
    }

    /// The extension files written before the rename carry; read, never written.
    var legacy: String {
        switch self {
        case .snapshot: return "rzsnapshot"
        case .actionBundle: return "rzaction"
        case .clipboardPayload: return "rzcb"
        case .rawLocalMask: return "rzmask"
        }
    }

    /// Whether `url`'s extension (case-insensitively) is `current` or `legacy`.
    func matches(_ url: URL) -> Bool {
        let ext = url.pathExtension.lowercased()
        return ext == current || ext == legacy
    }

    /// Whether `path`'s extension (case-insensitively) is `current` or `legacy`.
    func matches(path: String) -> Bool {
        matches(URL(fileURLWithPath: path))
    }

    /// Whether `name` is a non-empty stem followed by `.current` or `.legacy`
    /// (exact case), the shape a store's own file names take.
    func isFileName<S: StringProtocol>(_ name: S) -> Bool {
        for ext in [current, legacy] {
            let suffix = "." + ext
            if name.count > suffix.count, name.hasSuffix(suffix) { return true }
        }
        return false
    }
}
