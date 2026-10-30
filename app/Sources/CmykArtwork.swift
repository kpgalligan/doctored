import AppKit

// Type, shape and fill layers on a CMYK document (#547). A CMYK document's descriptions store INK:
// text v5, shape v6 and fill v2 (TextLayer.swift, ShapeLayer.swift, FillLayer.swift) hold
// `[c, m, y, k, alpha]` in the document's profile, and every hex field beside them holds that
// ink's sRGB APPEARANCE through the profile (relative colorimetric with BPC). The core renders the
// ink exactly (`core/src/cmyk_artwork.rs`); type and shape edges are the RGB path's coverage.
//
// Editing is display → commit. An editor or MCP handler works in sRGB: it opens on `cmykDisplay`
// (the appearance numbers) and hands `nativeArtwork` an sRGB description, which `cmykArtwork`
// separates through the profile ONCE. A colour whose sRGB is one of `matching`'s takes that ink
// exactly — the swatches' stored inks (`CmykInkMatch.swatches`), an MCP `ink` argument, and, when
// re-rendering a layer, its own inks paired with their appearance (`cmykMatches`) — so an
// unchanged colour keeps its ink bit for bit. A gradient stop, whose colour is an 8-bit hex, keeps
// the replaced description's ink at the same stored index while its hex is unchanged, and
// otherwise matches on the stored hex/255.
//
// Across a conversion (Image ▸ Mode, Convert to Profile, a paste between modes or profiles) the
// host captures each described layer first (`artworkCaptures`) and, after the core converted the
// pixels and dropped the descriptions, re-renders each one with its colours converted
// (`reinkingArtwork`): sRGB → ink, ink → ink, ink → sRGB, through `dr_cmyk_artwork_colors`.

/// One sRGB colour (`[r, g, b]`, 0…1) and the ink (`[c, m, y, k]`, 0…1) it stands for.
struct CmykInkMatch: Equatable {
    let srgb: [Double]
    let ink: [Double]

    /// Within this of each other per channel, two sRGB colours are the same colour.
    static let tolerance = 1e-9

    func matches(_ rgb: [Double]) -> Bool {
        rgb.count >= 3 && srgb.count == 3
            && zip(srgb, rgb.prefix(3)).allSatisfy { abs($0 - $1) <= Self.tolerance }
    }

    /// The foreground and background swatches' stored inks (a picked or eyedropped CMYK value,
    /// `SharedToolState.foregroundInk` / `backgroundInk`), each paired with its swatch colour's
    /// sRGB; a swatch with no stored ink contributes nothing.
    static func swatches(paint: NSColor, background: NSColor) -> [CmykInkMatch] {
        let shared = ToolOptionsStore.shared.sharedState
        return [(paint, shared.foregroundInk), (background, shared.backgroundInk)].compactMap { color, ink in
            guard let ink else { return nil }
            let rgb = Array(ArtworkColor.authored(color, in: .srgb).prefix(3))
            guard rgb.allSatisfy(\.isFinite) else { return nil }
            return CmykInkMatch(srgb: rgb, ink: ink.unit)
        }
    }
}

/// One end of a colour conversion: the built-in sRGB or a CMYK profile's ink.
enum ArtworkColorEnd: Equatable {
    case srgb
    case cmyk(profile: Data)

    /// The profile bytes `dr_cmyk_artwork_colors` takes; empty for sRGB.
    var profile: Data {
        if case let .cmyk(profile) = self { return profile }
        return Data()
    }
}

/// A described layer captured before a conversion (`artworkCaptures`).
struct ArtworkCapture {
    let index: Int
    let description: LayerDescription
    let anchor: CGPoint
    let name: String
}

/// The last CMYK artwork render's admitted plan (main thread), which the MCP replies echo as
/// `working_memory_plan_bytes` (`scripts/cmyk-admission/`).
enum CmykArtworkPlan {
    static var lastBytes: UInt64 = 0
}

/// A description's colours in a fixed order — text base then runs; shape fill then stroke; a solid
/// fill's colour or a gradient's stops in STORED order — each without its alpha, with the alphas
/// and, for a gradient, the stop hexes beside them.
struct ArtworkPalette {
    var colors: [[Double]]
    var alphas: [Double]
    var stopHexes: [String] = []
}

