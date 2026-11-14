import AppKit

/// Select ▸ Focus Area… and `select_focus_area` (`designs/focus-area.md`): byte coverage of
/// the in-focus part of the picture, computed by the core over the selection capture's 8-bit
/// analysis source at every depth, in RGB and in the colour modes whose source is a plain
/// picture of the document (`FocusArea.modes`).
struct FocusAreaParameters: Equatable {
    /// The In-Focus Range, 0–100 (lower selects more); nil is Auto.
    var range: Double?
    var softenEdge = true

    static let rangeRange = 0.0...100.0

    var coreParameters: [String: Any] {
        var out: [String: Any] = ["soften_edge": softenEdge]
        if let range { out["range"] = range }
        return out
    }
}

/// Auto's resolved range, handed from the worker to main.
private final class FocusAreaRange {
    var value = 0.0
}

enum FocusArea {
    static let actionName = "Focus Area"

    /// The colour modes Focus Area answers, each in its sets too (`GrayDocument`, `LabDocument`,
    /// `IndexedDocument`, the `.cmykTool` fact): the modes whose analysis source is a picture of
    /// the document. Duotone and Bitmap refuse (the ADR's decision 7).
    static let modes: Set<ColorModeTarget> = [.rgb, .gray, .lab, .indexed, .cmyk]

    /// Why Focus Area does not answer on `doc`, or nil. Duotone and Bitmap are refused first by
    /// their own gates, so this sentence is the menu's and the commit's backstop.
    static func refusal(_ doc: RasterDocument) -> String? {
        modes.contains(doc.colorMode) ? nil
            : "Select ▸ Focus Area answers in RGB, Grayscale, Lab Color, Indexed Color and CMYK "
                + "documents."
    }

    /// The recorded step: the resolved range, so a replay makes the same cut.
    static func record(range: Double, softenEdge: Bool, mode: SelectionCombineMode) -> [ActionStep] {
        .step("select_focus_area", ["range": range, "soften_edge": softenEdge, "mode": mode.agentName],
              note: "Select ▸ Focus Area…")
    }
}

extension SelectionCapture {
    /// The coverage and the range the core used, run as a cancellable job over `source`.
    func focusAreaShape(_ p: FocusAreaParameters,
                        in document: ImageDocument) throws -> (shape: CanvasSelection.Shape, range: Double) {
        guard let source else { throw DoctoredCoreError(message: "The image has no captured projection.") }
        let parameters = p.coreParameters
        let resolved = FocusAreaRange()
        // The core's own plan is 34 bytes a pixel plus the band rings (under a kilobyte a
        // column) and a megabyte; the output plane is the run's own.
        let pixels = UInt64(doc.width * doc.height)
        let scratch = pixels * 36 + UInt64(doc.width) * 1024 + 2_097_152
        let mask = try run(in: document, title: FocusArea.actionName, scratchBytes: scratch) { control in
            let result = try source.focusArea(parameters: parameters, control: control)
            resolved.value = result.range
            return result.mask
        }
        return (.mask(mask), resolved.value)
    }
}
