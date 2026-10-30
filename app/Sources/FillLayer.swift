import AppKit

// Solid Color and Gradient FILL LAYERS: a layer whose pixels are the
// rendering of one colour, or of one gradient, over the whole canvas. It is
// an artwork description like text and shape — the description rides the
// layer's `meta` slot at 8 bits and the typed description table at 16 and 32,
// the pixels are rendered in the core at the document's depth
// (`artwork_render`'s `fill` and `gradient` components), and the layer keeps
// an ordinary mask — so it saves in `.rz`, reopens, and is re-edited from the
// Layers panel or with `edit_fill_layer`.
//
// WHAT IS DIFFERENT FROM TEXT AND SHAPE. A fill layer has no position and no
// shape of its own: it always covers the canvas `(0, 0, width, height)`.
// So its description carries no transform and no anchor fraction; it carries
// the canvas size it was rendered for (`canvas`), and three rules keep the
// pixels and that size true:
//
// - REFIT. Every op that changes the canvas re-renders each fill layer to the
//   new canvas where the result is computed: crop and Canvas Size
//   (`RasterDocument.cropped` / `canvasResized`), a pasted or dropped layer
//   (`LayerPasteWork`), rotate, flip and Image Size
//   (`artworkDocumentGeometry`), straighten and Rotate Canvas
//   (`ArtworkGeometryWork`). `refittingFillLayers` is the main-thread form;
//   the worker paths capture the same drawing before they start.
// - TURN. A quarter turn or a flip turns a gradient with the canvas
//   (`composed(with:)`), and a LINKED pattern fill's tile
//   (`turningTile`); any other map leaves both alone, and the fill spans the
//   new canvas.
// - PIN. A solid or gradient fill layer cannot be moved or transformed: the
//   shared position refusals name it (`positionPin`). The core would still
//   move its raster if something reached it; the next refit puts it back. A
//   PATTERN fill moves and transforms: every move and Free
//   Transform commit re-renders it over the canvas — at 8 bits `moveLayers`
//   and `withLayerOffset` refit their result and the gestures refit before
//   `endLiveEdit`; at 16 and 32 bits `geometrySet` and `PrecisionGeometryWork`
//   capture its drawing on main (`carriesPatternFill`) and `prepareAfter`
//   redraws it at the moved reference — so its mask moves and a linked tile
//   follows the core-mapped reference point; an unlinked one stays on the
//   canvas origin. It stays pinned when a lock forbids the re-render, and on
//   Align and Distribute (`refitting: false`), which never re-render; a solid
//   or gradient fill moving with it pins the whole move.
//
// COLOUR. The description's colours are AUTHORED sRGB at every depth: a solid
// fill's `color` is `[r, g, b, a]` in 0…1 and a gradient's stops are the
// layer-style `#rrggbb` with an opacity. The core converts them into the
// document's profile when it renders; in a 32-bit float document the host
// first re-codes them to linear light (`ArtworkInk.converted`), so a gradient
// there interpolates between linear stops, as a layer style's does. A solid
// fill imported from a Photoshop `SoCo` whose colour lies outside sRGB
// also keeps `linear`: that colour as linear-light sRGB components beyond
// 0…1 (a style colour's wide carriage), which the drawing hands the
// core in `color`'s place; `color` is then its clipped sRGB stand-in. Any
// edit of `color` drops it (`color`'s `didSet`), so the sheet, the
// `edit_fill_layer` tool and the CMYK conversion all leave an ordinary
// colour; it is never written in the ink form. A gradient fill imported from a
// Photoshop `GdFl` whose stops leave sRGB keeps `stop_linears`
// the same way: one linear sRGB triple per stop in STORED order, each stop's
// `#rrggbb` its clipped stand-in, handed to the core in the drawing's stop
// order. Any change to the stops drops them (`gradient`'s `didSet`) — an
// angle, style, scale or reverse edit keeps them — and the CMYK conversion
// drops them (`CmykArtwork.repainted`); never in the ink form.
//
// PATTERN. A third kind, `"fill":"pattern"`, tiles one pattern
// over the canvas: `pattern_fill` holds the Pattern Overlay's keys (`PatternFill`,
// PatternOverlay.swift: the tile as a preset id or an inline custom tile,
// angle, scale, link and phase), never an `anchor`. Placement follows the
// layer's reference point (`fxrp`, `RasterDocument.layerReference`): linked,
// the tile's origin is phase + reference, which the drawing writes as the
// component's `anchor` = −reference, read at render; unlinked, it is phase.
// The tile's codes are DOCUMENT values (code / 255 is linear at 32 bits), so
// an RGB pattern fill has no ink conversion; in a CMYK document
// the description stays this version-1 one and the core separates each
// texel to ink at render, as a Pattern Overlay's (INK, below). A custom tile
// can take the description past 64 KiB, which `.rz` word 102 carries (up to
// `NativeArtwork.maximumDescriptionBytes`). A canvas quarter turn or flip
// moves a linked tile's origin with the mapped reference AND turns the tile
// (`composed(with:turningTile:)` through
// `RasterDocument.turnedPattern`, the core's one turn a linked Pattern
// Overlay takes: a custom tile under the same name). An unlinked fill keeps
// its tile. A Free Transform keeps the description (KEEP): the tile's Scale
// and Angle stay as they are and the fill re-renders over the canvas (the
// Photoshop take: FT-S and FT-R left `Scl ` and `Angl` unchanged and read 0
// codes from its render, and FT-F matched the unturned tile).
//
// ALIGN. A gradient fill's `align_with_layer` (Align with Layer; a
// new fill starts false, an imported `GdFl` keeps its `Algn`) makes
// the gradient span the layer's mask box instead of the canvas: the bounds of
// the mask's coverage clipped to the canvas, else the canvas — the core's one
// rule (`fill_align_box`), asked through `RasterDocument.fillAlignBox` at
// every render (the dialog and `edit_fill_layer`, a refit, a geometry op, the
// CMYK conversion) and handed to the core as the component's `box`. A mask
// edit (paint, enable, add, delete) does not re-render the fill; its next
// render reads the mask. Smoothness (`smoothness`, nil at 100 %) rides
// in the gradient object like every other key.
//
// INK (#547, version 2). In a CMYK document the description's colours are ink
// in the document's profile: `"ink":"cmyk"`, a solid `color` of
// `[c, m, y, k, a]`, and a gradient's `stop_inks`, one `[c, m, y, k]` per
// stop in STORED order (`stop_inks[i]` is `stops[i]`'s), each stop's `#rrggbb`
// then being that ink's sRGB appearance through the profile
// (CmykArtwork.swift). The drawing lists the inks through
// `orderedStopOffsets`, the same order as its stops. A pattern fill has no
// ink form: it stays version 1 and its texels separate at render through the
// Pattern Overlay's leg (`cmyk_style::TexelInk`), so Image ▸ Mode carries it
// both ways unchanged.