extension LayerDescription {
    /// The ink a description's numbers are in.
    var artworkInk: ArtworkInk {
        switch self {
        case let .text(payload): return payload.ink
        case let .shape(payload): return payload.ink
        case let .fill(payload): return payload.ink ?? .srgb
        }
    }

    /// The palette of an sRGB- or ink-form description (`ArtworkPalette`).
    var palette: ArtworkPalette {
        switch self {
        case let .text(payload):
            let style = payload.artwork
                ?? ArtworkTextStyle(rgba: ArtworkColor.authored(payload.nsColor), in: .srgb)
            let all = [style.rgba] + style.runs.map(\.rgba)
            return ArtworkPalette(colors: all.map { Array($0.dropLast()) }, alphas: all.map(ArtworkColor.alpha))
        case let .shape(payload):
            let all = [payload.artwork?.fill ?? payload.fillColor.map(ArtworkColor.authored),
                       payload.artwork?.stroke ?? payload.strokeColor.map(ArtworkColor.authored)]
                .compactMap { $0 }
            return ArtworkPalette(colors: all.map { Array($0.dropLast()) }, alphas: all.map(ArtworkColor.alpha))
        case let .fill(payload):
            if let color = payload.color {
                return ArtworkPalette(colors: [Array(color.dropLast())], alphas: [ArtworkColor.alpha(color)])
            }
            let stops = payload.gradient?.stops ?? []
            let colors = payload.stopInks ?? stops.map { CmykArtworkHex.unit($0.color) }
            return ArtworkPalette(colors: colors, alphas: stops.map(\.opacity),
                                  stopHexes: stops.map(\.color))
        }
    }

    /// This description with `colors` (the palette's order, without alpha) in `ink`, and its hex
    /// fields written from `appearance` (sRGB triples, the same order).
    func repainted(_ colors: [[Double]], appearance: [[Double]], ink: ArtworkInk) -> LayerDescription {
        let alphas = palette.alphas
        let full = zip(colors, alphas).map { $0 + [$1] }
        let hexes = zip(appearance, alphas).map { TextLayer.hex(numbers: $0 + [$1]) }
        switch self {
        case var .text(payload):
            let runs = payload.artwork?.runs ?? []
            let style = ArtworkTextStyle(rgba: full[0], runs: runs.enumerated().map { index, run in
                ArtworkColorRun(location: run.location, length: run.length, rgba: full[index + 1])
            }, ink: ink.stored)
            payload.artwork = style
            payload.color = hexes[0]
            return .text(payload)
        case var .shape(payload):
            let hasFill = payload.artwork?.fill != nil || (payload.artwork == nil && payload.fillColor != nil)
            let fill = hasFill ? full.first : nil
            let stroke = full.count > (hasFill ? 1 : 0) ? full.last : nil
            payload.artwork = ArtworkShapeStyle(fill: fill, stroke: stroke, ink: ink.stored)
            payload.fill = hasFill ? hexes[0] : ""
            payload.stroke = stroke == nil ? "" : hexes[hexes.count - 1]
            return .shape(payload)
        case var .fill(payload):
            let cmyk = ink == .cmyk
            payload.version = cmyk ? FillLayerPayload.cmykVersion : FillLayerPayload.currentVersion
            payload.ink = cmyk ? .cmyk : nil
            if payload.color != nil {
                payload.color = full.first
                payload.stopInks = nil
            } else if var gradient = payload.gradient {
                for index in gradient.stops.indices where index < appearance.count {
                    gradient.stops[index].color = CmykArtworkHex.six(appearance[index])
                }
                payload.gradient = gradient
                payload.stopInks = cmyk ? colors : nil
                // Wide stops are never kept across a mode
                // change: the new colours are the stops' own.
                payload.stopLinears = nil
            }
            return .fill(payload)
        }
    }
}

/// The hex arithmetic a gradient stop's `#rrggbb` is read and written with.
enum CmykArtworkHex {
    /// A stop hex as sRGB numbers: each byte / 255 (exactly what `FillLayer.drawing` reads).
    static func unit(_ hex: String) -> [Double] {
        guard let color = LayerStyleColor.color(fromHex: hex),
              let srgb = ColorProfile.converted(color, into: .sRGB) else { return [0, 0, 0] }
        return [srgb.redComponent, srgb.greenComponent, srgb.blueComponent].map(Double.init)
    }

