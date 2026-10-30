import Foundation

/// What File ▸ Quick Export does with no setup: the built-in preset it
/// falls back to when the library has no default, and the free name it takes
/// when the one the pattern produced is already a file.
///
/// The shared delivery is `ImageDocument.quickExport` (`ExportWorkspace.swift`);
/// this file only decides WHICH preset and WHICH name. A recorded step is not
/// changed by either: it still carries the resolved settings, destination and
/// pattern, and replaying it refuses a taken name as it always has.

extension ExportPreset {
    /// The preset Quick Export uses when no default is set: a PNG named after
    /// the document, beside the document's own file, or in `~/Downloads` for
    /// a document that has never been saved. It is built per call and never
    /// written to the library, so it cannot be listed, edited or deleted.
    static func builtInQuickExport(for document: ImageDocument) throws -> ExportPreset {
        let folder = document.fileURL?.deletingLastPathComponent().path ?? "~/Downloads"
        return ExportPreset(
            name: "PNG", settings: try ExportSettings.decode(["format": "png"]),
            destination: folder, pattern: "{name}")
    }
}

extension ExportPresetLibrary {
    /// The preset File ▸ Quick Export and an argument-less `quick_export`
    /// deliver with: the library's default when one is set, else the built-in.
    /// A default naming a preset that no longer exists is the library's own
    /// refusal, not a silent fallback.
    static func quickExportPreset(for document: ImageDocument) throws -> ExportPreset {
        guard let name = try defaultPresetName() else {
            return try ExportPreset.builtInQuickExport(for: document)
        }
        return try named(name)
    }
}

/// What Quick Export does when the name its pattern produced is taken.
enum QuickExportCollision {
    /// Refuse and write nothing: a recorded step, a batch delivery and an MCP
    /// call that spells out settings + destination + pattern.
    case refuse
    /// Take the first free `-2`, `-3`, … name: the menu command and an MCP
    /// call that names a preset or passes nothing.
    case suffix
}

enum QuickExportNaming {
    /// The last suffix tried. A folder holding this many copies of one name
    /// is a mistake to report, not a number to keep counting past.
    static let lastSuffix = 999

    /// `url` when no file is there, else the first of `<base>-2.<ext>` …
    /// `<base>-999.<ext>` that is free. Past that it refuses, naming the
    /// folder. Nothing is created: the caller still writes without overwrite.
    static func firstFree(_ url: URL) throws -> URL {
        let files = FileManager.default
        guard files.fileExists(atPath: url.path) else { return url }
        let folder = url.deletingLastPathComponent()
        let base = url.deletingPathExtension().lastPathComponent
        let ext = url.pathExtension
        for number in 2...lastSuffix {
            let candidate = folder.appendingPathComponent("\(base)-\(number)")
                .appendingPathExtension(ext)
            if !files.fileExists(atPath: candidate.path) { return candidate }
        }
        throw DoctoredCoreError(
            message: "\(folder.path) already holds \(url.lastPathComponent) and every name from "
                + "\(base)-2 to \(base)-\(lastSuffix). Quick Export never overwrites a file.")
    }
}
