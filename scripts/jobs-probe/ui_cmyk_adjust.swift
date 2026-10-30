import AppKit

// The CMYK document's adjustments-and-filters in-process UI cases (#547): Image ▸ Adjustments, Layer
// ▸ New Adjustment Layer, Image ▸ Auto and the Filter menu validate per `CmykAdjustFilter`'s table
// (take1's record) at the document's depth, Edit ▸ Fade and Repeat Last Filter
// after a blur, and the Channel Mixer and Gaussian Blur sheets apply on ink as one step each. The
// CMYK profile is macOS's Generic CMYK Profile, read from the system.

private let adjustGenericCMYK = URL(fileURLWithPath: "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc")

/// A fresh fixture converted to CMYK through Image ▸ Mode's commit.
private func adjustDocument(_ tag: String) throws -> (ImageDocument, EditorViewController) {
    let document = try fixture(width: 64, height: 48)
    let controller = try editor(document)
    controller.commitColorMode(.cmyk, profile: try Data(contentsOf: adjustGenericCMYK), destination: [:],
                               options: CmykConversionOptions(), document: document)
    guard document.doc?.isCmyk == true else {
        document.close()
        throw ProbeFailure(description: "\(tag) not CMYK")
    }
    return (document, controller)
}

/// The composite's ink16 at `(x, y)` of a CMYK document.
private func adjustInk(_ document: ImageDocument, _ x: Int, _ y: Int) throws -> [Int] {
    guard let precision = document.doc?.precision else { throw ProbeFailure(description: "no precision") }
    let context = try PrecisionWorkContext(source: precision)
    let sample = try precision.cmykSample(x: x, y: y, reach: 0, workingLimit: context.limit,
                                          otherRetained: context.otherRetained)
    return (sample["ink16"] as? [NSNumber])?.map(\.intValue) ?? []
}

private let adjustProbes = [(8, 8), (32, 24), (56, 40), (16, 40), (48, 8)]

/// Presses a sheet's Apply and waits for the published step.
private func applyAndWait(_ sheet: NSViewController, _ document: ImageDocument, base: RasterDocument?,
                          _ tag: String) throws {
    guard let apply = buttons(sheet.view).first(where: { $0.keyEquivalent == "\r" }) else {
        throw ProbeFailure(description: "\(tag) no Apply button")
    }
    try uiSend(apply, "\(tag) Apply")
    try pumpUntil({ document.doc !== base && sheet.view.window?.isVisible != true
                    && !BackgroundWork.occupied && DocumentJobs.active == nil }, seconds: 60)
}