/// A fill layer's description, as its meta stores it:
/// `{"type":"fill","version":1,"fill":"solid","color":[r,g,b,a],"canvas":[w,h]}`
/// or `{…,"fill":"gradient","gradient":{GradientFill},"canvas":[w,h]}`, the
/// gradient object in the spelling `LayerStyle.swift` writes
/// (`align_with_layer`), which is the object the core's style parser reads.
struct FillLayerPayload: Codable, Equatable {
    static let typeName = "fill"
    static let currentVersion = 1
    /// The ink form (#547): `ink` "cmyk", 5-number `color`, `stop_inks`.
    static let cmykVersion = 2
    static let solid = "solid"
    static let gradientKind = "gradient"
    static let patternKind = "pattern"

    var type = FillLayerPayload.typeName
    var version = FillLayerPayload.currentVersion
    /// "solid" | "gradient" | "pattern".
    var fill: String
    /// Solid only: authored sRGB `[r, g, b, a]`, each 0…1. Changing it drops
    /// `linear` (COLOUR, above).
    var color: [Double]? {
        didSet { if color != oldValue { linear = nil } }
    }
    /// Version-1 solid only (COLOUR, above): the colour as linear sRGB
    /// `[r, g, b]` beyond sRGB, each finite within ±4.
    var linear: [Double]?
    /// Gradient only. Spans the canvas, or with `alignWithLayer` the layer's
    /// mask box (ALIGN, above). Changing
    /// its stops drops `stopLinears` (COLOUR, above); an angle, style, scale,
    /// reverse or midpoint edit keeps them.
    var gradient: GradientFill? {
        didSet {
            if gradient?.stops.map(\.withoutMidpoints) != oldValue?.stops.map(\.withoutMidpoints) {
                stopLinears = nil
            }
        }
    }
    /// Version-1 gradient only (COLOUR, above): one linear sRGB
    /// `[r, g, b]` per stop in STORED order, each finite within ±4.
    var stopLinears: [[Double]]?
    /// Pattern only (JSON `pattern_fill`; PATTERN, above): the overlay's fill
    /// keys, `anchor` always nil.
    var patternFill: PatternFill?
    /// The canvas `[width, height]` the pixels were rendered for.
    var canvas: [Int]
    /// Version 2 only: `.cmyk` (INK, above); absent at version 1.
    var ink: ArtworkInk?
    /// Version 2 gradient only: one `[c, m, y, k]` per stop, stored order.
    var stopInks: [[Double]]?

