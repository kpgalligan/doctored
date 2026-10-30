import AppKit

/// Detection may use a display reduction; every correction reads native RGB.
enum PrecisionRedEye {
    static func correct(_ capture: PrecisionEditorCapture, layer: Int, rectangles: [CGRect],
                        pupilSize: Double, darken: Double) throws -> RasterPrecisionDocument? {
        let coverage = try PrecisionStrokeCoverage.selection(width: capture.facade.width,
            height: capture.facade.height, selection: nil)
        var current = capture.source
        var changed = false
        for rect in rectangles {
            let command = try PrecisionPixelCommand.redEye(layer: layer, rect: rect,
                pupilSize: pupilSize, darken: darken)
            if let next = try PrecisionPixelEdit(current, command: command, input: coverage)
                .run(title: "Remove Red Eye") {
                current = next
                changed = true
            }
        }
        return changed ? current : nil
    }
}

extension EditorViewController {
    func applyPrecisionRedEye(_ rectangles: [CGRect], pupilSize: Double, darken: Double,
                              record: [ActionStep]) -> Bool {
        guard let document, document.doc?.isHighDepth == true else { return false }
        do {
            let capture = try PrecisionEditorCapture(document)
            guard document.confirmRasterize(layer: capture.layer) else { return true }
            try capture.requireCurrent(document)
            let result = try PrecisionRedEye.correct(capture, layer: capture.layer, rectangles: rectangles,
                pupilSize: pupilSize, darken: darken)
            try capture.publish(result, to: document, name: "Remove Red Eye",
                record: ActionRecorder.shared.gestureRecord(record))
        } catch { document.presentError(error) }
        return true
    }
}
