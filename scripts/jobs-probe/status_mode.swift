import AppKit

/// "status mode segment": the status bar's mode segment (identifier `status-mode`) names the
/// document's own colour mode and sample format for every document kind. Each row opens a fresh
/// 64 × 48 fixture, converts it through the MCP tools the menus mirror (`convert_precision`,
/// `convert_mode` with gray, lab, cmyk, duotone, indexed and bitmap, and the Gray depth route),
/// confirms through the captured descriptor that the conversion landed, and then requires the
/// segment's prefix from this file's own table, written out as literals rather than computed from
/// the property under test. Every row is checked before the case fails, so a red run lists each.
/// The segment is also required to end in a non-empty profile name. Image ▸ Mode's 8, 16 and 32
/// Bits/Channel items are validated on each document too, and the checked one must be the row's
/// literal depth (none on a float RGB, Indexed or Bitmap document, which never check one). No
/// window is shown on screen and no event is synthesized.
func verifyStatusModeSegment() throws {
    let tag = "[status mode segment]"
    let cmyk: [String: Any] = [
        "mode": "cmyk", "profile": "file",
        "path": "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc",
    ]
    // Two inks, black and a warm brown, each on the straight 13-slot curve.
    let curve: [Double] = [0, 5, 10, 20, 30, 40, 50, 60, 70, 80, 90, 95, 100]
    let duotone: [String: Any] = [
        "mode": "duotone",
        "inks": [
            ["name": "Black", "lab": [0.0, 0.0, 0.0], "curve": curve],
            ["name": "Brown", "lab": [45.0, 18.0, 30.0], "curve": curve],
        ],
    ]
    let to16: (String, [String: Any]) = ("convert_precision", ["mode": "rgba16", "structure": "preserve"])
    let to32: (String, [String: Any]) = ("convert_precision", ["mode": "rgba32", "structure": "preserve"])
    let gray: (String, [String: Any]) = ("convert_mode", ["mode": "gray"])
    typealias Row = (name: String, steps: [(String, [String: Any])], landed: (RasterDocument) -> Bool,
                     prefix: String, checked: [Int])
    let rows: [Row] = [
        ("RGB 8", [], { $0.colorMode == .rgb && !$0.isHighDepth }, "RGB · 8-bit · ", [8]),
        ("RGB 16", [to16], { $0.colorMode == .rgb && $0.isHighDepth && !$0.isFloat }, "RGB · 16-bit · ",
         [16]),
        ("RGB 32", [to32], { $0.colorMode == .rgb && $0.isFloat }, "RGB · 32-bit float · ", []),
        ("Grayscale 8", [gray], { $0.colorMode == .gray && $0.grayDepth == 8 }, "Grayscale · 8-bit · ",
         [8]),
        ("Grayscale 16", [to16, gray], { $0.colorMode == .gray && $0.grayDepth == 16 },
         "Grayscale · 16-bit · ", [16]),
        ("Grayscale 32", [gray, ("convert_precision", ["mode": "gray32"])],
         { $0.colorMode == .gray && $0.grayDepth == 32 }, "Grayscale · 32-bit float · ", [32]),
        ("Duotone 8", [gray, ("convert_mode", duotone)], { $0.colorMode == .duotone && $0.grayDepth == 8 },
         "Duotone · 8-bit · ", [8]),
        ("Duotone 16", [to16, gray, ("convert_mode", duotone)],
         { $0.colorMode == .duotone && $0.grayDepth == 16 }, "Duotone · 16-bit · ", [16]),
        ("Lab 8", [("convert_mode", ["mode": "lab"])], { $0.colorMode == .lab && $0.labDepth == 8 },
         "Lab · 8-bit · ", [8]),
        ("Lab 16", [to16, ("convert_mode", ["mode": "lab"])], { $0.colorMode == .lab && $0.labDepth == 16 },
         "Lab · 16-bit · ", [16]),
        ("CMYK 8", [("convert_mode", cmyk)],
         { $0.colorMode == .cmyk && $0.precisionInfo?["cmyk_depth"] as? Int == 8 }, "CMYK · 8-bit · ", [8]),
        ("CMYK 16", [to16, ("convert_mode", cmyk)],
         { $0.colorMode == .cmyk && $0.precisionInfo?["cmyk_depth"] as? Int == 16 }, "CMYK · 16-bit · ",
         [16]),
        ("Indexed", [("convert_mode", ["mode": "indexed", "palette": "uniform", "dither": "none"])],
         { $0.colorMode == .indexed }, "Indexed · 8-bit · ", []),
        ("Bitmap", [gray, ("convert_mode", ["mode": "bitmap", "method": "threshold"])],
         { $0.colorMode == .bitmap }, "Bitmap · 1-bit · ", []),
    ]
    let depthItems = [(8, "convertToEightBits:"), (16, "convertToSixteenBits:"), (32, "convertToFloatPrecision:")]
    var failures: [String] = []
    for row in rows {
        let document = try fixture(width: 64, height: 48)
        defer { document.close() }
        let controller = try editor(document)
        for (tool, arguments) in row.steps {
            let reply = try agentCall(tool, document, arguments)
            try require(!reply.isError, "\(tag) \(row.name): \(tool) refused: \(reply.text)")
        }
        guard let doc = document.doc, row.landed(doc) else {
            throw ProbeFailure(description: "\(tag) \(row.name): the conversion did not land "
                + "(\(document.doc?.colorMode.rawValue ?? "no document"))")
        }
        let segment = try uiControl(controller.view, id: "status-mode", as: StatusSegment.self)
        // The conversion's own image-change notification sets the segment; nothing else runs.
        try? pumpUntil({ segment.text.hasPrefix(row.prefix) }, seconds: 2)
        let text = segment.text
        let checked = depthItems.compactMap { bits, selector -> Int? in
            let item = NSMenuItem(title: "", action: NSSelectorFromString(selector), keyEquivalent: "")
            _ = controller.validateUserInterfaceItem(item)
            return item.state == .on ? bits : nil
        }
        print("\(tag) \(row.name): \"\(text)\", Bits/Channel checked \(checked)")
        if checked != row.checked {
            failures.append("\(row.name) checks Bits/Channel \(checked), not \(row.checked)")
        }
        let profile = text.hasPrefix(row.prefix) ? String(text.dropFirst(row.prefix.count)) : ""
        if !text.hasPrefix(row.prefix) {
            failures.append("\(row.name) reads \"\(text)\", not \"\(row.prefix)…\"")
        } else if profile.trimmingCharacters(in: .whitespaces).isEmpty || profile.hasPrefix("·") {
            failures.append("\(row.name) names no profile: \"\(text)\"")
        }
    }
    try require(failures.isEmpty, "\(tag) \(failures.count) of \(rows.count) rows: "
        + failures.joined(separator: "; "))
    print("PASS: status mode segment — the status bar names RGB at 8, 16 and 32-bit float, Grayscale at "
        + "8, 16 and 32-bit float, Duotone at 8 and 16, Lab and CMYK at 8 and 16, Indexed at 8 and Bitmap "
        + "at 1 bit, each followed by its profile; Image ▸ Mode checks the declared Bits/Channel item")
}
