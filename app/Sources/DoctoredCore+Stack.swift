import AppKit

// **12E-2c** makes Stamp Visible and Flatten Image typed the same way as 12E-1's structure tools —
// `DoctoredCore.stampVisible(above:name:)` and `flattening()` route typed on their FIRST line, so
// `EditorViewController+MergeStamp`'s handlers, the frozen `EditorViewController.flattenImage`,
// `AgentServer+Groups.stampVisible` and `AgentServer.flattenImage` are untouched — and adds TEN
// selectors to BOTH whitelists in `EditorViewController+Precision.swift`: this run's
// `stampVisible:` / `flattenImage:` plus the EIGHT 12E-1b left behind (`mergeDown:`,
// `mergeVisible:`, `linkLayers:`, `unlinkLayers:`, `lockTransparency:`, `lockPixels:`,
// `lockPosition:`, `lockAll:`), whose cores and typed routes had worked since that run. The one
// thing a whitelist addition has to be checked against is the DEPTH-SPECIFIC FLOOR behind the item:
// the typed `merge_visible` / `stamp_visible` floor counts `Content::Raster`, which is NARROWER
// than `contributingLeaves()` because an adjustment leaf contributes to the projection but is not a
// raster. So `RasterDocument.contributingRasterLeaves()` lives in `LayerTree.swift` beside
// `contributingLeaves()`, and `canMergeVisible` (≥ 2) / `canStampVisible` (≥ 1) in
// `EditorViewController+MergeStamp.swift` branch on `isHighDepth` — those two files are the ONLY
// home of that floor, the 8-bit answers are unchanged, and the frozen controller was not touched.
// `stamp_visible` / `flatten_image` join `typedTools`, both `families` maps (the DEFAULT
// `["document"]` branch) and `FloatDocument.tools`.
//
// `RasterDocument.addingLayer(above:)` FORWARDS to the positional twin
// `addingLayer(anchor:position:name:)`, which routes typed on its first line through the core's
// `{"op":"add_layer"}` op — `precisionRemoveLayers`' shape — so the frozen
// `EditorViewController.newLayer` was not touched and the Layers panel's `addButton` is simply no
// longer disabled at depth.

extension RasterDocument {
    // MARK: - Stack operations

    /// Inserts a transparent canvas-sized layer ABOVE `idx`. Since 12E-2b this
    /// forwards to the positional twin at BOTH depths, so every existing
    /// caller — the New Layer menu item, the panel button, the sheets — gets
    /// the typed path with no branch of its own.
    func addingLayer(above idx: Int, name: String) throws -> RasterDocument? {
        try addingLayer(anchor: idx, position: .above, name: name)
    }

    /// Inserts a transparent canvas-sized layer at the slot `position` names
    /// relative to `anchor` — the positional twin of `addingLayer(above:)`
    /// (12E-2). The core owns the slot rule and every refusal
    /// (`core/src/layer_insert.rs`); 12E-2b routes the TYPED depths through
    /// the core's own `add_layer` structure op on the first line, exactly as
    /// `precisionRemoveLayers` does, so no typed edit uses a byte surrogate.
    func addingLayer(anchor: Int, position: LayerInsertPosition, name: String) throws -> RasterDocument? {
        if isHighDepth {
            return try precisionAddLayer(anchor: anchor, position: position, name: name)
        }
        guard isValidIndex(anchor) else { return nil }
        return wrap(dr_doc_adding_layer_at(ptr, anchor, position.code, name))
    }

    /// Inserts `image` as a new layer ABOVE `idx`.
    func addingImageLayer(above idx: Int, _ image: RasterImage, name: String) -> RasterDocument? {
        guard isValidIndex(idx) else { return nil }
        return wrap(dr_doc_adding_image_layer(ptr, idx, image.ptr, name))
    }

    /// Inserts `image` as a new layer ABOVE `idx` at `offset`: a paste is MADE there,
    /// so its Photoshop reference point (fxrp) is (0, 0), unlike a later move (8-bit only).
    func addingImageLayer(
        above idx: Int, _ image: RasterImage, name: String, offset: (x: Int, y: Int)
    ) -> RasterDocument? {
        guard isValidIndex(idx),
              let x = Int32(exactly: offset.x), let y = Int32(exactly: offset.y)
        else { return nil }
        return wrap(dr_doc_add_image_layer_at(ptr, idx, image.ptr, name, x, y))
    }

