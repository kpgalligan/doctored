import AppKit
import ObjectiveC.runtime

func binReference(_ values: [UInt16], coverage: [UInt8]) -> [[Double]] {
    var bins = Array(repeating: Array(repeating: 0.0, count: 256), count: 3)
    for i in coverage.indices {
        guard values[i*4+3] > 0 else { continue }
        for channel in 0..<3 {
            let bin = Int((Double(values[i*4+channel])/257).rounded())
            bins[channel][bin] += Double(coverage[i])/255
        }
    }
    return bins
}
func matches(_ bins: HistogramBins?, _ expected: [[Double]]) -> Bool {
    guard let bins else { return false }
    return zip([bins.red,bins.green,bins.blue],expected).allSatisfy { actual, expected in
        zip(actual,expected).allSatisfy { abs($0-$1) < 1e-9 }
    }
}
func rgbCodes(_ field: NSTextField) -> [Double]? {
    let prefix = field.stringValue.split(separator: "/").first ?? ""
    let values = prefix.split(separator: ",").compactMap { Double($0.trimmingCharacters(in: .whitespaces)) }
    return values.count == 4 ? values : nil
}
func runInfo(depth: Int, fixtures: URL, output: URL) throws -> [String: Any] {
    let name = "info-ui-\(depth)"
    trace("START \(name)")
    let (document,editor,source) = try openDocument(depth:depth,fixtures:fixtures)
    defer { document.close() }
    let mask = try installSelection(editor,width:source.width,height:source.height)
    let original = try values(source)
    let data = try JSONSerialization.data(withJSONObject: ["document_id":AgentServer.shared.documentID(for:document),
        "layer":0,"op":"bcs","params":["brightness":0.2]],options:.sortedKeys)
    let reply = AgentServer.shared.execute(tool:"add_adjustment_layer",argumentsJSON:String(decoding:data,as:UTF8.self))
    try waitUntil({ !BackgroundWork.occupied && DocumentJobs.active == nil },"Info fixture adjustment failed")
    try require(document.doc.layerCount == 2,"Info fixture did not add adjustment: \(reply)")
    let graded = document.doc
    editor.setActiveLayer(0)
    editor.showInfo(nil)
    guard let panel = editor.infoPanel else { throw DoctoredCoreError(message:"Actual Info panel missing") }
    _ = panel.view
    let popup = try find(panel.view,"info.analysis.source",NSPopUpButton.self)
    let weight = try find(panel.view,"info.analysis.selection",NSButton.self)
    let histogram = try find(panel.view,"info.analysis.histogram",HistogramView.self)
    let rgb = try find(panel.view,"info.analysis.rgb",NSTextField.self)
    try require(weight.state == .on,"Info selection weighting default is off")
    popup.selectItem(withTag:1); try activate(popup)
    let selected = binReference(original,coverage:mask)
    try waitUntil({ matches(histogram.bins,selected) },"Actual selected layer histogram differs from independent counts")
    let mass = mask.reduce(0.0) { $0+Double($1)/255 }
    try require(abs((histogram.bins?.total ?? -1)-mass) < 1e-9,"Info fractional mass was rounded")
    try require(histogram.bins?.completeEndpoints == true,"Info endpoints do not describe complete scope")
    // Native1 is in display bin0 but does not count as exact clipping.
    var zero = [0.0,0.0,0.0], maximum = [0.0,0.0,0.0]
    for i in mask.indices { for c in 0..<3 {
        if original[i*4+c] == 0 { zero[c] += Double(mask[i])/255 }
        if original[i*4+c] == 65535 { maximum[c] += Double(mask[i])/255 }
    }}
    try require(abs((histogram.bins?.shadowClipped ?? -1)-(zero.max() ?? 0)) < 1e-9,"Info exact shadow counter differs")
    try require(abs((histogram.bins?.highlightClipped ?? -1)-(maximum.max() ?? 0)) < 1e-9,"Info exact highlight counter differs")
    let x=9,y=4,index=y*source.width+x
    let expected = (0..<4).map { Double(original[index*4+$0])/(depth == 8 ? 257 : 1) }
    editor.cursorMoved(to:CGPoint(x:x,y:y))
    try waitUntil({ rgbCodes(rgb) == expected },"Actual raw-layer cursor did not update")
    let scale = panel.view.window?.backingScaleFactor ?? 0
    try screenshot(panel.view,output.appendingPathComponent(name+"-selected.png"))
    weight.state = .off; try activate(weight)
    let whole = binReference(original,coverage:Array(repeating:255,count:mask.count))
    try waitUntil({ matches(histogram.bins,whole) },"Info whole-source toggle kept selected bins")
    weight.state = .on; try activate(weight)
    try waitUntil({ matches(histogram.bins,selected) },"Info selected toggle did not restore fractional bins")
    // Bounds stay unchanged, and the cursor does not move. Both the displayed
    // readout and any queued old readout must notice the changed coverage.
    var changed = mask; changed[index] = 0
    guard let replacement = CanvasSelection(shape:.mask(changed),canvasWidth:source.width,canvasHeight:source.height) else { throw DoctoredCoreError(message:"Replacement selection missing") }
    let bounds = editor.canvas.selection?.bounds
    editor.agentSetSelection(replacement)
    try require(editor.canvas.selection?.bounds == bounds,"Probe changed bounds instead of only coverage")
    try waitUntil({ rgb.stringValue == "—" },"Stationary cursor retained an excluded old selection readout")
    _ = try installSelection(editor,width:source.width,height:source.height)
    try waitUntil({ rgbCodes(rgb) == expected },"Stationary cursor did not recover with coverage")
    editor.setActiveLayer(1)
    try waitUntil({ rgb.stringValue == "—" },"Changing active layer retained prior raster readout")
    popup.selectItem(withTag:2); try activate(popup)
    try waitUntil({ matches(histogram.bins,selected) && rgbCodes(rgb) == expected },"Backdrop below adjustment did not restore source counts/readout")
    try screenshot(panel.view,output.appendingPathComponent(name+"-backdrop.png"))
    popup.selectItem(withTag:0); try activate(popup)
    try waitUntil({ histogram.bins != nil && !matches(histogram.bins,selected) },"Composite scope omitted the later adjustment")
    try require(document.doc === graded,"Read-only Info controls changed document pixels")
    trace("PASS \(name)")
    return ["name":name,"passed":true,"backing_scale":scale,"weighted_mass":mass,
            "same_bounds_readout_refresh":true,"active_layer_refresh":true]
}