/// "cmyk adjustment menus": every tagged Image ▸ Adjustments and New Adjustment Layer item, every
/// adjustment item with its own selector and Image ▸ Auto Tone / Contrast / Color validate exactly
/// as `CmykAdjustFilter`'s table answers at the fixture's depth: Exposure, Vibrance, Black & White,
/// White Balance, Hue Rotate, Grayscale and Sepia disabled; Shadows/Highlights and Replace Color
/// enabled destructively and disabled as layers.
func verifyCmykAdjustmentMenus() throws {
    let tag = "[cmyk adjustment menus]"
    let (document, controller) = try adjustDocument(tag)
    defer { document.close() }
    let depth = CmykAdjustFilter.depth(of: document.doc)
    var checked = 0
    for (index, op) in AdjustmentMenuOrder.newOps.enumerated() {
        for (selector, layer) in [("showAdjustmentSheet:", false), ("newAdjustmentLayerOp:", true)] {
            let want = CmykAdjustFilter.allowsAdjustment(op.rawValue, depth: depth, layer: layer)
            try require(uiValidates(controller, selector, tag: index) == want,
                        "\(tag) \(selector) \(op.rawValue) validates \(want)")
            checked += 1
        }
    }
    for (selector, entry) in CmykAdjustFilter.legacySelectors {
        let want = CmykAdjustFilter.allowsAdjustment(entry.op, depth: depth, layer: entry.layer)
        try require(uiValidates(controller, selector) == want, "\(tag) \(selector) validates \(want)")
        checked += 1
    }
    for (op, want) in [("exposure", false), ("vibrance", false), ("black_and_white", false),
                       ("white_balance", false), ("shadows_highlights", true), ("replace_color", true),
                       ("channel_mixer", true), ("selective_color", true), ("gradient_map", true)] {
        guard let index = AdjustmentMenuOrder.newOps.firstIndex(where: { $0.rawValue == op }) else {
            throw ProbeFailure(description: "\(tag) no \(op) tag")
        }
        try require(uiValidates(controller, "showAdjustmentSheet:", tag: index) == want,
                    "\(tag) Image ▸ Adjustments ▸ \(op) is \(want ? "enabled" : "disabled")")
    }
    for op in ["shadows_highlights", "replace_color"] {
        guard let index = AdjustmentMenuOrder.newOps.firstIndex(where: { $0.rawValue == op }) else { continue }
        try require(!uiValidates(controller, "newAdjustmentLayerOp:", tag: index),
                    "\(tag) no \(op) adjustment layer")
    }
    for selector in ["autoTone:", "autoContrast:", "autoColor:", "showAdjustments:", "applyInvert:",
                     "showPosterize:", "showThreshold:"] {
        try require(uiValidates(controller, selector), "\(tag) \(selector) validates")
    }
    for selector in ["showHueRotate:", "applyGrayscale:", "applySepia:"] {
        try require(!uiValidates(controller, selector), "\(tag) \(selector) stays disabled")
    }
    print("PASS: \(tag) \(checked) adjustment items validate per the CMYK table at \(depth) bits")
}

/// "cmyk filter menus": every Filter item validates per `CmykAdjustFilter`'s table (Lens
/// Correction, Presence, Add Grain, the Develop Filter and Depth Blur disabled, Remove Red Eye
/// disabled); after a Gaussian Blur applied over MCP, Edit ▸ Fade and Repeat Last Filter validate.
func verifyCmykFilterMenus() throws {
    let tag = "[cmyk filter menus]"
    let (document, controller) = try adjustDocument(tag)
    defer { document.close() }
    let depth = CmykAdjustFilter.depth(of: document.doc)
    for (selector, op) in FloatDocument.filterActions {
        let want = CmykAdjustFilter.allowsFilter(op, depth: depth)
        try require(uiValidates(controller, selector) == want, "\(tag) \(selector) validates \(want)")
    }
    for selector in ["showLensCorrection:", "showPresence:", "showAddGrain:", "showDevelopFilter:",
                     "showDepthBlur:", "removeRedEye:"] {
        try require(!uiValidates(controller, selector), "\(tag) \(selector) stays disabled")
    }
    for selector in ["showBlur:", "showMotionBlur:", "showUnsharpMask:", "showSmartSharpen:", "showAddNoise:",
                     "applyClouds:", "applyEmboss:", "showPixelate:", "showOffset:", "showLiquify:"] {
        try require(uiValidates(controller, selector), "\(tag) \(selector) validates")
    }
    let base = document.doc
    _ = try agentOK("apply_filter", document, ["layer": 0, "filter": "blur", "sigma": 3])
    try pumpUntil({ document.doc !== base && !BackgroundWork.occupied && DocumentJobs.active == nil },
                  seconds: 60)
    try require(uiValidates(controller, "showFade:"), "\(tag) Fade validates after a blur")
    try require(uiValidates(controller, "repeatLastFilter:"), "\(tag) Repeat Last Filter validates after a blur")
    print("PASS: \(tag) the Filter menu validates per the CMYK table; Fade and Repeat Last Filter follow a blur")
}

