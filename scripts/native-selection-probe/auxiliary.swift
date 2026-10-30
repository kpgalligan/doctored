import AppKit
import ObjectiveC.runtime
import CoreVideo
import ImageIO
import AVFoundation

func verifyAuxiliaryMattes(output: URL) throws -> [[String: Any]] {
    let formats: [(OSType, String, Int, Bool, Bool)] = [
        (kCVPixelFormatType_OneComponent8,"u8",1,false,false),
        (kCVPixelFormatType_OneComponent16,"u16",2,false,false),
        (kCVPixelFormatType_OneComponent16Half,"half",2,false,false),
        (kCVPixelFormatType_OneComponent32Float,"float",4,false,false),
        (kCVPixelFormatType_DisparityFloat16,"disparity-half",2,true,false),
        (kCVPixelFormatType_DisparityFloat32,"disparity-float",4,true,false),
        (kCVPixelFormatType_DepthFloat16,"depth-half",2,true,true),
        (kCVPixelFormatType_DepthFloat32,"depth-float",4,true,true),
    ]
    let width = 7, height = 3
    var groups: [[String: Any]] = []
    for (format, name, size, normalized, inverse) in formats {
        var outputBuffer: CVPixelBuffer?
        let status = CVPixelBufferCreate(kCFAllocatorDefault, width, height, format,
            [kCVPixelBufferBytesPerRowAlignmentKey: 64] as CFDictionary, &outputBuffer)
        guard status == kCVReturnSuccess, let buffer = outputBuffer else { throw DoctoredCoreError(message: "CV fixture creation failed for \(name): \(status)") }
        try require(CVPixelBufferLockBaseAddress(buffer, []) == kCVReturnSuccess, "Lock fixture")
        guard let base = CVPixelBufferGetBaseAddress(buffer) else { throw DoctoredCoreError(message: "CV fixture missing allocation") }
        let stride = CVPixelBufferGetBytesPerRow(buffer)
        memset(base, 0xA7, stride * height)
        var actual: [Double] = []
        for y in 0..<height { for x in 0..<width {
            let i = y * width + x, offset = y * stride + x * size
            switch format {
            case kCVPixelFormatType_OneComponent8:
                let value: UInt8 = [0,1,2,128,253,254,255][i % 7]
                base.storeBytes(of: value, toByteOffset: offset, as: UInt8.self)
                actual.append(Double(value)/255)
            case kCVPixelFormatType_OneComponent16:
                let value: UInt16 = [0,1,17,256,32769,65534,65535][i % 7]
                base.storeBytes(of: value, toByteOffset: offset, as: UInt16.self)
                actual.append(Double(value)/65535)
            default:
                let value = normalized ? [Double.nan,Double.infinity,0.01,0.07,0.315,0.72,1.9][i % 7]
                    : [Double.nan,-0.001,0.00003,0.3157,0.99998,1,1.5][i % 7]
                if size == 2 {
                    let stored = Float16(value); actual.append(Double(stored))
                    base.storeBytes(of: stored, toByteOffset: offset, as: Float16.self)
                } else {
                    let stored = Float32(value); actual.append(Double(stored))
                    base.storeBytes(of: stored, toByteOffset: offset, as: Float32.self)
                }
            }
        } }
        CVPixelBufferUnlockBaseAddress(buffer, [])
        let got = try NativeAuxiliaryMattes.plane(from: buffer, name: name)
        let finite = actual.filter(\.isFinite), low = finite.min() ?? 0, high = finite.max() ?? 0
        let expected: [UInt16] = actual.map { value in
            guard value.isFinite else { return 0 }
            let t = normalized ? (value - low)/(high-low) : value
            return UInt16((min(max(inverse ? 1-t : t,0),1)*65535).rounded())
        }
        try require(got.samples == expected, "Native auxiliary decoder differs from independent \(name) values")
        try require(got.width == width && got.height == height, "Native auxiliary extent")
        try require((got.provenance["source_bits"] as? Int) == (name == "u8" ? 8 : size * 8), "Auxiliary source precision disclosure")
        let image = try RasterDocument.fromRawRGBA16(Array(repeating: [UInt16(123),257,4097,65535], count: width*height).flatMap { $0 },
            width: width, height: height, profile: RasterProfile.builtin(.sRGB))
        guard let source = image.precision else { throw DoctoredCoreError(message: "Native import fixture missing") }
        let command = try PrecisionPixelCommand(["op":"import_channel","name":name,"width":width,"height":height,
            "provenance":try SelectedAdjustment.json(got.provenance)])
        guard let native = try PrecisionScalarWork(source: source, command: command, samples: got.samples).run(title:"Auxiliary fixture") else {
            throw DoctoredCoreError(message: "Auxiliary native attachment failed")
        }
        let attached = try RasterDocument(precision: native)
        guard let initialRows = try native.info()["channels"] as? [[String:Any]],
              let stored = initialRows.first?["provenance"] as? String else {
            throw DoctoredCoreError(message:"Attached auxiliary provenance missing")
        }
        let sourceMetadata = try JSONSerialization.jsonObject(with: Data(stored.utf8))
        try require((sourceMetadata as? NSDictionary)?.isEqual(to:got.provenance) == true,
            "Auxiliary metadata fields differ from declared SDK provenance: \(stored)")
        let path = output.appendingPathComponent("auxiliary-\(name).rz")
        try attached.saveNative(to: path)
        let reopened = try RasterDocument.open(url: path)
        try require(try scalarSamples(reopened, .channel(0)) == expected, "Auxiliary codes survive independent native reopen")
        let info = try reopened.precision?.info()["channels"] as? [[String: Any]]
        let reopenedProvenance = info?.first?["provenance"] as? String
        try require(reopenedProvenance == stored,
            "Exact stored auxiliary provenance differs after reopen: expected \(stored), got \(reopenedProvenance ?? "nil")")
        groups.append(["name":"auxiliary-\(name)","passed":true,"row_stride":stride,"samples":expected.count,
                       "source_bits":got.provenance["source_bits"] ?? 0,
                       "provenance_import":"semantic equality across Foundation and Rust JSON number serialization",
                       "provenance_native_reopen":"exact stored canonical string equality"])
    }
    let matteWidth = 16, matteHeight = 12
    let matte: [UInt8] = (0..<(matteWidth * matteHeight)).map { index in
        let linear = index * 13
        let row = index / matteWidth
        return UInt8((linear + row * 7) % 256)
    }
    let description: [AnyHashable: Any] = [
        kCGImageAuxiliaryDataInfoData: Data(matte),
        kCGImageAuxiliaryDataInfoDataDescription: ["Width":matteWidth,"Height":matteHeight,
            "BytesPerRow":matteWidth,"PixelFormat":kCVPixelFormatType_OneComponent8],
        kCGImageAuxiliaryDataInfoMetadata: CGImageMetadataCreateMutable(),
    ]
    for raw in UInt32(1)...8 {
        guard let orientation = CGImagePropertyOrientation(rawValue: raw) else { throw DoctoredCoreError(message:"Invalid fixture orientation") }
        let buffer = try NativeAuxiliaryMattes.mattingImage(kCGImageAuxiliaryDataTypePortraitEffectsMatte, description, orientation)
        let plane = try NativeAuxiliaryMattes.plane(from: buffer, name:"Portrait Matte", orientation:raw)
        let width = raw >= 5 ? matteHeight : matteWidth, height = raw >= 5 ? matteWidth : matteHeight
        var expected = [UInt16](repeating:0,count:matte.count)
        for y in 0..<matteHeight { for x in 0..<matteWidth {
            let destination: (Int,Int)
            switch raw {
            case 1: destination = (x,y)
            case 2: destination = (matteWidth-1-x,y)
            case 3: destination = (matteWidth-1-x,matteHeight-1-y)
            case 4: destination = (x,matteHeight-1-y)
            case 5: destination = (y,x)
            case 6: destination = (matteHeight-1-y,x)
            case 7: destination = (matteHeight-1-y,matteWidth-1-x)
            default: destination = (y,matteWidth-1-x)
            }
            expected[destination.1 * width + destination.0] = UInt16(matte[y*matteWidth+x])*257
        } }
        try require(plane.width == width && plane.height == height && plane.samples == expected,
            "Actual SDK EXIF orientation differs from independent matte permutation")
        try require(plane.provenance["exif_orientation"] as? UInt32 == raw, "Matte orientation provenance")
        groups.append(["name":"auxiliary-orientation-\(raw)","passed":true,"samples":expected.count])
    }
    return groups
}