func runCurves(depth: Int, fixtures: URL, output: URL) throws -> [String: Any] {
    let name = "curves-ui-\(depth)"
    trace("START \(name)")
    let (document,editor,source) = try openDocument(depth:depth,fixtures:fixtures)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    _ = try installSelection(editor,width:source.width,height:source.height)
    editor.newAdjustmentLayerCurves(nil)
    guard let sheet = editor.presentedViewControllers?.last as? CurvesAdjustmentSheetController else {
        throw DoctoredCoreError(message:"Actual Curves sheet missing")
    }
    _ = sheet.view
    try waitUntil({ sheet.view.window?.sheetParent != nil },"Curves sheet did not attach")
    let previewRenderer = try renderer(sheet)
    let point = try find(sheet.view,"tone.curves.point",NSPopUpButton.self)
    point.selectItem(at:1); try activate(point)
    let units = try find(sheet.view,"tone.curves.units",NSPopUpButton.self)
    units.selectItem(at:1); try activate(units)
    let outputField = try find(sheet.view,"tone.curves.output",NSTextField.self)
    let nativeValue = depth == 16 ? 61003.0 : 240.25
    outputField.stringValue = String(nativeValue); try activate(outputField)
    units.selectItem(at:0); try activate(units)
    try require(abs(outputField.doubleValue-nativeValue/(depth == 16 ? 65535 : 255)) < 1e-14,"Curve units changed normalized value")
    try waitUntil({ idle(previewRenderer) && editor.canvas.previewImage != nil },"Fine Curves preview failed")
    guard let preview = editor.canvas.previewImage else { throw DoctoredCoreError(message:"Curves preview missing") }
    let previewBytes = try displayed(preview)
    let scale = sheet.view.window?.backingScaleFactor ?? 0
    try screenshot(sheet.view,output.appendingPathComponent(name+"-sheet.png"))
    ActionRecorder.shared.start()
    try button(sheet.view,"Apply").performClick(nil)
    try waitUntil({ editor.view.window?.attachedSheet == nil && !BackgroundWork.occupied },"Curves Apply did not finish")
    pump()
    let records = ActionRecorder.shared.stop()
    guard let edited = document.doc, let image = document.previewImage else { throw DoctoredCoreError(message:"Curves applied result missing") }
    let expected = try values(edited)
    try require(edited.layerCount == 2 && records.count == 1 && records[0].tool == "add_adjustment_layer","Curves creates one recorded layer")
    try require(try displayed(image) == previewBytes,"Fine Curves preview differs from applied output")
    guard let params = records[0].arguments["params"] as? [String:Any],
          let points = params["rgb"] as? [[Double]], let end = points.last else {
        throw DoctoredCoreError(message:"Curve Action omitted control points")
    }
    let historical = nativeValue/(depth == 16 ? 257 : 1)
    try require(abs(end[1]-historical) < 1e-12,"Curve Action rounded or reinterpreted historical coordinates")
    let path = output.appendingPathComponent(name+".rz")
    var save: [String:Any] = ["document_id":AgentServer.shared.documentID(for:document),"path":path.path]
    var data = try JSONSerialization.data(withJSONObject:save,options:.sortedKeys)
    _ = AgentServer.shared.execute(tool:"save_copy",argumentsJSON:String(decoding:data,as:UTF8.self))
    let reopened: RasterDocument
    if depth == 16 {
        let budget = try PrecisionWorkContext()
        reopened = try RasterDocument(precision:RasterPrecisionDocument.openPreserving(url:path,workingLimit:budget.limit,otherRetained:budget.otherRetained))
    } else { reopened = try RasterDocument.open(url:path) }
    try require(try values(reopened) == expected,"Curves native reopen changed result")
    document.undoManager?.undo(); pump()
    try require(document.doc === source,"Curves Undo did not restore source")
    save = records[0].arguments; save["document_id"] = AgentServer.shared.documentID(for:document)
    data = try JSONSerialization.data(withJSONObject:save,options:.sortedKeys)
    let response = AgentServer.shared.execute(tool:"add_adjustment_layer",argumentsJSON:String(decoding:data,as:UTF8.self))
    try waitUntil({ !BackgroundWork.occupied },"Curves Action did not finish")
    try require(try values(document.doc ?? source) == expected,"Curves replay differs: \(response)")
    trace("PASS \(name)")
    return ["name":name,"passed":true,"backing_scale":scale,"historical_white_output":end[1],"native_components":expected.count]
}

