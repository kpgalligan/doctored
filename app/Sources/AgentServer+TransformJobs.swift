import AppKit

extension AgentServer {
    /// Mirrors final Free Transform / Distort raster commits. Source rendering
    /// for editable text/shapes is handled before this helper on the
    /// main thread; only pure affine/perspective resampling enters the worker.
    @discardableResult
    func performTransformJob(
        _ document: ImageDocument, _ title: String, layers: [Int], pixelLayer: Int?,
        matrix: CGAffineTransform, quad: [CGPoint]? = nil, sampler: DrResizeFilter,
        sourceRect: CGRect? = nil, rasterizeArtwork: Bool = false
    ) throws -> DroppedDescription? {
        try rejectLockedMove(document, layers)
        guard let source = document.doc else { throw ToolError(message: "No image loaded.") }
        let dropped = pixelLayer.flatMap { Self.description(of: document, layer: $0) }
        let descriptions = source.describedEntries(in: source.movingSet(layers)).map { $0.layer }
        let work = try TransformWork(source: source, layers: layers, quad: quad, matrix: matrix,
                                 sampler: sampler, droppingDescriptions: descriptions,
                                 sourceRect: sourceRect, rasterizeArtwork: rasterizeArtwork)
        guard let prepared = try work.wait(in: document, title: title) else {
            throw ToolError(message: "\(title) failed — check the parameters")
        }
        document.preparedEdit = prepared
        try performGroupedEdit(document, title) { _ in prepared.document }
        // Transform Again's memory: a true perspective stores its corners.
        document.lastFreeTransform = RepeatableTransform(
            matrix: matrix, quad: quad, sourceRect: sourceRect, sampler: sampler)
        if let pixelLayer, Self.description(of: document, layer: pixelLayer) != nil {
            return nil
        }
        return dropped
    }
}
