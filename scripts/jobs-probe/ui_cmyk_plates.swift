import AppKit

// The CMYK document's remaining-gaps in-process UI case (#1): with one ink plate targeted in
// the Channels panel, the Filter, Adjustments, Fill, Apply Image, Calculations, Fill / Stroke Path
// and Gamut Warning items validate, and Edit ▸ Fill with Foreground Color on Cyan changes the cyan
// plate only (the foreground's grey, black = full ink). The CMYK profile is macOS's Generic CMYK
// Profile.

private let platesCMYK = URL(fileURLWithPath: "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc")

/// The stored ink16 at `(x, y)` (`cmykSample`, no reach).
private func plateInk(_ document: ImageDocument, _ x: Int, _ y: Int) throws -> [Int] {
    guard let precision = document.doc?.precision else { throw ProbeFailure(description: "no precision") }
    let context = try PrecisionWorkContext(source: precision)
    let sample = try precision.cmykSample(x: x, y: y, reach: 0, workingLimit: context.limit,
                                          otherRetained: context.otherRetained)
    return (sample["ink16"] as? [NSNumber])?.map(\.intValue) ?? []
}

/// "cmyk plate menus": a fresh fixture converted to CMYK, a selection and a work path; with
/// Cyan targeted every item the remaining-gaps batch opens validates, and a Fill with the
/// foreground (black) inside the selection makes cyan 100 % there and leaves the other plates
/// and every pixel outside exactly as they were.
func verifyCmykPlateMenus() throws {
    let tag = "[cmyk plate menus]"
    let document = try fixture(width: 64, height: 48)
    defer { document.close() }
    let controller = try editor(document)
    controller.commitColorMode(.cmyk, profile: try Data(contentsOf: platesCMYK), destination: [:],
                               options: CmykConversionOptions(), document: document)
    try require(document.doc?.isCmyk == true, "\(tag) the document is CMYK")
    _ = try agentOK("select_rect", document, ["x": 8, "y": 8, "width": 20, "height": 16])
    _ = try agentOK("selection_to_path", document)
    guard let doc = document.doc, let work = doc.paths.entries.first else {
        throw ProbeFailure(description: "\(tag) no work path")
    }
    document.currentPath = .path(work.id)
    controller.setPaintTarget(.ink(.cyan))
    try require(controller.paintTarget == .ink(.cyan), "\(tag) the cyan plate is the target")
    for selector in ["showBlur:", "showUnsharpMask:", "showLevels:", "showCurves:", "applyInvert:",
                     "fillSelection:", "fillWithForeground:", "strokeSelection:", "applyImageSheet:",
                     "calculationsSheet:", "fillCurrentPath:", "strokeCurrentPath:",
                     "toggleGamutWarning:"] {
        try require(uiValidates(controller, selector), "\(tag) \(selector) validates on the cyan plate")
    }
    let inside = (12, 12), outside = (40, 40)
    let before = try [inside, outside].map { try plateInk(document, $0.0, $0.1) }
    // A plate takes the foreground's grey (`planeInk`), never its stored ink: black is full ink.
    controller.paintColor = NSColor(srgbRed: 0, green: 0, blue: 0, alpha: 1)
    let base = document.doc
    controller.fillWithForeground(nil)
    try pumpUntil({ document.doc !== base && !BackgroundWork.occupied && DocumentJobs.active == nil },
                  seconds: 60)
    let after = try [inside, outside].map { try plateInk(document, $0.0, $0.1) }
    try require(after[0].count == 4 && after[0][0] == 65535 && Array(after[0][1...]) == Array(before[0][1...]),
                "\(tag) the fill makes cyan 100 % and keeps M, Y, K: \(before[0]) → \(after[0])")
    try require(after[1] == before[1], "\(tag) outside the selection nothing changes")
    try require(document.doc?.isCmyk == true, "\(tag) the document stays CMYK")
    print("PASS: \(tag) the plate-targeted items validate; Fill on Cyan changes only cyan")
}