func runAuto(depth: Int, fixtures: URL, output: URL) throws -> [String: Any] {
    let name = "auto-ui-\(depth)"
    trace("START \(name)")
    let (document,editor,source) = try openDocument(depth:depth,fixtures:fixtures)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    guard let area = CanvasSelection(shape:.rect(CGRect(x:4,y:3,width:14,height:9)),
                                     canvasWidth:source.width,canvasHeight:source.height) else {
        throw DoctoredCoreError(message:"Auto analysis fixture selection missing")
    }
    editor.agentSetSelection(area)
    let mask = area.maskBytes()
    let original = try values(source)
    var records: [[String:Any]] = []
    for tool in ["auto_tone","auto_contrast","auto_color"] {
        var whole: [UInt16] = []
        for selected in [false,true] {
            ActionRecorder.shared.start()
            let answers = try answerAlertSheets(with: selected ? "Selection" : "Whole Target") {
                switch tool {
                case "auto_tone": editor.autoTone(nil)
                case "auto_contrast": editor.autoContrast(nil)
                default: editor.autoColor(nil)
                }
            }
            try waitUntil({ answers.answered == 1 },"Auto scope choice not presented")
            try require(answers.prompts == [["Whole Target","Selection","Cancel"]],"Auto scope prompt buttons: \(answers.prompts)")
            try waitUntil({ document.doc !== source && editor.view.window?.attachedSheet == nil && !BackgroundWork.occupied },"Auto choice did not publish")
            pump()
            let action = ActionRecorder.shared.stop()
            let expected = try values(document.doc)
            try require(action.count == 1 && action[0].tool == tool,"Auto choice records one correct tool")
            try require(action[0].arguments["analysis_scope"] as? String == (selected ? "selection" : "whole_target"),"Auto choice lost analysis scope")
            for i in mask.indices where mask[i] == 0 {
                try require(Array(expected[i*4..<i*4+4]) == Array(original[i*4..<i*4+4]),"Auto changed samples outside write selection")
            }
            if selected { try require(expected != whole,"Auto analysis choices must differ on this fixture: \(tool)") }
            else { whole = expected }
            document.undoManager?.undo(); pump()
            try require(document.doc === source && !document.isDocumentEdited,"Auto Undo did not restore saved source")
            var arguments = action[0].arguments
            arguments["document_id"] = AgentServer.shared.documentID(for:document)
            let data = try JSONSerialization.data(withJSONObject:arguments,options:.sortedKeys)
            let response = AgentServer.shared.execute(tool:tool,argumentsJSON:String(decoding:data,as:UTF8.self))
            try waitUntil({ !BackgroundWork.occupied },"Auto replay did not finish")
            try require(try values(document.doc) == expected,"Auto UI/MCP replay differs: \(response)")
            document.undoManager?.undo(); pump()
            records.append(["tool":tool,"arguments":action[0].arguments])
        }
    }
    let cancelled = try answerAlertSheets(with: "Cancel") { editor.autoTone(nil) }
    try waitUntil({ cancelled.answered == 1 },"Auto cancel choice missing")
    try require(cancelled.prompts == [["Whole Target","Selection","Cancel"]],"Auto cancel prompt buttons: \(cancelled.prompts)")
    pump()
    let scale = editor.view.window?.backingScaleFactor ?? 0
    try require(document.doc === source,"Cancelled Auto changed source")
    trace("PASS \(name)")
    return ["name":name,"passed":true,"backing_scale":scale,"scope_cases":records]
}

