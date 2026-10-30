import AppKit

extension RasterDocument {
    // MARK: - Pure per-layer edits

    func withLayerName(_ idx: Int, _ name: String) throws -> RasterDocument? {
        if isHighDepth { return try applyingPrecisionBookkeepingEdit(.init(["op": "rename_layer", "layer": idx, "name": name])) }
        guard isValidIndex(idx) else { return nil }
        return wrap(dr_doc_with_layer_name(ptr, idx, name))
    }

    func withLayerOpacity(_ idx: Int, _ opacity: Double) throws -> RasterDocument? {
        if let precision { return try precision.settingLayerOpacity(idx, opacity: Float(opacity)).map { try RasterDocument(precision: $0) } }
        guard isValidIndex(idx) else { return nil }
        return wrap(dr_doc_with_layer_opacity(ptr, idx, Float(opacity)))
    }

    func withLayerBlendMode(_ idx: Int, _ mode: DrBlendMode) throws -> RasterDocument? {
        if isHighDepth { return try settingNativeLayerBlendMode(idx, mode: mode) }
        guard isValidIndex(idx) else { return nil }
        return wrap(dr_doc_with_layer_blend_mode(ptr, idx, mode))
    }

    func withLayerVisible(_ idx: Int, _ visible: Bool) throws -> RasterDocument? {
        if isHighDepth { return try performingPrecisionPixelEdit(.init(["op": "layer_visible", "layer": idx, "visible": visible])) }
        guard isValidIndex(idx) else { return nil }
        return wrap(dr_doc_with_layer_visible(ptr, idx, visible))
    }

    /// `withLayerVisible(idx, false)` for the sweeps that hide a set of
    /// entries to compose a plate — the transform preview's below/above
    /// plates, the histogram's backdrop. An entry that is ALREADY hidden
    /// answers nil, because an op that changes nothing returns nothing (the
    /// core's purity rule), and that is not a failure here: it is the state
    /// the sweep asked for. Keeping the document instead of collapsing the
    /// chain is what stops one hidden layer from emptying a whole plate.
    func hidingLayer(_ idx: Int) -> RasterDocument {
        (try? withLayerVisible(idx, false)) ?? self
    }

    /// A moved pattern fill is re-rendered over the canvas at both depths
    /// (`refittingFillLayers`, FillLayer.swift's PIN).
    func withLayerOffset(_ idx: Int, _ x: Int, _ y: Int) throws -> RasterDocument? {
        if isHighDepth {
            guard let info = layerInfo(idx) else { return nil }
            if x == info.offsetX && y == info.offsetY { return nil }
            return try precisionLayerOffset(idx, dx: x - info.offsetX, dy: y - info.offsetY)?
                .refittingFillLayers()
        }
        guard isValidIndex(idx) else { return nil }
        return try wrap(dr_doc_with_layer_offset(ptr, idx, Int32(clamping: x), Int32(clamping: y)))?
            .refittingFillLayers()
    }

    /// Replaces a layer's pixels (any size; offset and properties kept).
    func withLayerPixels(_ idx: Int, _ image: RasterImage) -> RasterDocument? {
        guard isValidIndex(idx) else { return nil }
        return wrap(dr_doc_with_layer_pixels(ptr, idx, image.ptr))
    }

    /// Writes a full-storage filter result only through canvas selection
    /// coverage. nil selection includes off-canvas storage; a malformed mask
    /// refuses rather than silently becoming an unselected edit. Spatial
    /// filtering and automatic analysis run on the full source beforehand.
    func applyingLayerPixels(
        _ idx: Int, _ result: RasterImage, selection: [UInt8]?
    ) -> RasterDocument? {
        guard isValidIndex(idx), selection == nil || selection?.count == width * height
        else { return nil }
        return withMask(selection) {
            wrap(dr_doc_apply_layer_pixels(ptr, idx, result.ptr, $0, UInt32(width), UInt32(height)))
        }
    }