    /// `#rrggbb` (lowercase, `LayerStyleColor.hex`'s spelling) of sRGB numbers.
    static func six(_ rgb: [Double]) -> String {
        let byte: (Double) -> Int = { Int((min(max($0.isFinite ? $0 : 0, 0), 1) * 255).rounded()) }
        return String(format: "#%02x%02x%02x", byte(rgb[0]), byte(rgb[1]), byte(rgb[2]))
    }
}

extension RasterDocument {
    /// The ink the UI and MCP author colours in: sRGB in a CMYK document (the commit separates
    /// it, `cmykArtwork`), the document's own ink otherwise.
    var authoringInk: ArtworkInk { isCmyk ? .srgb : artworkInk }

    /// This document's colour end: its CMYK profile, or sRGB.
    var artworkColorEnd: ArtworkColorEnd {
        guard isCmyk, let precision else { return .srgb }
        return .cmyk(profile: precision.cmykProfileData)
    }

    /// The default options of the display leg and of a separation: relative colorimetric with
    /// black-point compensation (`dr_cmyk_color`'s, the Color Picker's).
    static var artworkColorOptions: CmykConversionOptions {
        var options = CmykConversionOptions()
        options.intent = .relative
        options.blackPointCompensation = true
        return options
    }

    /// `description` in this CMYK document's ink (module comment): an ink-form description as it
    /// is; an sRGB (or linear) one separated, each colour taking a match's ink when its sRGB is
    /// the match's.
    func cmykArtwork(_ description: LayerDescription, matching: [CmykInkMatch]) throws -> LayerDescription {
        try cmykArtwork(description, matching: matching, previous: nil, options: Self.artworkColorOptions)
    }

    /// `cmykArtwork` with the replaced description (its gradient stops keep their ink by stored
    /// index while their hex is unchanged) and the separation's options.
    func cmykArtwork(_ description: LayerDescription, matching: [CmykInkMatch],
                     previous: LayerDescription?, options: CmykConversionOptions) throws -> LayerDescription {
        // A pattern fill has no ink form: its texels separate at render (FillLayer.swift's INK).
        if case let .fill(payload) = description, payload.fill == FillLayerPayload.patternKind {
            return description
        }
        guard description.artworkInk != .cmyk else { return description }
        let end = artworkColorEnd
        guard case .cmyk = end else { throw DoctoredCoreError(message: "Ink artwork needs a CMYK document.") }
        let source = description.inInk(.srgb)
        let palette = source.palette
        var previousStops: [(hex: String, ink: [Double])] = []
        if case let .fill(old)? = previous, old.ink == .cmyk, let inks = old.stopInks,
           let stops = old.gradient?.stops, stops.count == inks.count {
            previousStops = zip(stops, inks).map { ($0.color.lowercased(), $1) }
        }
        var inks = [[Double]?](repeating: nil, count: palette.colors.count)
        for (index, rgb) in palette.colors.enumerated() {
            if index < palette.stopHexes.count, index < previousStops.count,
               previousStops[index].hex == palette.stopHexes[index].lowercased() {
                inks[index] = previousStops[index].ink
            } else if let match = matching.first(where: { $0.matches(rgb) }) {
                inks[index] = match.ink
            }
        }
        let unmatched = inks.indices.filter { inks[$0] == nil }
        let separated = try RasterCmykArtworkCore.colors(unmatched.map { palette.colors[$0] },
                                                         from: .srgb, to: end, options: options)
        for (slot, index) in unmatched.enumerated() { inks[index] = separated[slot] }
        let final = inks.map { $0 ?? [0, 0, 0, 0] }
        return try inkForm(source, inks: final, end: end)
    }

    /// `description` (sRGB form) repainted with `inks` and their appearance through `end`.
    private func inkForm(_ description: LayerDescription, inks: [[Double]],
                         end: ArtworkColorEnd) throws -> LayerDescription {
        let appearance = try RasterCmykArtworkCore.colors(inks, from: end, to: .srgb,
                                                          options: Self.artworkColorOptions)
        return description.repainted(inks, appearance: appearance, ink: .cmyk)
    }

    /// The sRGB view of an ink-form `description` (module comment): every colour its appearance
    /// through this document's profile, a gradient keeping its stored stop hexes. Any other
    /// description as it is.
    func cmykDisplay(_ description: LayerDescription) throws -> LayerDescription {
        try displayed(description, through: artworkColorEnd)
    }

