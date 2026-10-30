import AppKit

extension RasterImage {
    // MARK: - Adjustments, statistics and auto tone

    /// The adjustment `op` with `params` applied destructively — the SAME
    /// object an adjustment layer's meta carries, run through the SAME core
    /// code the compositor runs (core/src/adjust.rs's schema table is the
    /// contract). One export covers every op, which is what makes the
    /// filter and the layer incapable of drifting.
    ///
    /// nil for an unknown op or parameters the core refuses; the core's
    /// message goes to the log only, because every caller validates against
    /// `AdjustmentSchema` first and a sheet has nowhere to put a second
    /// refusal.
    func applyingAdjustment(op: String, params: [String: Any]) -> RasterImage? {
        guard let data = try? JSONSerialization.data(
            withJSONObject: params, options: [.sortedKeys]),
              let json = String(data: data, encoding: .utf8)
        else { return nil }
        var err: UnsafeMutablePointer<CChar>? = nil
        let handle = dr_image_adjust_op(ptr, op, json, &err)
        if handle == nil, err != nil {
            NSLog("doctored: adjustment %@ refused: %@",
                  op, takeErrorMessage(err, fallback: "invalid parameters"))
        }
        return wrap(handle)
    }

    /// 1024 counts — 256 red, then green, then blue, then Rec. 709 luma —
    /// and the number of pixels counted. A pixel counts when its alpha is
    /// non-zero and, with a mask, its coverage is at least 128. `stride`
    /// counts every stride-th pixel in row-major order (0 and 1 both mean
    /// every pixel) and the total says how many were actually counted, so
    /// proportions stay comparable at any stride.
    ///
    /// Deliberately a flat array plus a count and not a struct: the typed
    /// value lives beside the panel that draws it, so this file stays free
    /// of feature types.
    func histogram(mask: [UInt8]?, stride: Int) -> (bins: [UInt32], total: UInt64)? {
        if let mask = mask, mask.count != width * height { return nil }
        var bins = [UInt32](repeating: 0, count: 1024)
        var total: UInt64 = 0
        let step = UInt32(max(1, min(stride, Int(UInt32.max))))
        let ok = bins.withUnsafeMutableBufferPointer { out -> Bool in
            guard let mask = mask else {
                return dr_image_histogram(ptr, nil, step, out.baseAddress, &total)
            }
            return mask.withUnsafeBufferPointer { coverage in
                dr_image_histogram(ptr, coverage.baseAddress, step, out.baseAddress, &total)
            }
        }
        return ok ? (bins, total) : nil
    }

    /// The straight RGBA at (x, y): the plain mean of the (2*reach+1)
    /// square about it with out-of-bounds pixels dropped (reach 0/1/2 =
    /// point, 3×3, 5×5), truncating. The CENTRE may be outside the image —
    /// nil only when NO pixel of the block is inside, which is exactly what
    /// the eyedropper needs at a canvas edge.
    func sample(x: Int, y: Int, reach: Int) -> (r: UInt8, g: UInt8, b: UInt8, a: UInt8)? {
        guard reach >= 0, reach <= 2 else { return nil }
        var rgba = [UInt8](repeating: 0, count: 4)
        let ok = rgba.withUnsafeMutableBufferPointer { out in
            dr_image_sample(
                ptr, Int32(clamping: x), Int32(clamping: y), UInt32(reach), out.baseAddress)
        }
        return ok ? (rgba[0], rgba[1], rgba[2], rgba[3]) : nil
    }

    /// Levels parameters derived from this image's own histogram — the same
    /// counting rule `histogram` uses, so a transparent surround never pins
    /// the black point at 0. `clip` is the share of counted pixels dropped
    /// at each end (0…0.1; 0.001 is the default 0.1 %). nil when the result
    /// would change nothing, so the caller registers no undo step.
    func autoLevels(mask: [UInt8]?, mode: DrAutoMode, clip: Double)
        -> (black: [Double], white: [Double], gamma: [Double])?
    {
        if let mask = mask, mask.count != width * height { return nil }
        var params = [Float](repeating: 0, count: 9)
        let ok = params.withUnsafeMutableBufferPointer { out -> Bool in
            guard let mask = mask else {
                return dr_image_auto_levels(ptr, nil, mode, Float(clip), out.baseAddress)
            }
            return mask.withUnsafeBufferPointer { coverage in
                dr_image_auto_levels(
                    ptr, coverage.baseAddress, mode, Float(clip), out.baseAddress)
            }
        }
        guard ok else { return nil }
        let values = params.map(Double.init)
        return (Array(values[0..<3]), Array(values[3..<6]), Array(values[6..<9]))
    }

