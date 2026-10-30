import AppKit

func verifyNativeSelections(fixtures: URL, output: URL) throws -> [[String: Any]] {
    var groups: [[String: Any]] = []
    for depth in [8, 16] {
        let (document, editor, source) = try openDocument(depth: depth, fixtures: fixtures)
        defer { if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }; document.close() }
        let width = source.width, height = source.height
        let originalPixels = try values(source)
        let codes = (0..<(width * height)).map { index -> UInt16 in [0, 1, 129, 32769, 65534, 65535][index % 6] }
        document.setCanvasSelection(CanvasSelection(shape: .mask16(codes), canvasWidth: width, canvasHeight: height))
        document.undoManager?.removeAllActions()
        let byteReplacement = codes.map { UInt8((UInt32($0) + 128) / 257) }
        let replacementCapture = try SelectionCapture(document: document, needsSource: false)
        try replacementCapture.commit(.mask(byteReplacement), in: document, name: "Declared byte replacement")
        try require(document.canvasSelection?.maskSamples16() == byteReplacement.map { UInt16($0) * 257 },
            "Explicit byte replacement was mistaken for native identity")
        document.undoManager?.undo(); pump()
        try require(document.canvasSelection?.maskSamples16() == codes, "Replacement Undo lost original native coverage")
        document.undoManager?.removeAllActions()
        groups.append(["name":"native-byte-replacement-\(depth)","passed":true])
        editor.invertSelection(nil)
        try require(document.canvasSelection?.maskSamples16() == codes.map { 65535 - $0 }, "Native UI invert reduced coverage")
        document.undoManager?.undo(); pump()
        try require(document.canvasSelection?.maskSamples16() == codes, "Selection invert Undo lost native codes")
        // The real morphology sheet captures the exact selection. Radius0 is an
        // independent identity reference including values that display as zero.
        editor.featherSelection(nil)
        guard let morph = editor.presentedViewControllers?.last else { throw DoctoredCoreError(message: "Native morphology sheet missing") }
        _ = morph.view
        let amount = try find(morph.view, "nativeSelection.radius", NSTextField.self)
        amount.stringValue = "0"
        try screenshot(morph.view, output.appendingPathComponent("selection-morph-\(depth).png"))
        try button(morph.view, "Apply").performClick(nil)
        try waitUntil({ editor.view.window?.attachedSheet == nil && !BackgroundWork.occupied }, "Native morphology Apply stalled")
        try require(document.canvasSelection?.maskSamples16() == codes, "Native radius-zero morphology was not identity")
        try require(document.undoManager?.canUndo != true, "Native radius-zero morphology created phantom Undo")
        groups.append(["name":"native-morph-ui-\(depth)","passed":true])

        editor.transformSelection(nil)
        guard let transform = editor.presentedViewControllers?.last as? SelectionTransformSheetController else { throw DoctoredCoreError(message: "Transform Selection UI missing") }
        _ = transform.view
        let move = try find(transform.view, "selectionTransform.translate_x", NSTextField.self)
        move.stringValue = "1"
        guard let popup = member(transform,"filterPopup") as? NSPopUpButton else { throw DoctoredCoreError(message:"Transform sampler missing") }
        popup.selectItem(at:0); try activate(move)
        let preview = try renderer(transform)
        try waitUntil({ idle(preview) }, "Native selection preview did not settle")
        let expected = (0..<(width*height)).map { $0 % width == 0 ? UInt16(0) : codes[$0-1] }
        guard let maskView = member(transform,"maskView") as? SelectionPreviewView, let image = maskView.image else { throw DoctoredCoreError(message:"Native selection preview missing") }
        let pixels = try displayed(image)
        for index in expected.indices { try require(pixels[index*4] == UInt8((UInt32(expected[index])+128)/257), "Native transform preview disagrees with exact translated codes") }
        try screenshot(transform.view, output.appendingPathComponent("selection-transform-\(depth).png"))
        try button(transform.view,"Apply").performClick(nil)
        try waitUntil({ editor.view.window?.attachedSheet == nil && !BackgroundWork.occupied },"Native transform Apply stalled")
        try require(document.canvasSelection?.maskSamples16() == expected, "Native translation lost coverage")
        document.undoManager?.undo(); pump()
        try require(document.canvasSelection?.maskSamples16() == codes, "Native transform one Undo")
        document.undoManager?.redo(); pump()
        try require(document.canvasSelection?.maskSamples16() == expected, "Native transform exact Redo")
        groups.append(["name":"native-transform-ui-\(depth)","passed":true])

        editor.showColorRange(nil)
        guard let range = editor.presentedViewControllers?.last as? ColorRangeSheetController else { throw DoctoredCoreError(message:"Actual Color Range UI missing") }
        _ = range.view
        range.sample(CGPoint(x:5,y:6))
        let rangeRenderer = try renderer(range)
        try waitUntil({ idle(rangeRenderer) },"Color Range preview stalled")
        if depth == 16 {
            let p=(6*width+5)*4
            try require(range.parameters.samples16 == [originalPixels[p..<p+3].map(Int.init)], "Color Range sampled display RGB instead of native source")
        }
        guard let rangeMaskView = member(range,"maskView") as? SelectionPreviewView, rangeMaskView.image != nil else { throw DoctoredCoreError(message:"Color Range preview absent") }
        try screenshot(range.view,output.appendingPathComponent("selection-range-\(depth).png"))
        try button(range.view,"Apply").performClick(nil)
        try waitUntil({ editor.view.window?.attachedSheet == nil && !BackgroundWork.occupied },"Color Range Apply stalled")
        try require(document.canvasSelection != nil,"Color Range selected nothing at its exact sample")
        try require(depth == 8 || document.canvasSelection?.hasNativeCoverage == true,"Color Range lost native coverage")
        try require(try values(document.doc) == originalPixels,"Selection UI rewrote native source RGB")
        groups.append(["name":"color-range-ui-\(depth)","passed":true])

        let constant=[UInt16](repeating:129,count:width*height)
        document.setCanvasSelection(CanvasSelection(shape:.mask16(constant),canvasWidth:width,canvasHeight:height))
        editor.toggleQuickMask(nil); pump()
        try require(document.quickMaskActive,"Actual Quick Mask mode did not enter")
        document.undoManager?.removeAllActions()
        editor.selectTool(.brush); editor.canvas.brushSize=4; editor.canvas.brushOpacity=1
        guard let window=editor.canvas.window else { throw DoctoredCoreError(message:"Quick Mask canvas window missing") }
        let point=editor.canvas.convert(CGPoint(x:8,y:6),to:nil)
        ActionRecorder.shared.start()
        for type in [NSEvent.EventType.leftMouseDown,.leftMouseUp] {
            guard let event=NSEvent.mouseEvent(with:type,location:point,modifierFlags:[],timestamp:ProcessInfo.processInfo.systemUptime,
                windowNumber:window.windowNumber,context:nil,eventNumber:1,clickCount:1,pressure:1) else { throw DoctoredCoreError(message:"Native gesture event missing") }
            if type == .leftMouseDown { editor.canvas.mouseDown(with:event) } else { editor.canvas.mouseUp(with:event) }
        }
        try waitUntil({ !BackgroundWork.occupied },"Quick Mask native writeback did not finish")
        let recorded=ActionRecorder.shared.stop()
        guard let painted=document.canvasSelection?.maskSamples16() else { throw DoctoredCoreError(message:"Quick Mask coverage missing") }
        try require(painted != constant && painted[0] == 129 && painted.contains(65535),"Actual Quick Mask did not preserve tiny outside coverage; min=\(painted.min() ?? 0), max=\(painted.max() ?? 0), first=\(painted.first ?? 0), changed=\(painted != constant)")
        try require(recorded.count == 1 && recorded[0].tool == "quick_mask_stroke","Quick Mask must record one canonical coverage Action")
        try require(!document.isDocumentEdited,"Selection/Quick Mask changed native saved identity")
        document.undoManager?.undo(); pump()
        try require(document.canvasSelection?.maskSamples16() == constant,"Quick Mask one Undo lost tiny native coverage")
        document.undoManager?.redo(); pump()
        try require(document.canvasSelection?.maskSamples16() == painted,"Quick Mask Redo changed samples")
        editor.toggleQuickMask(nil); pump()
        try require(document.canvasSelection?.maskSamples16() == painted,"Exit Quick Mask reduced selection coverage")
        try require(try values(document.doc) == originalPixels,"Quick Mask rewrote source samples")
        groups.append(["name":"quick-mask-native-ui-\(depth)","passed":true,"canonical_runs":recorded.count])
    }
    groups.append(contentsOf: try verifyNativeChannelMath(fixtures: fixtures, output: output))
    return groups
}

