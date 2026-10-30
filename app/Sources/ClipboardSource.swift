import AppKit

// `Clipboard.writeItem` writes a `ClipboardSource` beside every copy Doctored makes (Copy, Copy
// Merged, Cut and the layers copy, UI and MCP) in the same pasteboard item, under
// `ClipboardSource.type`; another app's write clears it with the item. `PasteContext.inPlace` reads
// it (`ClipboardSource.read(Clipboard.pasteboard)`; nil for MCP `payload_path`, a `.drcb` file
// has no source canvas) and `get_clipboard` replies its canvas as `source`.

/// The source canvas a copy was made on, so Paste in Place
/// (⇧⌘V) into a canvas of another size lands at the same relative place.
///
/// Measured against Photoshop 26.10.0 (`scripts/photoshop-probe/paste_placement.py`,
/// eight size cases, 2026-10-08): a Paste in Place across canvas sizes keeps
/// the copy's CENTRE at the same fraction of the canvas, the corner floored —
/// a 16 × 16 copy at (8, 8) of 64 × 48 lands at (42, 42) in 200 × 150, at
/// (17, 25) in 100 × 100 and at (9, 9) in 70 × 51; a 15 × 13 copy at (7, 9)
/// lands at (37, 41) in 200 × 150. The view and a destination selection do
/// not move it.
struct ClipboardSource: Equatable {
    static let type = NSPasteboard.PasteboardType("com.kgalligan.doctored.clipboard-source")

    /// The type a build before the Rasterize-to-Doctored rename wrote; read, never written.
    static let legacyType = NSPasteboard.PasteboardType("com.kgalligan.rasterize.clipboard-source")

    /// The source canvas at copy time.
    let width: Int
    let height: Int
    /// `identity(of:)` of the source document.
    let document: String

    init(document: ImageDocument, canvas: RasterDocument) {
        width = canvas.width
        height = canvas.height
        self.document = Self.identity(of: document)
    }

    private init(width: Int, height: Int, document: String) {
        self.width = width
        self.height = height
        self.document = document
    }

    /// UTF-8 JSON `{"canvas": [w, h], "document": id}`.
    var data: Data {
        let object: [String: Any] = ["canvas": [width, height], "document": document]
        return (try? JSONSerialization.data(withJSONObject: object)) ?? Data()
    }

    /// The source written beside the pasteboard's current item; nil when it
    /// is absent or malformed.
    static func read(_ pasteboard: NSPasteboard) -> ClipboardSource? {
        guard let data = pasteboard.data(forType: type) ?? pasteboard.data(forType: legacyType),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let canvas = object["canvas"] as? [Int], canvas.count == 2,
              canvas[0] > 0, canvas[1] > 0,
              let document = object["document"] as? String
        else { return nil }
        return ClipboardSource(width: canvas[0], height: canvas[1], document: document)
    }

    /// One identity per open document for this process's life: a
    /// globally unique string, held weakly against the document.
    private static let identities = NSMapTable<ImageDocument, NSString>.weakToStrongObjects()

    static func identity(of document: ImageDocument) -> String {
        if let known = identities.object(forKey: document) { return known as String }
        let made = ProcessInfo.processInfo.globallyUniqueString
        identities.setObject(made as NSString, forKey: document)
        return made
    }

    /// `get_clipboard`'s `source`.
    var json: [String: Any] { ["width": width, "height": height] }

    /// The "same relative location" (`relative_floor`, the one of
    /// seven candidates that matched all eight measured size cases): the
    /// copy's centre scaled by destination / source on each axis, its
    /// corner the exact rational ((2x + w)·W′ − w·W) / (2W) floored. The
    /// candidates it beat — rounding half up, half even or up, rounding the
    /// centre first, the source coordinates and the canvas centre — each
    /// missed at least one case (70 × 51, 71 × 50, 74 × 48 and the odd
    /// 15 × 13 copy decide between them).
    static func relativeOffset(of origin: (x: Int, y: Int), size: (width: Int, height: Int),
                               from source: (width: Int, height: Int),
                               into canvas: (width: Int, height: Int)) -> (x: Int, y: Int) {
        func axis(_ x: Int, _ w: Int, _ from: Int, _ to: Int) -> Int {
            guard from > 0 else { return x }
            let numerator = (2 * x + w) * to - w * from
            let denominator = 2 * from
            let quotient = numerator / denominator
            return numerator % denominator < 0 ? quotient - 1 : quotient
        }
        return (axis(origin.x, size.width, source.width, canvas.width),
                axis(origin.y, size.height, source.height, canvas.height))
    }
}
