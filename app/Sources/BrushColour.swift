import AppKit

/// Brush dynamics' colour (`designs/brush-dynamics.md`, slice 5; the rule is the core's,
/// `brush_colour.rs`): each dab of a Brush stroke takes its own ink, a seeded mix from the
/// foreground toward the background, then hue, saturation and brightness jitter, and the stroke
/// carries that ink per pixel beside its coverage. A layer of an RGB, Grayscale, Duotone or Lab
/// document in Normal mode only; Grayscale, Duotone and Lab take the mix and the brightness
/// (`reduced(for:)`), Lab's brightness scaling L* alone. The Brush only
/// (`EditorTool.takesColourDynamics`). A stroke records each dab's resolved ink
/// (`NativeBrushStroke.inks`), never the seed, the amounts or a swatch.
extension BrushDynamics {
    struct ColorDynamics: Codable, Equatable {
        /// Foreground/Background Mix: how far toward the background a dab may move (0…1).
        var mix: Double = 0
        /// Hue Jitter: up to ±180° × hue (0…1).
        var hue: Double = 0
        /// Saturation Jitter and Brightness Jitter: how far a dab's HSL saturation and
        /// lightness may drop (0…1; jitter only lowers).
        var saturation: Double = 0
        var brightness: Double = 0

        /// True when no amount is set, which `normalized` drops.
        var isZero: Bool { mix == 0 && hue == 0 && saturation == 0 && brightness == 0 }

        /// `{"mix", "hue", "saturation", "brightness"}`: the stored, saved and MCP form.
        var json: [String: Any] {
            ["mix": mix, "hue": hue, "saturation": saturation, "brightness": brightness]
        }

        /// The amounts `doc` takes: hue and saturation zeroed on a Grayscale, Duotone or Lab
        /// document (a grey has no hue, and Lab's codes are no RGB to rotate); unchanged
        /// otherwise.
        func reduced(for doc: RasterDocument) -> ColorDynamics {
            guard doc.isGray || doc.isDuotone || doc.isLab else { return self }
            var out = self
            out.hue = 0
            out.saturation = 0
            return out
        }

        /// The core's request (`dr_brush_colour_create`): `foreground` and `background` the
        /// document's own unit triples, `lab` writing the Lab model.
        func coreRequest(seed: UInt32, foreground: [Double], background: [Double],
                         lab: Bool) throws -> String {
            try NativeSelectionWork.encoding([
                "version": 1, "model": lab ? "lab" : "rgb", "seed": Int(seed),
                "foreground": Array(foreground.prefix(3)), "background": Array(background.prefix(3)),
                "mix": mix, "hue": hue, "saturation": saturation, "brightness": brightness])
        }
    }

    /// A stroke's colour handle, or nil when no amount is set. `foreground` and `background` are
    /// the document's unit triples (the twin's codes on a Lab document, `lab` true).
    static func colourHandle(_ colour: ColorDynamics?, seed: UInt32, foreground: [Double],
                             background: [Double], lab: Bool) throws -> RasterBrushColour? {
        guard let colour, !colour.isZero else { return nil }
        return try RasterBrushColour(request: colour.coreRequest(
            seed: seed, foreground: foreground, background: background, lab: lab), lab: lab)
    }
}

/// An extension, so `ColorDynamics` keeps its memberwise initializer.
extension BrushDynamics.ColorDynamics {
    /// Strict parse of `json`'s form, every key optional at 0, each refusal naming
    /// `dynamics.color.<key>`.
    init(json raw: Any) throws {
        guard let object = raw as? [String: Any] else {
            throw DoctoredCoreError(message: "dynamics.color must be an object.")
        }
        let keys = ["mix", "hue", "saturation", "brightness"]
        if let key = Set(object.keys).subtracting(keys).sorted().first {
            throw DoctoredCoreError(message: "dynamics.color takes " + keys.joined(separator: ", ")
                + "; it has no \(key).")
        }
        func amount(_ key: String) throws -> Double {
            guard let value = object[key] else { return 0 }
            guard let number = NativeBrushStroke.jsonNumber(value), number.isFinite,
                  (0...1).contains(number) else {
                throw DoctoredCoreError(message: "dynamics.color.\(key) must be a number from 0 to 1.")
            }
            return number
        }
        self.init(mix: try amount("mix"), hue: try amount("hue"),
                  saturation: try amount("saturation"), brightness: try amount("brightness"))
    }
}

extension EditorTool {
    /// The tools whose dynamics take colour: the Brush. The Pencil and the Eraser paint one ink
    /// (the Eraser none), so their `dynamics.color` is pinned nil (`ToolOptionsStore.pinnedDynamics`)
    /// and their sheets show no Colour rows.
    var takesColourDynamics: Bool { [.brush].contains(self) }
}
