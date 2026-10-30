import AppKit

// The CMYK colour workflow's in-process UI cases (#1 phase 5): Edit ▸ Color Settings… through its
// own controls (OK stores, Cancel keeps, the working CMYK reinstalled), File ▸ New's Color Mode
// popup making a CMYK document of bare paper, and Levels and Curves on a CMYK document — the
// histogram of L per ink, Auto, the three eyedroppers (driven through the dialog's own tool row)
// and CMYK presets (the library's CMYK entries and From Photoshop Preset…'s ink channels). The
// CMYK profile is macOS's Generic CMYK Profile, read from the system.

private let genericCMYKProfile = URL(fileURLWithPath: "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc")

/// A fresh 8-bit gradient fixture converted to CMYK through Image ▸ Mode's commit.
private func cmykColourFixture(_ tag: String) throws -> (ImageDocument, EditorViewController) {
    WorkingCMYK.install()
    let document = try fixture(width: 64, height: 48)
    let controller = try editor(document)
    controller.commitColorMode(.cmyk, profile: try Data(contentsOf: genericCMYKProfile), destination: [:],
                               options: CmykConversionOptions(), document: document)
    guard document.doc?.isCmyk == true else { throw ProbeFailure(description: "\(tag) not CMYK") }
    return (document, controller)
}

/// The value of `object`'s stored property `label`, by reflection (no product-only API).
private func reflected<T>(_ object: Any, _ label: String, as type: T.Type) -> T? {
    Mirror(reflecting: object).children.first { $0.label == label }?.value as? T
}