    init(color: [Double], canvas: (width: Int, height: Int)) {
        fill = Self.solid
        self.color = color
        gradient = nil
        self.canvas = [canvas.width, canvas.height]
    }

    init(gradient: GradientFill, canvas: (width: Int, height: Int)) {
        fill = Self.gradientKind
        color = nil
        // The align flag is kept (ALIGN, above); a new fill passes false.
        self.gradient = gradient
        self.canvas = [canvas.width, canvas.height]
    }

    init(pattern: PatternFill, canvas: (width: Int, height: Int)) {
        fill = Self.patternKind
        color = nil
        gradient = nil
        var stored = pattern
        // Placement follows the layer's reference at render (PATTERN, above).
        stored.anchor = nil
        patternFill = stored
        self.canvas = [canvas.width, canvas.height]
    }

    /// Strict, non-throwing decode: anything that is not a well-formed fill
    /// description means "not a fill layer", never an error.
    static func decode(_ meta: String) -> FillLayerPayload? {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        guard let data = meta.data(using: .utf8),
              let payload = try? decoder.decode(FillLayerPayload.self, from: data),
              payload.type == typeName,
              payload.version == currentVersion
                ? payload.ink == nil && payload.stopInks == nil
                : payload.version == cmykVersion && payload.ink == .cmyk,
              payload.canvas.count == 2, payload.canvas.allSatisfy({ $0 >= 1 })
        else { return nil }
        let inkForm = payload.ink == .cmyk
        switch payload.fill {
        case solid:
            guard payload.gradient == nil, payload.stopInks == nil, payload.patternFill == nil,
                  payload.stopLinears == nil,
                  let color = payload.color,
                  ArtworkColor.valid(color, in: inkForm ? .cmyk : .srgb),
                  payload.linear.map({ !inkForm && validLinear($0) }) ?? true
            else { return nil }
        case gradientKind:
            guard payload.color == nil, payload.patternFill == nil, payload.linear == nil,
                  let gradient = payload.gradient,
                  (GradientEditorView.minStops...GradientEditorView.maxStops)
                      .contains(gradient.stops.count),
                  GradientFill.styles.contains(gradient.style),
                  gradient.angle.isFinite, gradient.scale.isFinite,
                  gradient.smoothness.map({ $0.isFinite && (0...1).contains($0) }) ?? true,
                  gradient.stops.allSatisfy({
                      $0.position.isFinite && (0...1).contains($0.position)
                          && $0.opacity.isFinite && (0...1).contains($0.opacity)
                          && LayerStyleColor.color(fromHex: $0.color) != nil
                          && [$0.midpoint, $0.opacityMidpoint].allSatisfy { m in
                              m.map { GradientStop.midpointRange.contains($0) } ?? true
                          }
                  }),
                  !inkForm || payload.stopInks.map({ inks in
                      inks.count == gradient.stops.count
                          && inks.allSatisfy { ArtworkColor.valid($0 + [1], in: .cmyk) }
                  }) == true,
                  payload.stopLinears.map({ linears in
                      !inkForm && linears.count == gradient.stops.count
                          && linears.allSatisfy(validLinear)
                  }) ?? true
            else { return nil }
        case patternKind:
            // Version 1 at every ink (PATTERN and INK, above).
            guard !inkForm, payload.color == nil, payload.gradient == nil, payload.linear == nil,
                  payload.stopLinears == nil,
                  let pattern = payload.patternFill, validPattern(pattern)
            else { return nil }
        default:
            return nil
        }
        return payload
    }

