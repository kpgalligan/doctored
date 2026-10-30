import AppKit

// #1 Lab phase 6b-i: Apply Image's and Calculations' vocabulary on a Lab Color document, and
// Merge to HDR's stack conversion. Apply Image and Calculations blend each channel's codes alone
// through the channel-math kernel (`NativeChannelMath`, whose RGB path runs unchanged: its red,
// green and blue are Lightness, a and b); the Lab composite onto one plane reads Lightness. So
// the modes that need a colour triple read as one (Darker and Lighter Color and the four HSL
// modes) are not offered there. Merge to HDR converts a Lab stack to 16-bit ProPhoto RGB first
// and returns 32-bit RGB, because there is no 32-bit Lab.
extension LabDocument {
    /// The modes Apply Image and Calculations offer on a Lab document: `labModes` without the
    /// six that read a channel triple as a colour.
    static let channelMathModes: [DrBlendMode] = NativeLayerBlending.labModes.filter {
        ![DR_BLEND_DARKER_COLOR, DR_BLEND_LIGHTER_COLOR, DR_BLEND_HUE, DR_BLEND_SATURATION,
          DR_BLEND_COLOR, DR_BLEND_LUMINOSITY].contains($0)
    }

    static var channelMathLimitation: String {
        "Apply Image and Calculations on a Lab Color document blend each channel's codes alone and "
            + "support " + channelMathModes.map { DrBlendMode.displayName(for: $0) }
                .joined(separator: ", ")
            + "; every other mode is unavailable there."
    }

    /// `apply_image`'s and `calculations`' refusal: a named `blend_mode` outside
    /// `channelMathModes`; nil admits the call.
    static func channelMathRefusal(arguments: [String: Any]) -> String? {
        guard let name = arguments["blend_mode"] as? String,
              let mode = NativeLayerBlending.mode(named: name),
              !channelMathModes.contains(mode) else { return nil }
        return channelMathLimitation
    }

    /// A source-plane argument on a Lab document: "lab" (the composite or layer, channel for
    /// channel), "lightness", "a", "b" (the stored red, green and blue codes), "alpha", or an
    /// alpha channel's name.
    static func planeChoice(_ raw: String, in doc: RasterDocument, key: String) throws -> PlaneChoice {
        if raw == "lab" { return .rgb }
        if let plane = plane(named: raw) { return .plane(plane) }
        if raw == "alpha" { return .plane(.alpha) }
        if !["rgb", "red", "green", "blue", "luma"].contains(raw),
           let index = doc.channelIndex(named: raw) {
            return .channel(index)
        }
        throw AgentServer.ToolError(
            message: "\(key) must be \"lab\", \"lightness\", \"a\", \"b\", \"alpha\" or an alpha "
                + "channel's name on a Lab Color document (got \"\(raw)\")")
    }

    /// `planeChoice`'s inverse, as `apply_image` and `calculations` report it.
    static func planeChoiceName(_ choice: PlaneChoice, in doc: RasterDocument) -> String {
        switch choice {
        case .rgb: return "lab"
        case .plane(let plane): return agentPlaneName(plane)
        case .channel(let index): return doc.channelInfo(index)?.name ?? "channel \(index)"
        case .ink(let plate): return plate.key
        }
    }

    /// Merge to HDR's stack from a Lab document: Image ▸ Mode ▸ RGB Color into 16-bit ProPhoto
    /// RGB with black point compensation, `LabLayerClip.toRGB`'s cross-mode rule. The layers keep
    /// their indices; the source document is not changed.
    static func mergeStack(_ doc: RasterDocument) throws -> RasterDocument {
        guard let precision = doc.precision else {
            throw DoctoredCoreError(message: "A Lab Color document requires a typed document.")
        }
        let budget = try PrecisionWorkContext(source: precision)
        let rgb = try precision.convertLabToRGB(
            profile: RasterProfile.builtin(.proPhotoRGB),
            options: ["black_point_compensation": true], workingLimit: budget.limit,
            otherRetained: budget.otherRetained, control: nil)
        return try RasterDocument(precision: rgb)
    }
}
