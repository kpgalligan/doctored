import AppKit

func verifyScalarHistogram(fixtures: URL, output: URL) throws -> [[String:Any]] {
    let (document,editor,original)=try openDocument(depth:16,fixtures:fixtures)
    defer {document.close();pump()}
    let width=original.width,height=original.height,n=width*height
    let pixels=try values(original)
    let channel=(0..<n).map {[UInt16(0),1,129,257,32769,65534,65535][$0%7]}
    document.applyEdit("Histogram Channel",record:.notACommand) {
        try? PrecisionScalarOperations.channel($0,name:"Histogram",samples:channel,width:width,height:height)
    }
    let mw=width-4,mh=height-3
    let mask=(0..<(mw*mh)).map {[UInt16(0),17,129,32769,65534,65535][$0%6]}
    let capture=try PrecisionEditorCapture(document)
    let create=try PrecisionPixelEdit(capture.source,command:.setMask(layer:0,width:mw,height:mh,x:2,y:-1,linked:false),
        input:PrecisionScalarBuffer(samples:mask,width:mw,height:mh))
    _ = try capture.publish(create.run(),to:document,name:"Histogram Mask")
    let doc=document.doc ?? original
    let weights=(0..<n).map {[UInt16(0),1,17,32769,65534,65535][$0%6]}
    let selection=CanvasSelection(shape:.mask16(weights),canvasWidth:width,canvasHeight:height)
    let targets:[PaintTarget]=[.channel(0),.mask,.plane(.red),.plane(.green),.plane(.blue),.plane(.alpha)]
    var groups:[[String:Any]]=[]
    editor.panelTab=0;editor.updatePanelVisibility();pump()
    guard let panel=editor.layersPanel,let table=member(panel,"tableView") as? NSTableView else {
        throw DoctoredCoreError(message:"Actual Layers panel missing")
    }
    panel.reload()
    guard let cell=table.view(atColumn:0,row:0,makeIfNecessary:true),
          let thumb=member(cell,"thumbView") as? ThumbnailImageView,
          let maskThumb=member(cell,"maskView") as? ThumbnailImageView,
          let rgbImage=thumb.image?.cgImage(forProposedRect:nil,context:nil,hints:nil),
          let maskImage=maskThumb.image?.cgImage(forProposedRect:nil,context:nil,hints:nil) else {
        throw DoctoredCoreError(message:"Actual native layer/mask thumbnails were empty")
    }
    try require(try displayed(rgbImage)==pixels.map {UInt8((UInt32($0)+128)/257)},"Native layer thumbnail differs from explicit source display")
    let maskDisplay=mask.flatMap {value -> [UInt8] in
        let gray=UInt8((UInt32(value)+128)/257);return [gray,gray,gray,255]
    }
    try require(try displayed(maskImage)==maskDisplay,"Native raw mask thumbnail differs from stored coverage")
    try screenshot(panel.view,output.appendingPathComponent("native-layer-mask-thumbnails.png"))
    groups.append(["name":"native-layer-mask-thumbnails","passed":true])
    for target in targets {
        let samples:[UInt16],rw:Int,rh:Int,ox:Int,oy:Int
        switch target {
        case .channel:samples=channel;rw=width;rh=height;ox=0;oy=0
        case .mask:samples=mask;rw=mw;rh=mh;ox=2;oy = -1
        case .plane(let plane):
            let c=plane == .red ? 0:plane == .green ? 1:plane == .blue ? 2:3
            samples=(0..<n).map {pixels[$0*4+c]};rw=width;rh=height;ox=0;oy=0
        case .layer,.ink:throw DoctoredCoreError(message:"Invalid scalar fixture target")
        }
        for selected in [false,true] {
            for stride in [1,7] {
                let local=(0..<(rw*rh)).map { i -> UInt16 in
                    guard selected else{return 65535}
                    let x=i%rw+ox,y=i/rw+oy
                    return (0..<width).contains(x) && (0..<height).contains(y) ? weights[y*width+x]:0
                }
                let read=try NativeScalarStatisticsRead(source:doc,target:target,layer:0,selection:selected ? selection:nil,stride:stride)
                let report=try read.run()
                try compareScalarStatistics(report,samples:samples,weights:local,stride:stride)
                var arguments:[String:Any]=["source":"scalar","target":target.agentName(in:doc),"layer":0,
                    "selection":selected,"stride":stride,"document_id":AgentServer.shared.documentID(for:document)]
                editor.agentSetSelection(selection)
                let data=try JSONSerialization.data(withJSONObject:arguments,options:.sortedKeys)
                let response=AgentServer.shared.execute(tool:"histogram",argumentsJSON:String(decoding:data,as:UTF8.self))
                guard let envelope=try JSONSerialization.jsonObject(with:Data(response.utf8)) as? [String:Any],
                      envelope["isError"] as? Bool != true,
                      let content=envelope["content"] as? [[String:Any]],let text=content.first?["text"] as? String,
                      let parsed=try JSONSerialization.jsonObject(with:Data(text.utf8)) as? [String:Any] else {
                    throw DoctoredCoreError(message:"Scalar histogram MCP response missing: \(response)")
                }
                try compareScalarStatistics(parsed,samples:samples,weights:local,stride:stride)
                arguments.removeAll()
            }
        }
        editor.agentSetSelection(selection);editor.setPaintTarget(target)
        let sheet=try openSheet(editor,kind:"levels")
        guard let plot=member(sheet,"plot") as? HistogramView else {throw DoctoredCoreError(message:"Actual scalar Levels histogram missing")}
        try waitUntil({plot.bins != nil},"Native scalar sheet histogram did not arrive")
        let local=(0..<(rw*rh)).map {i -> UInt16 in
            let x=i%rw+ox,y=i/rw+oy
            return (0..<width).contains(x) && (0..<height).contains(y) ? weights[y*width+x]:0
        }
        var expected=[UInt64](repeating:0,count:256),mass:UInt64=0,black:UInt64=0,white:UInt64=0
        for i in samples.indices {
            let w=UInt64(local[i]);expected[Int((UInt32(samples[i])+128)/257)]+=w;mass+=w
            if samples[i]==0{black+=w};if samples[i]==65535{white+=w}
        }
        guard let bins=plot.bins else {throw DoctoredCoreError(message:"Scalar histogram vanished")}
        let binsExpected=expected.map {Double($0)/65535}
        let capturedTarget = (member(sheet,"precisionSession") as? PrecisionAdjustmentSession)?
            .histogramCapture.target.agentName(in:doc) ?? "missing"
        try require(capturedTarget == target.agentName(in:doc), "Scalar sheet captured another target")
        // Native counts below remain exact integers. Only normalized plot
        // doubles cross Rust JSON/Foundation decimal decoding; that transport
        // can move a finite positive value by a few ULPs. Zero stays exact.
        func displayULP(_ actual: Double, _ expected: Double) -> UInt64 {
            guard actual.isFinite, actual >= 0 else { return UInt64.max }
            guard expected != 0 else { return actual == 0 ? 0 : UInt64.max }
            let a = actual.bitPattern, b = expected.bitPattern
            return a >= b ? a-b : b-a
        }
        let maximumDisplayULP = binsExpected.indices.reduce(UInt64(0)) { largest, i in
            max(largest, max(displayULP(bins.red[i],binsExpected[i]),
                max(displayULP(bins.green[i],binsExpected[i]),displayULP(bins.blue[i],binsExpected[i]))))
        }
        if maximumDisplayULP > 4 {
            let differences = binsExpected.indices.filter { i in
                max(displayULP(bins.red[i],binsExpected[i]), max(displayULP(bins.green[i],binsExpected[i]),
                    displayULP(bins.blue[i],binsExpected[i]))) > 4
            }.map { i -> [String:Any] in
                ["bin":i,"expected":binsExpected[i],"red":bins.red[i],"green":bins.green[i],"blue":bins.blue[i],
                 "red_error":bins.red[i]-binsExpected[i]]
            }
            let detail:[String:Any] = ["target":target.agentName(in:doc),"captured_target":capturedTarget,
                "document_target":document.planeEditTarget.agentName(in:doc),"differences":differences]
            try JSONSerialization.data(withJSONObject:detail,options:[.prettyPrinted,.sortedKeys])
                .write(to:output.appendingPathComponent("scalar-histogram-differences.json"))
            throw DoctoredCoreError(message:"Scalar sheet histogram mismatch: \(detail)")
        }
        try require(bins.total==Double(mass)/65535 && bins.exactShadow==Double(black)/65535 && bins.exactHighlight==Double(white)/65535,
            "Scalar sheet histogram endpoint counts lost exact selection weights")
        try screenshot(sheet.view,output.appendingPathComponent("scalar-histogram-\(target.agentName(in:doc).replacingOccurrences(of:":",with:"-")).png"))
        try button(sheet.view,"Cancel").performClick(nil)
        try waitUntil({editor.view.window?.attachedSheet==nil},"Scalar histogram sheet Cancel stalled")
        // Synchronous retargeting before the queued delivery must clear the
        // result, even though it was computed from a valid immutable source.
        let stale=try PrecisionEditorCapture(document)
        var delivered=false,staleBins:HistogramBins?
        Histogram.loadForSheet(document,mode:.destructive,destructiveLayer:0,precisionCapture:stale) {value in
            staleBins=value;delivered=true
        }
        editor.setPaintTarget(.layer)
        try waitUntil({delivered},"Stale scalar histogram delivery did not drain")
        try require(staleBins==nil,"Stale scalar histogram was delivered after target changed")
        groups.append(["name":"scalar-histogram-\(target.agentName(in:doc))","passed":true,"raw_samples":samples.count,"scope_stride_cases":4,
            "maximum_normalized_display_ulp":maximumDisplayULP,"normalized_display_limit_ulp":4,
            "native_integer_counts":"exact","normalized_zero":"exact"])
    }
    return groups
}

