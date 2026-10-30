import AppKit

extension RasterDocument {
    // MARK: - Channels and planes

    /// Value snapshot of one alpha channel (see the header's "Channels"
    /// section): a named canvas-sized coverage plane plus the rubylith a host
    /// draws it with. The plane itself comes from `channelPlane` or
    /// `channelImage`.
    struct ChannelInfo {
        /// The core's stable per-channel identity: unique while the app runs,
        /// kept through a rename, a plane edit, the geometry ops and
        /// undo/redo, fresh for a duplicate, and gone when the channel is.
        /// It is how view state (which channel's eye is on) stays attached to
        /// a channel — names are not unique and indices renumber on every
        /// insert, delete and undo.
        let id: UInt64
        let name: String
        let red: UInt8
        let green: UInt8
        let blue: UInt8
        let opacity: Double
        /// false = the rubylith covers the MASKED areas (the default, and
        /// what the Quick Mask overlay already draws).
        let colorIndicatesSelected: Bool
        /// The ink of a SPOT channel (#1), nil for an alpha channel.
        var spot: SpotInk? = nil
    }

    var channelCount: Int { precisionMetadata?.channels.count ?? Int(dr_doc_channel_count(ptr)) }

    /// The largest channel count a `w` x `h` canvas may carry: the core's own
    /// budget (256 channels, and 900 M channel pixels in total — nine full
    /// canvases), asked as a number so a refusal can name it. Image Size,
    /// Canvas Size and every channel-creating command refuse on it, and a beep
    /// alone leaves the user no way to learn why.
    /// A size past what a canvas can be answers 0 — nothing fits a canvas
    /// that could not exist in the first place.
    static func maxChannels(width w: Int, height h: Int) -> Int {
        guard w >= 0, h >= 0, w <= Int(UInt32.max), h <= Int(UInt32.max) else { return 0 }
        return Int(dr_max_channels_at(UInt32(w), UInt32(h)))
    }

    func isValidChannel(_ i: Int) -> Bool {
        i >= 0 && i < channelCount
    }

    /// Channel `i`'s stable identity alone (see `ChannelInfo.id`), 0 for an
    /// out-of-range index. The cheap half of `channelInfo` — no name string
    /// crosses the boundary — for the view state that only needs identity.
    func channelID(_ i: Int) -> UInt64 {
        if let precisionMetadata { return precisionMetadata.channel(i)?.id ?? 0 }
        guard isValidChannel(i) else { return 0 }
        return dr_doc_channel_id(ptr, i)
    }

    func channelInfo(_ i: Int) -> ChannelInfo? {
        if let precisionMetadata { return precisionMetadata.channel(i) }
        guard isValidChannel(i) else { return nil }
        var name = ""
        if let cName = dr_doc_channel_name(ptr, i) {
            name = String(cString: cName)
            dr_string_free(cName)
        }
        var rgb = [UInt8](repeating: 0, count: 3)
        let ok = rgb.withUnsafeMutableBufferPointer { buffer in
            dr_doc_channel_overlay_color(ptr, i, buffer.baseAddress)
        }
        guard ok else { return nil }
        return ChannelInfo(
            id: dr_doc_channel_id(ptr, i),
            name: name, red: rgb[0], green: rgb[1], blue: rgb[2],
            opacity: Double(dr_doc_channel_overlay_opacity(ptr, i)),
            colorIndicatesSelected: dr_doc_channel_color_indicates_selected(ptr, i),
            spot: byteChannelSpot(i))
    }

    /// A canvas-sized plane read STRAIGHT into a Swift buffer — never through
    /// an image handle, which would cost five times the memory for bytes that
    /// are u8 coverage to begin with.
    private func readCanvasPlane(_ read: (UnsafeMutablePointer<UInt8>?) -> Bool) -> [UInt8]? {
        guard !isHighDepth, width > 0, height > 0 else { return nil }
        var plane = [UInt8](repeating: 0, count: width * height)
        let ok = plane.withUnsafeMutableBufferPointer { read($0.baseAddress) }
        return ok ? plane : nil
    }

    /// Channel `i`'s coverage bytes (canvas-sized, row 0 = top — the
    /// selection convention).
    func channelPlane(_ i: Int) -> [UInt8]? {
        guard isValidChannel(i) else { return nil }
        return readCanvasPlane { out in
            dr_doc_channel_plane(ptr, i, out, UInt32(width), UInt32(height))
        }
    }

    /// One plane of the FLATTENED composite, canvas-sized. A ONE-SHOT read:
    /// it runs the whole projection, so a display path reads the cached
    /// projection with `RasterImage.plane` instead. nil for `DR_PLANE_MASK`.
    func compositePlane(_ plane: DrPlane) -> [UInt8]? {
        readCanvasPlane { out in
            dr_doc_composite_plane(ptr, plane, out, UInt32(width), UInt32(height))
        }
    }

