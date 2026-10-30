import AppKit

// A 16-bit ⌘T or Crop straighten whose true preview only the core's interactive pixel-work
// cap refuses (`precision_render::MAX_WORK_UNITS`, 2e9) previews at reduced resolution instead of
// showing nothing. The size comes from the COMMIT plan's `work_units`
// (`PrecisionGeometryWork.commitWorkUnits`), so it holds at every sampler and layer count: plane
// work is destination pixels × channels × (taps² + 2), so a proxy of factor k carries about
// `workUnits / k²`, at most `workBudget`, half the cap. `PrecisionGeometryGesture` builds the
// reduced document once per gesture off main (`ProxySourceLoader`), maps each request onto it
// (`GeometryProxyScale.map`) and runs it through the same reservation and display path as a true
// preview. Commits never see a proxy. Float documents, the 8-bit `TransformMaskPreview`,
// `.transformSelected` and Move have none: a refused float or 8-bit preview runs the commit
// request as a cancellable preview job instead, and `.transformSelected` and Move have no work
// refusal to answer.

/// The integer reduction of a geometry preview proxy and the request map onto it.
struct GeometryProxyScale {
    /// Half the core's interactive `precision_render::MAX_WORK_UNITS` (2e9).
    static let workBudget: UInt64 = 1_000_000_000
    /// k = ceil(sqrt(workUnits / workBudget)).
    let factor: Int
    /// The proxy canvas: (W / k, H / k), integer division.
    let width: Int, height: Int
    /// width / W and height / H: what canvas coordinates are multiplied by.
    let sx: CGFloat, sy: CGFloat

    /// nil when k < 2 (the true preview is not what the work cap refuses) or
    /// a proxy side would be 0.
    init?(width: Int, height: Int, workUnits: UInt64) {
        guard width > 0, height > 0 else { return nil }
        let ratio = Double(workUnits) / Double(Self.workBudget)
        guard ratio.isFinite, ratio < 1e12 else { return nil }
        let k = Int(ratio.squareRoot().rounded(.up))
        guard k >= 2, width / k > 0, height / k > 0 else { return nil }
        factor = k
        self.width = width / k
        self.height = height / k
        sx = CGFloat(self.width) / CGFloat(width)
        sy = CGFloat(self.height) / CGFloat(height)
    }

    /// `request` on the proxy canvas: a transform's matrix becomes
    /// scale(1/sx, 1/sy), then the matrix, then scale(sx, sy), with its quad
    /// and source rectangle scaled, the same layers and sampler; a straighten
    /// (a crop with a nonzero angle) keeps its angle, sampler, Delete Cropped
    /// Pixels and Background fill over the scaled rectangle (an extended
    /// box previews its pad). nil for every other
    /// request: they have no proxy.
    func map(_ request: PrecisionGeometryRequest) -> PrecisionGeometryRequest? {
        switch request {
        case let .transform(layer, matrix, quad, sampler):
            return .transform(layer: layer, matrix: scaledMatrix(matrix), quad: quad.map(scaledPoints),
                              sampler: sampler)
        case let .transformSet(layers, sourceRect, matrix, quad, sampler):
            return .transformSet(layers: layers, sourceRect: scaledRect(sourceRect),
                                 matrix: scaledMatrix(matrix), quad: quad.map(scaledPoints), sampler: sampler)
        case let .duplicateTransform(layers, sourceRect, matrix, quad, sampler):
            return .duplicateTransform(layers: layers, sourceRect: scaledRect(sourceRect),
                                       matrix: scaledMatrix(matrix), quad: quad.map(scaledPoints),
                                       sampler: sampler)
        case let .crop(rect, angle, sampler, deleteCropped, background):
            guard angle != 0 else { return nil }
            return .crop(rect: scaledRect(rect), angle: angle, sampler: sampler,
                         deleteCropped: deleteCropped, background: background)
        case .move, .duplicateMove, .moveSelected, .transformSelected, .align, .distribute:
            return nil
        }
    }

    private func scaledMatrix(_ matrix: CGAffineTransform) -> CGAffineTransform {
        CGAffineTransform(scaleX: 1 / sx, y: 1 / sy)
            .concatenating(matrix)
            .concatenating(CGAffineTransform(scaleX: sx, y: sy))
    }

    private func scaledPoints(_ points: [CGPoint]) -> [CGPoint] {
        points.map { CGPoint(x: $0.x * sx, y: $0.y * sy) }
    }

    private func scaledRect(_ rect: CGRect) -> CGRect {
        CGRect(x: rect.minX * sx, y: rect.minY * sy, width: rect.width * sx, height: rect.height * sy)
    }
}

/// The reduced-resolution geometry preview.
enum GeometryProxyPreview {
    /// The status of a landed preview drawn from the proxy.
    static let status = "Preview current (reduced resolution; the commit renders at full size)"

    /// `facade` reduced to the proxy canvas with the document resize
    /// (bilinear), which plans and bounds its own work. Off main only: on
    /// main the typed resize would run a modal job. nil on any failure.
    static func source(of facade: RasterDocument, scale: GeometryProxyScale) -> RasterDocument? {
        assert(!Thread.isMainThread)
        guard let reduced = try? facade.resized(w: scale.width, h: scale.height,
                                                filter: DR_FILTER_BILINEAR) else { return nil }
        return reduced
    }
}