func verifyScalarAutoAndMask(fixtures: URL, output: URL) throws -> [[String: Any]] {
    let (document, editor, original) = try openDocument(depth:16,fixtures:fixtures)
    defer { if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }; document.close(); pump() }
    let width=original.width,height=original.height
    let codes=(0..<(width*height)).map { UInt16(2001 + $0 * 91) }
    document.applyEdit("Fixture Channel",record:.notACommand) {
        try? PrecisionScalarOperations.channel($0,name:"Auto Plane",samples:codes,width:width,height:height)
    }
    let source=document.doc ?? original
    try isolateChannel(editor,index:0)
    let mask=(0..<(width*height)).map { i -> UInt16 in
        let x=i%width,y=i/width
        return x>=4 && x<18 && y>=3 && y<12 ? (x%2==0 ? 32769 : 65535) : 0
    }
    editor.agentSetSelection(CanvasSelection(shape:.mask16(mask),canvasWidth:width,canvasHeight:height))
    document.undoManager?.removeAllActions()
    var cases:[[String:Any]]=[]
    for tool in ["auto_tone","auto_contrast","auto_color"] {
        var whole:[UInt16]=[]
        for selected in [false,true] {
            ActionRecorder.shared.start()
            let answers=try answerAlertSheets(with:selected ? "Selection":"Whole Target") {
                switch tool {
                case "auto_tone":editor.autoTone(nil)
                case "auto_contrast":editor.autoContrast(nil)
                default:editor.autoColor(nil)
                }
            }
            try waitUntil({answers.answered==1},"Scalar Auto scope prompt missing")
            try require(answers.prompts==[["Whole Target","Selection","Cancel"]],"Scalar Auto prompt buttons: \(answers.prompts)")
            try waitUntil({editor.view.window?.attachedSheet == nil && !BackgroundWork.occupied && document.doc !== source},"Scalar Auto did not publish")
            let steps=ActionRecorder.shared.stop()
            let expected=try scalarSamples(document.doc ?? source,.channel(0))
            try require(steps.count==1 && steps[0].tool==tool,"Scalar Auto must record one Action")
            try require(steps[0].arguments["target"] as? String == "channel:Auto Plane","Scalar Auto lost target")
            try require(steps[0].arguments["analysis_scope"] as? String == (selected ? "selection":"whole_target"),"Scalar Auto lost analysis choice")
            try require(try values(document.doc ?? source)==values(source),"Scalar Auto changed RGB/alpha")
            for i in codes.indices where mask[i]==0 {try require(expected[i]==codes[i],"Scalar Auto changed uncovered code")}
            if selected {try require(expected != whole,"Scalar Auto analysis choices should differ on fixture")} else {whole=expected}
            document.undoManager?.undo();pump()
            try require(document.doc === source,"Scalar Auto one Undo")
            var arguments=steps[0].arguments;arguments["document_id"]=AgentServer.shared.documentID(for:document)
            let data=try JSONSerialization.data(withJSONObject:arguments,options:.sortedKeys)
            let response=AgentServer.shared.execute(tool:tool,argumentsJSON:String(decoding:data,as:UTF8.self))
            try waitUntil({!BackgroundWork.occupied},"Scalar Auto Action did not complete")
            try require(try scalarSamples(document.doc ?? source,.channel(0))==expected,"Scalar Auto replay differs: \(response)")
            document.undoManager?.undo();pump()
            cases.append(["tool":tool,"analysis_scope":selected ? "selection":"whole_target"])
        }
    }
    let cancelled=try answerAlertSheets(with:"Cancel") {editor.autoTone(nil)}
    try waitUntil({cancelled.answered==1},"Scalar Auto cancel missing")
    try require(cancelled.prompts==[["Whole Target","Selection","Cancel"]],"Scalar Auto cancel prompt buttons: \(cancelled.prompts)")
    pump()
    try require(document.doc === source,"Scalar Auto Cancel changed source")
    editor.agentSetSelection(nil);editor.setPaintTarget(.layer)
    let maskCodes=(0..<(width*height)).map { [UInt16(0),1,17,32769,65534,65535][$0%6] }
    let capture=try PrecisionEditorCapture(document)
    let add=try PrecisionPixelEdit(capture.source,command:.setMask(layer:0,width:width,height:height,
        x:2,y:-1,linked:false,density:0.75,feather:0,enabled:false),
        input:PrecisionScalarBuffer(samples:maskCodes,width:width,height:height))
    _ = try capture.publish(add.run(),to:document,name:"Fixture Mask")
    let masked=document.doc ?? source
    let originalPixels=try values(source)
    document.undoManager?.removeAllActions()
    editor.applyLayerMask(nil)
    try waitUntil({!BackgroundWork.occupied && document.doc !== masked},"Apply native layer mask stalled")
    let applied=document.doc ?? masked
    try require(!applied.layerHasMask(0),"Applied native mask was not removed")
    let rawAlpha=try scalarSamples(applied,.plane(.alpha))
    for y in 0..<height {for x in 0..<width {
        let sx=x-2,sy=y+1
        let raw=(0..<width).contains(sx) && (0..<height).contains(sy) ? Double(maskCodes[sy*width+sx])/65535 : 0
        let expected=UInt16(((1-0.75+0.75*raw)*65535).rounded())
        try require(abs(Int(rawAlpha[y*width+x])-Int(expected))<=1,"Independent disabled mask alpha/density/placement reference")
    }}
    for channel in [RasterPlane.red,.green,.blue] {
        let index=channel == .red ? 0:channel == .green ? 1:2
        let expected=(0..<(width*height)).map {originalPixels[$0*4+index]}
        try require(try scalarSamples(applied,.plane(channel))==expected,"Applying mask altered RGB")
    }
    document.undoManager?.undo();pump()
    try require(document.doc === masked,"Apply Mask one Undo")
    document.undoManager?.redo();pump()
    try require(document.doc === applied,"Apply Mask exact Redo")
    return [["name":"scalar-auto-ui","passed":true,"cases":cases],["name":"native-apply-mask-ui","passed":true,"samples":rawAlpha.count]]
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
