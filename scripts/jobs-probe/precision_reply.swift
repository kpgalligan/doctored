import AppKit

/// "precision reply modes": `get_precision` and the document summary name each document's own
/// mode and depth. For every document kind (`colourModeRows`) `get_precision` is asked through the
/// in-process MCP entry, and its `mode` and `bits_per_channel` and the summary's `precision` (the
/// reply's `document`, and `get_document`'s own field, which merges the same summary) are required
/// from this file's table, written out as literals rather than computed from the property under
/// test. A Bitmap, Indexed Color or CMYK document must also answer its own gate's lists:
/// `get_precision`'s `supported_tools` (and for Bitmap and Indexed its `color_mode` and
/// `supported_menu_items`) and `get_document`'s `precision_support.supported_tools` are the mode's
/// own sets. Every row is checked before the case fails, so a red run lists each. No window is
/// shown on screen and no event is synthesized.
func verifyPrecisionReplyModes() throws {
    let tag = "[precision reply modes]"
    let expected: [String: (mode: String, bits: Int, summary: String)] = [
        "RGB 8": ("rgba8", 8, "rgba8"),
        "RGB 16": ("rgba16", 16, "rgba16"),
        "RGB 32": ("rgba32", 32, "rgba32_float"),
        "Grayscale 8": ("gray8", 8, "gray8"),
        "Grayscale 16": ("gray16", 16, "gray16"),
        "Grayscale 32": ("gray32", 32, "gray32"),
        "Duotone 8": ("gray8", 8, "gray8"),
        "Duotone 16": ("gray16", 16, "gray16"),
        "Lab 8": ("lab8", 8, "lab8"),
        "Lab 16": ("lab16", 16, "lab16"),
        "CMYK 8": ("cmyk8", 8, "cmyk8"),
        "CMYK 16": ("cmyk16", 16, "cmyk16"),
        "Indexed": ("indexed8", 8, "indexed8"),
        "Bitmap": ("bitmap1", 1, "bitmap1"),
    ]
    // The rows whose gate answers its own lists, with the literal colour mode the reply names.
    let gated: [String: (colorMode: String, tools: Set<String>, menus: Set<String>?)] = [
        "Indexed": ("indexed", IndexedDocument.tools, IndexedDocument.selectors),
        "Bitmap": ("bitmap", BitmapDocument.tools, BitmapDocument.selectors),
        "CMYK 8": ("cmyk", CmykDocument.tools, nil),
        "CMYK 16": ("cmyk", CmykDocument.tools, nil),
    ]
    let rows = colourModeRows()
    try require(Set(rows.map(\.name)) == Set(expected.keys), "\(tag) the table names every row")
    var failures: [String] = []
    for row in rows {
        guard let want = expected[row.name] else { continue }
        let (document, _, _) = try openColourModeRow(row, tag: tag)
        defer { document.close() }
        let precision = try agentCall("get_precision", document)
        let info = try agentCall("get_document", document)
        guard !precision.isError, !info.isError else {
            failures.append("\(row.name): get_precision or get_document refused: "
                + (precision.isError ? precision.text : info.text))
            continue
        }
        let reply = precision.result
        let mode = reply["mode"] as? String ?? "none"
        let bits = (reply["bits_per_channel"] as? NSNumber)?.intValue ?? -1
        let summary = (reply["document"] as? [String: Any])?["precision"] as? String ?? "none"
        let documentPrecision = info.result["precision"] as? String ?? "none"
        print("\(tag) \(row.name): get_precision mode \(mode), bits_per_channel \(bits); "
            + "summary precision \(summary); get_document precision \(documentPrecision)")
        if mode != want.mode { failures.append("\(row.name) get_precision mode \(mode), not \(want.mode)") }
        if bits != want.bits {
            failures.append("\(row.name) get_precision bits_per_channel \(bits), not \(want.bits)")
        }
        if summary != want.summary {
            failures.append("\(row.name) the summary's precision \(summary), not \(want.summary)")
        }
        if documentPrecision != want.summary {
            failures.append("\(row.name) get_document's precision \(documentPrecision), not \(want.summary)")
        }
        guard let gate = gated[row.name] else { continue }
        let colorMode = reply["color_mode"] as? String ?? "none"
        if colorMode != gate.colorMode {
            failures.append("\(row.name) get_precision color_mode \(colorMode), not \(gate.colorMode)")
        }
        if reply["supported_tools"] as? [String] != gate.tools.sorted() {
            failures.append("\(row.name) get_precision supported_tools are not the mode's own")
        }
        if let menus = gate.menus, reply["supported_menu_items"] as? [String] != menus.sorted() {
            failures.append("\(row.name) get_precision supported_menu_items are not the mode's own")
        }
        let support = info.result["precision_support"] as? [String: Any] ?? [:]
        if support["supported_tools"] as? [String] != gate.tools.sorted() {
            failures.append("\(row.name) get_document precision_support.supported_tools are not the mode's own")
        }
        if support["color_mode"] as? String != gate.colorMode {
            failures.append("\(row.name) get_document precision_support.color_mode "
                + "\(support["color_mode"] as? String ?? "none"), not \(gate.colorMode)")
        }
    }
    try require(failures.isEmpty, "\(tag) \(failures.count) failures over \(rows.count) rows: "
        + failures.joined(separator: "; "))
    print("PASS: precision reply modes — get_precision answers rgba8, rgba16 and rgba32, gray8, gray16 "
        + "and gray32, gray8 and gray16 on Duotone, lab8 and lab16, cmyk8 and cmyk16, indexed8 and bitmap1 "
        + "with each mode's bits_per_channel, the summary's precision names the same mode and depth "
        + "(rgba32_float for RGB 32), and Bitmap, Indexed and CMYK documents answer their own gate's lists "
        + "in get_precision and get_document's precision_support")
}
