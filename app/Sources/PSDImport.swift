import AppKit

// On the import side, a 16-bit RGB PSD (`inspect_psd`'s `import_precision` `"rgba16"`) opens as a
// 16-bit document on the worker through this file, with either compatibility choice. A Large
// Document PSB is a PSD here (same `8BPS` signature, same chooser, same core calls).
//
// A TIFF carrying Photoshop's layer block (`isLayeredTIFF`) is the one case that is NOT forced
// through the chooser's two PSD answers: it is a flat TIFF first, so it opens flat exactly as
// before unless the user or the caller picks Import Layers (`openLayeredTIFF`, whose `nil` means
// "take the ordinary open"). Its chooser lists "Open Flattened" FIRST, which is also the answer an
// automated run gives every alert.
//
// A CMYK file (`import_precision` `"cmyk"`, #1) — PSD, PSB or a big-endian layered CMYK TIFF —
// opens as a CMYK document with either choice: Import Layers its layers, Stored Composite (or a
// TIFF's Open Flattened, the ordinary CMYK TIFF open) its flat picture. Both follow Edit ▸ Color
// Settings' CMYK policy (`CmykProfileEdit.OpenPolicy`), which `openPrecisionPSD` applies as the
// ordinary CMYK open does.
//
// A 32-bit RGB PSD or PSB (`import_precision` `"rgba32"`) opens as a 32-bit float document
// through the same `openPrecisionPSD`, with either choice; so does a 32-bit float RGB layered TIFF
// of either byte order, its Import Layers answer, its Open Flattened the ordinary float TIFF
// open.
//
// A Grayscale file at 8 or 16 bits (`import_precision` `"gray"`, #1 phase 3a) — PSD, PSB or a
// big-endian layered gray TIFF, which `isLayeredTIFF` claims too — or a 32-bit PSD or PSB (phase
// 3c) opens as a Grayscale document of its depth with either choice through the same
// `openPrecisionPSD` (`openGrayPSD`, Color Settings' working Gray assigned to an untagged file); a
// TIFF's Open Flattened is the ordinary gray TIFF open.
//
// A Lab file at 8 or 16 bits (`import_precision` `"lab"`, #1 Lab phase 3a) — PSD, PSB or a
// big-endian layered Lab TIFF, which `isLayeredTIFF` claims too — opens as a Lab Color document of
// its depth with either choice through the same `openPrecisionPSD` (`openLabPSD`; an embedded
// profile is not used); a TIFF's Open Flattened is the ordinary flat Lab TIFF open.
//
// A Bitmap PSD or PSB (`import_precision` `"bitmap"`, colour mode 0, #1 Bitmap phase 1c) holds
// no layers, so Stored Composite is its one choice; it opens as a Bitmap document through the
// same `openPrecisionPSD` (`openBitmapPSD`, Color Settings' working Gray unless resource 1039 is
// a Gray profile).
//
// An Indexed Color PSD or PSB (`import_precision` `"indexed"`, colour mode 2, #1) opens from its
// stored composite, its one choice, as an Indexed Color document keeping its palette, transparent
// index and every index, through the same `openPrecisionPSD` (`openIndexedPSD`).

/// The explicit decision made after inspecting PSD compatibility. A stored
/// composite is the author's saved appearance, not our imported-layer render.
enum PSDImportChoice: String {
    case layers, composite
    var coreMode: Int { self == .layers ? 0 : 1 }
    var availabilityKey: String { self == .layers ? "layered_available" : "stored_composite_available" }
    var refusalKey: String { self == .layers ? "layered_refusal" : "stored_composite_refusal" }
}

struct PSDImportState {
    let choice: PSDImportChoice
    let report: [String: Any]
    /// The document precision the file opened at, "rgba8", "rgba16", "rgba32",
    /// "cmyk", "gray" or "lab": a 16-bit RGB PSD opens as a 16-bit document, a 32-bit
    /// one as a 32-bit float document, a CMYK file as a CMYK document,
    /// a Grayscale one as a Grayscale document and a Lab one as a Lab Color
    /// document with either choice (#1), never as a converted copy.
    let precision: String
    var json: [String: Any] {
        var result = report
        result["chosen_mode"] = choice.rawValue
        result["import_precision"] = precision
        return result
    }
}

enum PSDImportMode {
    case ask
    case explicit(PSDImportChoice, fingerprint: String?)
    case refuse
}

/// AppKit's synchronous read callback has no option arguments. Callers scope
/// this main-thread slot with defer, exactly like RawImportRequest.
enum PSDImportRequest {
    static var mode: PSDImportMode = .ask
}

