import AppKit

/// Text and shape remain editable after a filtered whole-canvas operation.
func retainedArtworkGeometryUI() throws -> [String: Any] {
    var trials: [[String: Any]] = []
    for typed in [false, true] {
        let (document, editor, _) = typed
            ? try openDocument(width: 48, height: 36)
            : try openByteDocument(width: 48, height: 36)
        defer { document.close(); pump() }
        _ = try geometryCall("add_text_layer",
            ["text": "Vector", "x": 7, "y": 11, "size": 13], document)
        guard let before = document.doc,
              let index = (0..<before.layerCount).first(where: before.hasArtworkDescription),
              let original = before.layerMeta(index) else {
            throw DoctoredCoreError(message: "Owned editable text fixture unavailable")
        }
        let steps = document.history.entries.count
        editor.commitCanvasRotation(degrees: 7.5, sampler: DR_FILTER_BILINEAR)
        pump()
        guard let after = document.doc, after !== before,
              let description = after.layerMeta(index) else {
            throw DoctoredCoreError(message: "Filtered canvas rotation lost editable text")
        }
        try require(description != original,
                    "Filtered rotation did not compose the artwork description")
        try require(document.history.entries.count == steps + 1,
                    "Filtered artwork rotation did not make one undo step")
        document.undoManager?.undo(); pump()
        try require(document.doc === before,
                    "Filtered artwork Undo did not restore the original description")

        let window = CGRect(x: 4, y: 3, width: 38, height: 28)
        let center = CGPoint(x: window.midX, y: window.midY)
        let matrix = CGAffineTransform(translationX: center.x, y: center.y)
            .rotated(by: -6.0 * .pi / 180)
            .translatedBy(x: -center.x, y: -center.y)
        guard let straightened = try before.straightenedCrop(matrix,
            sampler: DR_FILTER_BILINEAR, x: 4, y: 3, w: 38, h: 28,
            described: [index])?.document else {
            throw DoctoredCoreError(message: "Straightened artwork crop refused")
        }
        try require(straightened.layerMeta(index) != nil,
                    "Straighten/crop lost editable artwork")
        trials.append(["depth": typed ? 16 : 8, "rotate_editable": true,
                       "straighten_editable": true, "one_undo": true])
    }
    return ["case": "retained-artwork-geometry-ui", "trials": trials]
}
