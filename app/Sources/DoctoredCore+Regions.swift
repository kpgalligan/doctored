import AppKit

extension RasterDocument {
    // MARK: - Selection regions and region painting

    /// Similar-color mask from the flattened composite: canvas-sized
    /// 0/255 bytes (row 0 top), or nil for an out-of-canvas seed or a
    /// `sampleSize` outside `SelectToolOptions.wandSampleSizes` (1 is
    /// Point Sample, else the box mean about the seed is the reference).
    func magicWand(x: Int, y: Int, tolerance: Int, contiguous: Bool, sampleSize: Int = 1) -> [UInt8]? {
        guard x >= 0, y >= 0, x < width, y < height, sampleSize > 0 else { return nil }
        var mask = [UInt8](repeating: 0, count: width * height)
        let ok = mask.withUnsafeMutableBufferPointer { buffer in
            dr_doc_magic_wand(
                ptr, UInt32(x), UInt32(y), UInt8(tolerance.clamped(0, 255)),
                contiguous, UInt32(sampleSize), buffer.baseAddress)
        }
        return ok ? mask : nil
    }

    /// The wand's "Sample: Current layer": the same region grown over
    /// layer `idx` alone on a transparent canvas. nil for an out-of-canvas
    /// seed, a bad index, an adjustment layer or an unlisted `sampleSize`.
    func magicWandLayer(_ idx: Int, x: Int, y: Int, tolerance: Int, contiguous: Bool,
                        sampleSize: Int = 1) -> [UInt8]? {
        guard x >= 0, y >= 0, x < width, y < height, idx >= 0, sampleSize > 0 else { return nil }
        var mask = [UInt8](repeating: 0, count: width * height)
        let ok = mask.withUnsafeMutableBufferPointer { buffer in
            dr_doc_magic_wand_layer(
                ptr, idx, UInt32(x), UInt32(y), UInt8(tolerance.clamped(0, 255)),
                contiguous, UInt32(sampleSize), buffer.baseAddress)
        }
        return ok ? mask : nil
    }

    /// Select ▸ Grow (`contiguous`) / Select ▸ Similar from the flattened
    /// composite: the canvas-sized 0/255 region whose colours lie in the range
    /// of every pixel `mask` covers at least half, widened by `tolerance`.
    /// Throws the core's refusal sentence; byte documents only.
    func selectSimilar(mask: [UInt8], tolerance: Int, contiguous: Bool) throws -> [UInt8] {
        guard !isHighDepth else {
            throw DoctoredCoreError(message: "Byte Grow/Similar requires an 8-bit document.")
        }
        var out = [UInt8](repeating: 0, count: width * height)
        var err: UnsafeMutablePointer<CChar>? = nil
        let ok = mask.withUnsafeBufferPointer { input in
            out.withUnsafeMutableBufferPointer { buffer in
                dr_doc_select_similar(
                    ptr, input.baseAddress, input.count, UInt8(tolerance.clamped(0, 255)),
                    contiguous, buffer.baseAddress, &err)
            }
        }
        guard ok else {
            throw DoctoredCoreError(
                message: takeErrorMessage(err, fallback: "Grow/Similar could not be computed."))
        }
        return out
    }

    /// Bucket fill on layer `idx` from canvas point (x, y). `mask` is a
    /// canvas-sized selection coverage buffer or nil.
    func bucketFilled(
        _ idx: Int, x: Int, y: Int, tolerance: Int, rgba: [UInt8], contiguous: Bool,
        mask: [UInt8]?
    ) -> RasterDocument? {
        guard isValidIndex(idx), rgba.count == 4 else { return nil }
        return withMask(mask) { maskPtr in
            rgba.withUnsafeBufferPointer { color in
                wrap(
                    dr_doc_bucket_fill(
                        ptr, idx, Int32(x), Int32(y), UInt8(tolerance.clamped(0, 255)),
                        color.baseAddress, contiguous, maskPtr))
            }
        }
    }

    /// Edit ▸ Fill… on layer `idx`: straight `rgba` through `mode` at
    /// `opacity` (0…1 — the core's unit; the Fill sheet's 0…100 is divided
    /// by its caller), weighted by `mask` (canvas-sized coverage, or nil for
    /// the whole canvas). `preserveTransparency` (or a transparency lock)
    /// keeps every pixel's alpha exactly; on a semi-transparent pixel the
    /// colour mixes in at the fill's own opacity × coverage. nil
    /// when nothing would change or on a refusal. A wrong-sized mask
    /// refuses here rather than reaching `withMask`, which would read it as
    /// "whole canvas".
    func selectionFilled(
        _ idx: Int, mask: [UInt8]?, rgba: [UInt8], mode: DrBlendMode,
        opacity: Double, preserveTransparency: Bool
    ) -> RasterDocument? {
        guard isValidIndex(idx), rgba.count == 4,
            mask == nil || mask?.count == width * height
        else { return nil }
        return withMask(mask) { maskPtr in
            rgba.withUnsafeBufferPointer { color in
                wrap(
                    dr_doc_fill_selection(
                        ptr, idx, maskPtr, UInt32(width), UInt32(height), color.baseAddress,
                        mode, Float(opacity), preserveTransparency))
            }
        }
    }

