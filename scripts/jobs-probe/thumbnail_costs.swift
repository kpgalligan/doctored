import AppKit

/// The Layers panel's thumbnail costs. A 6000 x 4000 layer's cache miss
/// is fitted in the core before any whole-layer copy (8 and 16 bits) and
/// beats the old path it replaces; mask thumbnails are cached on the mask
/// plane's identity; float thumbnails are cached under a tone-map/profile
/// key. A 16-bit mask miss and a float layer miss are fitted in the core
/// too, and each is measured against the path it replaced; so are an 8-bit
/// mask miss and the 16-bit Channels panel's mask and alpha rows. The cache
/// is driven directly (`LayerThumbnailCache`, the type the Layers panel
/// owns), so the counts are the cache's own.

private func thumbnailCall(_ tool: String, _ document: ImageDocument,
                           _ arguments: [String: Any] = [:]) throws {
    let (response, raw) = try transformResponse(tool, document: document, arguments: arguments)
    try require(response["isError"] as? Bool != true, "\(tool) refused: \(raw)")
}

private func plate(_ image: RasterImage) -> [UInt8] {
    image.withPlateBytes {
        Array(UnsafeBufferPointer(start: $0, count: image.width * image.height * 4))
    } ?? []
}

/// The largest difference on alpha and on each premultiplied channel, in codes.
private func premultipliedDrift(_ a: [UInt8], _ b: [UInt8]) -> Int {
    guard a.count == b.count else { return Int.max }
    func pm(_ p: [UInt8], _ i: Int, _ c: Int) -> Int {
        (Int(p[i + c]) * Int(p[i + 3]) + 127) / 255
    }
    var drift = 0
    for i in stride(from: 0, to: a.count, by: 4) {
        drift = max(drift, abs(Int(a[i + 3]) - Int(b[i + 3])))
        for c in 0..<3 { drift = max(drift, abs(pm(a, i, c) - pm(b, i, c))) }
    }
    return drift
}

private func milliseconds(_ body: () throws -> Void) rethrows -> Double {
    let start = DispatchTime.now().uptimeNanoseconds
    try body()
    return Double(DispatchTime.now().uptimeNanoseconds - start) / 1_000_000
}

private func median(_ values: [Double]) -> Double {
    values.sorted()[values.count / 2]
}

private func layerID(_ doc: RasterDocument, _ index: Int) throws -> UInt64 {
    guard let id = doc.layerInfo(index)?.id, id != 0 else {
        throw ProbeFailure(description: "Layer \(index) has no id")
    }
    return id
}

