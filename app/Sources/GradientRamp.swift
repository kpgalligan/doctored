import AppKit

/// The Gradient tool's ramp as the options bar resolves it: the stops
/// a drag paints, in order, and the shape it paints them in. One value for
/// every depth and target, so the layer, plane, 16-bit and Quick Mask commits
/// and the bar's preview strip cannot disagree about what the bar says.
///
/// The rules, applied in this order:
/// - `options.stops == nil` is Foreground → Background, the two rail
///   swatches with their own alpha; otherwise each stored stop is its
///   authored sRGB colour at its own opacity.
/// - Transparency off paints every stop opaque.
/// - Reverse mirrors the positions (`p → 1 − p`) and the order with them.
/// - The bar's Opacity multiplies every stop's alpha.
/// - Each stop carries its colour and opacity midpoints toward the next stop
///   (`midpoints`, parallel to `stops`; 0.5 when the stored stop has
///   none); Reverse turns stop `k`'s into `1 − m` of the stop that came
///   before it, and the last stop's is 0.5.
/// - Smoothness (`smoothness`) is the stored fraction, non-finite read
///   as 1 and clamped to 0…1; Reverse leaves it alone. The core blends the
///   linear and Classic ramps by it (`gradient_field::smooth_color`).
///
/// `isLegacyTwoStop` — two stops at 0 and 1 in a linear or radial shape,
/// their midpoints at 0.5, Smoothness at 1 — is the gradient the tool painted
/// before it had stops. It keeps that path
/// exactly: the two-colour core exports, the two-colour typed request and the
/// recorded `start_color` / `end_color` step, so every recorded action and
/// every pixel a two-colour drag wrote is unchanged. Anything else takes the
/// stops path and records `stops` and `shape`.
struct GradientRamp {
    /// A stop's colour and opacity midpoints toward the next stop, as
    /// fractions; 0.5 is the default 50 %.
    struct StopMidpoint: Equatable {
        var color = 0.5
        var opacity = 0.5

        /// The entries that are not 0.5, as a stop's JSON keys (`midpoint`,
        /// `opacity_midpoint`): what a request or a recorded step carries.
        var keys: [String: Any] {
            var out: [String: Any] = [:]
            if color != 0.5 { out["midpoint"] = color }
            if opacity != 0.5 { out["opacity_midpoint"] = opacity }
            return out
        }
    }

    var stops: [(position: Double, color: NSColor)]
    /// Each stop's midpoints, parallel to `stops`.
    var midpoints: [StopMidpoint]
    var style: DrGradientKind
    /// Smoothness as a fraction; 1 is the Classic ramp.
    var smoothness: Double
    var isLegacyTwoStop: Bool

    /// `shape`'s five names, indexed by `DrGradientKind` raw value — the MCP
    /// `gradient` tool's vocabulary and the core's `FieldStyle` spelling.
    static let shapeNames = ["linear", "radial", "angle", "reflected", "diamond"]

    /// Why a non-Normal Mode refuses on a coverage target, in the
    /// options bar's alert and the MCP `gradient` tool's reply alike.
    static let coverageTargetRefusal = "Gradient's blend mode applies to a layer's pixels. "
        + "A mask, colour plane, channel or the Quick Mask holds coverage: use Normal there."

    static func kind(named name: String) -> DrGradientKind? {
        shapeNames.firstIndex(of: name).map { DrGradientKind(rawValue: UInt32($0)) }
    }

    /// A ramp given outright — the MCP `gradient` tool's, already resolved;
    /// `midpoints` nil is every stop at 0.5; `smoothness` a fraction 0…1.
    init(
        stops: [(position: Double, color: NSColor)], style: DrGradientKind,
        midpoints: [StopMidpoint]? = nil, smoothness: Double = 1
    ) {
        self.stops = stops
        self.midpoints = midpoints ?? Array(repeating: StopMidpoint(), count: stops.count)
        self.style = style
        self.smoothness = smoothness
        isLegacyTwoStop = style.rawValue <= DR_GRADIENT_RADIAL.rawValue && stops.count == 2
            && stops[0].position == 0 && stops[1].position == 1
            && self.midpoints.first == StopMidpoint() && smoothness >= 1
    }