    /// The flattened composite's grey in a Gray working space, canvas-sized (Load Selection's
    /// luminosity at 8 bits, `ColorSettings.workingGray`). nil when the profile has no Y to read.
    func compositeGrayPlane(_ space: GrayWorkingSpace) -> [UInt8]? {
        readCanvasPlane { out in
            dr_doc_composite_gray_plane(ptr, space.rawValue, out, UInt32(width), UInt32(height))
        }
    }

    /// One plane of layer `idx`, CANVAS-sized: pixels outside the layer's
    /// rect read 0, `DR_PLANE_MASK` included. nil when the layer has no mask
    /// and `DR_PLANE_MASK` was asked for.
    func layerPlane(_ idx: Int, _ plane: DrPlane) -> [UInt8]? {
        guard isValidIndex(idx) else { return nil }
        return readCanvasPlane { out in
            dr_doc_layer_plane(ptr, idx, plane, out, UInt32(width), UInt32(height))
        }
    }

    /// One-shot plane image (it re-flattens). For a DISPLAY path use
    /// `RasterImage.planeImage` on the cached projection instead.
    /// `maxSide` 0 means full size.
    func compositePlaneImage(_ plane: DrPlane, maxSide: Int) -> RasterImage? {
        guard maxSide >= 0 else { return nil }
        return wrapImage(dr_doc_composite_plane_image(ptr, plane, UInt32(maxSide)))
    }

    /// One CANVAS-sized plane of layer `idx` as an opaque grayscale image.
    func layerPlaneImage(_ idx: Int, _ plane: DrPlane, maxSide: Int) -> RasterImage? {
        if isHighDepth {
            guard let named = RasterPlane.allCases.first(where: { $0.dr == plane }) else { return nil }
            return named == .mask ? (try? nativePlaneDisplay(target: "mask", index: idx, maxSide: maxSide)) : (try? nativeSourcePlaneDisplay(named, layer: idx, maxSide: maxSide))
        }
        guard isValidIndex(idx), maxSide >= 0 else { return nil }
        return wrapImage(dr_doc_layer_plane_image(ptr, idx, plane, UInt32(maxSide)))
    }

    /// Channel `i` as an opaque grayscale image — the panel's thumbnail
    /// source (the core does the downsampling; `maxSide` 0 means full size).
    func channelImage(_ i: Int, maxSide: Int) -> RasterImage? {
        if isHighDepth { return try? nativePlaneDisplay(target: "channel", index: i, maxSide: maxSide) }
        guard isValidChannel(i), maxSide >= 0 else { return nil }
        return wrapImage(dr_doc_channel_image(ptr, i, UInt32(maxSide)))
    }

    /// Appends a channel from `plane` (`width * height` coverage bytes). A
    /// plane that is not canvas-sized is resampled to the canvas bilinearly —
    /// the iPhone auxiliary-matte path. nil when the list is full or the nine
    /// would not fit the total pixel budget.
    func addingChannel(
        name: String, plane: [UInt8], width w: Int, height h: Int,
        red: UInt8 = 255, green: UInt8 = 0, blue: UInt8 = 0, opacity: Double = 0.5
    ) throws -> RasterDocument? {
        guard w > 0, h > 0, plane.count == w * h else { return nil }
        if let precision {
            // The overlay step refuses on the main thread while a job owns the
            // worker, so refuse before computing the import rather than after.
            if Thread.isMainThread, BackgroundWork.occupied {
                throw DoctoredCoreError(message: BackgroundWork.preparingBusyMessage)
            }
            let command = try PrecisionPixelCommand(["op": "import_channel", "name": name, "width": w, "height": h,
                "provenance": NSNull()])
            let work = try PrecisionScalarWork(source: precision, command: command, samples: plane.map { UInt16($0) * 257 })
            guard let result = try work.compute() else { return nil }
            let facade = try RasterDocument(precision: result)
            // The pixel-edit path, not the main-only bookkeeping one: an
            // opener's worker thread (AuxiliaryMattes) runs it directly.
            return try facade.performingPrecisionPixelEdit(.init(["op": "channel_properties",
                "index": facade.channelCount - 1, "overlay_color": [red, green, blue],
                "overlay_opacity": opacity, "color_indicates_selected": false])) ?? facade
        }
        return plane.withUnsafeBufferPointer { buffer in
            wrap(
                dr_doc_add_channel(
                    ptr, name, buffer.baseAddress, UInt32(w), UInt32(h),
                    red, green, blue, Float(opacity)))
        }
    }