/// One depth's 6000 x 4000 miss: the old path (median of 3) against the
/// cache's miss (median of 3 fresh caches), then a hit.
private func verifyLayerMiss(depth: Int) throws {
    let document = try depth == 8 ? fixture(width: 600, height: 400)
        : precisionFixture(width: 600, height: 400)
    defer { document.close() }
    _ = try editor(document)
    try thumbnailCall("image_size", document, ["width": 6000, "height": 4000])
    guard let doc = document.doc, doc.width == 6000, doc.height == 4000 else {
        throw ProbeFailure(description: "[\(depth)] image_size did not land at 6000 x 4000")
    }
    let id = try layerID(doc, 0)
    let side = 34

    var before: [Double] = []
    var old: RasterImage?
    for _ in 0..<3 {
        before.append(try milliseconds {
            if depth == 8 {
                old = doc.layerImage(0)?.resized(w: 34, h: 23, filter: DR_FILTER_BILINEAR)
            } else {
                guard let source = doc.precision?.layerSource(0) else {
                    throw ProbeFailure(description: "[16] no layer source")
                }
                let context = try PrecisionWorkContext(image: source)
                let preview = try source.previewRGBA8(workingLimit: context.limit,
                                                      otherRetained: context.otherRetained)
                old = RasterDocument.fitted(preview, maxSide: side)
            }
        })
    }
    var misses: [Double] = []
    var fresh: RasterImage?
    var cache = LayerThumbnailCache()
    for _ in 0..<3 {
        cache = LayerThumbnailCache()
        misses.append(milliseconds {
            fresh = cache.thumbnail(in: doc, index: 0, layerID: id, maxSide: side)
        })
        try require(cache.misses == 1 && cache.hits == 0, "[\(depth)] a fresh cache missed once")
    }
    var hit: RasterImage?
    let hitMs = milliseconds { hit = cache.thumbnail(in: doc, index: 0, layerID: id, maxSide: side) }
    try require(cache.hits == 1 && cache.misses == 1, "[\(depth)] the second lookup hit: "
                + "\(cache.hits) hits, \(cache.misses) misses")
    guard let old, let fresh, let hit else {
        throw ProbeFailure(description: "[\(depth)] a thumbnail did not render")
    }
    try require(hit === fresh, "[\(depth)] the hit is the stored thumbnail")
    try require(old.width == fresh.width && old.height == fresh.height,
                "[\(depth)] equal dims: old \(old.width)x\(old.height), "
                    + "new \(fresh.width)x\(fresh.height)")
    let drift = premultipliedDrift(plate(old), plate(fresh))
    let beforeMs = median(before), missMs = median(misses)
    print(String(format: "INFO: layer thumbnail costs [%d-bit 6000x4000 -> %dx%d] before_ms %.1f "
                    + "(runs %@) miss_ms %.1f (runs %@) hit_ms %.3f drift %d",
                 depth, fresh.width, fresh.height, beforeMs,
                 before.map { String(format: "%.1f", $0) }.joined(separator: "/"), missMs,
                 misses.map { String(format: "%.1f", $0) }.joined(separator: "/"), hitMs, drift))
    try require(drift <= 2, "[\(depth)] within 2 codes of the old path: \(drift)")
    try require(missMs < beforeMs, "[\(depth)] the miss beats the old path: "
                + "\(missMs) ms vs \(beforeMs) ms")

    try verifyMaskCache(document, label: "\(depth)-bit 6000x4000", compareOldPath: depth == 8)
}

/// The 8-bit mask thumbnail's former Swift path: the whole mask image,
/// then the fit `layerMaskThumbnail` used (scale `min(maxSide / longest, 1)`,
/// rounded, at least 1, `resized(BILINEAR)` when it shrinks).
private func formerByteMaskThumbnail(_ doc: RasterDocument, maxSide: Int) -> RasterImage? {
    guard let mask = doc.layerMaskImage(0), mask.width > 0, mask.height > 0 else { return nil }
    let longest = max(mask.width, mask.height)
    let scale = min(CGFloat(maxSide) / CGFloat(longest), 1)
    let w = max(Int((CGFloat(mask.width) * scale).rounded()), 1)
    let h = max(Int((CGFloat(mask.height) * scale).rounded()), 1)
    return (w == mask.width && h == mask.height)
        ? mask : (mask.resized(w: w, h: h, filter: DR_FILTER_BILINEAR) ?? mask)
}