extension ImageDocument {
    static func isPSD(at url: URL) -> Bool {
        guard let file = try? FileHandle(forReadingFrom: url) else { return false }
        defer { try? file.close() }
        return (try? file.read(upToCount: 4)) == Data("8BPS".utf8)
    }

    static func openPSD(at url: URL) throws -> (doc: RasterDocument, state: PSDImportState) {
        let report = try RasterDocument.psdImportReport(at: url)
        guard let fingerprint = report["source_fingerprint"] as? String else {
            throw DoctoredCoreError(message: "PSD compatibility report lacks a source fingerprint.")
        }
        let choice: PSDImportChoice
        switch PSDImportRequest.mode {
        case .ask:
            guard let selected = PSDCompatibilityNotice.choose(url: url, report: report) else {
                throw CocoaError(.userCancelled)
            }
            choice = selected
        case .explicit(let requested, let expected):
            if let expected, expected != fingerprint {
                throw DoctoredCoreError(message: "PSD changed since preflight; inspect it again before opening.")
            }
            choice = requested
        case .refuse:
            throw DoctoredCoreError(message: "PSD batch import requires an explicit compatibility choice. "
                + "Open it with Import Layers or Stored Composite, save a native .doctored copy, then batch that copy.")
        }
        return try openChosen(at: url, choice: choice, report: report, fingerprint: fingerprint)
    }

    /// The open both entry points end in: the chosen mode must be available
    /// in `report`, and the file opens at the precision the report names.
    private static func openChosen(at url: URL, choice: PSDImportChoice, report: [String: Any],
                                   fingerprint: String)
        throws -> (doc: RasterDocument, state: PSDImportState) {
        guard report[choice.availabilityKey] as? Bool == true else {
            throw DoctoredCoreError(message: report[choice.refusalKey] as? String
                ?? "The selected PSD import mode is unavailable.")
        }
        let precision = report["import_precision"] as? String ?? "rgba8"
        let title: String
        switch precision {
        case "cmyk": title = "Opening CMYK Photoshop Document"
        case "rgba32": title = "Opening 32-bit Photoshop Document"
        case "gray": title = "Opening Grayscale Photoshop Document"
        case "lab": title = "Opening Lab Color Photoshop Document"
        case "bitmap": title = "Opening Bitmap Photoshop Document"
        case "indexed": title = "Opening Indexed Color Photoshop Document"
        default: title = "Opening 16-bit Photoshop Document"
        }
        let doc = ["rgba16", "rgba32", "cmyk", "gray", "lab", "bitmap", "indexed"]
            .contains(precision)
            ? try openPrecisionPSD(at: url, mode: choice.coreMode, fingerprint: fingerprint,
                                   cmyk: precision == "cmyk", gray: precision == "gray",
                                   lab: precision == "lab", bitmap: precision == "bitmap",
                                   indexed: precision == "indexed", title: title)
            : try RasterDocument.openPSD(at: url, mode: choice.coreMode, fingerprint: fingerprint)
        return (doc, PSDImportState(choice: choice, report: report, precision: precision))
    }

    /// True for a TIFF whose Photoshop layer block the core reads as layers:
    /// a big-endian unsigned 8- or 16-bit RGB, CMYK, gray or Lab (#1) TIFF
    /// holding one, or a 32-bit float RGB one of either byte order.
    /// Every other TIFF — a 16-bit float, little-endian at 8 or 16 bits —
    /// answers false and is never offered the layered open. It is asked before the ordinary CMYK,
    /// gray and Lab TIFF opens (`openPrecisionDocumentIfRequested`), so a layered
    /// CMYK, gray or Lab TIFF reaches the chooser and its Open Flattened answer
    /// that open.
    static func isLayeredTIFF(at url: URL) -> Bool {
        RasterDocument.tiffHasLayers(at: url)
    }

