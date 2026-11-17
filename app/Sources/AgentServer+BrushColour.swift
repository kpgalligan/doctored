import AppKit

/// Colour dynamics over MCP (`BrushColour.swift`, `designs/brush-dynamics.md` slice 5), mirroring
/// the Brush's Dynamics… sheet Colour rows (`EditorViewController.strokeColourHandle`).
/// `dynamics.color` is a generation argument of `brush_stroke` alone: `nativeStrokeArguments`
/// resolves the call's colour (the foreground) and `dynamics.color.background` into the
/// document's own unit triple, builds the colour handle on `dynamics.seed` and records each dab's
/// ink in `native_stroke.inks`, so a recorded step holds the inks, never the seed, the amounts or
/// a swatch. Where the canvas paints its one ink, a call refuses by name and changes nothing.
extension AgentServer {
    /// `dynamics.color` or `native_stroke.inks` on this call, named for a refusal; nil for none.
    private static func colourSubject(_ arguments: [String: Any]) -> String? {
        if let dynamics = arguments["dynamics"] as? [String: Any], dynamics["color"] != nil {
            return "dynamics.color"
        }
        if let native = arguments["native_stroke"] as? [String: Any], native["inks"] != nil {
            return "native_stroke inks"
        }
        return nil
    }

    /// The limits of colour dynamics, refused by name before anything runs (the canvas paints
    /// its one ink instead): any tool but `brush_stroke`; the Pencil's tip (`aliased`); a mask,
    /// plane or channel target, an adjustment layer (whose strokes paint its mask) and Quick
    /// Mask; a blend mode other than Normal; a CMYK, Indexed or Bitmap document; a non-zero hue
    /// or saturation on a Grayscale, Duotone or Lab document; `rgba32` (an above-white float
    /// ink) and `ink` (a CMYK ink).
    func refuseColourLimits(tool: String, arguments a: [String: Any],
                            document: ImageDocument?) throws {
        guard let subject = Self.colourSubject(a) else { return }
        guard tool == "brush_stroke" else {
            throw ToolError(message: "\(subject) is the Brush's colour dynamics: brush_stroke takes "
                + "it; \(tool) has none.")
        }
        func refuse(_ why: String) -> ToolError {
            ToolError(message: "\(subject) paints each dab of a Brush stroke in its own ink on a "
                + "layer in Normal mode; \(why).")
        }
        if NativeBrushStroke.jsonBoolean(a["aliased"]) == true
            || (a["aliased"] as? String)?.lowercased() == "true" {
            throw refuse("the Pencil's tip (aliased) paints one ink")
        }
        guard let document, let doc = document.doc else { return }
        if doc.isCmyk || doc.isIndexed || doc.isBitmap {
            let mode = doc.isCmyk ? "a CMYK" : doc.isIndexed ? "an Indexed" : "a Bitmap"
            throw refuse("\(mode) document paints one ink a stroke")
        }
        if document.quickMaskActive { throw refuse("Quick Mask paints coverage, not colour") }
        if try paintTarget(a, document) != .layer
            || doc.layerIsAdjustment(try structuralLayerIndex(a, document)) {
            throw refuse("a mask, a plane or a channel is coverage, not colour")
        }
        if let blend = try blendModeArg(a, paintOnly: true), blend != DR_BLEND_NORMAL {
            throw refuse("blend_mode \(DrBlendMode.displayName(for: blend)) takes one ink a stroke")
        }
        if let key = ["rgba32", "ink"].first(where: { a[$0] != nil }) {
            throw refuse("\(key) is one ink a stroke")
        }
        if let colour = (a["dynamics"] as? [String: Any])?["color"] as? [String: Any],
           doc.isGray || doc.isDuotone || doc.isLab,
           let key = ["hue", "saturation"].first(where: {
               colour[$0].flatMap(NativeBrushStroke.jsonNumber).map { $0 != 0 } ?? false
           }) {
            let what = key == "hue" ? "turns an RGB hue" : "lowers an RGB saturation"
            throw ToolError(message: "dynamics.color.\(key) \(what); a "
                + (doc.isLab ? "Lab" : doc.isDuotone ? "Duotone" : "Grayscale")
                + " document takes the mix and the brightness alone"
                + (doc.isLab ? " (brightness scaling L*)." : "."))
        }
    }

    /// `arguments` with `dynamics.color.background` taken out (the dynamics parser takes the
    /// amounts alone), and that background; the arguments unchanged without one.
    static func strippingColourBackground(_ arguments: [String: Any]) -> ([String: Any], Any?) {
        guard var dynamics = arguments["dynamics"] as? [String: Any],
              var colour = dynamics["color"] as? [String: Any],
              let background = colour.removeValue(forKey: "background") else {
            return (arguments, nil)
        }
        dynamics["color"] = colour
        var out = arguments
        out["dynamics"] = dynamics
        return (out, background)
    }

    /// A generated stroke's colour handle: nil without colour amounts (or without a dynamics
    /// handle, which `strokeDynamics` answers nil for). The foreground is the call's colour and
    /// the background `background` in `color`'s forms (a hex string, or an `[r, g, b, a]` array
    /// as `rgba` takes it), both read under the call's `color_space` and resolved into the
    /// document's own unit triple. `mix` above 0 needs a background.
    func strokeColourHandle(_ arguments: [String: Any], colour: BrushDynamics.ColorDynamics?,
                            background: Any?, seed: UInt32,
                            document: ImageDocument?) throws -> RasterBrushColour? {
        guard let colour, !colour.isZero, let document, let doc = document.doc else { return nil }
        if colour.mix > 0, background == nil {
            throw ToolError(message: "dynamics.color.mix moves each dab toward a background "
                + "colour; pass dynamics.color.background in color's forms.")
        }
        let foreground = try documentTriple(arguments, document: document)
        var back = foreground
        if let background {
            var b = arguments
            b.removeValue(forKey: "color")
            b.removeValue(forKey: "rgba")
            if background is [Any] { b["rgba"] = background } else { b["color"] = background }
            do { back = try documentTriple(b, document: document) } catch {
                throw ToolError(message: "dynamics.color.background: "
                    + error.localizedDescription)
            }
        }
        do {
            return try BrushDynamics.colourHandle(colour, seed: seed, foreground: foreground,
                                                  background: back, lab: doc.isLab)
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    /// The call's colour as the document's own unit triple: a `document` (or Display P3) colour
    /// as given, an sRGB one converted into the document's space, as the canvas converts its
    /// swatches (`PrecisionInk(color:space:)`).
    private func documentTriple(_ a: [String: Any], document: ImageDocument) throws -> [Double] {
        let ink = try precisionInk(a, document: document)
        guard try precisionInkSpace(a) == .srgb else { return Array(ink.rgba.prefix(3)) }
        let srgb = NSColor(srgbRed: CGFloat(ink.rgba[0]), green: CGFloat(ink.rgba[1]),
                           blue: CGFloat(ink.rgba[2]), alpha: CGFloat(ink.rgba[3]))
        return Array(try PrecisionInk(color: srgb, space: document.nsColorSpace).rgba.prefix(3))
    }
}