/// A mask miss against the path it replaced: `before_ms` is the median of
/// three runs of the former path — at 16 bits
/// `nativePlaneDisplay(target: "mask", canvas: false)` + `RasterDocument.fitted`,
/// at 8 bits `formerByteMaskThumbnail` — and `miss_ms` the median of
/// three fresh-cache misses.
private func compareMaskMiss(_ doc: RasterDocument, id: UInt64, label: String) throws {
    var before: [Double] = []
    var old: RasterImage?
    for _ in 0..<3 {
        before.append(try milliseconds {
            if doc.isHighDepth {
                guard let display = try? doc.nativePlaneDisplay(target: "mask", index: 0,
                                                                canvas: false) else {
                    throw ProbeFailure(description: "[\(label)] the old mask display did not render")
                }
                old = RasterDocument.fitted(display, maxSide: 28)
            } else {
                old = formerByteMaskThumbnail(doc, maxSide: 28)
            }
        })
    }
    var misses: [Double] = []
    var fresh: RasterImage?
    for _ in 0..<3 {
        var cache = LayerThumbnailCache()
        misses.append(milliseconds {
            fresh = cache.maskThumbnail(in: doc, index: 0, layerID: id, maxSide: 28)
        })
        try require(cache.maskMisses == 1 && cache.maskHits == 0, "[\(label)] a fresh cache missed once")
    }
    guard let old, let fresh else {
        throw ProbeFailure(description: "[\(label)] a mask thumbnail did not render")
    }
    try require(old.width == fresh.width && old.height == fresh.height,
                "[\(label)] equal mask dims: old \(old.width)x\(old.height), "
                    + "new \(fresh.width)x\(fresh.height)")
    let drift = premultipliedDrift(plate(old), plate(fresh))
    let beforeMs = median(before), missMs = median(misses)
    print(String(format: "INFO: layer thumbnail costs [%@ mask -> %dx%d] before_ms %.1f (runs %@) "
                    + "miss_ms %.1f (runs %@) drift %d",
                 label, fresh.width, fresh.height, beforeMs,
                 before.map { String(format: "%.1f", $0) }.joined(separator: "/"), missMs,
                 misses.map { String(format: "%.1f", $0) }.joined(separator: "/"), drift))
    try require(drift <= 2, "[\(label)] the mask within 2 codes of the old path: \(drift)")
    try require(missMs < beforeMs, "[\(label)] the mask miss beats the old path: "
                + "\(missMs) ms vs \(beforeMs) ms")
}

/// Mask thumbnails: a miss, then a hit; a rename hits; a removal and a new
/// hide_all mask miss once. With `compareOldPath`, the miss is first measured
/// against the path it replaced (`compareMaskMiss`).
private func verifyMaskCache(_ document: ImageDocument, label: String,
                             compareOldPath: Bool = false) throws {
    try thumbnailCall("add_layer_mask", document, ["layer": 0, "kind": "reveal_all"])
    guard let masked = document.doc else { throw ProbeFailure(description: "[\(label)] no doc") }
    let id = try layerID(masked, 0)
    if compareOldPath { try compareMaskMiss(masked, id: id, label: label) }
    var cache = LayerThumbnailCache()
    var first: RasterImage?
    let missMs = milliseconds {
        first = cache.maskThumbnail(in: masked, index: 0, layerID: id, maxSide: 28)
    }
    try require(first != nil && cache.maskMisses == 1 && cache.maskHits == 0,
                "[\(label)] the first mask lookup missed")
    let hitMs = milliseconds {
        _ = cache.maskThumbnail(in: masked, index: 0, layerID: id, maxSide: 28)
    }
    try require(cache.maskHits == 1 && cache.maskMisses == 1, "[\(label)] the second mask lookup hit")
    try thumbnailCall("set_layer_properties", document, ["index": 0, "name": "Renamed mask"])
    guard let renamed = document.doc, renamed !== masked else {
        throw ProbeFailure(description: "[\(label)] the rename did not land")
    }
    _ = cache.maskThumbnail(in: renamed, index: 0, layerID: id, maxSide: 28)
    try require(cache.maskHits == 2 && cache.maskMisses == 1, "[\(label)] a rename hit")
    try thumbnailCall("remove_layer_mask", document, ["layer": 0])
    try thumbnailCall("add_layer_mask", document, ["layer": 0, "kind": "hide_all"])
    guard let replaced = document.doc else { throw ProbeFailure(description: "[\(label)] no doc") }
    _ = cache.maskThumbnail(in: replaced, index: 0, layerID: id, maxSide: 28)
    _ = cache.maskThumbnail(in: replaced, index: 0, layerID: id, maxSide: 28)
    try require(cache.maskMisses == 2 && cache.maskHits == 3,
                "[\(label)] a replaced mask missed once: \(cache.maskHits) hits, "
                    + "\(cache.maskMisses) misses")
    try require(cache.hits == 0 && cache.misses == 0, "[\(label)] layer counters untouched")
    print(String(format: "INFO: layer thumbnail costs [%@ mask] miss_ms %.1f hit_ms %.3f",
                 label, missMs, hitMs))
}

