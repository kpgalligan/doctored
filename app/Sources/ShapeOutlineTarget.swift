import AppKit

// A path shape layer's outline as a current-path target (`PathTarget.shapeOutline`,
// `designs/v1-paths.md`): the Pen, Path Selection and Direct Selection edit the outline in CANVAS
// coordinates, as they edit a path of the list or a vector mask, and each finished gesture
// re-renders the layer as ONE undo step (`ImageDocument.commitShapeOutline`). The payload keeps
// the outline box-local (ShapeOutline.swift) under the layer's 2×2 `transform`, so an outline
// point lies on the canvas at `anchor + T·local` (`NativeArtwork.shape`'s placement); this file
// is the one home of that map in both directions. `edit_shape_layer` with `subpaths` or `path`
// is the MCP twin (AgentServer+PathShapes.swift), and every canvas commit records it.

extension PathShape {
    /// Every point and handle through `map`; structure, smoothness and fill rule kept.
    fileprivate func mappingPoints(_ map: (CGPoint) -> CGPoint) -> PathShape {
        var result = self
        for s in result.subpaths.indices {
            result.subpaths[s].anchors = result.subpaths[s].anchors.map { anchor in
                PathAnchor(point: map(anchor.point), handleIn: map(anchor.handleIn),
                           handleOut: map(anchor.handleOut), smooth: anchor.smooth)
            }
        }
        return result
    }
}

extension ShapeLayerPayload {
    /// The outline in canvas coordinates with the box's top-left at `anchor`: each point
    /// `anchor + T·local`, T the payload's transform. nil for any other kind.
    func canvasOutline(anchor: CGPoint) -> PathShape? {
        guard kind == "path", let path else { return nil }
        let map = transform
        return path.shape(origin: .zero).mappingPoints { local in
            let mapped = map.apply(local)
            return CGPoint(x: anchor.x + mapped.x, y: anchor.y + mapped.y)
        }
    }

    /// This path shape with its outline replaced by `canvas` (canvas coordinates), and the anchor
    /// the layer takes for it, so that `canvasOutline` of the result reads `canvas` back. The
    /// canvas shape is the truth; style, colours (a CMYK layer's stored inks too) and transform
    /// are kept.
    ///
    /// Untransformed, the outline is placed as a new one is (`ShapeOutline.placed`): its box is
    /// the outline's tight box with its origin floored to a whole pixel, which becomes the anchor,
    /// so the layer equals a fresh `add_shape_layer` of the same subpaths and any fraction the old
    /// anchor had moves into the outline. Under a map T, the canvas shape is first quantized by
    /// the same rule, then taken into the box's space (`local = T⁻¹(p − anchor)`); the new box is
    /// that outline's tight box with its origin `o` floored to a whole pixel, the outline is
    /// stored relative to it, and the anchor moves by `T·o` (snapped within 1e-9 of an integer,
    /// `DescribedLayer.snappedAnchor`). Reading back gives `anchor + T·o + T·(local − o)`: the
    /// quantized canvas shape within floating-point error, far inside `PathSelectSession.matches`'
    /// 1e-3, which is what keeps a selection across its own commit.
    ///
    /// nil for another kind, for what `placed` refuses (an empty shape, one past the caps or the
    /// ±1,000,000 wall, one with no extent) and for a local outline past `ShapeOutline.isValid`'s
    /// bounds. The style is not checked against the new box here (a fill-only outline under 1 px
    /// across on one axis shows nothing): `edit_shape_layer` refuses that with its own sentence
    /// after any new style is applied, and the render refuses it for a canvas gesture.
    func replacingOutline(_ canvas: PathShape, anchor: CGPoint)
        -> (payload: ShapeLayerPayload, anchor: CGPoint)?
    {
        guard kind == "path" else { return nil }
        var result = self
        let placedAnchor: CGPoint
        if transform.isIdentity {
            guard let placed = ShapeOutline.placed(canvas) else { return nil }
            result.path = placed.outline
            result.w = Double(placed.box.width)
            result.h = Double(placed.box.height)
            placedAnchor = placed.box.origin
        } else {
            guard let shape = ShapeOutline.quantized(canvas), let inverse = transform.inverted()
            else { return nil }
            let local = shape.mappingPoints { p in
                inverse.apply(CGPoint(x: p.x - anchor.x, y: p.y - anchor.y))
            }
            let bounds = local.cgPath().boundingBoxOfPath
            guard !bounds.isNull, [bounds.minX, bounds.minY, bounds.maxX, bounds.maxY]
                .allSatisfy({ $0.isFinite && abs($0) <= ShapeOutline.maxLocalCoordinate })
            else { return nil }
            let origin = CGPoint(x: bounds.minX.rounded(.down), y: bounds.minY.rounded(.down))
            result.path = ShapeOutline(local, origin: origin)
            result.w = Double(bounds.maxX - origin.x)
            result.h = Double(bounds.maxY - origin.y)
            let shift = transform.apply(origin)
            placedAnchor = DescribedLayer.snappedAnchor(
                CGPoint(x: anchor.x + shift.x, y: anchor.y + shift.y))
        }
        guard result.path?.isValid == true else { return nil }
        return (result, placedAnchor)
    }
}

extension RasterDocument {
    /// Layer `idx`'s outline in canvas coordinates; nil unless it is a path shape whose anchor is
    /// known. What `PathTarget.shapeOutline` resolves to.
    func shapeOutlinePath(_ idx: Int) -> PathShape? {
        guard let payload = shapePayload(idx), payload.kind == "path",
              let anchor = describedAnchor(idx)
        else { return nil }
        return payload.canvasOutline(anchor: anchor)
    }
}

/// What `ImageDocument.commitShapeOutline` did.
enum ShapeOutlineCommit {
    /// One undo step re-rendered the layer.
    case committed
    /// Nothing to tell: the outline was the layer's already, or `applyEdit` itself refused and
    /// said so.
    case silent
    /// Refused before `applyEdit` ran — the layer gone or no longer a path shape, an outline the
    /// shape cannot hold, a render the core refuses (a lock) — for the caller to report.
    case refused
}

extension ImageDocument {
    /// One Pen, Path Selection or Direct Selection gesture on a path shape's OUTLINE, as ONE undo
    /// step: `shape` (canvas coordinates) becomes the outline (`replacingOutline`) and the layer
    /// is re-rendered from it, its style, inks, transform and mask kept. The recorded step is the
    /// `edit_shape_layer` call that writes the same outline (`subpaths` + `fill_rule`, canvas px).
    ///
    /// The render is computed before `applyEdit` runs, so a refused anchor is reported by the
    /// caller (a beep, or the lock's alert) instead of stopping the drawing behind a sheet:
    /// `commitPenVectorMask`'s rule.
    func commitShapeOutline(
        _ shape: PathShape, layerID: UInt64, name: String, note: String
    ) -> ShapeOutlineCommit {
        guard let before = doc, let index = before.layerIndex(id: layerID),
              let payload = before.shapePayload(index), let anchor = before.describedAnchor(index),
              let replaced = payload.replacingOutline(shape, anchor: anchor)
        else { return .refused }
        if replaced.payload.path == payload.path, replaced.payload.w == payload.w,
           replaced.payload.h == payload.h, replaced.anchor == anchor {
            return .silent
        }
        guard let next = try? before.rerenderingArtworkLayer(
            index, .shape(replaced.payload), anchor: replaced.anchor)
        else { return .refused }
        applyEdit(name, record: .editShapeOutline(shape, layer: before.layerRef(index), note: note)) {
            _ in next
        }
        return doc === next ? .committed : .silent
    }
}