    func duplicatingLayer(_ idx: Int) throws -> RasterDocument? {
        if isHighDepth { return try precisionDuplicate([idx]) }
        guard isValidIndex(idx) else { return nil }
        return wrap(dr_doc_duplicating_layer(ptr, idx))
    }

    /// nil when removing the last remaining layer.
    func removingLayer(_ idx: Int) throws -> RasterDocument? {
        if isHighDepth { return try precisionRemoveLayers([idx]) }
        guard isValidIndex(idx) else { return nil }
        return wrap(dr_doc_removing_layer(ptr, idx))
    }

    /// Merges layer `idx` (idx >= 1) into the layer below it.
    func mergingDown(_ idx: Int) throws -> RasterDocument? {
        if isHighDepth { return try precisionMergeDown(idx) }
        guard idx >= 1, isValidIndex(idx) else { return nil }
        return wrap(dr_doc_merging_down(ptr, idx))
    }

    /// Single-layer document containing the projection.
    func flattening() throws -> RasterDocument? {
        if isHighDepth { return try precisionFlatten() }
        return wrap(dr_doc_flattening(ptr))
    }

    /// Paints a CANVAS-frame premultiplied overlay (top row first, no row
    /// padding) onto layer `idx`, mapped through the layer's offset. `w`/`h`
    /// must equal the canvas size exactly.
    func paintingLayer(
        _ idx: Int, overlay data: UnsafePointer<UInt8>, w: Int, h: Int,
        mode: DrCompositeMode, alpha: Double
    ) -> RasterDocument? {
        guard isValidIndex(idx), w == width, h == height else { return nil }
        return wrap(dr_doc_painting_layer(ptr, idx, data, UInt32(w), UInt32(h), mode, Float(alpha)))
    }

    /// `paintingLayer`, through a layer blend mode — the paint tools' Blend
    /// option. Normal delegates to `paintingLayer` with `DR_COMPOSITE_OVER`
    /// (byte-identical, refusal rules included); every OTHER mode also
    /// answers nil when no pixel would change.
    func paintingLayerBlend(
        _ idx: Int, overlay data: UnsafePointer<UInt8>, w: Int, h: Int,
        mode: DrBlendMode, alpha: Double
    ) -> RasterDocument? {
        guard isValidIndex(idx), w == width, h == height else { return nil }
        return wrap(
            dr_doc_painting_layer_blend(
                ptr, idx, data, UInt32(w), UInt32(h), mode, Float(alpha)))
    }

    /// The colour brush's Blend option: `paintingLayerBlend` with the
    /// STRAIGHT ink `rgb` (3 bytes in the document's space) and only the
    /// overlay's alpha read, as coverage, so a low-coverage stroke blends the
    /// exact colour. Normal delegates to `paintingLayer` with the
    /// overlay itself. nil unless `rgb.count == 3`, a valid `idx` and
    /// `w`/`h` equal to the canvas size, and on `paintingLayerBlend`'s
    /// refusals.
    func paintingLayerBlendColor(
        _ idx: Int, overlay data: UnsafePointer<UInt8>, w: Int, h: Int,
        rgb: [UInt8], mode: DrBlendMode, alpha: Double
    ) -> RasterDocument? {
        guard rgb.count == 3, isValidIndex(idx), w == width, h == height else { return nil }
        return rgb.withUnsafeBufferPointer { ink in
            wrap(
                dr_doc_painting_layer_blend_color(
                    ptr, idx, data, UInt32(w), UInt32(h), ink.baseAddress, mode, Float(alpha)))
        }
    }

    /// Dodge/burn layer `idx` where the stroke overlay covers it — the same
    /// canvas-sized premultiplied overlay `paintingLayer` takes; only its
    /// alpha (the stroke's coverage) is read. `range`: 0 shadows, 1 midtones,
    /// 2 highlights; `protectTones` moves each pixel's channels together
    /// (#547). nil when nothing would change.
    func dodgeBurnLayer(
        _ idx: Int, overlay data: UnsafePointer<UInt8>, w: Int, h: Int,
        exposure: Double, range: Int, burn: Bool, protectTones: Bool = false
    ) -> RasterDocument? {
        guard isValidIndex(idx), w == width, h == height, (0...2).contains(range) else {
            return nil
        }
        return wrap(
            dr_doc_dodge_burn_layer(
                ptr, idx, data, UInt32(w), UInt32(h), Float(exposure), UInt8(range), burn,
                protectTones))
    }
}