    /// Two-color gradient over layer `idx` (scaled by `mask` if given).
    func gradiented(
        _ idx: Int, from p0: CGPoint, to p1: CGPoint, start: [UInt8], end: [UInt8],
        kind: DrGradientKind, mask: [UInt8]?
    ) -> RasterDocument? {
        guard isValidIndex(idx), start.count == 4, end.count == 4 else { return nil }
        return withMask(mask) { maskPtr in
            start.withUnsafeBufferPointer { s in
                end.withUnsafeBufferPointer { e in
                    wrap(
                        dr_doc_gradient(
                            ptr, idx, Float(p0.x), Float(p0.y), Float(p1.x), Float(p1.y),
                            s.baseAddress, e.baseAddress, kind, maskPtr))
                }
            }
        }
    }

    /// A stops gradient over layer `idx` (`dr_doc_gradient_stops`):
    /// `stops` is seven floats per stop, `[position, r, g, b, a, midpoint,
    /// opacity_midpoint]`, straight and 0…1 in the document's own numbers,
    /// each midpoint 0.05…0.95, 2…32 stops in non-decreasing
    /// position; `kind` is any of the five shapes; `mode` is the blend
    /// mode each stop colour composites through, Normal being plain
    /// source-over; `smoothness` is the Smoothness as a
    /// fraction 0…1, 1 the Classic ramp. nil on a refusal or when no pixel
    /// changes.
    func gradientedStops(
        _ idx: Int, from p0: CGPoint, to p1: CGPoint, stops: [Float],
        kind: DrGradientKind, mode: DrBlendMode = DR_BLEND_NORMAL, smoothness: Double = 1,
        mask: [UInt8]?
    ) -> RasterDocument? {
        guard isValidIndex(idx), !stops.isEmpty, stops.count % 7 == 0 else { return nil }
        return withMask(mask) { maskPtr in
            stops.withUnsafeBufferPointer { values in
                wrap(
                    dr_doc_gradient_stops(
                        ptr, idx, Float(p0.x), Float(p0.y), Float(p1.x), Float(p1.y),
                        values.baseAddress, stops.count / 7, kind, mode, Float(smoothness), maskPtr))
            }
        }
    }

    /// The segment fraction `f` remapped through a stop's `midpoint`
    /// (`dr_gradient_midpoint_fraction`, over `gradient_field::midpoint_fraction`,
    /// the ONE remap): the gradient editor's strip draws through it, so no
    /// Swift copy of the formula exists. `f` is clamped to 0…1 and `midpoint`
    /// to 0.05…0.95; at 0.5 it answers `f`.
    static func gradientMidpointFraction(_ f: Double, midpoint: Double) -> Double {
        Double(dr_gradient_midpoint_fraction(Float(f), Float(midpoint)))
    }

    /// Erases the selected region of layer `idx` in PROPORTION to the
    /// selection's coverage: `mask` is a canvas-sized coverage buffer (the
    /// one fill and gradient take), so full coverage erases a pixel, partial
    /// coverage scales its alpha down — a feathered selection cuts a
    /// soft-edged hole. Everything else about the layer survives.
    func clearingSelection(_ idx: Int, mask: [UInt8]) -> RasterDocument? {
        guard isValidIndex(idx), mask.count == width * height else { return nil }
        return mask.withUnsafeBufferPointer { buffer in
            wrap(
                dr_doc_clear_selection(
                    ptr, idx, buffer.baseAddress, UInt32(width), UInt32(height)))
        }
    }

    /// Writes the canvas rect (`x`, `y`, `width`, `height`) of layer `idx`
    /// with REPLACE semantics (`dr_doc_put_pixels`): each sample whose `mask`
    /// byte is nonzero (every sample when `mask` is nil) takes its four
    /// straight RGBA8 bytes from `rgba` verbatim. Samples off the layer are
    /// ignored; the layer never grows. Nil when nothing would change, the
    /// rect leaves the canvas, or the layer is locked or not a raster.
    func puttingPixels(
        _ idx: Int, x: Int, y: Int, width: Int, height: Int,
        rgba: [UInt8], mask: [UInt8]?
    ) -> RasterDocument? {
        guard isValidIndex(idx), width > 0, height > 0, x >= 0, y >= 0,
            rgba.count == width * height * 4
        else { return nil }
        if let mask, mask.count != width * height { return nil }
        return rgba.withUnsafeBufferPointer { pixels in
            let call = { (maskPointer: UnsafePointer<UInt8>?) in
                self.wrap(
                    dr_doc_put_pixels(
                        self.ptr, idx, UInt32(x), UInt32(y), UInt32(width), UInt32(height),
                        pixels.baseAddress, maskPointer))
            }
            guard let mask else { return call(nil) }
            return mask.withUnsafeBufferPointer { call($0.baseAddress) }
        }
    }

    func withMask<T>(_ mask: [UInt8]?, _ body: (UnsafePointer<UInt8>?) -> T) -> T {
        guard let mask = mask, mask.count == width * height else { return body(nil) }
        return mask.withUnsafeBufferPointer { body($0.baseAddress) }
    }
}