    /// Levels with a black point, white point and gamma per channel — the
    /// twin `autoLevels`' nine numbers feed. nil unless every channel has
    /// 0 ≤ black < white ≤ 1 and gamma in 0.1…10.
    func levelsChannels(black: [Double], white: [Double], gamma: [Double]) -> RasterImage? {
        guard black.count == 3, white.count == 3, gamma.count == 3 else { return nil }
        let b = black.map(Float.init)
        let w = white.map(Float.init)
        let g = gamma.map(Float.init)
        return b.withUnsafeBufferPointer { bp in
            w.withUnsafeBufferPointer { wp in
                g.withUnsafeBufferPointer { gp in
                    wrap(dr_image_levels_channels(
                        ptr, bp.baseAddress, wp.baseAddress, gp.baseAddress))
                }
            }
        }
    }

    /// Multiplies each pixel's alpha by a full-frame u8 coverage mask (the
    /// selection convention: 0 hides, 255 keeps, intermediate values scale
    /// proportionally). `coverage` must be exactly width*height bytes, row 0
    /// = top — the buffer `CanvasSelection.maskBytes()` produces.
    func masked(by coverage: [UInt8]) -> RasterImage? {
        let (w, h) = (width, height)
        guard coverage.count == w * h else { return nil }
        return coverage.withUnsafeBufferPointer { buffer in
            wrap(dr_image_apply_mask(ptr, buffer.baseAddress, UInt32(w), UInt32(h)))
        }
    }

    func save(to url: URL, format: DrFormat, jpegQuality: Int) throws {
        var err: UnsafeMutablePointer<CChar>? = nil
        let quality = UInt8(min(max(jpegQuality, 1), 100))
        guard dr_image_save(ptr, url.path, format, quality, &err) else {
            throw DoctoredCoreError(
                message: takeErrorMessage(err, fallback: "Could not save \(url.lastPathComponent)."))
        }
    }

    // The CGImage reads the Rust pixel buffer in place (no copy): the data
    // provider holds a retain on this wrapper, so the buffer outlives the
    // CGImage even if the last Swift reference to the handle goes away. The
    // pixel data behind a handle never changes, so this stays coherent, and
    // deliberately nothing is cached — a strong cache would cycle through the
    // provider's retain, and undo-stack handles would pin full-size CGImages.
    //
    // `space` is the colour space to TAG the pixels with; NOTHING is
    // converted here (the buffer is read in place, and converting would mean
    // a copy per frame). Document pixels pass `doc.colorSpace`; coverage
    // planes, mask thumbnails and deliberately-sRGB exits pass
    // `ColorProfile.sRGB`. There is deliberately no default: a default is
    // exactly the trap that leaves a new call site silently sRGB.
    func makeCGImage(in space: CGColorSpace) -> CGImage? {
        let w = width
        let h = height
        guard w > 0, h > 0, let pixels = dr_image_pixels_rgba(ptr) else { return nil }
        let info = Unmanaged.passRetained(self).toOpaque()
        guard let provider = CGDataProvider(
            dataInfo: info,
            data: UnsafeRawPointer(pixels),
            size: w * h * 4,
            releaseData: { info, _, _ in
                Unmanaged<RasterImage>.fromOpaque(info!).release()
            })
        else {
            Unmanaged<RasterImage>.fromOpaque(info).release()
            return nil
        }
        return CGImage(
            width: w,
            height: h,
            bitsPerComponent: 8,
            bitsPerPixel: 32,
            bytesPerRow: w * 4,
            space: space,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
            provider: provider,
            decode: nil,
            shouldInterpolate: true,
            intent: .defaultIntent)
    }
}

extension RasterDocument {
    /// The destructive twin of a described adjustment, using this immutable
    /// document revision's ICC profile. Alpha and selection writeback retain
    /// their existing contracts; the core owns all calibrated pixel math.
    func applyingAdjustment(to image: RasterImage, op: String, params: [String: Any]) -> RasterImage? {
        guard JSONSerialization.isValidJSONObject(params),
              let data = try? JSONSerialization.data(withJSONObject: params, options: [.sortedKeys]),
              let json = String(data: data, encoding: .utf8)
        else { return nil }
        var err: UnsafeMutablePointer<CChar>? = nil
        let result = dr_doc_adjust_image(ptr, image.ptr, op, json, &err)
        if let err = err {
            NSLog("doctored: adjustment %@ refused: %@", op,
                  takeErrorMessage(err, fallback: "invalid colour context"))
        }
        return result.map { RasterImage(owning: $0) }
    }
}