/// "cmyk channel mixer sheet": Image ▸ Adjustments ▸ Channel Mixer… lists the CMYK outputs (Cyan,
/// Magenta, Yellow, Black), and a Cyan source at 50 % applies as one "Channel Mixer" step that keeps
/// the document CMYK and changes the layer's ink.
func verifyCmykChannelMixerSheet() throws {
    let tag = "[cmyk channel mixer sheet]"
    let (document, controller) = try adjustDocument(tag)
    defer { document.close() }
    guard let index = AdjustmentMenuOrder.newOps.firstIndex(of: .channelMixer) else {
        throw ProbeFailure(description: "\(tag) no Channel Mixer tag")
    }
    let before = try adjustProbes.map { try adjustInk(document, $0.0, $0.1) }
    let base = document.doc
    let entries = document.history.entries.count
    let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    let item = NSMenuItem(title: "Channel Mixer…", action: #selector(EditorViewController.showAdjustmentSheet(_:)),
                          keyEquivalent: "")
    item.tag = index
    controller.showAdjustmentSheet(item)
    let opened = {
        (controller.presentedViewControllers ?? []).compactMap { $0 as? ChannelMixerSheet }
            .last { !known.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 20)
    guard let sheet = opened() else { throw ProbeFailure(description: "\(tag) no Channel Mixer sheet") }
    guard let popup = uiViews(sheet.view, as: NSPopUpButton.self).first(where: {
        $0.itemTitles.contains("Cyan")
    }) else { throw ProbeFailure(description: "\(tag) no Output popup with Cyan") }
    try require(popup.itemTitles == ["Cyan", "Magenta", "Yellow", "Black"],
                "\(tag) the outputs are the inks: \(popup.itemTitles)")
    let sliders = uiViews(sheet.view, as: NSSlider.self).filter { $0.minValue == -200 && $0.maxValue == 200 }
    try require(sliders.count == 5, "\(tag) four ink sources and a constant: \(sliders.count)")
    sliders[0].doubleValue = 50
    try uiSend(sliders[0], "\(tag) Cyan source 50 %")
    try applyAndWait(sheet, document, base: base, tag)
    try require(document.history.entries.count == entries + 1
                && document.undoManager?.undoActionName == "Channel Mixer",
                "\(tag) one \"Channel Mixer\" step: \(document.history.entries.map(\.name))")
    try require(document.doc?.isCmyk == true, "\(tag) the document stays CMYK")
    let after = try adjustProbes.map { try adjustInk(document, $0.0, $0.1) }
    try require(after != before, "\(tag) the layer's ink changes: \(before) -> \(after)")
    print("PASS: \(tag) the CMYK outputs are listed and a Cyan mix applies as one step on ink")
}

/// "cmyk gaussian blur sheet": Filter ▸ Blur ▸ Gaussian Blur… on a CMYK document applies as one
/// step that keeps the document CMYK and changes the layer's ink.
func verifyCmykGaussianBlurSheet() throws {
    let tag = "[cmyk gaussian blur sheet]"
    let (document, controller) = try adjustDocument(tag)
    defer { document.close() }
    try require(uiValidates(controller, "showBlur:"), "\(tag) Gaussian Blur… validates")
    let before = try adjustProbes.map { try adjustInk(document, $0.0, $0.1) }
    let base = document.doc
    let entries = document.history.entries.count
    let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    controller.showBlur(nil)
    let opened = {
        (controller.presentedViewControllers ?? []).compactMap { $0 as? SliderSheetController }
            .last { !known.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 20)
    guard let sheet = opened() else { throw ProbeFailure(description: "\(tag) no Gaussian Blur sheet") }
    try applyAndWait(sheet, document, base: base, tag)
    try require(document.history.entries.count == entries + 1,
                "\(tag) one step: \(document.history.entries.map(\.name))")
    try require(document.doc?.isCmyk == true, "\(tag) the document stays CMYK")
    let after = try adjustProbes.map { try adjustInk(document, $0.0, $0.1) }
    try require(after != before, "\(tag) the layer's ink changes: \(before) -> \(after)")
    print("PASS: \(tag) Gaussian Blur applies as one step on ink")
}