    init(options: GradientToolOptions, foreground: NSColor, background: NSColor) {
        var resolved: [(position: Double, color: NSColor)]
        var mids: [StopMidpoint]
        if let stored = options.stops, stored.count >= 2 {
            let read = { (m: Double?) in
                m.map { GradientStop.midpointRange.contains($0) ? $0 : 0.5 } ?? 0.5
            }
            let entries = stored.map { stop -> (position: Double, color: NSColor, mid: StopMidpoint) in
                let color = LayerStyleColor.color(fromHex: stop.color) ?? .black
                let opacity = stop.opacity.isFinite ? min(max(stop.opacity, 0), 1) : 1
                let position = stop.position.isFinite ? min(max(stop.position, 0), 1) : 0
                return (position, color.withAlphaComponent(CGFloat(opacity)),
                        StopMidpoint(color: read(stop.midpoint), opacity: read(stop.opacityMidpoint)))
            }
            // Position order, ties in stored order: the core's rule.
            let sorted = entries.enumerated().sorted {
                $0.element.position == $1.element.position
                    ? $0.offset < $1.offset : $0.element.position < $1.element.position
            }.map(\.element)
            resolved = sorted.map { ($0.position, $0.color) }
            mids = sorted.map(\.mid)
            // The last stop's midpoints govern no segment (the core's reset).
            mids[mids.count - 1] = StopMidpoint()
        } else {
            resolved = [(0, foreground), (1, background)]
            mids = [StopMidpoint(), StopMidpoint()]
        }
        if !options.transparency {
            resolved = resolved.map { ($0.position, $0.color.withAlphaComponent(1)) }
        }
        if options.reverse {
            resolved = resolved.reversed().map { (1 - $0.position, $0.color) }
            let n = mids.count
            mids = (0..<n).map { k in
                guard k < n - 1 else { return StopMidpoint() }
                // Whole percents, as every midpoint is stored.
                let m = mids[n - 2 - k], mirror = { (v: Double) in ((1 - v) * 100).rounded() / 100 }
                return StopMidpoint(color: mirror(m.color), opacity: mirror(m.opacity))
            }
        }
        midpoints = mids
        let opacity = CGFloat(min(max(options.opacity, 0), 100) / 100)
        stops = resolved.map {
            ($0.position, $0.color.withAlphaComponent($0.color.alphaComponent * opacity))
        }
        let index = min(max(options.typeIndex, 0), Self.shapeNames.count - 1)
        style = DrGradientKind(rawValue: UInt32(index))
        smoothness = options.smoothness.isFinite ? min(max(options.smoothness, 0), 1) : 1
        isLegacyTwoStop = index <= 1 && stops.count == 2
            && stops[0].position == 0 && stops[1].position == 1
            && midpoints[0] == StopMidpoint() && smoothness >= 1
    }

    /// The ramp's shape as `shape` spells it.
    var shapeName: String { Self.shapeNames[Int(style.rawValue)] }

    /// The stops as `dr_doc_gradient_stops` takes them — seven floats each,
    /// `[position, r, g, b, a, midpoint, opacity_midpoint]`, straight and
    /// 0…1 — with each colour read through `bytes`, the caller's conversion
    /// into the numbers its target stores (the document's bytes for a layer,
    /// coverage gray for a plane). nil when a colour cannot be converted.
    func flattened(_ bytes: (NSColor) -> [UInt8]?) -> [Float]? {
        var out: [Float] = []
        out.reserveCapacity(stops.count * 7)
        for (stop, mid) in zip(stops, midpoints) {
            guard let rgba = bytes(stop.color), rgba.count == 4 else { return nil }
            out.append(Float(stop.position))
            out.append(contentsOf: rgba.map { Float($0) / 255 })
            out.append(Float(mid.color))
            out.append(Float(mid.opacity))
        }
        return out
    }

    /// The `stops` a recorded `gradient` step carries: each stop's position,
    /// its authored `#RRGGBB`, its opacity and its midpoints when not 0.5
    /// — or the document's own bytes
    /// with `color_space: "document"` when `space` can spell them, `fill`'s
    /// rule (`ActionArgs.documentHex`) — and `smoothness` when below 1.
    func recordedArguments(space: NSColorSpace?) -> [String: Any] {
        let opaque = stops.map { $0.color.withAlphaComponent(1) }
        let documentHexes = space.flatMap { space in
            let hexes = opaque.compactMap { ActionArgs.documentHex($0, in: space) }
            return hexes.count == stops.count ? hexes : nil
        }
        var arguments: [String: Any] = [
            "shape": shapeName,
            "stops": stops.enumerated().map { index, stop -> [String: Any] in
                var entry: [String: Any] = [
                    "position": ActionArgs.number(stop.position),
                    "color": documentHexes?[index] ?? ActionArgs.hex(opaque[index]),
                    "opacity": ActionArgs.number(Double(stop.color.alphaComponent)),
                ]
                entry.merge(midpoints[index].keys) { _, new in new }
                return entry
            },
        ]
        if documentHexes != nil { arguments["color_space"] = "document" }
        if smoothness < 1 { arguments["smoothness"] = ActionArgs.number(smoothness) }
        return arguments
    }
}

extension Array where Element == ActionStep {
    /// The Gradient tool's drag on the stops path: `gradient` with
    /// `stops` and `shape`, the ramp `GradientRamp.recordedArguments` spells.
    /// A two-colour linear or radial drag keeps recording `gradient(from:…)`.
    /// `mode` is recorded as `blend_mode` only when it is not Normal.
    static func gradientStops(
        from a: CGPoint, to b: CGPoint, ramp: GradientRamp, target: String,
        space: NSColorSpace? = nil, mode: DrBlendMode = DR_BLEND_NORMAL
    ) -> [ActionStep] {
        var arguments: [String: Any] = [
            "x0": ActionArgs.number(a.x), "y0": ActionArgs.number(a.y),
            "x1": ActionArgs.number(b.x), "y1": ActionArgs.number(b.y),
            "layer": ActionArgs.activeLayer, "target": target,
        ]
        arguments.merge(ramp.recordedArguments(space: space)) { _, new in new }
        if mode != DR_BLEND_NORMAL {
            arguments["blend_mode"] = DrBlendMode.displayName(for: mode)
        }
        return [ActionStep(tool: "gradient", arguments: arguments, note: "Gradient tool: drag")]
    }
}
