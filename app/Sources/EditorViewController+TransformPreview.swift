import AppKit

extension EditorViewController {
    /// Masked-set sessions use raw pixel bounds. Reading effective content
    /// bounds could build a large feather cache on the main thread before
    /// preview admission; masks are evaluated on the worker instead.
    func transformPixelSourceRect(_ doc: RasterDocument, layers: [Int]) -> CGRect? {
        var union: CGRect?
        for index in layers {
            if !doc.layerIsAdjustment(index), let info = doc.layerInfo(index) {
                let rect = CGRect(x: info.offsetX, y: info.offsetY,
                                  width: info.width, height: info.height)
                if rect.width > 0, rect.height > 0 {
                    union = union.map { $0.union(rect) } ?? rect
                }
            }
            // An adjustment has no raster content; its linked mask is its
            // entire transformable extent. Independent linked masks on groups
            // and rasters also participate in the shared handle rectangle.
            if let mask = doc.layerMaskProperties(index), mask.linked {
                let rect = CGRect(x: mask.offsetX, y: mask.offsetY,
                                  width: mask.width, height: mask.height)
                if rect.width > 0, rect.height > 0 {
                    union = union.map { $0.union(rect) } ?? rect
                }
            }
        }
        return union
    }

    /// The box updates immediately; core projection results arrive only for
    /// the same immutable source, revision, session and parameter generation.
    func refreshMaskedTransformPreview(_ session: TransformSession) {
        guard let preview = session.corePreview, let document,
              document.doc === (session.duplicate?.base ?? preview.source),
              document.revision == preview.revision else { return }
        preview.onUpdate = { [weak self, weak preview] in
            guard let self, let preview, let current = self.transformSession,
                  current.corePreview === preview,
                  self.document?.doc === (current.duplicate?.base ?? preview.source),
                  self.document?.revision == preview.revision else { return }
            self.displayMaskedTransformPreview(current, preview: preview)
            self.updateStatus()
        }
        let quad = session.transform.hasCornerOffsets
            ? session.transform.warpedQuad(of: session.sourceRect) : nil
        if let work = try? TransformWork(source: preview.source, layers: session.layers, quad: quad,
            matrix: session.transform.matrix, sampler: session.sampler,
            droppingDescriptions: [], sourceRect: session.sourceRect,
            rasterizeArtwork: true, interactive: true) {
            preview.request(work)
        }
        displayMaskedTransformPreview(session, preview: preview)
    }

    func displayMaskedTransformPreview(_ session: TransformSession,
                                              preview: TransformMaskPreview) {
        canvas.transformPreview = ImageCanvasView.TransformPreview(
            composite: preview.image, usesCoreComposite: true,
            below: nil, above: nil, layer: nil, mask: nil,
            sourceRect: session.sourceRect, matrix: session.transform.matrix, opacity: 1,
            quad: session.transform.warpedQuad(of: session.sourceRect),
            pivot: session.transform.pivotInCanvas,
            interpolate: session.sampler != DR_FILTER_NEAREST,
            warped: session.transform.hasCornerOffsets)
    }
}
