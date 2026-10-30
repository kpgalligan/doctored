import AppKit

// A selection carried through a canvas change. Crop, Trim, Crop to
// Selection, Canvas Size, Reveal All, the rotations, the flips, Image Size
// and the straightening crop used to clear the selection, because its
// coordinates belonged to the old canvas. Each of them is an affine map from
// old-canvas to new-canvas coordinates, so the selection now goes through the
// same map and lands in the same undo step as the pixels
// (`applyEdit(resultingSelection:)`, which still clears first). Perspective
// Crop is a homography and still clears.
//
// THE MATRICES, old canvas → new canvas:
// - crop, Trim, Crop to Selection: `translate(−x, −y)`, the window's origin;
// - Canvas Size, Reveal All: `translate(originX, originY)`, where the old
//   canvas's top-left lands;
// - rotate 90 / 180 / 270, flip, Image Size: `DocumentGeometry.canvasMatrix`;
// - the straightening crop and Rotate Canvas: the map each hands
//   `ArtworkGeometryWork` (`CropSession.canvasMap`,
//   `CanvasRotationPlan.map`).

extension CanvasSelection {
    /// This selection after the canvas went through `matrix` and became
    /// `toWidth × toHeight`; nil when nothing of it remains on the new
    /// canvas, or when it cannot be carried.
    ///
    /// The shape stays what it was wherever the map allows: a rectangle or
    /// an ellipse under an axis-aligned map (a translation, a scale, a flip, a
    /// quarter turn) is that shape on the mapped rectangle, a rectangle
    /// clipped to the new canvas; a polygon maps its points; a rectangle
    /// under any other map becomes the polygon of its four mapped corners.
    /// An ellipse under any other map, and coverage (byte or u16), is
    /// resampled: laid into a plane that holds both canvases, mapped by the
    /// selection-transform kernel the Transform Selection command uses, and
    /// cropped to the new canvas. A map of whole numbers only moves samples,
    /// so it takes the nearest filter and stays exact; anything else is
    /// bilinear. A working plane past `RasterImage.maxResizePixels` is not
    /// attempted.
    func carried(by matrix: CGAffineTransform, toWidth: Int, toHeight: Int) -> CanvasSelection? {
        let elements = [matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty]
        guard toWidth > 0, toHeight > 0, elements.allSatisfy(\.isFinite) else { return nil }
        let axisAligned = (matrix.b == 0 && matrix.c == 0) || (matrix.a == 0 && matrix.d == 0)
        let canvas = CGRect(x: 0, y: 0, width: toWidth, height: toHeight)
        let mapped: Shape
        switch shape {
        case .rect(let rect):
            if axisAligned {
                let clipped = rect.applying(matrix).intersection(canvas)
                guard !clipped.isNull, !clipped.isEmpty else { return nil }
                mapped = .rect(clipped)
            } else {
                mapped = .polygon([
                    CGPoint(x: rect.minX, y: rect.minY), CGPoint(x: rect.maxX, y: rect.minY),
                    CGPoint(x: rect.maxX, y: rect.maxY), CGPoint(x: rect.minX, y: rect.maxY),
                ].map { $0.applying(matrix) })
            }
        case .polygon(let points):
            mapped = .polygon(points.map { $0.applying(matrix) })
        case .ellipse(let rect) where axisAligned:
            let moved = rect.applying(matrix)
            guard Self.reaches(moved, canvas) else { return nil }
            mapped = .ellipse(moved)
        case .ellipse, .mask, .mask16:
            guard let coverage = resampled(
                by: matrix, integral: elements.allSatisfy { $0 == $0.rounded() },
                toWidth: toWidth, toHeight: toHeight)
            else { return nil }
            mapped = coverage
        }
        // A polygon (a lasso, or a rectangle under a rotation) that landed
        // wholly off the new canvas is nothing remaining, like the clipped
        // rectangle above.
        var outline: CGRect?
        if case .polygon(let points) = mapped {
            let xs = points.map(\.x), ys = points.map(\.y)
            guard let minX = xs.min(), let maxX = xs.max(), let minY = ys.min(),
                  let maxY = ys.max()
            else { return nil }
            let bounds = CGRect(x: minX, y: minY, width: maxX - minX, height: maxY - minY)
            guard Self.reaches(bounds, canvas) else { return nil }
            outline = bounds
        } else if case .ellipse(let rect) = mapped {
            outline = rect
        }
        guard let carried = CanvasSelection(
            shape: mapped, canvasWidth: toWidth, canvasHeight: toHeight) else { return nil }
        // Bounds that reach the canvas do not mean the shape does: a triangle
        // or an ellipse can leave only an empty corner of its box on it. One
        // that is not wholly inside is kept only if it covers a sample.
        if let outline, !canvas.contains(outline),
           !carried.maskBytes().contains(where: { $0 != 0 }) {
            return nil
        }
        return carried
    }

    /// Whether `bounds` has any area on `canvas`.
    private static func reaches(_ bounds: CGRect, _ canvas: CGRect) -> Bool {
        let clipped = bounds.intersection(canvas)
        return !clipped.isNull && !clipped.isEmpty
    }

