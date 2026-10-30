import Foundation

// The local editing preview and masks stay in canonical before-optics coordinates while Global
// shows the final source; nonneutral optics require reviewing Global before Apply. Presets/sync
// move ordinary controls only, never source calibration or registered vignette geometry. Versions
// 1–3 retain their old meaning and do not upgrade on neutral work.

private struct RawGeometryFieldKey: CodingKey {
    let stringValue: String
    var intValue: Int? { nil }
    init(stringValue: String) { self.stringValue = stringValue }
    init?(intValue: Int) { return nil }
}

private func requireRawGeometryFields(_ decoder: Decoder, allowed: Set<String>) throws {
    let fields = try decoder.container(keyedBy: RawGeometryFieldKey.self)
    guard Set(fields.allKeys.map(\.stringValue)).isSubset(of: allowed) else {
        throw DoctoredCoreError(message: "Unknown RAW geometry field.")
    }
}

/// The same flat, strict version-one request understood by the native map seam.
struct RawSourceMap: Codable, Equatable {
    enum Kind: String, Codable { case exif, affine, lens }
    enum Filter: String, Codable { case bilinear, bicubic }
    var version = 1
    var kind: Kind
    var orientation: UInt32?
    var width: UInt32?
    var height: UInt32?
    var inverse: [Double]?
    var filter: Filter?
    var lens: RawLensGeometry?

    enum CodingKeys: String, CodingKey, CaseIterable {
        case version, kind, orientation, width, height, inverse, filter
        case profileID = "profile_id", center, radial
        case redScale = "red_scale", blueScale = "blue_scale"
        case referenceSize = "reference_size", referenceFromSource = "reference_from_source"
    }
    init(orientation: UInt32) {
        kind = .exif
        self.orientation = orientation
    }
    init(width: UInt32, height: UInt32, inverse: [Double], filter: Filter = .bilinear) {
        kind = .affine
        self.width = width; self.height = height; self.inverse = inverse; self.filter = filter
    }
    init(lens: RawLensGeometry) {
        kind = .lens
        self.lens = lens; filter = lens.filter
    }
    init(from decoder: Decoder) throws {
        try requireRawGeometryFields(decoder, allowed: Set(CodingKeys.allCases.map(\.stringValue)))
        let c = try decoder.container(keyedBy: CodingKeys.self)
        version = try c.decode(Int.self, forKey: .version)
        kind = try c.decode(Kind.self, forKey: .kind)
        guard version == 1 else { throw DoctoredCoreError(message: "Unsupported RAW source map version.") }
        let allowed: Set<CodingKeys>
        switch kind {
        case .exif:
            allowed = [.version, .kind, .orientation]
            orientation = try c.decode(UInt32.self, forKey: .orientation)
            guard (1...8).contains(orientation ?? 0) else {
                throw DoctoredCoreError(message: "Invalid EXIF orientation.")
            }
        case .affine:
            allowed = [.version, .kind, .width, .height, .inverse, .filter]
            width = try c.decode(UInt32.self, forKey: .width)
            height = try c.decode(UInt32.self, forKey: .height)
            inverse = try c.decode([Double].self, forKey: .inverse)
            filter = try c.decode(Filter.self, forKey: .filter)
            guard width != 0, height != 0, inverse?.count == 6,
                  inverse?.allSatisfy(\.isFinite) == true else {
                throw DoctoredCoreError(message: "Invalid RAW affine source map.")
            }
        case .lens:
            allowed = [.version, .kind, .profileID, .center, .radial, .redScale, .blueScale,
                       .filter, .referenceSize, .referenceFromSource]
            lens = RawLensGeometry(
                profileID: try c.decode(String.self, forKey: .profileID),
                center: try c.decode([Double].self, forKey: .center),
                radial: try c.decode([Double].self, forKey: .radial),
                redScale: try c.decode(Double.self, forKey: .redScale),
                blueScale: try c.decode(Double.self, forKey: .blueScale),
                filter: try c.decode(Filter.self, forKey: .filter),
                referenceSize: try c.decodeIfPresent([UInt32].self, forKey: .referenceSize),
                referenceFromSource: try c.decodeIfPresent([Double].self, forKey: .referenceFromSource))
            try lens?.validate()
            filter = lens?.filter
        }
        guard Set(c.allKeys).isSubset(of: allowed) else {
            throw DoctoredCoreError(message: "Unknown RAW source map field.")
        }
    }
    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(version, forKey: .version)
        try c.encode(kind, forKey: .kind)
        switch kind {
        case .exif:
            try c.encode(orientation, forKey: .orientation)
        case .affine:
            try c.encode(width, forKey: .width)
            try c.encode(height, forKey: .height)
            try c.encode(inverse, forKey: .inverse)
            try c.encode(filter, forKey: .filter)
        case .lens:
            guard let lens else { throw DoctoredCoreError(message: "Missing RAW lens geometry.") }
            try lens.validate()
            try c.encode(lens.profileID, forKey: .profileID)
            try c.encode(lens.center, forKey: .center)
            try c.encode(lens.radial, forKey: .radial)
            try c.encode(lens.redScale, forKey: .redScale)
            try c.encode(lens.blueScale, forKey: .blueScale)
            try c.encode(lens.filter, forKey: .filter)
            try c.encodeIfPresent(lens.referenceSize, forKey: .referenceSize)
            try c.encodeIfPresent(lens.referenceFromSource, forKey: .referenceFromSource)
        }
    }
    func json() throws -> String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        guard let text = String(data: try encoder.encode(self), encoding: .utf8) else {
            throw DoctoredCoreError(message: "RAW source map is not UTF8.")
        }
        return text
    }
}