    /// Scalar counterpart for the layer/group mask, preserving its enabled
    /// flag. Source and result are full raw storage from layerMaskStorage.
    func applyingLayerMask(
        _ idx: Int, _ result: RasterImage, selection: [UInt8]?
    ) -> RasterDocument? {
        guard isValidIndex(idx), selection == nil || selection?.count == width * height
        else { return nil }
        return withMask(selection) {
            wrap(dr_doc_apply_layer_mask(ptr, idx, result.ptr, $0, UInt32(width), UInt32(height)))
        }
    }

    /// Writes the result's gray into one RGBA plane, at the layer's full
    /// stored size. Derived Luma/Mask planes refuse. Selection is canvas-sized.
    func applyingLayerPlane(
        _ idx: Int, _ plane: DrPlane, _ result: RasterImage, selection: [UInt8]?
    ) -> RasterDocument? {
        guard isValidIndex(idx), selection == nil || selection?.count == width * height
        else { return nil }
        return withMask(selection) {
            wrap(dr_doc_apply_layer_plane(
                ptr, idx, plane, result.ptr, $0, UInt32(width), UInt32(height)))
        }
    }

    /// Writes the result's gray into a saved channel through canvas coverage.
    func applyingChannel(
        _ idx: Int, _ result: RasterImage, selection: [UInt8]?
    ) -> RasterDocument? {
        guard isValidChannel(idx), selection == nil || selection?.count == width * height
        else { return nil }
        return withMask(selection) {
            wrap(dr_doc_apply_channel(ptr, idx, result.ptr, $0, UInt32(width), UInt32(height)))
        }
    }

    /// Full raw mask storage and its placement. Group masks retain their
    /// stored offset and off-canvas data, unlike the display thumbnail read.
    func layerMaskStorage(_ idx: Int) -> (image: RasterImage, offsetX: Int, offsetY: Int)? {
        guard isValidIndex(idx) else { return nil }
        var x: Int32 = 0
        var y: Int32 = 0
        guard let image = wrapImage(dr_doc_layer_mask_storage_image(ptr, idx, &x, &y))
        else { return nil }
        return (image, Int(x), Int(y))
    }

    /// The ONE-step twin of withLayerPixels + withLayerOffset (+ a mask):
    /// straight RGBA8 `pixels`, new offset, and `mask` (nil = the layer
    /// ends with no mask; a plane must be exactly width*height bytes) —
    /// dr_doc_set_layer_content, the re-render primitive behind described
    /// layers (DescribedLayer.swift). A given mask keeps the layer's
    /// mask-enabled flag; no mask resets it. nil on a bad size, a mask of
    /// the wrong length, or an out-of-range index.
    func setLayerContent(
        _ idx: Int, rgba pixels: [UInt8], width: Int, height: Int,
        offsetX: Int, offsetY: Int, mask: [UInt8]?, preservingMask: Bool = false
    ) -> RasterDocument? {
        guard isValidIndex(idx), width > 0, height > 0,
              pixels.count == width * height * 4,
              width * height <= RasterImage.maxResizePixels,
              mask == nil || mask?.count == width * height
        else { return nil }
        let x = Int32(clamping: offsetX)
        let y = Int32(clamping: offsetY)
        return pixels.withUnsafeBufferPointer { buffer in
            if preservingMask {
                return wrap(dr_doc_set_layer_content_preserving_mask(
                    ptr, idx, buffer.baseAddress, UInt32(width), UInt32(height), x, y))
            }
            guard let mask = mask else {
                return wrap(
                    dr_doc_set_layer_content(
                        ptr, idx, buffer.baseAddress, UInt32(width), UInt32(height), x, y, nil))
            }
            return mask.withUnsafeBufferPointer { plane in
                wrap(
                    dr_doc_set_layer_content(
                        ptr, idx, buffer.baseAddress, UInt32(width), UInt32(height), x, y,
                        plane.baseAddress))
            }
        }
    }
}
