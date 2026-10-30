import AppKit

// The shape tools' stroke styles: the options bar's Corner style,
// Dash pattern and Align stroke. A shape layer's payload carries them
// (`ShapeLayerPayload.strokeJoin` / `strokeDash` / `strokeAlign`, written only
// when off their defaults, at payload version 5) and `ShapeLayer.strokeOutline`
// is the ONE place they become geometry: the byte renderer, the native
// renderer and the canvas preview all fill that outline, so a styled stroke
// looks the same wherever it is drawn. A stroke at its defaults (miter, no
// dash, centred) never reaches the outline: it is stroked as it always was,
// so default shapes render byte-identical to the ones older builds made.

/// How two stroke segments meet at a corner.
enum ShapeStrokeJoin: String, CaseIterable, Codable {
    case miter, round, bevel

    var title: String {
        switch self {
        case .miter: return "Miter"
        case .round: return "Round"
        case .bevel: return "Bevel"
        }
    }

    var index: Int { Self.allCases.firstIndex(of: self) ?? 0 }

    var cgJoin: CGLineJoin {
        switch self {
        case .miter: return .miter
        case .round: return .round
        case .bevel: return .bevel
        }
    }
}

/// The stroke's dash pattern, in multiples of its width `w`: dashed is 4w on,
/// 2w off with butt ends; dotted is a zero-length dash every 2w with round
/// caps, which draws a row of dots one stroke width across.
enum ShapeStrokeDash: String, CaseIterable, Codable {
    case none, dashed, dotted

    var title: String {
        switch self {
        case .none: return "None"
        case .dashed: return "Dashed"
        case .dotted: return "Dotted"
        }
    }

    var index: Int { Self.allCases.firstIndex(of: self) ?? 0 }

    /// The dash lengths for a stroke `width` wide; nil for a solid stroke.
    func lengths(width: CGFloat) -> [CGFloat]? {
        switch self {
        case .none: return nil
        case .dashed: return [4 * width, 2 * width]
        case .dotted: return [0, 2 * width]
        }
    }
}

/// Which side of the path the stroke's width lies on. A line has no inside
/// or outside, so its stroke is always centred.
enum ShapeStrokeAlign: String, CaseIterable, Codable {
    case inside, center, outside

    var title: String {
        switch self {
        case .inside: return "Inside"
        case .center: return "Center"
        case .outside: return "Outside"
        }
    }

    var index: Int { Self.allCases.firstIndex(of: self) ?? 1 }
}

extension ShapeToolOptions {
    /// The stored indices as styles; an index out of range reads as the default.
    var strokeJoin: ShapeStrokeJoin {
        ShapeStrokeJoin.allCases.indices.contains(strokeJoinIndex)
            ? ShapeStrokeJoin.allCases[strokeJoinIndex] : .miter
    }

    var strokeDash: ShapeStrokeDash {
        ShapeStrokeDash.allCases.indices.contains(strokeDashIndex)
            ? ShapeStrokeDash.allCases[strokeDashIndex] : .none
    }

    var strokeAlign: ShapeStrokeAlign {
        ShapeStrokeAlign.allCases.indices.contains(strokeAlignIndex)
            ? ShapeStrokeAlign.allCases[strokeAlignIndex] : .center
    }
}

extension ShapeLayerPayload {
    /// True when any stroke style is off its default: the payload renders
    /// through `ShapeLayer.strokeOutline` and encodes at version 5.
    var hasStrokeStyle: Bool {
        strokeJoin != .miter || strokeDash != .none || strokeAlign != .center
    }

    /// The payload with these stroke styles. A line keeps a centred stroke.
    /// A payload with no `artwork` (version 1 or 2) and a non-default style
    /// first takes one from its sRGB hex colours, since version 5 always
    /// carries `artwork`.
    func withStrokeStyle(
        join: ShapeStrokeJoin, dash: ShapeStrokeDash, align: ShapeStrokeAlign
    ) -> ShapeLayerPayload {
        var result = self
        result.strokeJoin = join
        result.strokeDash = dash
        result.strokeAlign = kind == "line" ? .center : align
        if result.hasStrokeStyle, result.artwork == nil {
            result.artwork = ArtworkShapeStyle(
                fill: fillColor.map(ArtworkColor.authored),
                stroke: strokeColor.map(ArtworkColor.authored), in: .srgb)
        }
        return result
    }
}

extension ShapeLayer {
    /// The filled outline of `payload`'s stroke along `path` (the payload's
    /// own `vectorPath`, in source space) when a stroke style is off its
    /// default; nil for a default stroke, which every renderer keeps
    /// stroking as before, and for a stroke with no width.
    ///
    /// Centred: the path (dashed first when the style asks) stroked at the
    /// stroke width with the style's join. Inside and outside stroke it at
    /// TWICE the width and keep the half on that side of the path:
    /// `stroke(2w) ∩ path` and `stroke(2w) − path`. Dashed ends are butt,
    /// dotted ones round; a line keeps its round caps.
    static func strokeOutline(_ payload: ShapeLayerPayload, path: CGPath) -> CGPath? {
        guard payload.hasStrokeStyle, payload.strokeWidth.isFinite, payload.strokeWidth > 0
        else { return nil }
        let width = CGFloat(payload.strokeWidth)
        let isLine = payload.kind == "line"
        let align: ShapeStrokeAlign = isLine ? .center : payload.strokeAlign
        let cap: CGLineCap = payload.strokeDash == .dotted || isLine ? .round : .butt
        let source = payload.strokeDash.lengths(width: width)
            .map { path.copy(dashingWithPhase: 0, lengths: $0) } ?? path
        let stroked = source.copy(
            strokingWithWidth: align == .center ? width : 2 * width,
            lineCap: cap, lineJoin: payload.strokeJoin.cgJoin, miterLimit: 10)
        switch align {
        case .center: return stroked
        case .inside: return stroked.intersection(path, using: .winding)
        case .outside: return stroked.subtracting(path, using: .winding)
        }
    }
}