struct RawLensGeometry: Codable, Equatable {
    var profileID: String
    var center: [Double]
    var radial: [Double]
    var redScale: Double
    var blueScale: Double
    var filter: RawSourceMap.Filter
    var referenceSize: [UInt32]?
    var referenceFromSource: [Double]?

    init(profileID: String, center: [Double], radial: [Double], redScale: Double,
         blueScale: Double, filter: RawSourceMap.Filter, referenceSize: [UInt32]?,
         referenceFromSource: [Double]?) {
        self.profileID = profileID
        self.center = center
        self.radial = radial
        self.redScale = redScale
        self.blueScale = blueScale
        self.filter = filter
        self.referenceSize = referenceSize
        self.referenceFromSource = referenceFromSource
    }

    enum CodingKeys: String, CodingKey, CaseIterable {
        case profileID = "profile_id", center, radial
        case redScale = "red_scale", blueScale = "blue_scale", filter
        case referenceSize = "reference_size", referenceFromSource = "reference_from_source"
    }
    init(from decoder: Decoder) throws {
        try requireRawGeometryFields(decoder, allowed: Set(CodingKeys.allCases.map(\.stringValue)))
        let c = try decoder.container(keyedBy: CodingKeys.self)
        self.init(profileID: try c.decode(String.self, forKey: .profileID),
            center: try c.decode([Double].self, forKey: .center),
            radial: try c.decode([Double].self, forKey: .radial),
            redScale: try c.decode(Double.self, forKey: .redScale),
            blueScale: try c.decode(Double.self, forKey: .blueScale),
            filter: try c.decode(RawSourceMap.Filter.self, forKey: .filter),
            referenceSize: try c.decodeIfPresent([UInt32].self, forKey: .referenceSize),
            referenceFromSource: try c.decodeIfPresent([Double].self, forKey: .referenceFromSource))
        try validate()
    }
    func validate() throws {
        guard !profileID.isEmpty, profileID.utf8.count <= 128, !profileID.utf8.contains(0),
              center.count == 2, center.allSatisfy({ $0.isFinite && (0...1).contains($0) }),
              radial.count == 3, radial.allSatisfy({ $0.isFinite && (-1...1).contains($0) }),
              redScale.isFinite, (0.9...1.1).contains(redScale),
              blueScale.isFinite, (0.9...1.1).contains(blueScale),
              referenceSize == nil || (referenceSize?.count == 2 && referenceSize?.allSatisfy({ $0 > 0 }) == true),
              referenceFromSource == nil || (referenceFromSource?.count == 6
                  && referenceFromSource?.allSatisfy(\.isFinite) == true),
              (referenceSize == nil) == (referenceFromSource == nil) else {
            throw DoctoredCoreError(message: "Invalid RAW lens geometry or registration.")
        }
    }
    var isNeutral: Bool { radial == [0, 0, 0] && redScale == 1 && blueScale == 1 }
    func map() -> RawSourceMap { RawSourceMap(lens: self) }
    var jsonObject: [String: Any] {
        var result: [String: Any] = ["profile_id": profileID, "center": center,
            "radial": radial, "red_scale": redScale, "blue_scale": blueScale,
            "filter": filter.rawValue]
        if let referenceSize { result["reference_size"] = referenceSize }
        if let referenceFromSource { result["reference_from_source"] = referenceFromSource }
        return result
    }
}