func compareScalarStatistics(_ report:[String:Any],samples:[UInt16],weights:[UInt16],stride:Int) throws {
    guard let rows=report["bin_weight_units"] as? [[NSNumber]],rows.count==4,
          let zero=report["exact_zero_weight_units_rgba"] as? [NSNumber],zero.count==4,
          let max=report["exact_max_weight_units_rgba"] as? [NSNumber],max.count==4,
          let sums=report["weighted_code_sums_rgba"] as? [NSNumber],sums.count==4 else {
        throw DoctoredCoreError(message:"Native scalar histogram exact fields missing: \(report)")
    }
    var bins=[UInt64](repeating:0,count:256),mass:UInt64=0,black:UInt64=0,white:UInt64=0,sum:UInt64=0
    for i in samples.indices {
        let w=UInt64(weights[i]);mass+=w;sum+=UInt64(samples[i])*w
        if samples[i]==0{black+=w};if samples[i]==65535{white+=w}
        if i%stride==0{bins[Int((UInt32(samples[i])+128)/257)]+=w}
    }
    try require(rows[0].map(\.uint64Value)==bins && rows[1].map(\.uint64Value)==bins && rows[2].map(\.uint64Value)==bins,"Independent scalar display bin weights")
    try require(zero.prefix(3).allSatisfy {$0.uint64Value==black} && max.prefix(3).allSatisfy {$0.uint64Value==white},"Independent scalar exact endpoint weights")
    try require(sums.prefix(3).allSatisfy {$0.uint64Value==sum},"Independent scalar weighted code sums")
    try require((report["full_rgb_weight_units"] as? NSNumber)?.uint64Value==mass,"Independent scalar total mass")
    try require(report["source"] as? String=="scalar" && (report["coverage_denominator"] as? NSNumber)?.intValue==65535,"Scalar statistics schema/scope")
}