/// Opens a sheet through `open` and answers the new `T`.
private func presented<T: NSViewController>(_ controller: EditorViewController, _ type: T.Type, _ tag: String,
                                            _ open: () -> Void) throws -> T {
    let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    open()
    let opened = {
        (controller.presentedViewControllers ?? []).compactMap { $0 as? T }
            .last { !known.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 30)
    guard let sheet = opened() else { throw ProbeFailure(description: "\(tag) no \(T.self)") }
    return sheet
}

private func dismissSheet(_ sheet: NSViewController, _ tag: String) throws {
    if let cancel = buttons(sheet.view).first(where: { $0.keyEquivalent == "\u{1b}" }) {
        try uiSend(cancel, "\(tag) Cancel")
    } else { sheet.dismiss(nil) }
    try pumpUntil({ sheet.view.window?.isVisible != true && !BackgroundWork.occupied }, seconds: 20)
}

/// "cmyk color settings": Edit ▸ Color Settings… on ⇧⌘K; Cancel stores nothing; OK stores the
/// CMYK policy, the working Gray (Gray Gamma 2.2), the intent and BPC in the launcher's private
/// store (`ColorSettingsStore`, the `color-settings.plist` under `DR_COLOR_SETTINGS_DIR`, never
/// the preferences domain) and posts
/// the change, on which the working CMYK stays installed; every key it changed is put back through
/// `ColorSettingsStore.set`/`remove`, never by rewriting the file behind the store's cache.
func verifyCmykColorSettings() throws {
    let tag = "[cmyk color settings]"
    WorkingCMYK.install()
    let delegate = AppDelegate()
    guard let item = delegate.editMenu().items.first(where: { $0.title == "Color Settings…" }) else {
        throw ProbeFailure(description: "\(tag) Edit has no Color Settings… item")
    }
    try require(item.keyEquivalent == "k" && item.keyEquivalentModifierMask == [.command, .shift],
                "\(tag) Color Settings… is on ⇧⌘K")
    guard let folder = ProcessInfo.processInfo.environment[ColorSettingsStore.directoryEnvironment],
          !folder.isEmpty else {
        throw ProbeFailure(description: "\(tag) the launcher set no \(ColorSettingsStore.directoryEnvironment)")
    }
    let file = URL(fileURLWithPath: folder).appendingPathComponent(ColorSettingsStore.fileName)
    let before = (ColorSettings.cmykPolicy, ColorSettings.intent, ColorSettings.blackPointCompensation,
                  ColorSettings.workingGray)
    try require(before == (.preserve, .relative, true, .dotGain20),
                "\(tag) the private store starts at the defaults: \(before)")
    let keys = [ColorSettings.workingSpaceKey, ColorSettings.workingCMYKKey, ColorSettings.rgbPolicyKey,
                ColorSettings.cmykPolicyKey, ColorSettings.intentKey, ColorSettings.blackPointCompensationKey,
                ColorSettings.askWhenPastingKey, ColorSettings.workingGrayKey]
    /// The store's file as written (read only; the store owns every write).
    func stored() -> [String: Any] {
        guard let data = try? Data(contentsOf: file),
              let plist = try? PropertyListSerialization.propertyList(from: data, format: nil) else { return [:] }
        return plist as? [String: Any] ?? [:]
    }
    func preferences() -> [String: Any] {
        UserDefaults.standard.persistentDomain(forName: Bundle.main.bundleIdentifier ?? "") ?? [:]
    }
    let storedBefore = stored()
    let preferencesBefore = preferences().filter { keys.contains($0.key) }
    defer {
        for key in keys {
            if let value = storedBefore[key] {
                ColorSettingsStore.set(value, forKey: key)
            } else if stored()[key] != nil {
                ColorSettingsStore.remove(forKey: key)
            }
        }
        NotificationCenter.default.post(name: ColorSettings.didChange, object: nil)
    }
    // Cancel: nothing stored.
    try ModalSteps(title: "Color Settings", steps: [
        ("change the CMYK policy, then Cancel", { window in
            guard let card = window.contentView else { return false }
            let policy = try uiControl(card, id: "color-settings-cmyk-policy", as: NSPopUpButton.self)
            policy.selectItem(at: ColorPolicy.allCases.firstIndex(of: .convert) ?? 2)
            try uiSend(try uiControl(card, id: "color-settings-cancel", as: NSButton.self), "\(tag) Cancel")
            return true
        }),
    ]).run { _ = ColorSettingsWindowController.run() }
    try require(stored()[ColorSettings.cmykPolicyKey] as? String == storedBefore[ColorSettings.cmykPolicyKey] as? String,
                "\(tag) Cancel stores nothing")
    // OK: stored, and the change posted.
    var noticed = false
    let observer = NotificationCenter.default.addObserver(forName: ColorSettings.didChange, object: nil,
                                                          queue: nil) { _ in noticed = true }
    defer { NotificationCenter.default.removeObserver(observer) }
    try ModalSteps(title: "Color Settings", steps: [
        ("Convert to Working, Perceptual, no BPC; OK", { window in
            guard let card = window.contentView else { return false }
            let engine = try uiControl(card, id: "color-settings-engine", as: NSPopUpButton.self)
            try require(engine.itemTitles == ["Doctored ICC"], "\(tag) the engine is this build's")
            let cmyk = try uiControl(card, id: "color-settings-cmyk", as: NSPopUpButton.self)
            try require(cmyk.itemTitles.first?.hasPrefix("Automatic") == true
                        && cmyk.itemTitles.last == "Load…", "\(tag) the CMYK popup: \(cmyk.itemTitles)")
            let policy = try uiControl(card, id: "color-settings-cmyk-policy", as: NSPopUpButton.self)
            policy.selectItem(at: ColorPolicy.allCases.firstIndex(of: .convert) ?? 2)
            let gray = try uiControl(card, id: "color-settings-gray", as: NSPopUpButton.self)
            try require(gray.itemTitles == ["Dot Gain 10%", "Dot Gain 15%", "Dot Gain 20%", "Dot Gain 25%",
                                            "Dot Gain 30%", "Gray Gamma 1.8", "Gray Gamma 2.2"]
                        && gray.titleOfSelectedItem == "Dot Gain 20%",
                        "\(tag) the Gray popup: \(gray.itemTitles), \(gray.titleOfSelectedItem ?? "none")")
            gray.selectItem(withTitle: "Gray Gamma 2.2")
            let intent = try uiControl(card, id: "color-settings-intent", as: NSPopUpButton.self)
            intent.selectItem(at: RenderingIntent.allCases.firstIndex(of: .perceptual) ?? 0)
            let bpc = try uiControl(card, id: "color-settings-bpc", as: NSButton.self)
            bpc.state = .off
            try uiSend(try uiControl(card, id: "color-settings-ok", as: NSButton.self), "\(tag) OK")
            return true
        }),
    ]).run { _ = ColorSettingsWindowController.run() }
    let after = stored()
    try require(after[ColorSettings.cmykPolicyKey] as? String == "convert"
                && after[ColorSettings.intentKey] as? String == "perceptual"
                && after[ColorSettings.blackPointCompensationKey] as? Bool == false
                && after[ColorSettings.workingCMYKKey] as? String == "automatic"
                && after[ColorSettings.workingGrayKey] as? String == "gray_gamma_22",
                "\(tag) OK stores the keys in the private file: \(after.filter { keys.contains($0.key) })")
    try require((ColorSettings.cmykPolicy, ColorSettings.intent, ColorSettings.blackPointCompensation,
                 ColorSettings.workingGray) == (.convert, .perceptual, false, .grayGamma22),
                "\(tag) the reads follow the private store")
    let preferencesAfter = preferences().filter { keys.contains($0.key) }
    try require(NSDictionary(dictionary: preferencesAfter).isEqual(to: preferencesBefore),
                "\(tag) the preferences domain is untouched: \(preferencesAfter)")
    try require(noticed && WorkingCMYK.installedData != nil,
                "\(tag) OK posts the change; the working CMYK stays installed")
    print("PASS: \(tag) Color Settings stores on OK and keeps on Cancel")
}

/// "cmyk new document": File ▸ New… with Color Mode CMYK Color, 16 bits and a White background
/// makes a CMYK document of bare paper in the working CMYK.
func verifyCmykNewDocument() throws {
    let tag = "[cmyk new document]"
    WorkingCMYK.install()
    let known = Set(NSDocumentController.shared.documents.map(ObjectIdentifier.init))
    try ModalSteps(title: "New Document", steps: [
        ("CMYK Color, 16 bits, White; Create", { window in
            guard let card = window.contentView else { return false }
            let mode = try uiControl(card, id: "new-document-color-mode", as: NSPopUpButton.self)
            try require(mode.itemTitles == ["RGB Color", "CMYK Color"], "\(tag) the modes: \(mode.itemTitles)")
            mode.selectItem(at: 1)
            try uiSend(mode, "\(tag) CMYK Color")
            let profile = try uiControl(card, id: "new-document-profile", as: NSPopUpButton.self)
            try require(profile.itemTitles.first?.hasPrefix("Working CMYK") == true,
                        "\(tag) the CMYK profiles: \(profile.itemTitles)")
            guard let depth = uiViews(card, as: NSPopUpButton.self).first(where: {
                $0.itemTitles.contains("16 bits/channel")
            }) else { throw ProbeFailure(description: "\(tag) no depth popup") }
            try require(depth.item(at: 2)?.isHidden == true, "\(tag) CMYK hides 32 bits")
            depth.selectItem(at: 1)
            guard let background = uiViews(card, as: NSPopUpButton.self).first(where: {
                $0.itemTitles.contains("Transparent")
            }) else { throw ProbeFailure(description: "\(tag) no background popup") }
            background.selectItem(at: 1)
            let unit = uiViews(card, as: NSPopUpButton.self).first { $0.itemTitles.contains("Pixels") }
            unit?.selectItem(withTitle: "Pixels")
            if let unit { try uiSend(unit, "\(tag) Pixels") }
            for field in uiViews(card, as: NSTextField.self) where field.isEditable {
                if field.stringValue == "8" { field.stringValue = "40" }
                if field.stringValue == "10" { field.stringValue = "30" }
            }
            try uiSend(try uiControl(card, id: "new-document-create", as: NSButton.self), "\(tag) Create")
            return true
        }),
    ]).run { AppDelegate().newDocument(nil) }
    let opened = NSDocumentController.shared.documents.filter { !known.contains(ObjectIdentifier($0)) }
    guard opened.count == 1, let document = opened.first as? ImageDocument else {
        throw ProbeFailure(description: "\(tag) Create opened \(opened.count) documents")
    }
    defer { document.close() }
    guard let precision = document.doc?.precision, precision.isCmyk else {
        throw ProbeFailure(description: "\(tag) the new document is not CMYK")
    }
    let info = precision.cmykInfo() ?? [:]
    try require(info["depth"] as? Int == 16, "\(tag) 16 bits: \(info)")
    try require(info["profile"] as? String == WorkingCMYK.installedName, "\(tag) the working CMYK: \(info)")
    let context = try PrecisionWorkContext(source: precision)
    let sample = try precision.cmykSample(x: 3, y: 3, reach: 0, workingLimit: context.limit,
                                          otherRetained: context.otherRetained)
    try require((sample["ink16"] as? [NSNumber])?.map(\.intValue) == [0, 0, 0, 0]
                && (sample["alpha16"] as? NSNumber)?.intValue == 65535, "\(tag) white is bare paper: \(sample)")
    try require(document.doc?.layerInfo(0)?.name == "Background", "\(tag) the filled layer is Background")
    print("PASS: \(tag) File ▸ New makes a 16-bit CMYK document of paper in the working CMYK")
}

/// "cmyk tone auto": on a CMYK document Levels shows the histogram of L per ink and its Auto
/// replaces the stages and applies as one "Levels" step; Curves' Auto sets the ink curves.
func verifyCmykToneAuto() throws {
    let tag = "[cmyk tone auto]"
    let (document, controller) = try cmykColourFixture(tag)
    defer { document.close() }
    let levels = try presented(controller, LevelsAdjustmentEditor.self, tag) { controller.showLevels(nil) }
    guard let plot = reflected(levels, "plot", as: HistogramView.self) else {
        throw ProbeFailure(description: "\(tag) no Levels plot")
    }
    try require(plot.bins != nil && plot.unavailableReason == nil, "\(tag) the plot holds the CMYK histogram")
    try require((try? uiControl(levels.view, id: "levels-auto-tone", as: NSButton.self))?.superview?.isHidden != false,
                "\(tag) RGB's Auto Tone row is hidden")
    let auto = try uiControl(levels.view, id: "cmyk-tone-auto", as: NSButton.self)
    _ = try uiControl(levels.view, id: "cmyk-tone-auto-options", as: NSButton.self)
    try uiSend(auto, "\(tag) Levels Auto")
    try pumpUntil({ !BackgroundWork.occupied }, seconds: 20)
    let stages = reflected(levels, "stages", as: [LevelsChannel: LevelsStage].self) ?? [:]
    try require(stages.values.contains { !$0.isIdentity }, "\(tag) Auto set a stage: \(stages)")
    let base = document.doc
    let entries = document.history.entries.count
    guard let apply = buttons(levels.view).first(where: { $0.keyEquivalent == "\r" }) else {
        throw ProbeFailure(description: "\(tag) no Apply")
    }
    try uiSend(apply, "\(tag) Levels Apply")
    try pumpUntil({ document.doc !== base && levels.view.window?.isVisible != true
                    && !BackgroundWork.occupied && DocumentJobs.active == nil }, seconds: 60)
    try require(document.history.entries.count == entries + 1 && document.undoManager?.undoActionName == "Levels",
                "\(tag) one \"Levels\" step: \(document.history.entries.map(\.name))")
    // Curves: Auto writes ink curves.
    let curves = try presented(controller, CurvesAdjustmentSheetController.self, tag) { controller.showCurves(nil) }
    guard let curveView = uiViews(curves.view, as: CurveEditorView.self).first else {
        throw ProbeFailure(description: "\(tag) no curve")
    }
    try require(curveView.histogram != nil, "\(tag) Curves draws the CMYK histogram")
    try uiSend(try uiControl(curves.view, id: "cmyk-tone-auto", as: NSButton.self), "\(tag) Curves Auto")
    try pumpUntil({ !BackgroundWork.occupied }, seconds: 20)
    let points = reflected(curves, "channelPoints", as: [CurveChannel: [CurvePoint]].self) ?? [:]
    try require([CurveChannel.cyan, .magenta, .yellow, .black].contains { points[$0] != nil && points[$0] != CurvePoint.identity },
                "\(tag) Curves Auto set an ink curve: \(points)")
    try dismissSheet(curves, tag)
    print("PASS: \(tag) Levels and Curves show the ink histogram and Auto sets them")
}

/// "cmyk tone eyedroppers": the three eyedroppers, through the dialog's own tool row — white on
/// a pixel moves Levels' per-ink white and output white; grey on Curves adds a point per ink.
func verifyCmykToneEyedroppers() throws {
    let tag = "[cmyk tone eyedroppers]"
    let (document, controller) = try cmykColourFixture(tag)
    defer { document.close() }
    let levels = try presented(controller, LevelsAdjustmentEditor.self, tag) { controller.showLevels(nil) }
    for kind in CmykEyedropper.allCases {
        _ = try uiControl(levels.view, id: "cmyk-tone-eyedropper-\(kind.rawValue)", as: NSButton.self)
    }
    guard let tools = reflected(levels, "cmykTools", as: CmykToneControls?.self) ?? nil else {
        throw ProbeFailure(description: "\(tag) the Levels dialog has no CMYK tool row")
    }
    tools.sample(.white, at: CGPoint(x: 20.5, y: 20.5))
    let stages = reflected(levels, "stages", as: [LevelsChannel: LevelsStage].self) ?? [:]
    try require([LevelsChannel.c, .m, .y, .k].contains { (stages[$0]?.white ?? 1) < 1 },
                "\(tag) the white eyedropper moved an ink's white: \(stages)")
    try dismissSheet(levels, tag)
    let curves = try presented(controller, CurvesAdjustmentSheetController.self, tag) { controller.showCurves(nil) }
    guard let curveTools = reflected(curves, "cmykTools", as: CmykToneControls?.self) ?? nil else {
        throw ProbeFailure(description: "\(tag) the Curves dialog has no CMYK tool row")
    }
    curveTools.sample(.gray, at: CGPoint(x: 30.5, y: 10.5))
    let points = reflected(curves, "channelPoints", as: [CurveChannel: [CurvePoint]].self) ?? [:]
    try require([CurveChannel.cyan, .magenta, .yellow, .black].contains { (points[$0]?.count ?? 2) > 2 },
                "\(tag) the grey eyedropper added a point: \(points)")
    try dismissSheet(curves, tag)
    print("PASS: \(tag) the eyedroppers set Levels' points and Curves' ink points")
}

/// "cmyk tone presets": a CMYK Curves preset lists in a CMYK document's Curves dialog and not in
/// an RGB one's, and From Photoshop Preset… reads a five-curve .acv's ink channels into Curves.
func verifyCmykTonePresets() throws {
    let tag = "[cmyk tone presets]"
    let (document, controller) = try cmykColourFixture(tag)
    defer { document.close() }
    let name = "CMYK Probe \(UInt32.random(in: 1000...9999))"
    try AdjustmentPresetLibrary.save(try AdjustmentPreset.validated(
        name: name, op: .curves, params: ["c": [[0, 0], [128, 160], [255, 255]]], cmyk: true), replace: true)
    defer { try? AdjustmentPresetLibrary.delete(op: .curves, name: name) }
    let curves = try presented(controller, CurvesAdjustmentSheetController.self, tag) { controller.showCurves(nil) }
    let popup = try uiControl(curves.view, id: "adjustment-preset-popup", as: NSPopUpButton.self)
    try require(popup.superview?.isHidden == false, "\(tag) the preset row shows on CMYK")
    try require(popup.itemTitles.contains(name), "\(tag) the CMYK preset is listed: \(popup.itemTitles)")
    try dismissSheet(curves, tag)
    let rgbDocument = try fixture(width: 32, height: 24)
    defer { rgbDocument.close() }
    let rgbController = try editor(rgbDocument)
    let rgbCurves = try presented(rgbController, CurvesAdjustmentSheetController.self, tag) {
        rgbController.showCurves(nil)
    }
    let rgbPopup = try uiControl(rgbCurves.view, id: "adjustment-preset-popup", as: NSPopUpButton.self)
    try require(!rgbPopup.itemTitles.contains(name), "\(tag) an RGB document does not list it")
    try dismissSheet(rgbCurves, tag)
    // From Photoshop Preset…: version 4, five curves of (output, input); cyan bends.
    var words: [UInt16] = [4, 5, 2, 0, 0, 255, 255, 3, 0, 0, 140, 128, 255, 255]
    for _ in 0..<3 { words += [2, 0, 0, 255, 255] }
    let url = directory.appendingPathComponent("cmyk-probe.acv")
    try Data(words.flatMap { [UInt8($0 >> 8), UInt8($0 & 0xFF)] }).write(to: url)
    try require(uiValidates(controller, "newAdjustmentLayerFromPhotoshopPreset:"),
                "\(tag) From Photoshop Preset… validates on CMYK")
    let prefilled = try presented(controller, CurvesAdjustmentSheetController.self, tag) {
        controller.newAdjustmentLayer(fromPhotoshopPreset: url)
    }
    let points = reflected(prefilled, "channelPoints", as: [CurveChannel: [CurvePoint]].self) ?? [:]
    try require(points[.cyan] == [CurvePoint(x: 0, y: 0), CurvePoint(x: 127, y: 115), CurvePoint(x: 255, y: 255)],
                "\(tag) the .acv's cyan curve, reflected onto ink: \(String(describing: points[.cyan]))")
    try dismissSheet(prefilled, tag)
    print("PASS: \(tag) CMYK presets list on CMYK only and a CMYK .acv opens on the ink axes")
}