func verifyNativeChannelMath(fixtures: URL, output: URL) throws -> [[String: Any]] {
    let (document, editor, source) = try openDocument(depth:16,fixtures:fixtures)
    defer { if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }; document.close() }
    let width=source.width, height=source.height, n=width*height
    let original=try values(source)
    let budgetCapture = try PrecisionEditorCapture(document)
    var budgetParameters = ApplyImageParameters()
    budgetParameters.source = 0; budgetParameters.sourcePlane = .plane(.red)
    budgetParameters.target = .plane(.red)
    budgetParameters.blend = DR_BLEND_MULTIPLY
    let budget = try NativeChannelMath.scratch(source, apply: budgetParameters)
    let small = try RasterJobControl()
    try require(small.setScratchLimit(1_048_576), "Could not set channel budget refusal witness")
    var refused = false
    do { _ = try NativeChannelMath.apply(budgetCapture, parameters: budgetParameters, control: small) }
    catch { refused = true; try require(error.localizedDescription.contains("working memory"), "Unexpected channel budget refusal: \(error)") }
    try require(refused && document.doc === source, "Under-budget channel math changed the document")
    let bounded = try PrecisionCapturedWork.run(scratchEstimate: budget, title: "Native channel budget regression") {
        try NativeChannelMath.apply(budgetCapture, parameters: budgetParameters, control: $0)
    }
    guard let bounded else { throw DoctoredCoreError(message: "Planned channel math did not produce its independent witness") }
    let boundedPixels = try values(RasterDocument(precision: bounded))
    for i in 0..<n {
        let expected = (Double(original[i*4]) * Double(original[i*4]) / 65535).rounded()
        try require(abs(Double(boundedPixels[i*4]) - expected) <= 1, "Planned channel math changed the arithmetic")
        try require(boundedPixels[i*4+1..<i*4+4] == original[i*4+1..<i*4+4], "Planned scalar math changed another component")
    }
    let weights=(0..<n).map { [UInt16(0),1,32769,65535][$0%4] }
    document.setCanvasSelection(CanvasSelection(shape:.mask16(weights),canvasWidth:width,canvasHeight:height))
    document.undoManager?.removeAllActions()
    editor.applyImageSheet(nil)
    guard let apply=editor.presentedViewControllers?.last as? ApplyImageSheetController else { throw DoctoredCoreError(message:"Actual Apply Image sheet missing") }
    _=apply.view
    guard let blend=member(apply,"blendPopup") as? NSPopUpButton else { throw DoctoredCoreError(message:"Apply Image blend control missing") }
    blend.selectItem(withTag:Int(DR_BLEND_MULTIPLY.rawValue)); try activate(blend)
    let previewRenderer=try renderer(apply)
    try waitUntil({ idle(previewRenderer) && editor.canvas.previewImage != nil },"Apply Image native preview missing")
    guard let applyPreview = editor.canvas.previewImage else { throw DoctoredCoreError(message:"Apply Image native preview absent") }
    let shown=try displayed(applyPreview)
    try screenshot(apply.view,output.appendingPathComponent("apply-image-native.png"))
    ActionRecorder.shared.start()
    try button(apply.view,"Apply").performClick(nil)
    try waitUntil({ editor.view.window?.attachedSheet == nil && !BackgroundWork.occupied },"Apply Image native commit stalled")
    let steps=ActionRecorder.shared.stop()
    let changed=try values(document.doc)
    try require(changed != original,"Apply Image witness must change")
    for i in 0..<n {
        for c in 0..<3 {
            let value=Double(original[i*4+c]), t=Double(weights[i])/65535
            let expected=(value*(1-t)+(value*value/65535).rounded()*t).rounded()
            try require(abs(Double(changed[i*4+c])-expected) <= (weights[i] == 0 ? 0 : 1),"Independent native selected Multiply reference")
        }
        try require(changed[i*4+3] == original[i*4+3],"Apply Image RGB altered alpha")
    }
    try require(steps.count == 1 && steps[0].tool == "apply_image","Apply Image canonical Action")
    guard let display=document.previewImage else { throw DoctoredCoreError(message:"Apply Image display missing") }
    try require(try displayed(display) == shown,"Apply Image preview/commit mismatch")
    document.undoManager?.undo(); pump()
    try require(document.doc === source,"Apply Image one Undo")
    var arguments=steps[0].arguments; arguments["document_id"]=AgentServer.shared.documentID(for:document)
    let response=AgentServer.shared.execute(tool:"apply_image",argumentsJSON:String(decoding:try JSONSerialization.data(withJSONObject:arguments),as:UTF8.self))
    try require(try values(document.doc) == changed,"Apply Image Action replay mismatch: \(response)")
    document.undoManager?.undo(); pump()

    editor.calculationsSheet(nil)
    guard let calculations=editor.presentedViewControllers?.last as? CalculationsSheetController else { throw DoctoredCoreError(message:"Actual Calculations sheet missing") }
    _=calculations.view
    for (key,plane) in [("plane1Popup",RasterPlane.red),("plane2Popup",RasterPlane.blue)] {
        guard let popup=member(calculations,key) as? NSPopUpButton,
              let item=popup.itemArray.first(where:{ ($0.representedObject as? PlaneChoice) == .plane(plane) }) else { throw DoctoredCoreError(message:"Calculations source plane control missing") }
        popup.select(item); try activate(popup)
    }
    guard let calcBlend=member(calculations,"blendPopup") as? NSPopUpButton,
          let result=member(calculations,"resultPopup") as? NSPopUpButton else { throw DoctoredCoreError(message:"Calculations result controls missing") }
    calcBlend.selectItem(withTag:Int(DR_BLEND_MULTIPLY.rawValue)); try activate(calcBlend)
    result.selectItem(at:1); try activate(result)
    let calcRenderer=try renderer(calculations)
    try waitUntil({ idle(calcRenderer) && editor.canvas.previewImage != nil },"Calculations native preview missing")
    try screenshot(calculations.view,output.appendingPathComponent("calculations-native.png"))
    ActionRecorder.shared.start()
    try button(calculations.view,"Apply").performClick(nil)
    try waitUntil({ editor.view.window?.attachedSheet == nil && !BackgroundWork.occupied },"Calculations native Apply stalled")
    let calculationSteps=ActionRecorder.shared.stop()
    guard let selected=document.canvasSelection?.maskSamples16() else { throw DoctoredCoreError(message:"Calculations native selection missing") }
    for i in 0..<n {
        let expected=(Double(original[i*4])*Double(original[i*4+2])/65535).rounded()
        try require(abs(Double(selected[i])-expected) <= 1,"Independent native Calculations Multiply reference")
    }
    try require(document.doc === source,"Calculations Selection rewrote source document")
    try require(calculationSteps.count == 1 && calculationSteps[0].tool == "calculations","Calculations one Action")
    document.undoManager?.undo(); pump()
    try require(document.canvasSelection?.maskSamples16() == weights,"Calculations one selection Undo")
    var replay=calculationSteps[0].arguments; replay["document_id"]=AgentServer.shared.documentID(for:document)
    let replayResponse=AgentServer.shared.execute(tool:"calculations",argumentsJSON:String(decoding:try JSONSerialization.data(withJSONObject:replay),as:UTF8.self))
    try require(document.canvasSelection?.maskSamples16() == selected,"Calculations Action mismatch: \(replayResponse)")

    ActionRecorder.shared.start()
    editor.addLuminosityMasks(nil)
    try waitUntil({ !BackgroundWork.occupied },"Luminosity set stalled")
    let luminositySteps=ActionRecorder.shared.stop()
    try require(document.doc.channelCount == 9,"All nine native luminosity masks must land")
    for (band,name) in ["Lights","Darks","Midtones"].enumerated() {
        for level in 1...3 {
            let index=band*3+level-1
            try require(document.doc.channelInfo(index)?.name == "\(name) \(level)","Native luminosity channel order")
            let values=try PrecisionScalarOperations.read(document.doc,target:"channel",index:index).samples
            for i in 0..<n {
                // The source luminance is a native plane with one scalar
                // quantization, then the shared luminosity ladder evaluates it.
                let code=(0.2126*Double(original[i*4])+0.7152*Double(original[i*4+1])+0.0722*Double(original[i*4+2])).rounded()
                let l=code/65535
                let expected=band == 0 ? pow(l,Double(level)) : band == 1 ? pow(1-l,Double(level)) : max(0,1-pow(l,Double(level+1))-pow(1-l,Double(level+1)))
                try require(abs(Double(values[i])-(expected*65535).rounded()) <= 2,"Independent nine-mask luminosity reference")
            }
        }
    }
    try require(luminositySteps.count == 1 && luminositySteps[0].tool == "add_luminosity_masks","One complete luminosity Action")
    try require(try values(document.doc) == original,"Luminosity masks rewrote source RGB")
    document.undoManager?.undo(); pump()
    try require(document.doc === source,"All nine luminosity masks must undo together")
    return [["name":"native-channel-budget","passed":true,"reserved_bytes":budget],
        ["name":"apply-image-native-ui","passed":true],["name":"calculations-native-ui","passed":true],["name":"luminosity-native-ui","passed":true]]
}
