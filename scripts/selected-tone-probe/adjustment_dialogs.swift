import AppKit

// The destructive Curves dialog (Image ▸ Adjustments ▸
// Curves…) and Layer ▸ New Adjustment Layer ▸ From Photoshop Preset…, each at
// 8 and 16 bits, through the real controllers.

func runCurvesDestructive(depth: Int, fixtures: URL, output: URL) throws -> [String: Any] {
    let name = "curves-destructive-ui-\(depth)"
    trace("START \(name)")
    let (document,editor,source) = try openDocument(depth:depth,fixtures:fixtures)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let mask = try installSelection(editor,width:source.width,height:source.height)
    let original = try values(source)
    try waitUntil({ editor.view.window?.attachedSheet == nil },"Previous sheet did not detach")
    editor.showCurves(nil)
    guard let sheet = editor.presentedViewControllers?.last as? CurvesAdjustmentSheetController else {
        throw DoctoredCoreError(message:"Actual destructive Curves sheet missing")
    }
    _ = sheet.view
    try waitUntil({ sheet.view.window?.sheetParent != nil },"Destructive Curves sheet did not attach")
    let previewRenderer = try renderer(sheet)
    try waitUntil({ idle(previewRenderer) && editor.canvas.previewImage != nil },"Initial destructive Curves preview failed")
    let point = try find(sheet.view,"tone.curves.point",NSPopUpButton.self)
    point.selectItem(at:1); try activate(point)
    let units = try find(sheet.view,"tone.curves.units",NSPopUpButton.self)
    units.selectItem(at:1); try activate(units)
    let outputField = try find(sheet.view,"tone.curves.output",NSTextField.self)
    let nativeValue = depth == 16 ? 61003.0 : 240.25
    outputField.stringValue = String(nativeValue); try activate(outputField)
    units.selectItem(at:0); try activate(units)
    try require(abs(outputField.doubleValue-nativeValue/(depth == 16 ? 65535 : 255)) < 1e-14,"Curve units changed normalized value")
    try waitUntil({ idle(previewRenderer) && editor.canvas.previewImage != nil },"Fine destructive Curves preview failed")
    guard let preview = editor.canvas.previewImage else { throw DoctoredCoreError(message:"Destructive Curves preview missing") }
    let previewBytes = try displayed(preview)
    try require(document.doc === source,"Destructive Curves preview published a document edit")
    let scale = sheet.view.window?.backingScaleFactor ?? 0
    try screenshot(sheet.view,output.appendingPathComponent(name+"-sheet.png"))
    ActionRecorder.shared.start()
    try button(sheet.view,"Apply").performClick(nil)
    try waitUntil({ editor.view.window?.attachedSheet == nil && !BackgroundWork.occupied && DocumentJobs.active == nil },"Destructive Curves Apply did not finish")
    pump()
    let records = ActionRecorder.shared.stop()
    guard let edited = document.doc, let projection = document.previewImage else { throw DoctoredCoreError(message:"Destructive Curves result missing") }
    let expected = try values(edited)
    try require(edited !== source && expected != original,"Destructive Curves changed nothing")
    try require(edited.layerCount == source.layerCount,"Destructive Curves added a layer")
    try require(records.count == 1 && records[0].tool == "apply_filter","One apply_filter Action: \(records.map(\.tool))")
    try require(records[0].arguments["filter"] as? String == "curves","Recorded filter is not curves")
    guard let params = records[0].arguments["params"] as? [String:Any],
          let points = params["rgb"] as? [[Double]], let end = points.last else {
        throw DoctoredCoreError(message:"Destructive Curves Action omitted params.rgb")
    }
    let historical = nativeValue/(depth == 16 ? 257 : 1)
    try require(abs(end[1]-historical) < 1e-12,"Destructive Curves Action rounded the historical coordinate")
    try require(try displayed(projection) == previewBytes,"Destructive Curves preview differs from committed display")
    for i in mask.indices {
        let p = i*4
        if mask[i] == 0 { try require(Array(expected[p..<p+4]) == Array(original[p..<p+4]),"Destructive Curves changed an unselected pixel") }
        try require(expected[p+3] == original[p+3],"Destructive Curves changed alpha")
    }
    document.undoManager?.undo(); pump()
    try require(document.doc === source && !document.isDocumentEdited,"One Undo did not restore the source")
    var arguments = records[0].arguments
    arguments["document_id"] = AgentServer.shared.documentID(for:document)
    let data = try JSONSerialization.data(withJSONObject:arguments,options:.sortedKeys)
    let response = AgentServer.shared.execute(tool:"apply_filter",argumentsJSON:String(decoding:data,as:UTF8.self))
    try waitUntil({ !BackgroundWork.occupied && DocumentJobs.active == nil },"Destructive Curves replay did not finish")
    try require(try values(document.doc ?? source) == expected,"Destructive Curves replay differs: \(response)")
    document.undoManager?.undo(); pump()
    try require(document.doc === source,"Replay Undo did not restore the source")
    editor.showCurves(nil)
    guard let cancelled = editor.presentedViewControllers?.last as? CurvesAdjustmentSheetController else {
        throw DoctoredCoreError(message:"Second destructive Curves sheet missing")
    }
    _ = cancelled.view
    try waitUntil({ cancelled.view.window?.sheetParent != nil },"Second Curves sheet did not attach")
    ActionRecorder.shared.start()
    try button(cancelled.view,"Cancel").performClick(nil)
    try waitUntil({ editor.view.window?.attachedSheet == nil },"Destructive Curves Cancel did not detach")
    pump()
    let cancelRecords = ActionRecorder.shared.stop()
    try require(document.doc === source && editor.canvas.previewImage == nil && cancelRecords.isEmpty,"Cancel published a preview, an edit or an Action")
    trace("PASS \(name)")
    return ["name":name,"passed":true,"backing_scale":scale,"historical_white_output":end[1],"native_components":expected.count]
}

