import Foundation

/// File ▸ File Info… — the four fields a person authors about a picture:
/// who made it, the copyright notice, a description and keywords.
///
/// The core owns where they live (`core/src/file_info.rs`): the XMP packet's
/// `dc:creator`, `dc:rights`, `dc:description` and `dc:subject`, with the IIM
/// datasets and three EXIF tags read as fallbacks. They are part of the
/// document's metadata, so an edit is one undo step, a `.rz` keeps it, and a
/// JPEG, PNG or TIFF export carries it. This type is the value the sheet
/// (`FileInfoSheetController`), the editor (`EditorViewController+FileInfo`)
/// and the MCP tools (`AgentServer+FileInfo`) pass around; the limits and
/// every refusal sentence are the core's.
struct FileInfo: Equatable {
    var creator: [String] = []
    var copyright = ""
    var description = ""
    var keywords: [String] = []

    /// The wire form the core reads and writes, and what `get_file_info`
    /// answers: all four keys, always.
    var json: [String: Any] {
        [
            "creator": creator, "copyright": copyright,
            "description": description, "keywords": keywords,
        ]
    }

    /// Reads the wire form. Every key must be present with its own type; the
    /// core never answers anything else, so a failure here is a defect.
    static func decode(_ object: [String: Any]) throws -> FileInfo {
        guard let creator = object["creator"] as? [String],
              let copyright = object["copyright"] as? String,
              let description = object["description"] as? String,
              let keywords = object["keywords"] as? [String]
        else { throw DoctoredCoreError(message: "The file info could not be read.") }
        return FileInfo(
            creator: creator, copyright: copyright, description: description, keywords: keywords)
    }

    /// The fields whose value differs from `other`, in wire form: what a
    /// recorded step carries, so replaying it leaves the rest of the target
    /// document's file info alone.
    func changes(from other: FileInfo) -> [String: Any] {
        var out: [String: Any] = [:]
        if creator != other.creator { out["creator"] = creator }
        if copyright != other.copyright { out["copyright"] = copyright }
        if description != other.description { out["description"] = description }
        if keywords != other.keywords { out["keywords"] = keywords }
        return out
    }

    // MARK: - The sheet's one-line list fields

    /// Creators as the Author field shows them, `;` between names.
    var creatorText: String { creator.joined(separator: "; ") }

    /// Keywords as the Keywords field shows them.
    var keywordsText: String { keywords.joined(separator: ", ") }

    /// Text as a sheet field holds it, made into text the core accepts. The
    /// Description view takes a Tab as a character and a paste can carry a
    /// carriage return, and the core refuses every control character but a
    /// line feed in the description. This is the core's `tidy`
    /// (`core/src/file_info.rs`, the master copy), which it runs on what it
    /// reads: a CR LF pair or a bare CR becomes a line feed where line feeds
    /// are kept, every other control character becomes a space, and the ends
    /// are trimmed.
    static func typed(_ text: String, lineFeeds: Bool) -> String {
        // Scalars, not Characters: Swift reads a CR LF pair as one Character.
        var out = String.UnicodeScalarView()
        var afterCarriageReturn = false
        for scalar in text.unicodeScalars {
            let pairedLineFeed = afterCarriageReturn && scalar == "\n"
            afterCarriageReturn = scalar == "\r"
            if pairedLineFeed { continue }
            let unified: Unicode.Scalar = scalar == "\r" ? "\n" : scalar
            let control = unified.properties.generalCategory == .control
            out.append(control && !(lineFeeds && unified == "\n") ? " " : unified)
        }
        return String(out).trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// A one-line list field split into entries: trimmed, blanks dropped.
    /// Author splits at `;` only (a name may hold a comma); Keywords splits
    /// at `,` and `;`.
    static func entries(_ text: String, separators: String) -> [String] {
        text.components(separatedBy: CharacterSet(charactersIn: separators))
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
    }
}