    func removingChannel(_ i: Int) throws -> RasterDocument? {
        if isHighDepth { return try performingPrecisionPixelEdit(.removeChannel(index: i)) }
        guard isValidChannel(i) else { return nil }
        return wrap(dr_doc_remove_channel(ptr, i))
    }

    /// nil for a name the channel already has (renaming to itself is not an
    /// edit).
    func renamingChannel(_ i: Int, _ name: String) throws -> RasterDocument? {
        if isHighDepth { return try applyingPrecisionBookkeepingEdit(.init(["op": "rename_channel", "index": i, "name": name])) }
        guard isValidChannel(i) else { return nil }
        return wrap(dr_doc_rename_channel(ptr, i, name))
    }

    /// All the display options at once, so the options sheet is one undo
    /// step. nil when none of them would change.
    func settingChannelOverlay(
        _ i: Int, red: UInt8, green: UInt8, blue: UInt8, opacity: Double,
        indicatesSelected: Bool
    ) throws -> RasterDocument? {
        if isHighDepth { return try applyingPrecisionBookkeepingEdit(.init(["op": "channel_properties", "index": i,
            "overlay_color": [red, green, blue], "overlay_opacity": opacity, "color_indicates_selected": indicatesSelected])) }
        guard isValidChannel(i) else { return nil }
        return wrap(
            dr_doc_set_channel_overlay(
                ptr, i, red, green, blue, Float(opacity), indicatesSelected))
    }

    /// Replaces channel `i`'s coverage with a CANVAS-sized plane; nil when
    /// the bytes are what the channel already holds.
    func settingChannelData(_ i: Int, _ plane: [UInt8]) -> RasterDocument? {
        guard isValidChannel(i), plane.count == width * height else { return nil }
        return plane.withUnsafeBufferPointer { buffer in
            wrap(
                dr_doc_set_channel_data(
                    ptr, i, buffer.baseAddress, UInt32(width), UInt32(height)))
        }
    }

    func duplicatingChannel(_ i: Int) throws -> RasterDocument? {
        if isHighDepth { return try performingPrecisionPixelEdit(.init(["op": "duplicate_channel", "index": i])) }
        guard isValidChannel(i) else { return nil }
        return wrap(dr_doc_duplicate_channel(ptr, i))
    }

    func invertingChannel(_ i: Int) throws -> RasterDocument? {
        if isHighDepth { return try performingPrecisionPixelEdit(.init(["op": "scalar_edit", "target": "channel", "index": i,
            "operation": ["kind": "morphology", "operation": ["kind": "invert"]]])) }
        guard isValidChannel(i) else { return nil }
        return wrap(dr_doc_invert_channel(ptr, i))
    }

    /// Appends the nine luminosity masks ("Lights 1".."Midtones 3") built
    /// from the composite's Rec. 709 luma; nil when they would not fit.
    func addingLuminosityMasks() -> RasterDocument? {
        wrap(dr_doc_add_luminosity_masks(ptr))
    }

    /// Replaces ONLY `plane` of layer `idx`'s pixels from a CANVAS-sized
    /// buffer, inside the layer's rect. nil for `DR_PLANE_LUMA`/
    /// `DR_PLANE_MASK` and — importantly — when NO BYTE WOULD CHANGE, so a
    /// caller writing red, green and blue in turn must fall through per
    /// plane rather than chain the optionals.
    func withLayerPlane(_ idx: Int, _ plane: DrPlane, _ src: [UInt8]) -> RasterDocument? {
        guard isValidIndex(idx), src.count == width * height else { return nil }
        return src.withUnsafeBufferPointer { buffer in
            wrap(
                dr_doc_with_layer_plane(
                    ptr, idx, plane, buffer.baseAddress, UInt32(width), UInt32(height)))
        }
    }

    /// Paints channel `i` with a CANVAS-frame premultiplied overlay — the
    /// very buffer `paintingLayer` takes: white paints toward 255, black
    /// toward 0. nil when no byte would change.
    func paintingChannel(
        _ i: Int, overlay data: UnsafePointer<UInt8>, w: Int, h: Int
    ) -> RasterDocument? {
        guard isValidChannel(i), w == width, h == height else { return nil }
        return wrap(dr_doc_painting_channel(ptr, i, data, UInt32(w), UInt32(h)))
    }

    /// The same coverage paint into ONE colour plane of layer `idx`. nil for
    /// `DR_PLANE_LUMA`/`DR_PLANE_MASK` or when no byte would change.
    func paintingLayerPlane(
        _ idx: Int, _ plane: DrPlane, overlay data: UnsafePointer<UInt8>, w: Int, h: Int
    ) -> RasterDocument? {
        guard isValidIndex(idx), w == width, h == height else { return nil }
        return wrap(dr_doc_painting_layer_plane(ptr, idx, plane, data, UInt32(w), UInt32(h)))
    }
}