/// Float thumbnails: a miss, then a hit; a rename hits; a tone-map change misses.
private func verifyFloatCache() throws {
    let float = try floatFixture()
    defer { float.close() }
    _ = try editor(float)
    guard let doc = float.doc else { throw ProbeFailure(description: "[float] no doc") }
    let id = try layerID(doc, 0)
    var cache = LayerThumbnailCache()
    var first: RasterImage?
    let missMs = milliseconds {
        first = cache.thumbnail(in: doc, index: 0, layerID: id, maxSide: 34)
    }
    try require(first != nil && cache.misses == 1 && cache.hits == 0, "[float] the first lookup missed")
    let hitMs = milliseconds { _ = cache.thumbnail(in: doc, index: 0, layerID: id, maxSide: 34) }
    try require(cache.hits == 1 && cache.misses == 1, "[float] the second lookup hit")
    try thumbnailCall("set_layer_properties", float, ["index": 0, "name": "Renamed float"])
    guard let renamed = float.doc, renamed !== doc else {
        throw ProbeFailure(description: "[float] the rename did not land")
    }
    _ = cache.thumbnail(in: renamed, index: 0, layerID: id, maxSide: 34)
    try require(cache.hits == 2 && cache.misses == 1, "[float] a rename hit")
    let mapBefore = renamed.toneMap
    try thumbnailCall("set_tone_map", float, ["operator": "reinhard"])
    guard let mapped = float.doc, mapped.toneMap != mapBefore else {
        throw ProbeFailure(description: "[float] the tone map did not change: "
                               + "\(String(describing: mapBefore))")
    }
    try require(mapped.layerStorageIdentity(0) == renamed.layerStorageIdentity(0),
                "[float] a tone map keeps the layer's storage")
    _ = cache.thumbnail(in: mapped, index: 0, layerID: id, maxSide: 34)
    try require(cache.hits == 2 && cache.misses == 2, "[float] a tone-map change missed: "
                + "\(cache.hits) hits, \(cache.misses) misses")
    print(String(format: "INFO: layer thumbnail costs [float %dx%d] miss_ms %.1f hit_ms %.3f",
                 doc.width, doc.height, missMs, hitMs))
}

