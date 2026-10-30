import AppKit

// The core expands subtree/link membership; the host captures editable text/shape on main and
// regenerates it at native depth after affine or whole-document geometry. True perspective requires
// explicit artwork rasterization authorization and reports only actual description loss;
// RAW masters are never rasterized by that option. Keep C calls in `DoctoredCore.swift`, and never construct a preview by
// committing its display plate. `geometrySetPrepared` estimates per stage, `max(core, artwork)`
// (the sum under a true perspective loss), and with no control on main admits that and re-admits
// the artwork stage itself.

/// One captured canvas map over the selected postorder records. The Rust core
/// expands subtrees and links and applies the result atomically at either depth.
enum GeometrySetRequest {
    case affine(layers: [Int], matrix: CGAffineTransform, sampler: DrResizeFilter)
    case perspective(layers: [Int], sourceRect: CGRect, quad: [CGPoint], sampler: DrResizeFilter)

    var layers: [Int] {
        switch self {
        case .affine(let layers, _, _), .perspective(let layers, _, _, _): return layers
        }
    }

    /// `interactive` (a ⌘T preview) keeps the core's interactive
    /// pixel-work limit; a commit omits it and has no work limit under its
    /// job control.
    func command(interactive: Bool = false) throws -> PrecisionPixelCommand {
        var fields: [String: Any]
        switch self {
        case .affine(let layers, let matrix, let sampler):
            fields = [
                "op": "affine_set", "layers": layers,
                "matrix": [matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty].map(Double.init),
                "filter": try RasterDocument.precisionFilter(sampler).rawValue
            ]
        case .perspective(let layers, let rect, let quad, let sampler):
            guard quad.count == 4 else {
                throw DoctoredCoreError(message: "Perspective requires four TL/TR/BR/BL corners.")
            }
            fields = [
                "op": "perspective_set", "layers": layers,
                "source_rect": [rect.minX, rect.minY, rect.width, rect.height].map(Double.init),
                "quad": quad.flatMap { [Double($0.x), Double($0.y)] },
                "filter": try RasterDocument.precisionFilter(sampler).rawValue
            ]
        }
        if interactive { fields["interactive"] = true }
        return try PrecisionPixelCommand(fields)
    }
}

extension RasterDocument {
    func geometrySet(_ request: GeometrySetRequest, rasterizeArtwork: Bool = false,
                     control: RasterJobControl? = nil) throws -> RasterDocument? {
        let affected = movingSet(request.layers)
        let artwork = affected.filter(hasArtworkDescription)
        // A pattern fill re-renders after any move (FillLayer.swift's PIN),
        // so a set carrying one never takes the integer-translation shortcut.
        if artwork.isEmpty
            || (request.isIntegerTranslation && !carriesPatternFill(moving: request.layers)) {
            return try editingGeometry(request: request.command().request, control: control)
        }
        guard Thread.isMainThread else {
            throw DoctoredCoreError(message:
                "Capture editable artwork on the main thread before geometry work starts.")
        }
        let prepared = try ArtworkGeometryWork(source: self, request: request,
                                               rasterizeArtwork: rasterizeArtwork)
        return try geometrySetPrepared(request, artwork: prepared, control: control)
    }

    /// `admittingArtwork` (the final jobs' worker only — never a
    /// preview, whose own reservation would count against it) admits the
    /// artwork stage immediately before it does work: before `prepareBefore`
    /// for a true perspective set, else before `prepareAfter`.
    ///
    /// The estimate is per stage: the larger of the core and the
    /// artwork, or their sum under a true perspective loss, whose
    /// `prepareBefore` output stays live through the core. With no control on
    /// main it is admitted here and the artwork stage re-admits itself too.
    /// `interactive` marks a preview's core request.
    func geometrySetPrepared(_ request: GeometrySetRequest, artwork: ArtworkGeometryWork?,
                             control: RasterJobControl? = nil,
                             admittingArtwork: Bool = false,
                             interactive: Bool = false) throws -> RasterDocument? {
        let command = try request.command(interactive: interactive)
        guard let artwork, !artwork.members.isEmpty else {
            return try editingGeometry(request: command.request, control: control)
        }
        let core = try PrecisionDocumentDescriptor.integer(
            geometryEditPlan(request: command.request), "scratch_bytes")
        let estimate = try artwork.perspectiveLoss
            ? PrecisionDocumentDescriptor.sum(core, artwork.scratchBytes)
            : max(core, artwork.scratchBytes)
        let admitsOnMain = control == nil && Thread.isMainThread
        if admitsOnMain {
            _ = try RasterResources.shared.admit(scratch: estimate)
        }
        let workControl: RasterJobControl
        if let control { workControl = control }
        else {
            workControl = try RasterJobControl()
            guard workControl.setScratchLimit(estimate) else {
                throw DoctoredCoreError(message: "Could not bound artwork geometry work.")
            }
        }
        try workControl.beginCompoundOperation()
        if (admittingArtwork || admitsOnMain) && artwork.perspectiveLoss {
            _ = try RasterResources.shared.admit(scratch: artwork.scratchBytes)
        }
        let input = try artwork.prepareBefore(self, control: workControl)
        guard let transformed = try workControl.withChildOperation({ child in
            try input.editingGeometry(request: command.request, control: child)
        }) else { return nil }
        if (admittingArtwork || admitsOnMain) && !artwork.perspectiveLoss {
            _ = try RasterResources.shared.admit(scratch: artwork.scratchBytes)
        }
        return try artwork.prepareAfter(transformed, control: workControl)
    }
}

extension GeometrySetRequest {
    var isIntegerTranslation: Bool {
        guard case .affine(_, let matrix, _) = self else { return false }
        return matrix.a == 1 && matrix.b == 0 && matrix.c == 0 && matrix.d == 1
            && matrix.tx.isFinite && matrix.ty.isFinite
            && matrix.tx == matrix.tx.rounded() && matrix.ty == matrix.ty.rounded()
    }
}
