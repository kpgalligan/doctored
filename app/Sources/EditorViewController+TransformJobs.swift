import AppKit

extension EditorViewController {
    /// Final Free Transform raster resampling. All prompts and described-source
    /// composition finish before this worker captures its immutable input. The
    /// caller keeps its session open when refusal/staleness returns false.
    ///
    /// `name` titles the wait, the refusal beep and the step; `base` is the
    /// handle the document must hold (nil: `source`). Free Transform a Copy
    /// passes the duplicated `source` and the document's own `base`.
    /// `sourceRect` is the rectangle a `quad` maps from: ⌘T/⌥⌘T pass their
    /// session box so the commit maps what the preview showed, Transform
    /// Again a remembered perspective's own rectangle; nil is
    /// `transformPixelSourceRect(source, layers:)`.
    func commitRasterTransformJob(
        document: ImageDocument, source: RasterDocument, layers: [Int], quad: [CGPoint]?,
        matrix: CGAffineTransform, sampler: DrResizeFilter, record: [ActionStep],
        name: String = "Transform Layer", base: RasterDocument? = nil,
        sourceRect: CGRect? = nil
    ) -> Bool {
        let dropped = source.describedEntries(in: source.movingSet(layers)).map { $0.layer }
        do {
            let work = try TransformWork(source: source, layers: layers, quad: quad, matrix: matrix,
                sampler: sampler, droppingDescriptions: dropped,
                sourceRect: sourceRect ?? transformPixelSourceRect(source, layers: layers),
                rasterizeArtwork: true)
            guard let prepared = try work.wait(in: document, title: name, base: base) else {
                SessionCommitRefusal.beep("\(name) was refused and nothing changed.")
                return false
            }
            document.preparedEdit = prepared
            guard document.admitsPublication(prepared.document, record: record) else { return false }
            withUndoGroup(document.undoManager) {
                document.applyEdit(name, record: record) { _ in prepared.document }
            }
            return document.doc !== (base ?? source)
        } catch is JobCancelled {
            SessionCommitRefusal.note(JobCancelled.message)
            return false
        } catch {
            document.presentError(error)
            return false
        }
    }
}