    /// The coverage resampled through `matrix` (see `carried`); nil when the
    /// working plane is too large, the kernel refuses, or no sample of it is
    /// left on the new canvas.
    private func resampled(
        by matrix: CGAffineTransform, integral: Bool, toWidth: Int, toHeight: Int
    ) -> Shape? {
        // One plane holding the old canvas (the kernel's input) and the new
        // one (its output), both from the top-left corner.
        let width = max(canvasWidth, toWidth), height = max(canvasHeight, toHeight)
        guard width <= RasterImage.maxResizePixels / height else { return nil }
        if hasNativeCoverage {
            let padded = Self.replaced(
                maskSamples16(), from: (canvasWidth, canvasHeight), to: (width, height))
            let operation: [String: Any] = [
                "kind": "transform",
                "matrix": [matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty]
                    .map(Double.init),
                "filter": integral ? "nearest" : "bilinear",
            ]
            guard let moved = try? NativeSelectionWork(
                width: width, height: height, operation: operation, input: padded
            ).run(title: "Transforming Selection") else { return nil }
            let cropped = Self.replaced(moved, from: (width, height), to: (toWidth, toHeight))
            return cropped.contains(where: { $0 != 0 }) ? .mask16(cropped) : nil
        }
        let padded = Self.replaced(
            maskBytes(), from: (canvasWidth, canvasHeight), to: (width, height))
        guard let moved = try? RasterImage.transformingSelection(
            padded, width: width, height: height, matrix: matrix,
            filter: integral ? DR_FILTER_NEAREST : DR_FILTER_BILINEAR)
        else { return nil }
        let cropped = Self.replaced(moved, from: (width, height), to: (toWidth, toHeight))
        return cropped.contains(where: { $0 != 0 }) ? .mask(cropped) : nil
    }

    /// `samples` (row-major, `from` sized) re-laid into a `to` sized plane
    /// from the top-left corner: zero where `to` is larger, cropped where it
    /// is smaller.
    private static func replaced<T: FixedWidthInteger>(
        _ samples: [T], from: (width: Int, height: Int), to: (width: Int, height: Int)
    ) -> [T] {
        if from == to { return samples }
        var out = [T](repeating: 0, count: to.width * to.height)
        let columns = min(from.width, to.width)
        for row in 0..<min(from.height, to.height) {
            let source = row * from.width, target = row * to.width
            out.replaceSubrange(
                target..<(target + columns), with: samples[source..<(source + columns)])
        }
        return out
    }
}

extension ImageDocument {
    /// The selection state a canvas change by `matrix` to a `width × height`
    /// canvas leaves: the selection carried (`CanvasSelection.carried`), out
    /// of Quick Mask, with no previous selection (Reselect's is not
    /// carried). nil when there is no selection, or none of it remains —
    /// `applyEdit` then clears, as it always did.
    func selectionCarried(
        by matrix: CGAffineTransform, width: Int, height: Int
    ) -> SelectionHistoryState? {
        guard let carried = canvasSelection?.carried(by: matrix, toWidth: width, toHeight: height)
        else { return nil }
        return SelectionHistoryState(selection: carried, previous: nil, quickMaskActive: false)
    }

    /// `selectionCarried` for a whole-document geometry op on the current
    /// canvas: its matrix and the canvas it produces.
    func selectionCarried(through op: DocumentGeometry) -> SelectionHistoryState? {
        guard let doc else { return nil }
        let matrix = op.canvasMatrix(canvasWidth: doc.width, canvasHeight: doc.height)
        let size = CGRect(x: 0, y: 0, width: doc.width, height: doc.height).applying(matrix)
        return selectionCarried(
            by: matrix, width: Int(size.width.rounded()), height: Int(size.height.rounded()))
    }

    /// `selectionCarried` for a crop to the window at (`x`, `y`).
    func selectionCarried(croppedToX x: Int, y: Int, width: Int, height: Int)
        -> SelectionHistoryState?
    {
        selectionCarried(
            by: CGAffineTransform(translationX: CGFloat(-x), y: CGFloat(-y)),
            width: width, height: height)
    }

    /// `selectionCarried` for a canvas resize that lands the old canvas's
    /// top-left at (`originX`, `originY`).
    func selectionCarried(canvasWidth width: Int, height: Int, originX: Int, originY: Int)
        -> SelectionHistoryState?
    {
        selectionCarried(
            by: CGAffineTransform(translationX: CGFloat(originX), y: CGFloat(originY)),
            width: width, height: height)
    }
}

extension DocumentGeometry {
    /// The op's full old-canvas → new-canvas map on a `canvasWidth ×
    /// canvasHeight` canvas: `linearMap` with `translation`, as one affine
    /// matrix.
    func canvasMatrix(canvasWidth: Int, canvasHeight: Int) -> CGAffineTransform {
        let map = linearMap(canvasWidth: canvasWidth, canvasHeight: canvasHeight)
        let shift = translation(canvasWidth: canvasWidth, canvasHeight: canvasHeight)
        return CGAffineTransform(
            a: CGFloat(map.a), b: CGFloat(map.b), c: CGFloat(map.c), d: CGFloat(map.d),
            tx: shift.x, ty: shift.y)
    }
}
