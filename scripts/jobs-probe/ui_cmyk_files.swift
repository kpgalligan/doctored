import AppKit
import ObjectiveC

// The CMYK files batch's in-process UI case (#547 phase 3): a two-layer CMYK document exported
// in-probe as a layered PSD and a layered TIFF (`PSDExportCapture`, the File ▸ Export as
// PSD… writer), then opened through `ImageDocument.read` with `NSAlert.runModal`
// intercepted (the template is `scripts/color-metadata-probe/psd.swift`'s `verifyPSDImports`).
// The PSD's compatibility sheet carries the CMYK line and both choices; Import Layers opens a CMYK
// document with the same layer count and Stored Composite one layer. The TIFF reaches the
// layered-TIFF chooser (Open Flattened, Import Layers, Cancel) ahead of the ordinary CMYK TIFF
// open; Import Layers opens its layers and Open Flattened one CMYK layer. The CMYK profile is
// macOS's Generic CMYK Profile, read from the system.

private let filesGenericCMYK = URL(fileURLWithPath: "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc")

/// The report sheet's CMYK line (`PSDImport.swift`).
private let cmykSheetLine = "This file opens as a CMYK document, keeping its own ink and embedded CMYK profile, with either choice."

/// "cmyk psd import report" (module doc).
func verifyCmykPsdImportReport() throws {
    let tag = "[cmyk psd import report]"
    let folder = try uiCaseFolder("cmyk-files")
    let source = try fixture(width: 64, height: 48)
    defer { source.close() }
    let controller = try editor(source)
    controller.commitColorMode(.cmyk, profile: try Data(contentsOf: filesGenericCMYK), destination: [:],
                               options: CmykConversionOptions(), document: source)
    guard source.doc?.isCmyk == true else { throw ProbeFailure(description: "\(tag) not CMYK") }
    source.applyEdit("Add", record: .notACommand) { try $0.addingLayer(above: 0, name: "Two") }
    let layers = source.doc?.layerCount ?? 0
    try require(layers == 2, "\(tag) a two-layer CMYK document")
    let psd = folder.appendingPathComponent("layers.psd")
    let tiff = folder.appendingPathComponent("layers.tif")
    for (container, url) in [(LayeredContainer.psd, psd), (.tiff, tiff)] {
        let capture = try PSDExportCapture(source, container: container, mode: .layers)
        try require(capture.report.json["depth"] as? String == "cmyk", "\(tag) \(container) report is CMYK")
        try capture.write(to: url)
    }

    guard let method = class_getInstanceMethod(NSAlert.self, #selector(NSAlert.runModal)) else {
        throw ProbeFailure(description: "\(tag) alert interception unavailable")
    }
    let previousPSD = PSDImportRequest.mode
    let previousPrecision = PrecisionImportRequest.mode
    defer {
        PSDImportRequest.mode = previousPSD
        PrecisionImportRequest.mode = previousPrecision
    }
    var selectedTitle = "Cancel"
    var observedReport = ""
    var observedButtons: [String] = []
    var observedTitle = ""
    let interceptor: @convention(block) (AnyObject) -> Int = { object in
        guard let alert = object as? NSAlert else { return NSApplication.ModalResponse.abort.rawValue }
        observedTitle = alert.messageText
        observedButtons = alert.buttons.map(\.title)
        if let scroll = alert.accessoryView as? NSScrollView,
           let text = scroll.documentView as? NSTextView { observedReport = text.string }
        let index = alert.buttons.firstIndex { $0.title == selectedTitle } ?? alert.buttons.count - 1
        return NSApplication.ModalResponse.alertFirstButtonReturn.rawValue + index
    }
    let replacement = imp_implementationWithBlock(interceptor)
    let original = method_setImplementation(method, replacement)
    defer { method_setImplementation(method, original); imp_removeBlock(replacement) }

    // The PSD: the compatibility sheet, then each choice.
    for (title, expected) in [("Import Layers", layers), ("Open Stored Composite", 1)] {
        let document = ImageDocument()
        defer { document.close() }
        selectedTitle = title
        observedReport = ""
        observedButtons = []
        PSDImportRequest.mode = .ask
        try document.read(from: psd, ofType: "com.adobe.photoshop-image")
        try require(observedButtons.contains("Open Stored Composite") && observedButtons.contains("Import Layers"),
                    "\(tag) the PSD sheet offers both choices: \(observedButtons)")
        try require(observedReport.contains(cmykSheetLine), "\(tag) the PSD sheet's CMYK line: \(observedReport)")
        try require(document.doc?.isCmyk == true, "\(tag) \(title) opens a CMYK document")
        try require(document.doc?.layerCount == expected,
                    "\(tag) \(title) holds \(document.doc?.layerCount ?? -1) layers, not \(expected)")
    }
    print("PASS \(tag) PSD: the CMYK sheet line, both choices, Import Layers \(layers) layers, Stored Composite 1")

    // The layered TIFF: the chooser ahead of the ordinary CMYK TIFF open.
    try require(ImageDocument.isLayeredTIFF(at: tiff), "\(tag) the exported TIFF is a layered TIFF")
    for (title, expected) in [("Import Layers", layers), ("Open Flattened", 1)] {
        let document = ImageDocument()
        defer { document.close() }
        selectedTitle = title
        observedButtons = []
        observedTitle = ""
        PSDImportRequest.mode = .ask
        PrecisionImportRequest.mode = .ask
        try document.read(from: tiff, ofType: "public.tiff")
        try require(observedTitle.hasPrefix("Open layered TIFF"), "\(tag) the layered-TIFF chooser: \(observedTitle)")
        try require(observedButtons == ["Open Flattened", "Import Layers", "Cancel"],
                    "\(tag) the chooser's buttons: \(observedButtons)")
        try require(document.doc?.isCmyk == true, "\(tag) \(title) opens a CMYK document")
        try require(document.doc?.layerCount == expected,
                    "\(tag) \(title) holds \(document.doc?.layerCount ?? -1) layers, not \(expected)")
    }
    print("PASS \(tag) TIFF: the layered-TIFF chooser, Import Layers \(layers) layers, Open Flattened 1")
}