private final class AlertSheetAnswers {
    var prompts: [[String]] = []
    var answered = 0
}

// Answers every NSAlert sheet begun inside `body` with the button titled
// `title` (`.abort` when absent), without attaching a sheet: a temporary IMP
// over the CURRENT one (the psd.swift swap-and-restore pattern), so it stacks
// on AutomationAlerts' first-button hook and restores it afterwards.
private func answerAlertSheets(with title: String, _ body: () throws -> Void) throws -> AlertSheetAnswers {
    let selector = #selector(NSAlert.beginSheetModal(for:completionHandler:))
    guard let method = class_getInstanceMethod(NSAlert.self, selector) else {
        throw DoctoredCoreError(message: "NSAlert sheet interception unavailable")
    }
    let answers = AlertSheetAnswers()
    let interceptor: @convention(block) (NSAlert, NSWindow, (@convention(block) (Int) -> Void)?) -> Void = {
        alert, _, handler in
        let titles = alert.buttons.map(\.title)
        answers.prompts.append(titles)
        let response = titles.firstIndex(of: title)
            .map { NSApplication.ModalResponse.alertFirstButtonReturn.rawValue + $0 }
            ?? NSApplication.ModalResponse.abort.rawValue
        RunLoop.main.perform(inModes: [.common]) {
            handler?(response)
            answers.answered += 1
        }
    }
    let replacement = imp_implementationWithBlock(interceptor)
    let original = method_setImplementation(method, replacement)
    defer { method_setImplementation(method, original); imp_removeBlock(replacement) }
    try body()
    return answers
}