struct RawDecoderFrame: Codable, Equatable {
    var version = 1
    var orientation: UInt32
    var extent: [Double]
    init(orientation: UInt32, extent: [Double]) {
        self.orientation = orientation
        self.extent = extent
    }
    enum CodingKeys: String, CodingKey, CaseIterable { case version, orientation, extent }
    init(from decoder: Decoder) throws {
        try requireRawGeometryFields(decoder, allowed: Set(CodingKeys.allCases.map(\.stringValue)))
        let c = try decoder.container(keyedBy: CodingKeys.self)
        self.init(orientation: try c.decode(UInt32.self, forKey: .orientation),
            extent: try c.decode([Double].self, forKey: .extent))
        version = try c.decode(Int.self, forKey: .version)
        try validate()
    }
    func validate() throws {
        guard version == 1, (1...8).contains(orientation), extent.count == 4,
              extent.allSatisfy({ $0.isFinite && $0.rounded() == $0 && abs($0) <= 1e9 }),
              extent[2] > 0, extent[3] > 0 else {
            throw DoctoredCoreError(message: "Invalid pinned RAW decoder frame.")
        }
    }
}

struct RawVignetteReference: Codable, Equatable {
    var version = 1
    var referenceSize: [UInt32]
    var referenceFromSource: [Double]
    init(referenceSize: [UInt32], referenceFromSource: [Double]) {
        self.referenceSize = referenceSize
        self.referenceFromSource = referenceFromSource
    }
    enum CodingKeys: String, CodingKey, CaseIterable {
        case version, referenceSize, referenceFromSource
    }
    init(from decoder: Decoder) throws {
        try requireRawGeometryFields(decoder, allowed: Set(CodingKeys.allCases.map(\.stringValue)))
        let c = try decoder.container(keyedBy: CodingKeys.self)
        self.init(referenceSize: try c.decode([UInt32].self, forKey: .referenceSize),
            referenceFromSource: try c.decode([Double].self, forKey: .referenceFromSource))
        version = try c.decode(Int.self, forKey: .version)
        try validate()
    }
    func validate() throws {
        guard version == 1, referenceSize.count == 2,
              referenceSize.allSatisfy({ $0 > 0 }),
              UInt64(referenceSize[0]) * UInt64(referenceSize[1]) <= 64_000_000,
              referenceFromSource.count == 6,
              referenceFromSource.allSatisfy(\.isFinite),
              abs(referenceFromSource[0] * referenceFromSource[3]
                  - referenceFromSource[1] * referenceFromSource[2]) >= 1e-12 else {
            throw DoctoredCoreError(message: "Invalid registered RAW vignette frame.")
        }
    }
}
