import AppKit

extension EditorViewController {
    @objc func rasterizeArtwork(_ sender: Any?) {
        guard let document else { return }
        guard commitPendingSessions() else { return }
        let index = document.activeLayerIndex
        guard !refuseLockedEdit(layer: index, kind: DR_EDIT_PIXELS),
              document.doc?.artworkIndices.contains(index) == true
                || document.doc?.isGeneratedLayer(index) == true else { NSSound.beep(); return }
        document.applyEdit("Rasterize Artwork", record: .rasterizeArtwork()) { $0.withLayerMeta(index, nil) }
    }
}

extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let artworkRasterizationHandlers: [String: ToolHandler] = [
        "rasterize_artwork": rasterizeArtwork,
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let artworkRasterizationToolFacts: [String: ToolFacts] = [
        // Also in a 32-bit float document; #1: and in a CMYK one.
        "rasterize_artwork": [.commonTool, .floatTool, .cmykTool],
    ]

    /// Mirrors Layer > Rasterize Artwork; samples, masks and profile stay exact.
    func rasterizeArtwork(_ arguments: [String: Any]) throws -> String {
        let document = try target(arguments)
        let index = try paintLayerIndex(arguments, document)
        // A Generative Layer too: at 16 bits its tag is a typed description, which
        // Puppet and Perspective Warp ask to rasterize first.
        guard document.doc?.artworkIndices.contains(index) == true
                || document.doc?.isGeneratedLayer(index) == true else {
            throw ToolError(message: "Choose an editable text, shape, fill or generated layer.")
        }
        try rejectLockedEdit(document, index, DR_EDIT_PIXELS)
        try performGroupedEdit(document, "Rasterize Artwork") { $0.withLayerMeta(index, nil) }
        return try jsonResult(["ok": true, "layer": index, "editable_description": false,
                               "note": "Current pixels, mask and profile remain exact; Undo restores the artwork description."])
    }
}

extension Array where Element == ActionStep {
    static func rasterizeArtwork() -> [ActionStep] {
        step("rasterize_artwork", note: "Layer ▸ Rasterize Artwork")
    }
}