    /// `cmykDisplay` through `end`'s profile.
    private func displayed(_ description: LayerDescription, through end: ArtworkColorEnd) throws -> LayerDescription {
        guard description.artworkInk == .cmyk, case .cmyk = end else { return description }
        let palette = description.palette
        var appearance = try RasterCmykArtworkCore.colors(palette.colors, from: end, to: .srgb,
                                                          options: Self.artworkColorOptions)
        if !palette.stopHexes.isEmpty { appearance = palette.stopHexes.map(CmykArtworkHex.unit) }
        return description.repainted(appearance, appearance: appearance, ink: .srgb)
    }

    /// Each ink of an ink-form `description` paired with its appearance (`cmykDisplay`'s numbers;
    /// a gradient stop's stored hex/255); [] for any other description.
    func cmykMatches(of description: LayerDescription) throws -> [CmykInkMatch] {
        guard description.artworkInk == .cmyk, case .cmyk = artworkColorEnd else { return [] }
        let palette = description.palette
        let display = try cmykDisplay(description).palette
        return zip(display.colors, palette.colors).map { CmykInkMatch(srgb: $0, ink: $1) }
    }

    /// An editor's commit in a CMYK document (module comment): `description` in ink, the
    /// swatches' stored inks and — when it replaces layer `replacing` — that layer's own inks among
    /// the matches, so the converted description is what is recorded and rendered. Any other
    /// document's description as it is.
    func inkedArtwork(_ description: LayerDescription, replacing: Int?,
                      swatches: [CmykInkMatch]) throws -> LayerDescription {
        guard isCmyk else { return description }
        let previous = replacing.flatMap { LayerDescription.decode(layerMeta($0)) }
        let own = try previous.map { try cmykMatches(of: $0) } ?? []
        return try cmykArtwork(description, matching: swatches + own, previous: previous,
                               options: Self.artworkColorOptions)
    }

    /// Layer `idx`'s description as an editor opens it: its sRGB appearance in a CMYK document
    /// (`cmykDisplay`); the stored one otherwise. nil for a plain layer, or an ink the display leg
    /// cannot convert.
    func displayedDescription(_ idx: Int) -> LayerDescription? {
        guard let description = layerDescription(idx) else { return nil }
        return isCmyk ? try? cmykDisplay(description) : description
    }

    /// `displayedDescription` of a text, shape or fill layer.
    func displayedTextPayload(_ idx: Int) -> TextLayerPayload? {
        guard case let .text(payload)? = displayedDescription(idx) else { return nil }
        return payload
    }

    func displayedShapePayload(_ idx: Int) -> ShapeLayerPayload? {
        guard case let .shape(payload)? = displayedDescription(idx) else { return nil }
        return payload
    }

    func displayedFillPayload(_ idx: Int) -> FillLayerPayload? {
        guard case let .fill(payload)? = displayedDescription(idx) else { return nil }
        return payload
    }

    /// Every described layer, captured for a conversion (`reinkingArtwork`). Throws, before any
    /// work, when one cannot be re-rendered here (a missing font), as `artworkProfile` does.
    func artworkCaptures() throws -> [ArtworkCapture] {
        try artworkIndices.map { index in
            guard let description = LayerDescription.decode(layerMeta(index)), description.isRenderable,
                  let anchor = describedAnchor(index), let info = layerInfo(index) else {
                throw NativeArtworkEvaluation.failure(
                    "A required artwork font or description is unavailable; the conversion was not applied.")
            }
            return ArtworkCapture(index: index, description: description.resolved(rasterWidth: info.width),
                                  anchor: anchor, name: info.name)
        }
    }

    /// This converted document with each capture re-rendered (module comment): its colours
    /// converted from `source` into this document's end under `options`, then published over its
    /// own record, mask kept.
    func reinkingArtwork(_ captures: [ArtworkCapture], from source: ArtworkColorEnd,
                         options: CmykConversionOptions) throws -> RasterDocument {
        var result = self
        for capture in captures {
            let converted = try result.convertedArtwork(capture.description, from: source, options: options)
            if let updated = try result.nativeArtwork(converted, anchor: capture.anchor, layer: capture.index,
                                                      replacing: true, name: capture.name) {
                result = updated
            }
        }
        return result
    }