    /// A solid's wide colour: three finite components within ±4, the
    /// core's `authored_color::LINEAR_LIMIT`.
    static func validLinear(_ linear: [Double]) -> Bool {
        linear.count == 3 && linear.allSatisfy { $0.isFinite && abs($0) <= 4 }
    }

    /// A pattern fill as a fill layer stores it: a known preset id alone, or
    /// a custom tile with sides 1…512 whose `rgba` is exactly the standard
    /// base64 length of w × h × 4 bytes; finite angle and phase, scale
    /// 0.01…10, and no anchor.
    static func validPattern(_ fill: PatternFill) -> Bool {
        guard let pattern = fill.pattern, fill.anchor == nil, fill.angle.isFinite,
              (0.01...10).contains(fill.scale), fill.phase.count == 2,
              fill.phase.allSatisfy(\.isFinite)
        else { return false }
        if let preset = pattern.preset {
            return pattern.name == nil && pattern.width == nil && pattern.height == nil
                && pattern.rgba == nil
                && RasterDocument.patternPresets().contains { $0.id == preset }
        }
        guard let width = pattern.width, let height = pattern.height, let rgba = pattern.rgba,
              (1...512).contains(width), (1...512).contains(height)
        else { return false }
        return rgba.utf8.count == (width * height * 4 + 2) / 3 * 4
    }

    /// The JSON to store as the layer's meta. Keys are sorted, so re-encoding
    /// an unchanged description produces identical bytes.
    func json() -> String? {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        encoder.keyEncodingStrategy = .convertToSnakeCase
        guard let data = try? encoder.encode(self) else { return nil }
        return String(data: data, encoding: .utf8)
    }

    var canvasSize: (width: Int, height: Int) { (canvas[0], canvas[1]) }

    /// The description after the canvas went through `map` (TURN, above).
    /// Only a gradient under one of the eight exact forms changes: its angle
    /// becomes that of its mapped direction — angle `a` points along
    /// `(cos a, −sin a)` on the y-down canvas — and an `angle`-style sweep,
    /// which a reflection runs the other way round, also toggles `reverse`.
    /// A solid fill, the identity and any other map answer `self`. A pattern
    /// fill's tile origin moves with the mapped reference point; with
    /// `turningTile` (a canvas quarter turn or flip) a LINKED
    /// fill's tile also becomes its turned copy (`RasterDocument.turnedPattern`,
    /// a linked Pattern Overlay's own turn). A Free Transform, an unlinked fill
    /// and any other map keep the tile (KEEP).
    func composed(with map: LinearMap, turningTile: Bool = false) -> FillLayerPayload {
        if turningTile, var pattern = patternFill, pattern.linkWithLayer,
           let tile = pattern.pattern,
           let turned = RasterDocument.turnedPattern(tile, by: map) {
            pattern.pattern = turned
            var out = self
            out.patternFill = pattern
            return out
        }
        guard var gradient, map.isExactForm, !map.isIdentity else { return self }
        let radians = gradient.angle * .pi / 180
        let direction = map.apply(CGPoint(x: cos(radians), y: -sin(radians)))
        let degrees = atan2(-Double(direction.y), Double(direction.x)) * 180 / .pi
        // Four decimals, the precision the core's angle is quantized at; an
        // exact form takes a whole angle to a whole angle.
        gradient.angle = LinearMap.snap((degrees * 1e4).rounded() / 1e4)
        if gradient.style == "angle", map.determinant < 0 { gradient.reverse.toggle() }
        var out = self
        out.gradient = gradient
        return out
    }