/// One 2400 x 1600 float layer's miss: the old path (`displayRGBA8`
/// + `RasterDocument.fitted`, median of 3) against the cache's miss (median
/// of 3 fresh caches).
private func verifyFloatMiss() throws {
    let (width, height) = (2400, 1600)
    let url = try writeFloatTIFF(width: width, height: height)
    let float = try floatFixture(url: url)
    defer { float.close() }
    _ = try editor(float)
    guard let doc = float.doc, let precision = doc.precision else {
        throw ProbeFailure(description: "[float miss] no float document")
    }
    let id = try layerID(doc, 0)
    var before: [Double] = []
    var old: RasterImage?
    for _ in 0..<3 {
        before.append(try milliseconds {
            guard let source = precision.layerSource(0) else {
                throw ProbeFailure(description: "[float miss] no layer source")
            }
            let context = try PrecisionWorkContext(image: source)
            let display = try precision.displayRGBA8(of: source, workingLimit: context.limit,
                                                     otherRetained: context.otherRetained)
            old = RasterDocument.fitted(display, maxSide: 34)
        })
    }
    var misses: [Double] = []
    var fresh: RasterImage?
    for _ in 0..<3 {
        var cache = LayerThumbnailCache()
        misses.append(milliseconds {
            fresh = cache.thumbnail(in: doc, index: 0, layerID: id, maxSide: 34)
        })
        try require(cache.misses == 1 && cache.hits == 0, "[float miss] a fresh cache missed once")
    }
    guard let old, let fresh else {
        throw ProbeFailure(description: "[float miss] a thumbnail did not render")
    }
    try require(old.width == fresh.width && old.height == fresh.height,
                "[float miss] equal dims: old \(old.width)x\(old.height), "
                    + "new \(fresh.width)x\(fresh.height)")
    let drift = premultipliedDrift(plate(old), plate(fresh))
    let beforeMs = median(before), missMs = median(misses)
    print(String(format: "INFO: layer thumbnail costs [float %dx%d -> %dx%d] before_ms %.1f "
                    + "(runs %@) miss_ms %.1f (runs %@) drift %d",
                 width, height, fresh.width, fresh.height, beforeMs,
                 before.map { String(format: "%.1f", $0) }.joined(separator: "/"), missMs,
                 misses.map { String(format: "%.1f", $0) }.joined(separator: "/"), drift))
    // Timing is reported, not required: both paths tone-map every sample
    // through the same per-pixel encode, so the difference between them is
    // only the whole-layer copy the miss saves.
    try require(drift <= 2, "[float miss] within 2 codes of the old path: \(drift)")
}

/// An uncompressed little-endian RGBA 32-bit float TIFF in `directory`:
/// r = 2x/w (above white on the right), g = y/h, b a hard edge at 37% of the
/// width (0.05 to 1.5), a = 1 (unassociated alpha).
private func writeFloatTIFF(width: Int, height: Int) throws -> URL {
    var data = Data()
    func u16(_ v: Int) { var x = UInt16(v).littleEndian; data.append(Data(bytes: &x, count: 2)) }
    func u32(_ v: Int) { var x = UInt32(v).littleEndian; data.append(Data(bytes: &x, count: 4)) }
    let entries = 12
    let ifdEnd = 8 + 2 + entries * 12 + 4
    let bitsOffset = ifdEnd, formatOffset = ifdEnd + 8, pixelOffset = ifdEnd + 16
    let byteCount = width * height * 16
    data.append(contentsOf: [0x49, 0x49, 0x2A, 0x00])
    u32(8)
    u16(entries)
    // (tag, type 3 SHORT / 4 LONG, count, value or offset), ascending tags.
    let fields: [(Int, Int, Int, Int)] = [
        (256, 4, 1, width), (257, 4, 1, height), (258, 3, 4, bitsOffset), (259, 3, 1, 1),
        (262, 3, 1, 2), (273, 4, 1, pixelOffset), (277, 3, 1, 4), (278, 4, 1, height),
        (279, 4, 1, byteCount), (284, 3, 1, 1), (338, 3, 1, 2), (339, 3, 4, formatOffset),
    ]
    for (tag, type, count, value) in fields {
        u16(tag); u16(type); u32(count)
        if type == 3 && count == 1 { u16(value); u16(0) } else { u32(value) }
    }
    u32(0)
    for _ in 0..<4 { u16(32) }
    for _ in 0..<4 { u16(3) }
    // Native f32 is little-endian on arm64, the only target.
    var pixels = [Float](repeating: 1, count: width * height * 4)
    for y in 0..<height {
        for x in 0..<width {
            let i = (y * width + x) * 4
            pixels[i] = 2 * Float(x) / Float(width)
            pixels[i + 1] = Float(y) / Float(height)
            pixels[i + 2] = Double(x) > 0.37 * Double(width) ? 1.5 : 0.05
        }
    }
    pixels.withUnsafeBytes { data.append(contentsOf: $0) }
    try require(data.count == pixelOffset + byteCount, "the float TIFF is \(data.count) bytes")
    let url = directory.appendingPathComponent("thumbnail-float-\(width)x\(height).tiff")
    try data.write(to: url)
    return url
}