// ------------------------------------------------------------ Photoshop presets

private func be16(_ out: inout Data, _ value: UInt16) { out.append(contentsOf: [UInt8(value >> 8), UInt8(value & 0xff)]) }

/// An `.acv`: version, count, then each curve's (output, input) pairs.
private func acvFile(version: UInt16, _ curves: [[(UInt16, UInt16)]]) -> Data {
    var out = Data()
    be16(&out, version); be16(&out, UInt16(curves.count))
    for curve in curves {
        be16(&out, UInt16(curve.count))
        for (output, input) in curve { be16(&out, output); be16(&out, input) }
    }
    return out
}

/// A composite-only `.alv`: version 2, record 0, 28 identity records.
private func alvFile(black: UInt16, white: UInt16, gammaHundredths: UInt16) -> Data {
    var out = Data()
    be16(&out, 2)
    for record in 0..<29 {
        let fields: [UInt16] = record == 0 ? [black, white, 0, 255, gammaHundredths] : [0, 255, 0, 255, 100]
        for field in fields { be16(&out, field) }
    }
    out.append(contentsOf: Array("Lvls".utf8))
    return out
}

/// A colorize `.ahu` with Photoshop's default band ranges.
private func ahuFile(colorize: (Int16, Int16, Int16)) -> Data {
    var out = Data()
    be16(&out, 2); out.append(contentsOf: [1, 0])
    for value in [colorize.0, colorize.1, colorize.2, 0, 0, 0] { be16(&out, UInt16(bitPattern: value)) }
    let ranges: [[Int16]] = [[315,345,15,45],[15,45,75,105],[75,105,135,165],[135,165,195,225],[195,225,255,285],[255,285,315,345]]
    for range in ranges { for value in range + [0, 0, 0] { be16(&out, UInt16(bitPattern: value)) } }
    return out
}

/// `member`, but through the superclass chain too: the phase-5 sheets keep
/// their renderer on `AdjustmentSheet`, which a subclass Mirror omits.
private func deepMember(_ owner: Any, _ key: String) -> Any? {
    var mirror: Mirror? = Mirror(reflecting: owner)
    while let current = mirror {
        if let value = current.children.first(where: { $0.label == key })?.value {
            let inner = Mirror(reflecting: value)
            return inner.displayStyle == .optional ? inner.children.first?.value : value
        }
        mirror = current.superclassMirror
    }
    return nil
}

private func sheetRenderer(_ sheet: NSViewController) throws -> PreviewRenderer {
    let owner = deepMember(sheet, "precisionSession") ?? sheet
    guard let result = deepMember(owner, "renderer") as? PreviewRenderer else {
        throw DoctoredCoreError(message: "Actual sheet preview renderer missing")
    }
    return result
}

private func nearly(_ a: Any?, _ b: Double) -> Bool {
    guard let value = (a as? NSNumber)?.doubleValue else { return false }
    return abs(value - b) < 1e-9
}