    /// The stored index of each stop in position order, ties in stored order
    /// — the order the core's parser sorts them into, and the order a render
    /// component must list them (and a CMYK fill's `stop_inks`) in.
    var orderedStopOffsets: [Int] {
        (gradient?.stops ?? []).enumerated().sorted {
            $0.element.position == $1.element.position
                ? $0.offset < $1.offset : $0.element.position < $1.element.position
        }.map(\.offset)
    }

    /// The stops in `orderedStopOffsets`' order.
    var orderedStops: [GradientStop] {
        let stops = gradient?.stops ?? []
        return orderedStopOffsets.map { stops[$0] }
    }
}

extension GradientFill {
    /// This fill as the JSON object the core's style parser reads, its stops
    /// in `ordered` order: what a `gradient` render component carries, with
    /// `smoothness` and each stop's midpoints when set (the box itself
    /// is the component's, ALIGN above).
    func componentObject(stops ordered: [GradientStop]) -> [String: Any] {
        var object: [String: Any] = [
            "stops": ordered.map { stop -> [String: Any] in
                var entry: [String: Any] = [
                    "position": stop.position, "color": stop.color, "opacity": stop.opacity,
                ]
                if let midpoint = stop.midpoint { entry["midpoint"] = midpoint }
                if let opacity = stop.opacityMidpoint { entry["opacity_midpoint"] = opacity }
                return entry
            },
            "style": style, "angle": angle, "scale": scale, "reverse": reverse,
            "align_with_layer": alignWithLayer,
        ]
        if let smoothness { object["smoothness"] = smoothness }
        return object
    }
}

enum FillLayer {
    /// The names a new fill layer takes.
    static func layerName(for payload: FillLayerPayload) -> String {
        switch payload.fill {
        case FillLayerPayload.gradientKind: return "Gradient Fill"
        case FillLayerPayload.patternKind: return "Pattern Fill"
        default: return "Color Fill"
        }
    }

    /// The destructive-edit prompt, in a fill layer's words (the shape
    /// layer's, `ShapeLayer.confirmRasterize`). True to proceed.
    static func confirmRasterize(layerName: String, reason: String? = nil) -> Bool {
        let alert = NSAlert()
        alert.messageText = "Rasterize fill layer?"
        var text =
            "This edit paints over “\(layerName)”, so the layer will no longer be a fill "
            + "layer: its color, gradient or pattern can no longer be edited, and it stops following "
            + "the canvas. The pixels themselves are kept. To choose where the fill shows, "
            + "paint on its mask instead."
        if let reason = reason { text += "\n\n" + reason }
        alert.informativeText = text
        alert.addButton(withTitle: "Rasterize")
        alert.addButton(withTitle: "Cancel")
        return alert.runModal() == .alertFirstButtonReturn
    }