/// The 16-bit Channels panel's mask and alpha rows against the path they
/// replaced: `before_ms` is the median of three runs of
/// `nativePlaneDisplay(target:index:maxSide: 34)` (the whole canvas plane read
/// and converted in Swift), `miss_ms` the median of three
/// `planeImage(for:composite: nil, activeLayer: 0, maxSide: 34)` calls, which
/// fit in the core. Equal dimensions, at most 2 codes and `miss_ms <
/// before_ms` are required.
private func verifyChannelPlaneThumbnails(_ document: ImageDocument) throws {
    _ = try select(document, side: 200)
    try thumbnailCall("save_selection", document, ["name": "Probe alpha"])
    guard let doc = document.doc, doc.isHighDepth, doc.channelCount >= 1 else {
        throw ProbeFailure(description: "[channels] save_selection left no 16-bit alpha channel")
    }
    try require(doc.layerHasMask(0), "[channels] layer 0 keeps its mask")
    for (label, row, target) in [("mask", ChannelRow.layerMask, "mask"),
                                 ("alpha 0", ChannelRow.alpha(0), "channel")] {
        var before: [Double] = []
        var old: RasterImage?
        for _ in 0..<3 {
            before.append(milliseconds {
                old = try? doc.nativePlaneDisplay(target: target, index: 0, maxSide: 34)
            })
        }
        var misses: [Double] = []
        var fresh: RasterImage?
        for _ in 0..<3 {
            misses.append(milliseconds {
                fresh = doc.planeImage(for: row, composite: nil, activeLayer: 0, maxSide: 34)
            })
        }
        guard let old, let fresh else {
            throw ProbeFailure(description: "[channels \(label)] a row thumbnail did not render")
        }
        try require(old.width == fresh.width && old.height == fresh.height,
                    "[channels \(label)] equal dims: old \(old.width)x\(old.height), "
                        + "new \(fresh.width)x\(fresh.height)")
        let drift = premultipliedDrift(plate(old), plate(fresh))
        let beforeMs = median(before), missMs = median(misses)
        print(String(format: "INFO: layer thumbnail costs [16-bit 600x400 channels %@ -> %dx%d] "
                        + "before_ms %.1f (runs %@) miss_ms %.1f (runs %@) drift %d",
                     label, fresh.width, fresh.height, beforeMs,
                     before.map { String(format: "%.1f", $0) }.joined(separator: "/"), missMs,
                     misses.map { String(format: "%.1f", $0) }.joined(separator: "/"), drift))
        try require(drift <= 2, "[channels \(label)] within 2 codes of the old path: \(drift)")
        try require(missMs < beforeMs, "[channels \(label)] the row beats the old path: "
                    + "\(missMs) ms vs \(beforeMs) ms")
    }
}

func verifyLayerThumbnailCosts() throws {
    try verifyLayerMiss(depth: 8)
    try verifyLayerMiss(depth: 16)
    // A 16-bit mask miss against the path it replaced, on a 600 x 400
    // document (the old path converts the whole plane in Swift at -Onone).
    let small = try precisionFixture(width: 600, height: 400)
    defer { small.close() }
    _ = try editor(small)
    try verifyMaskCache(small, label: "16-bit 600x400", compareOldPath: true)
    try verifyChannelPlaneThumbnails(small)
    try verifyFloatCache()
    try verifyFloatMiss()
    print("PASS: layer thumbnail costs — 8 and 16-bit 6000x4000 misses fit before any whole-layer "
        + "copy, beat the old path within 2 codes and then hit; mask thumbnails key on the mask "
        + "plane and float thumbnails on the tone map; a 16-bit mask miss fits in the core "
        + "and beats the old path, and a 2400x1600 float miss fits in the core, both within 2 "
        + "codes; an 8-bit 6000x4000 mask miss and the 16-bit Channels mask and "
        + "alpha rows fit in the core and beat the old path within 2 codes")
}