func runPhotoshopPreset(depth: Int, fixtures: URL, output: URL) throws -> [String: Any] {
    let name = "photoshop-preset-ui-\(depth)"
    trace("START \(name)")
    let folder = output.appendingPathComponent(name, isDirectory: true)
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    let identity: [(UInt16, UInt16)] = [(0, 0), (255, 255)]
    let files: [(String, Data)] = [
        ("Contrast.acv", acvFile(version: 4, [[(0, 0), (150, 128), (255, 255)], [(10, 0), (255, 255)], identity, [(0, 20), (240, 255)], identity])),
        ("Shadows.alv", alvFile(black: 10, white: 240, gammaHundredths: 120)),
        ("Toned.ahu", ahuFile(colorize: (200, 40, -10))),
        ("Press.acv", acvFile(version: 1, [identity, identity, identity, identity, [(30, 0), (255, 255)]])),
    ]
    for (file, bytes) in files { try bytes.write(to: folder.appendingPathComponent(file)) }
    var applied: [String] = []
    for (file, _) in files.prefix(3) {
        let url = folder.appendingPathComponent(file)
        let expected = try PhotoshopAdjustmentPreset.read(url)
        let (document,editor,source) = try openDocument(depth:depth,fixtures:fixtures)
        defer {
            if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
            document.close()
        }
        try waitUntil({ editor.view.window?.attachedSheet == nil },"Previous sheet did not detach")
        editor.newAdjustmentLayer(fromPhotoshopPreset: url)
        guard let sheet = editor.presentedViewControllers?.last else { throw DoctoredCoreError(message:"\(file) presented no sheet") }
        switch url.pathExtension {
        case "acv": try require(sheet is CurvesAdjustmentSheetController,"\(file) opened \(type(of: sheet))")
        case "alv": try require(sheet is AdjustmentLayerSheetController,"\(file) opened \(type(of: sheet))")
        default: try require(sheet is HueSaturationSheet,"\(file) opened \(type(of: sheet))")
        }
        _ = sheet.view
        try waitUntil({ sheet.view.window?.sheetParent != nil },"\(file) sheet did not attach")
        let previewRenderer = try sheetRenderer(sheet)
        try waitUntil({ idle(previewRenderer) && editor.canvas.previewImage != nil },"\(file) preview failed")
        try screenshot(sheet.view,output.appendingPathComponent("\(name)-\(url.pathExtension).png"))
        ActionRecorder.shared.start()
        try button(sheet.view,"Apply").performClick(nil)
        try waitUntil({ editor.view.window?.attachedSheet == nil && !BackgroundWork.occupied && DocumentJobs.active == nil },"\(file) Apply did not finish")
        pump()
        let records = ActionRecorder.shared.stop()
        try require(records.count == 1 && records[0].tool == "add_adjustment_layer","\(file): one add_adjustment_layer Action: \(records.map(\.tool))")
        try require(document.doc !== source && document.doc?.layerCount == source.layerCount + 1,"\(file) created no layer")
        guard let params = records[0].arguments["params"] as? [String:Any] else { throw DoctoredCoreError(message:"\(file) recorded no params") }
        switch url.pathExtension {
        case "acv":
            var channels: [CurveChannel: [CurvePoint]] = [:]
            for channel in CurveChannel.allCases { channels[channel] = CurvesMeta.points(of: channel, in: expected) }
            let want = CurvesMeta.params(channels)
            try require(Set(params.keys) == Set(want.keys),"\(file) channels \(params.keys.sorted()) != \(want.keys.sorted())")
            for (key, value) in want {
                try require((params[key] as? [[Double]]) == (value as? [[Double]]),"\(file) \(key) \(String(describing: params[key])) != \(value)")
            }
            try require(want["rgb"] != nil && want["b"] != nil,"\(file) lost its curves")
        case "alv":
            for key in ["black","white","gamma"] {
                try require(nearly(params[key], expected.number(key, default: .nan)),"\(file) \(key) \(String(describing: params[key])) != \(expected.params[key] ?? "nil")")
            }
        default:
            try require(params["colorize"] as? Bool == true,"\(file) lost colorize")
            for key in ["colorize_hue","colorize_saturation","colorize_lightness"] {
                try require(nearly(params[key], expected.number(key, default: .nan)),"\(file) \(key) \(String(describing: params[key])) != \(expected.params[key] ?? "nil")")
            }
        }
        applied.append(file)
    }
    // A CMYK Curves preset presents no sheet and says why, once.
    let (document,editor,source) = try openDocument(depth:depth,fixtures:fixtures)
    defer { document.close() }
    try waitUntil({ editor.view.window?.attachedSheet == nil },"Previous sheet did not detach")
    AutomationAlerts.reset()
    let before = editor.presentedViewControllers?.count ?? 0
    editor.newAdjustmentLayer(fromPhotoshopPreset: folder.appendingPathComponent("Press.acv"))
    pump()
    try require((editor.presentedViewControllers?.count ?? 0) == before && editor.view.window?.attachedSheet == nil,"CMYK preset presented a sheet")
    try require(AutomationAlerts.recorded.count == 1,"CMYK preset recorded \(AutomationAlerts.recorded.count) alerts")
    try require(document.doc === source,"CMYK preset changed the document")
    trace("PASS \(name)")
    return ["name":name,"passed":true,"applied":applied,"refused_alerts":AutomationAlerts.recorded.count]
}