    /// The native drawing of a fill layer: one plane-less part covering the
    /// description's canvas at offset (0, 0). `ink` is the document's
    /// (`RasterDocument.artworkInk`): at 32 bits the authored sRGB numbers
    /// are re-coded to linear light first. `reference` is the layer's
    /// reference point (`layerReference`), which a linked pattern's origin
    /// follows (PATTERN, above). `alignBox` is the layer's Align with Layer
    /// box (`RasterDocument.fillAlignBox`, ALIGN above): an aligned gradient
    /// spans it; nil, or any other fill, spans the canvas. Pure: callable off
    /// the main thread.
    static func drawing(_ payload: FillLayerPayload, ink: ArtworkInk,
                        reference: (x: Int, y: Int),
                        alignBox: [Int]? = nil) throws -> NativeArtworkDrawing {
        let size = payload.canvasSize
        let box = payload.gradient?.alignWithLayer == true ? alignBox : nil
        let part: NativeArtworkPart
        if let pattern = payload.patternFill {
            part = .pattern(patternComponent(pattern, reference: reference))
        } else if let color = payload.color {
            // An ink colour is drawn as it is stored (INK, above); a wide
            // colour is linear light already (COLOUR, above).
            part = payload.ink == .cmyk
                ? .fill(rgba: color, linear: nil)
                : .fill(rgba: ArtworkInk.converted(color, from: .srgb, to: ink), linear: payload.linear)
        } else if let gradient = payload.gradient, payload.ink == .cmyk {
            let offsets = payload.orderedStopOffsets
            guard let inks = payload.stopInks, inks.count == gradient.stops.count else {
                throw NativeArtworkEvaluation.failure("A gradient fill's stop inks do not match its stops.")
            }
            part = .gradient(gradient.componentObject(stops: offsets.map { gradient.stops[$0] }),
                             colors: offsets.map { inks[$0] }, linear: nil, box: box)
        } else if let gradient = payload.gradient {
            let ordered = payload.orderedStops
            let colors = try ordered.map { stop -> [Double] in
                guard let color = LayerStyleColor.color(fromHex: stop.color),
                      let rgba = ArtworkColor.rgba(color.withAlphaComponent(1))
                else { throw NativeArtworkEvaluation.failure("A gradient stop's color is invalid.") }
                return Array(ArtworkInk.converted(rgba, from: .srgb, to: ink).prefix(3))
            }
            // Wide stops are linear light already (COLOUR, above), listed in
            // the same order as the stops.
            var linear: [[Double]]?
            if let linears = payload.stopLinears {
                guard linears.count == gradient.stops.count else {
                    throw NativeArtworkEvaluation.failure("A gradient fill's wide stop colours do not match its stops.")
                }
                linear = payload.orderedStopOffsets.map { linears[$0] }
            }
            part = .gradient(gradient.componentObject(stops: ordered), colors: colors, linear: linear,
                             box: box)
        } else {
            throw NativeArtworkEvaluation.failure("The fill layer has no color or gradient.")
        }
        return NativeArtworkDrawing(
            width: size.width, height: size.height, offsetX: 0, offsetY: 0, parts: [part])
    }

    /// A `pattern` render component's `fill` object (`artwork_render`'s
    /// "Pattern fill layers"): the stored keys, plus `anchor` = −reference
    /// when linked, so the core places the tile at phase + reference.
    static func patternComponent(_ fill: PatternFill, reference: (x: Int, y: Int)) -> [String: Any] {
        var pattern: [String: Any] = [:]
        if let preset = fill.pattern?.preset {
            pattern["preset"] = preset
        } else if let ref = fill.pattern {
            pattern["name"] = ref.name ?? "Custom"
            pattern["width"] = ref.width ?? 0
            pattern["height"] = ref.height ?? 0
            pattern["rgba"] = ref.rgba ?? ""
        }
        var object: [String: Any] = [
            "angle": fill.angle, "scale": fill.scale,
            "link_with_layer": fill.linkWithLayer, "phase": fill.phase,
        ]
        if !pattern.isEmpty { object["pattern"] = pattern }
        if fill.linkWithLayer { object["anchor"] = [-reference.x, -reference.y] }
        return object
    }
}

extension LayerDescription {
    /// A fill description sized for `toCanvas` (REFIT, above); every other
    /// kind unchanged.
    func fitted(toCanvas canvas: (width: Int, height: Int)) -> LayerDescription {
        guard case var .fill(payload) = self else { return self }
        payload.canvas = [canvas.width, canvas.height]
        return .fill(payload)
    }
}

extension RasterDocument {
    /// The indices of this document's fill layers, by the `type` each meta
    /// claims.
    var fillIndices: [Int] {
        (0..<layerCount).filter {
            // At 16 and 32 bits the description table says, without a read,
            // which records carry one at all.
            (!isHighDepth || hasArtworkDescription($0))
                && LayerDescription.claimedKind(of: layerMeta($0)) == .fill
        }
    }

    /// Layer `idx`'s fill description, or nil for any other layer.
    func fillPayload(_ idx: Int) -> FillLayerPayload? {
        layerMeta(idx).flatMap(FillLayerPayload.decode)
    }

    /// Whether fill layer `idx` (with `payload`) does not cover this canvas:
    /// its description was sized for another canvas, or its raster is not the
    /// canvas rect.
    private func fillNeedsRefit(_ idx: Int, _ payload: FillLayerPayload) -> Bool {
        guard let info = layerInfo(idx) else { return false }
        return payload.canvas != [width, height] || info.offsetX != 0 || info.offsetY != 0
            || info.width != width || info.height != height
    }