    /// The layered open of a TIFF `isLayeredTIFF` claims, or nil to take the
    /// ordinary flat open — which is the answer for every route but an
    /// explicit Import Layers, so a TIFF that opened flat before this existed
    /// still does.
    static func openLayeredTIFF(at url: URL) throws -> (doc: RasterDocument, state: PSDImportState)? {
        switch PSDImportRequest.mode {
        case .refuse:
            return nil
        case .explicit(.composite, let expected):
            // The flat open, but a caller that inspected first is still told
            // when the file is no longer the one it inspected.
            if let expected {
                let report = try RasterDocument.psdImportReport(at: url)
                if report["source_fingerprint"] as? String != expected {
                    throw DoctoredCoreError(
                        message: "TIFF changed since preflight; inspect it again before opening.")
                }
            }
            return nil
        case .explicit(.layers, let expected):
            let report = try RasterDocument.psdImportReport(at: url)
            guard let fingerprint = report["source_fingerprint"] as? String else {
                throw DoctoredCoreError(message: "TIFF layer report lacks a source fingerprint.")
            }
            if let expected, expected != fingerprint {
                throw DoctoredCoreError(
                    message: "TIFF changed since preflight; inspect it again before opening.")
            }
            return try openChosen(at: url, choice: .layers, report: report, fingerprint: fingerprint)
        case .ask:
            // No chooser unless Import Layers would really work: a report
            // that cannot be built, or one that retains nothing, opens flat.
            guard let report = try? RasterDocument.psdImportReport(at: url),
                  report["layered_available"] as? Bool == true,
                  let fingerprint = report["source_fingerprint"] as? String else {
                return nil
            }
            switch PSDCompatibilityNotice.chooseLayeredTIFF(url: url, report: report) {
            case .flattened:
                return nil
            case .layers:
                return try openChosen(at: url, choice: .layers, report: report, fingerprint: fingerprint)
            case .cancel:
                throw CocoaError(.userCancelled)
            }
        }
    }

    /// A 16-bit RGB, a CMYK, a Grayscale or a Lab file opens as a typed document on a
    /// worker, under the same admission the other high-depth opens use. A CMYK
    /// one (`cmyk`) takes Color Settings' CMYK policy, and a Grayscale one
    /// (`gray`) its working Gray, both read here on the main thread, as the
    /// ordinary CMYK and gray opens do (`ImageDocument+PrecisionGuard`); a Lab one
    /// (`lab`) takes neither. A Bitmap one (`bitmap`, #1 Bitmap phase 1c) opens its stored
    /// composite under the working Gray, its only choice; so does an Indexed Color one
    /// (`indexed`, #1), keeping its palette and every index.
    private static func openPrecisionPSD(at url: URL, mode: Int, fingerprint: String, cmyk: Bool,
                                         gray: Bool, lab: Bool, bitmap: Bool, indexed: Bool,
                                         title: String) throws -> RasterDocument {
        let context = try PrecisionWorkContext()
        let policy = CmykProfileEdit.OpenPolicy.current
        let workingGray = ColorSettings.workingGray.rawValue
        let task = try WorkerTask {
            var source = bitmap
                ? try RasterPrecisionDocument.openBitmapPSD(
                    at: url, fingerprint: fingerprint, workingGray: workingGray,
                    workingLimit: context.limit, otherRetained: context.otherRetained)
                : indexed
                ? try RasterPrecisionDocument.openIndexedPSD(
                    at: url, fingerprint: fingerprint,
                    workingLimit: context.limit, otherRetained: context.otherRetained)
                : gray
                ? try RasterPrecisionDocument.openGrayPSD(
                    at: url, mode: mode, fingerprint: fingerprint, workingGray: workingGray,
                    workingLimit: context.limit, otherRetained: context.otherRetained)
                : lab
                ? try RasterPrecisionDocument.openLabPSD(
                    at: url, mode: mode, fingerprint: fingerprint,
                    workingLimit: context.limit, otherRetained: context.otherRetained)
                : try RasterPrecisionDocument.openPSD(
                    at: url, mode: mode, fingerprint: fingerprint,
                    workingLimit: context.limit, otherRetained: context.otherRetained)
            if cmyk { source = try policy.applied(to: source, context: context) }
            return try RasterDocument(precision: source)
        }
        return try BackgroundWork.wait(task, title: title)
    }
}

/// Shown before any document window is created. The complete report stays
/// scrollable, with per-file facts clearly separated from general limits.
private enum PSDCompatibilityNotice {
    /// What the layered-TIFF form of the notice answers.
    enum LayeredTIFFChoice {
        case flattened, layers, cancel
    }

