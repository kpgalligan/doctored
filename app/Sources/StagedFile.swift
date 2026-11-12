import Foundation
import Darwin

// The staged names a save or export writes beside its destination, and the sweep of the ones a
// killed process left. Foundation and Darwin only, like `DurableFile.swift`.

/// A save or export writes its file as `<prefix><pid>-<UUID>` in the destination's folder and
/// publishes it over the destination (`DurableFile.publish`); a `defer` removes it on any failure.
/// A process that is killed or loses power runs no `defer`, so `sweep(beside:)` runs before each
/// staging and removes what a dead process left there.
///
/// A name is removed only when the WHOLE name is one of `Kind`'s, it is what that kind stages (a
/// regular file, or a directory for a package kind; never a symlink), and either its pid is not
/// this process's and `kill(pid, 0)` answers ESRCH (no such process), or it is a UUID-only name of
/// a file kind, which builds before the pid was added wrote, whose modification time is more than
/// `legacyAge` old. Another running instance's file is safe under both rules: its pid is alive,
/// and a file still being written was modified moments ago. A name with anything after the UUID
/// (the core's `….dr-tmp-<pid>-<seq>` inside a staged path) is the core's to judge
/// (`stale_temp.rs`), in the same save. On a folder shared between machines another machine's pid
/// reads as dead here; that save then fails at its publish with the previous file intact.
enum StagedFile {
    enum Kind: String, CaseIterable {
        case save = ".doctored-save-"
        case precisionSave = ".doctored-precision-save-"
        case export = ".doctored-export-"
        case cmykIntermediate = ".doctored-cmyk-"
        case pdf = ".dr-pdf-"
        /// A `.drsnapshot` package, staged as a directory. Older builds named it
        /// `.snapshot-<UUID>`, too plain a name to delete a directory by, so those are not swept.
        case snapshot = ".doctored-snapshot-"

        /// What follows the UUID: the CMYK intermediate is a TIFF the host encoder reads back.
        var suffix: String { self == .cmykIntermediate ? ".tif" : "" }

        /// Staged as a directory rather than a file.
        var isPackage: Bool { self == .snapshot }
    }

    /// How old a UUID-only staged name must be before the sweep removes it.
    static let legacyAge: TimeInterval = 24 * 60 * 60

    /// `<kind's prefix><pid>-`, what `DurableFile.writeTemporary` puts before its UUID.
    static func prefix(_ kind: Kind) -> String {
        kind.rawValue + "\(getpid())-"
    }

    /// A fresh staged URL beside `destination`: `<prefix><pid>-<UUID><suffix>`.
    static func url(beside destination: URL, kind: Kind) -> URL {
        destination.deletingLastPathComponent()
            .appendingPathComponent(prefix(kind) + UUID().uuidString + kind.suffix)
    }

    /// Removes, best effort, what a dead process staged in `destination`'s folder.
    static func sweep(beside destination: URL) {
        let folder = destination.deletingLastPathComponent().path
        guard let names = try? FileManager.default.contentsOfDirectory(atPath: folder) else { return }
        let now = Date().timeIntervalSince1970
        for name in names {
            guard let (kind, owner) = owner(of: name) else { continue }
            let path = (folder as NSString).appendingPathComponent(name)
            var info = stat()
            let staged = kind.isPackage ? S_IFDIR : S_IFREG
            guard lstat(path, &info) == 0, info.st_mode & S_IFMT == staged else { continue }
            let stale: Bool
            switch owner {
            case .process(let pid):
                stale = pid != getpid() && kill(pid, 0) == -1 && errno == ESRCH
            case .unknown:
                stale = !kind.isPackage
                    && now - TimeInterval(info.st_mtimespec.tv_sec) > legacyAge
            }
            guard stale else { continue }
            if kind.isPackage {
                try? FileManager.default.removeItem(atPath: path)
            } else {
                _ = unlink(path)
            }
        }
    }

    /// Who staged a file: the pid in its name, or `unknown` for a UUID-only name.
    private enum Owner {
        case process(pid_t)
        case unknown
    }

    /// The kind and owner of a name that is exactly `<prefix>[<pid>-]<UUID><suffix>`; nil for
    /// any other.
    private static func owner(of name: String) -> (Kind, Owner)? {
        for kind in Kind.allCases where name.hasPrefix(kind.rawValue) && name.hasSuffix(kind.suffix) {
            guard name.count > kind.rawValue.count + kind.suffix.count else { return nil }
            let rest = String(name.dropFirst(kind.rawValue.count).dropLast(kind.suffix.count))
            if isUUID(rest) { return (kind, .unknown) }
            guard let dash = rest.firstIndex(of: "-") else { return nil }
            let digits = rest[..<dash]
            guard !digits.isEmpty, digits.allSatisfy({ $0.isASCII && $0.isNumber }),
                  let pid = Int32(digits), pid > 0,
                  isUUID(String(rest[rest.index(after: dash)...])) else { return nil }
            return (kind, .process(pid))
        }
        return nil
    }

    /// `text` is a UUID as `UUID().uuidString` spells it.
    private static func isUUID(_ text: String) -> Bool {
        UUID(uuidString: text)?.uuidString == text
    }
}