    /// `description`'s colours from `source` into this document's end.
    func convertedArtwork(_ description: LayerDescription, from source: ArtworkColorEnd,
                          options: CmykConversionOptions) throws -> LayerDescription {
        let destination = artworkColorEnd
        switch (source, destination) {
        case (.srgb, .cmyk):
            return try cmykArtwork(description, matching: [], previous: nil, options: options)
        case (.cmyk, .srgb):
            guard description.artworkInk == .cmyk else { return description }
            let palette = description.palette
            var rgb = try RasterCmykArtworkCore.colors(palette.colors, from: source, to: .srgb, options: options)
            // A gradient stop converts as a colour, and its hex is the converted colour's.
            if !palette.stopHexes.isEmpty { rgb = rgb.map { CmykArtworkHex.unit(CmykArtworkHex.six($0)) } }
            return description.repainted(rgb, appearance: rgb, ink: .srgb)
        case (.cmyk, .cmyk):
            guard description.artworkInk == .cmyk, source != destination else { return description }
            let inks = try RasterCmykArtworkCore.colors(description.palette.colors, from: source,
                                                        to: destination, options: options)
            return try inkForm(description, inks: inks, end: destination)
        case (.srgb, .srgb):
            return description
        }
    }

    /// `nativeArtwork`'s CMYK arm (NativeArtworkEvaluation.swift): the description sized and
    /// placed as the RGB arm does, separated into ink (`cmykArtwork`, the replaced layer's own inks
    /// among the matches), captured on the main thread and assembled and published by the core.
    func cmykNativeArtwork(_ description: LayerDescription, anchor: CGPoint, layer: Int, replacing: Bool,
                           name: String, position: LayerInsertPosition,
                           matching: [CmykInkMatch]) throws -> RasterDocument? {
        guard let precision else { throw NativeArtworkEvaluation.failure("Ink artwork needs a CMYK document.") }
        let previous = replacing ? LayerDescription.decode(layerMeta(layer)) : nil
        let own = try previous.map { try cmykMatches(of: $0) } ?? []
        let placed = description.fitted(toCanvas: (width, height))
            .withOriginFraction(DescribedLayer.anchorFraction(anchor))
        let inked = try cmykArtwork(placed, matching: matching + own, previous: previous,
                                    options: Self.artworkColorOptions)
        guard let json = inked.json(), json.utf8.count <= NativeArtwork.maximumDescriptionBytes else {
            throw NativeArtworkEvaluation.failure("Artwork needs a bounded description.")
        }
        let profile = precision.cmykProfileData
        // A replaced layer keeps its reference point; a new one is made at (0, 0) (the RGB arm's
        // rule: a linked pattern fill follows it).
        let reference = replacing ? layerReference(layer) : (x: 0, y: 0)
        // An aligned gradient fill spans the replaced layer's mask box (the RGB arm's rule).
        let alignBox = replacing ? fillAlignBox(layer) : nil
        let evaluation = try NativeArtworkEvaluation(
            NativeArtwork.drawing(inked, anchor: anchor, ink: .cmyk, reference: reference,
                                  alignBox: alignBox),
            float: false, cmyk: true)
        let metadata = try PrecisionEditPayload.admitted(
            bytes: UInt64(json.utf8.count) * 2 + UInt64(profile.count) * 2 + 4096)
        let pixels = UInt64(evaluation.width * evaluation.height)
        let scratch = evaluation.scratchBytes + NativeArtworkEvaluation.cmykMarginPerPixel * pixels
            + UInt64(profile.count) * 64
        CmykArtworkPlan.lastBytes = scratch
        let steps = Double(evaluation.parts.count + 2)
        return try withExtendedLifetime(metadata) {
            try PrecisionCapturedWork.run(scratchEstimate: scratch, title: "Rendering Retained Artwork") { control in
                let parts = try evaluation.components(control: control)
                return try control.withChildOperation(from: (steps - 1) / steps, to: 1) { child in
                    guard let published = try precision.publishingCmykArtwork(
                        layer: layer, replacing: replacing, position: position,
                        width: evaluation.width, height: evaluation.height, components: parts.json,
                        samples: parts.samples, intrinsic: parts.intrinsic,
                        x: evaluation.offsetX, y: evaluation.offsetY, name: name, description: json,
                        control: child) else { return nil }
                    return try RasterDocument(precision: published)
                }
            }
        }
    }
}
