import AppKit

extension RasterDocument {
    // MARK: - Layer masks

    /// Gives layer `idx` a mask (replacing any existing one) and enables it,
    /// at exactly the layer's pixel size. `selection` is a CANVAS-sized
    /// coverage buffer, required by `DR_MASK_FROM_SELECTION` (it is cropped
    /// to the layer) and unused — pass nil — by the other kinds.
    func addingLayerMask(
        _ idx: Int, kind: DrMaskKind, selection: [UInt8]? = nil
    ) -> RasterDocument? {
        guard isValidIndex(idx) else { return nil }
        guard kind == DR_MASK_FROM_SELECTION else {
            return wrap(dr_doc_adding_layer_mask(ptr, idx, kind, nil, 0, 0))
        }
        guard let selection = selection, selection.count == width * height else { return nil }
        return selection.withUnsafeBufferPointer { buffer in
            wrap(
                dr_doc_adding_layer_mask(
                    ptr, idx, kind, buffer.baseAddress, UInt32(width), UInt32(height)))
        }
    }

    /// Replaces raster layer `idx`'s mask with `coverage` (`width`×`height`,
    /// row 0 at top) placed at canvas (`x`, `y`) — never cropped to the layer.
    /// Byte documents only: a typed facade's NULL handle answers nil.
    /// nil for a group, Lock All, `outsideCoverage` other than 0/255 or an
    /// identical mask.
    func settingLayerMask(
        _ idx: Int, coverage: [UInt8], width: Int, height: Int, x: Int, y: Int,
        linked: Bool, outsideCoverage: UInt8
    ) -> RasterDocument? {
        guard isValidIndex(idx), width > 0, height > 0, coverage.count == width * height,
              let x = Int32(exactly: x), let y = Int32(exactly: y)
        else { return nil }
        return coverage.withUnsafeBufferPointer { buffer in
            wrap(
                dr_doc_set_mask(
                    ptr, idx, buffer.baseAddress, UInt32(width), UInt32(height), x, y, linked,
                    outsideCoverage))
        }
    }

    /// Drops layer `idx`'s mask. With `apply` the coverage is first baked
    /// into the layer's alpha (regardless of the enabled flag).
    func removingLayerMask(_ idx: Int, apply: Bool) throws -> RasterDocument? {
        if isHighDepth {
            if apply {
                try NativeMaskApplication.validate(self, layer: idx)
                return try performingPrecisionPixelEdit(.init(["op": "apply_mask", "layer": idx]))
            }
            return try performingPrecisionPixelEdit(.removeMask(layer: idx))
        }
        guard isValidIndex(idx) else { return nil }
        return wrap(dr_doc_removing_layer_mask(ptr, idx, apply))
    }

    /// Enables or disables layer `idx`'s mask; a disabled mask is retained
    /// (and saved) but ignored while compositing.
    func withLayerMaskEnabled(_ idx: Int, _ enabled: Bool) throws -> RasterDocument? {
        if isHighDepth { return try precisionMaskProperties(idx, enabled: enabled) }
        guard isValidIndex(idx) else { return nil }
        return wrap(dr_doc_with_layer_mask_enabled(ptr, idx, enabled))
    }

    /// Paints layer `idx`'s MASK with a CANVAS-frame premultiplied overlay —
    /// the very buffer `paintingLayer` takes — mapped through the layer's
    /// mask offset: white reveals, black hides, the overlay's alpha is the blend.
    func paintingLayerMask(
        _ idx: Int, overlay data: UnsafePointer<UInt8>, w: Int, h: Int
    ) -> RasterDocument? {
        guard isValidIndex(idx), w == width, h == height else { return nil }
        return wrap(dr_doc_painting_layer_mask(ptr, idx, data, UInt32(w), UInt32(h)))
    }

    /// Raw mask thumbnail: independent stored size for raster layers, legacy
    /// canvas-aligned view for groups. Use layerMaskStorage for exact raw data.
    func layerMaskImage(_ idx: Int) -> RasterImage? {
        guard isValidIndex(idx) else { return nil }
        if isHighDepth { return try? nativePlaneDisplay(target: "mask", index: idx, canvas: false) }
        return wrapImage(dr_doc_layer_mask_image(ptr, idx))
    }

    /// Layer `idx`'s byte mask as its Layers-panel thumbnail, fitted in the
    /// core to `maxSide` before any whole-mask image: the picture
    /// `layerMaskImage` shows (a group's canvas-aligned view), never
    /// upscaled. nil without a mask, at high depth, or for a negative
    /// `maxSide`.
    func fittedLayerMaskImage(_ idx: Int, maxSide: Int) -> RasterImage? {
        guard !isHighDepth, isValidIndex(idx), maxSide >= 0 else { return nil }
        return wrapImage(dr_doc_layer_mask_thumbnail(ptr, idx, UInt32(clamping: maxSide)))
    }

    /// Layer `idx`'s mask as a coverage plane (width*height bytes, row 0 =
    /// top — raw raster storage or canvas-aligned group view), read back
    /// through dr_doc_layer_mask_image (opaque grayscale RGBA: the red
    /// channel is the coverage). nil without a mask.
    func layerMaskCoverage(_ idx: Int) -> [UInt8]? {
        guard isValidIndex(idx), let image = dr_doc_layer_mask_image(ptr, idx) else {
            return nil
        }
        defer { dr_image_free(image) }
        let width = Int(dr_image_width(image))
        let height = Int(dr_image_height(image))
        guard width > 0, height > 0, let pixels = dr_image_pixels_rgba(image) else {
            return nil
        }
        var plane = [UInt8](repeating: 0, count: width * height)
        for i in 0..<(width * height) {
            plane[i] = pixels[i * 4]
        }
        return plane
    }

    func layerHasMask(_ idx: Int) -> Bool {
        if let precisionMetadata { return precisionMetadata.layer(idx)?.mask != nil }
        guard isValidIndex(idx) else { return false }
        return dr_doc_layer_has_mask(ptr, idx)
    }

    /// True only for a layer that HAS a mask and has it enabled, so it can
    /// drive a checkbox or menu item state directly.
    func layerMaskEnabled(_ idx: Int) -> Bool {
        if let precisionMetadata { return precisionMetadata.layer(idx)?.maskEnabled ?? false }
        guard isValidIndex(idx) else { return false }
        return dr_doc_layer_mask_enabled(ptr, idx)
    }
}
