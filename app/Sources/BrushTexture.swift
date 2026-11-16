import AppKit

/// Brush dynamics' texture (`designs/brush-dynamics.md`, slice 3; the rule is the core's,
/// `brush_texture.rs`): a pattern tile anchored at the canvas origin and scaled, whose Gray
/// luminosity times alpha `t` thins each dab's coverage at a canvas pixel to
/// `1 − depth · (1 − t)` before the stroke's accumulation. Coverage only, so every depth, mode
/// and target takes it; the Brush and the Eraser only (`EditorTool.takesTextureDynamics`).
///
/// The pattern is a built-in `{preset}` or a custom RGBA8 tile. A library pattern is embedded as
/// its tile when it is chosen (the sheet's popup, an MCP `{name}`), so no stroke, preset or
/// recorded step reads the library later; a tile's native samples are dropped, since the core
/// reads the raster alone.
extension BrushDynamics {
    struct Texture: Codable, Equatable {
        var pattern: PatternRef
        var scale: Double = 1
        var depth: Double = 1

        static let scaleRange = 0.01...10.0

        init(pattern: PatternRef, scale: Double = 1, depth: Double = 1) {
            self.pattern = pattern.withoutNative
            self.scale = scale
            self.depth = depth
        }

        /// `{"pattern": <the core's pattern object>, "scale", "depth"}`: the stored, saved and
        /// recorded form.
        var json: [String: Any] {
            ["pattern": pattern.patternObject, "scale": scale, "depth": depth]
        }

        /// What a reply echoes: the pattern's `{preset}`, or its name and size without pixels.
        var echo: [String: Any] {
            ["pattern": pattern.echo, "scale": scale, "depth": depth]
        }
    }
}

extension BrushDynamics.Texture {
    /// Strict parse of `json`'s form: `pattern` required, `scale` 0.01…10 and `depth` 0…1, each
    /// 1 when absent. `library` true resolves a `{"name"}`-only pattern through `PatternLibrary`
    /// (the MCP stroke and save forms); false refuses it (a recorded `native_stroke` and a preset
    /// file, which hold the tile). A custom tile's `native` samples are dropped.
    init(json raw: Any, library: Bool) throws {
        guard let object = raw as? [String: Any] else {
            throw DoctoredCoreError(message: "dynamics.texture must be an object.")
        }
        if let key = Set(object.keys).subtracting(["pattern", "scale", "depth"]).sorted().first {
            throw DoctoredCoreError(message: "dynamics.texture takes pattern, scale and depth; it "
                + "has no \(key).")
        }
        guard let pattern = object["pattern"] else {
            throw DoctoredCoreError(message: "dynamics.texture needs a pattern: " + Self.patternForms)
        }
        func number(_ key: String, _ range: ClosedRange<Double>) throws -> Double {
            guard let raw = object[key] else { return 1 }
            guard let value = NativeBrushStroke.jsonNumber(raw), value.isFinite,
                  range.contains(value) else {
                throw DoctoredCoreError(message: "dynamics.texture.\(key) must be a number from "
                    + String(format: "%g to %g.", range.lowerBound, range.upperBound))
            }
            return value
        }
        let resolved = try Self.pattern(pattern, library: library)
        let scale = try number("scale", Self.scaleRange)
        let depth = try number("depth", 0...1)
        self.init(pattern: resolved, scale: scale, depth: depth)
    }

    private static let patternForms = "{preset}, {name} (a library pattern) or a custom tile "
        + "{name?, width, height, rgba}."

    private static func pattern(_ raw: Any, library: Bool) throws -> PatternRef {
        guard let tile = raw as? [String: Any] else {
            throw DoctoredCoreError(message: "dynamics.texture.pattern is " + patternForms)
        }
        if tile.count == 1, let name = tile["name"] as? String {
            guard library else {
                throw DoctoredCoreError(message: "dynamics.texture.pattern {name} names a library "
                    + "pattern; a recorded stroke or a preset file holds its tile instead.")
            }
            return try PatternLibrary.named(name).ref.withoutNative
        }
        if tile["preset"] != nil {
            let ids = RasterDocument.patternPresets().map(\.id)
            guard tile.count == 1, let preset = tile["preset"] as? String, ids.contains(preset) else {
                throw DoctoredCoreError(message: "dynamics.texture.pattern.preset must be one of "
                    + ids.joined(separator: ", ") + ", alone.")
            }
            return PatternRef(preset: preset)
        }
        let known: Set<String> = ["name", "width", "height", "rgba", "native"]
        if let stray = tile.keys.sorted().first(where: { !known.contains($0) }) {
            throw DoctoredCoreError(message: "dynamics.texture.pattern has no key \"\(stray)\"; a "
                + "custom tile's keys are name, width, height and rgba.")
        }
        let side = 1...LibraryPattern.maximumSide
        guard let width = NativeBrushStroke.jsonNumber(tile["width"]).flatMap({ Int(exactly: $0) }),
              let height = NativeBrushStroke.jsonNumber(tile["height"]).flatMap({ Int(exactly: $0) }),
              side.contains(width), side.contains(height),
              let rgba = tile["rgba"] as? String,
              Data(base64Encoded: rgba)?.count == width * height * 4,
              tile["name"] == nil || tile["name"] is String else {
            throw DoctoredCoreError(message: "dynamics.texture.pattern's custom tile is 1 to "
                + "\(LibraryPattern.maximumSide) pixels a side, with rgba the standard base64 of "
                + "width × height × 4 straight RGBA8 bytes and name a string.")
        }
        return PatternRef(name: (tile["name"] as? String) ?? "Custom", width: width, height: height,
                          rgba: rgba)
    }
}

/// A stroke's texture: its value (what the stroke records) and the core's handle, which holds
/// the tile's level at its scale, built once for the stroke's every rasterization.
struct StrokeTexture {
    let value: BrushDynamics.Texture
    let handle: RasterBrushTexture
    /// The tile's RGBA8 bytes, which a stroke's payload holds beside its dabs.
    let tileBytes: UInt64

    /// Admits the handle's level (`pattern_tile::fill_scratch_bytes`' figure, 28 bytes a texel)
    /// and builds it; refuses a pattern whose tile cannot be read and every core refusal.
    init(_ value: BrushDynamics.Texture) throws {
        guard let tile = value.pattern.tileBytes() else {
            throw DoctoredCoreError(message: NativeBrushStroke.textureRefusal)
        }
        _ = try RasterResources.shared.admit(scratch: 28 * UInt64(tile.width * tile.height) + 4096)
        self.value = value
        handle = try RasterBrushTexture(rgba: tile.rgba, width: tile.width, height: tile.height,
                                        scale: value.scale, depth: value.depth)
        tileBytes = UInt64(tile.rgba.count)
    }
}

extension EditorTool {
    /// The tools whose dynamics take a texture: the Brush and the Eraser. The Pencil's dab is
    /// two-valued, so its `dynamics.texture` is pinned nil (`ToolOptionsStore.pinnedDynamics`)
    /// and its sheet shows no Texture rows.
    var takesTextureDynamics: Bool { [.brush, .eraser].contains(self) }
}