    /// This document with every fill layer that does not cover the canvas
    /// re-rendered to it (REFIT, above); `self` when none needs it. Main
    /// thread only: it renders through `nativeArtwork`.
    ///
    /// Before any work it throws, naming the layer, when a fill layer that
    /// needs the re-render is locked against it: Lock Pixels or Lock All
    /// refuse a re-render outright, and Lock Transparency refuses one that
    /// changes the raster's size. A re-render the core answers nil for (the
    /// pixels and description are already right) keeps the document.
    func refittingFillLayers() throws -> RasterDocument {
        let stale = fillIndices.compactMap { index -> (Int, FillLayerPayload)? in
            guard let payload = fillPayload(index), fillNeedsRefit(index, payload) else {
                return nil
            }
            return (index, payload)
        }
        guard !stale.isEmpty else { return self }
        for (index, _) in stale
        where lockRefusal(index, kind: DR_EDIT_PIXELS) != nil
            || transparencyLockOwner(index) != nil {
            let name = layerInfo(index)?.name ?? "layer \(index)"
            throw DoctoredCoreError(message: "“\(name)” is a fill layer and is locked, so it "
                + "cannot be re-rendered to cover the new canvas. Unlock it in Layer ▸ Lock "
                + "first. Nothing was changed.")
        }
        var current = self
        for (index, payload) in stale {
            guard let info = current.layerInfo(index) else { continue }
            if let updated = try current.nativeArtwork(
                .fill(payload), anchor: .zero, layer: index, replacing: true, name: info.name) {
                current = updated
            }
        }
        // Layer comps recorded the offsets the core op left; the fills are
        // back at (0, 0), and the comps follow (`followingHostPlacement`).
        return try current.followingHostPlacement(of: stale.map(\.0), placedBy: self)
    }

    /// The refusal a move or transform of layer `idx` gets when it is a fill
    /// layer (PIN, above); nil for any other layer. `refitting` false is a
    /// path that does not re-render (Align, Distribute). A solid or gradient
    /// fill answers its pin either way; a pattern fill answers nil on a
    /// refitting path unless Lock Pixels, Lock All or Lock Transparency
    /// forbids the re-render, and its own sentence on any other path.
    func positionPin(_ idx: Int, refitting: Bool = true) -> String? {
        guard LayerDescription.claimedKind(of: layerMeta(idx)) == .fill else { return nil }
        let name = layerInfo(idx)?.name ?? "This layer"
        guard fillPayload(idx)?.fill == FillLayerPayload.patternKind else {
            return "“\(name)” is a fill layer: it always covers the canvas, so it cannot be "
                + "moved or transformed."
        }
        guard refitting else {
            return "“\(name)” is a pattern fill layer: it always covers the canvas, so Align "
                + "and Distribute do not move it. Move it with the Move tool or move_layers "
                + "instead."
        }
        guard lockRefusal(idx, kind: DR_EDIT_PIXELS) != nil || transparencyLockOwner(idx) != nil
        else { return nil }
        return "“\(name)” is a pattern fill layer and is locked, so it cannot be re-rendered "
            + "after the move. Unlock it in Layer ▸ Lock first. Nothing was changed."
    }

    /// The first non-nil `positionPin` over every fill layer a move of
    /// `layers` would carry — the layers themselves, a group's children,
    /// link partners (`movingSet`) — so a solid or gradient fill moving with
    /// a pattern fill still pins the whole move. nil when none pins it.
    func positionPin(moving layers: [Int], refitting: Bool = true) -> String? {
        let fills = Set(fillIndices)
        guard !fills.isEmpty else { return nil }
        return movingSet(layers).lazy.filter(fills.contains)
            .compactMap { self.positionPin($0, refitting: refitting) }.first
    }

    /// Whether a move of `layers` carries a pattern fill layer (`movingSet`),
    /// which the 16/32-bit move paths re-render (PIN, above).
    func carriesPatternFill(moving layers: [Int]) -> Bool {
        let fills = Set(fillIndices)
        guard !fills.isEmpty else { return false }
        return movingSet(layers).contains {
            fills.contains($0) && fillPayload($0)?.fill == FillLayerPayload.patternKind
        }
    }
}