    static func choose(url: URL, report: [String: Any]) -> PSDImportChoice? {
        let alert = NSAlert()
        alert.messageText = "Open Photoshop document “\(url.lastPathComponent)”"
        alert.informativeText = "Stored Composite preserves the file's saved appearance as one raster layer. "
            + "Import Layers keeps the supported editable layer subset and may change appearance. "
            + "Review the compatibility report before choosing."
        var choices: [PSDImportChoice] = []
        if report["stored_composite_available"] as? Bool == true {
            alert.addButton(withTitle: "Open Stored Composite")
            choices.append(.composite)
        }
        if report["layered_available"] as? Bool == true {
            alert.addButton(withTitle: "Import Layers")
            choices.append(.layers)
        }
        alert.addButton(withTitle: "Cancel")
        alert.accessoryView = reportView(report, refusalKeys: ["layered_refusal", "stored_composite_refusal"],
                                         note: report["stored_composite_note"] as? String)
        let selected = alert.runModal().rawValue - NSApplication.ModalResponse.alertFirstButtonReturn.rawValue
        return choices.indices.contains(selected) ? choices[selected] : nil
    }

    /// The TIFF form: the flat open is always there and always first, so no
    /// path through this notice ends with Cancel as the only button, and an
    /// automated run (which takes the first button) opens the file flat. The
    /// stored-composite refusal is not listed — a TIFF has none by design.
    static func chooseLayeredTIFF(url: URL, report: [String: Any]) -> LayeredTIFFChoice {
        let alert = NSAlert()
        alert.messageText = "Open layered TIFF “\(url.lastPathComponent)”"
        alert.informativeText = "Open Flattened opens the TIFF's own flat image as one layer, as any "
            + "other TIFF opens. Import Layers keeps the supported editable layer subset from its "
            + "Photoshop layer block and may change appearance. Review the compatibility report "
            + "before choosing."
        alert.addButton(withTitle: "Open Flattened")
        alert.addButton(withTitle: "Import Layers")
        alert.addButton(withTitle: "Cancel")
        alert.accessoryView = reportView(report, refusalKeys: ["layered_refusal"], note: nil)
        let choices: [LayeredTIFFChoice] = [.flattened, .layers]
        let selected = alert.runModal().rawValue - NSApplication.ModalResponse.alertFirstButtonReturn.rawValue
        return choices.indices.contains(selected) ? choices[selected] : .cancel
    }

    private static func reportView(_ report: [String: Any], refusalKeys: [String],
                                   note: String?) -> NSView {
        let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 540, height: 280))
        scroll.hasVerticalScroller = true
        scroll.borderType = .bezelBorder
        let text = NSTextView(frame: scroll.bounds)
        text.isEditable = false
        text.isSelectable = true
        text.font = DS.sans(12)
        text.textContainerInset = NSSize(width: 8, height: 8)
        text.isHorizontallyResizable = false
        text.isVerticallyResizable = true
        text.autoresizingMask = [.width]
        text.textContainer?.widthTracksTextView = true
        var lines = ["\(report["depth"] ?? "?")-bit \(report["color_mode"] ?? "Unknown") · "
            + "\(report["width"] ?? "?") × \(report["height"] ?? "?") pixels"]
        if report["import_precision"] as? String == "rgba16" {
            lines.append("This file opens as a 16-bit document, with either choice.")
        }
        if report["import_precision"] as? String == "rgba32" {
            lines.append("This file opens as a 32-bit float document, with either choice.")
        }
        if report["import_precision"] as? String == "cmyk" {
            lines.append("This file opens as a CMYK document, keeping its own ink and embedded CMYK "
                + "profile, with either choice.")
        }
        if report["import_precision"] as? String == "gray" {
            lines.append("This file opens as a Grayscale document, keeping its own gray values, "
                + "with either choice.")
        }
        if report["import_precision"] as? String == "lab" {
            lines.append("This file opens as a Lab Color document, keeping its own L*a*b* values, "
                + "with either choice.")
        }
        if report["import_precision"] as? String == "bitmap" {
            lines.append("This file opens as a Bitmap document, keeping its own black and white "
                + "pixels, from its stored composite.")
        }
        if report["import_precision"] as? String == "indexed" {
            lines.append("This file opens as an Indexed Color document, keeping its palette, "
                + "transparent index and every index, from its stored composite.")
        }
        for (title, key) in [("Verified losses in this file", "verified_losses"),
                              ("Observed in this file", "observations"),
                              ("General importer limitations", "general_limitations")] {
            let items = report[key] as? [String] ?? []
            lines += ["", title]
            lines += items.isEmpty ? ["None detected by the current parser."] : items.map { "• " + $0 }
        }
        for key in refusalKeys {
            if let refusal = report[key] as? String { lines += ["", refusal] }
        }
        if let note { lines += ["", note] }
        text.string = lines.joined(separator: "\n")
        scroll.documentView = text
        return scroll
    }
}
