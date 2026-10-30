import AppKit

// Edit ▸ Auto-Align Layers…'s projections: Auto, Perspective, Collage, Cylindrical, Spherical and
// Reposition, and the maps Collage and Reposition reduce a registration to (the translation since
// phase 2b-5, AutoAlignPlanarLens.swift). Auto behaves as Perspective. Cylindrical and Spherical
// reproject every layer (AutoAlignReprojection.swift). The registration itself is
// `AutoAlign.register`'s; this file only names the choices and fits the similarity and the
// translation.

extension AutoAlign.Registration {
    /// The dialog's and the MCP tool's choices, in the dialog's order.
    static let projections: [AutoAlign.Registration] = [
        .auto, .perspective, .collage, .cylindrical, .spherical, .translation,
    ]

    /// Whether the projection reprojects every layer, the reference too,
    /// onto the reference camera's surface instead of warping each one onto
    /// the reference's plane.
    var reprojects: Bool { self == .cylindrical || self == .spherical }

    /// The dialog's radio title.
    var title: String {
        switch self {
        case .auto: return "Auto"
        case .perspective: return "Perspective"
        case .collage: return "Collage"
        case .cylindrical: return "Cylindrical"
        case .spherical: return "Spherical"
        case .translation: return "Reposition"
        }
    }

    /// The MCP `projection` spelling: the dialog's names, `reposition` for
    /// `translation`.
    var projectionName: String {
        switch self {
        case .auto: return "auto"
        case .perspective: return "perspective"
        case .collage: return "collage"
        case .cylindrical: return "cylindrical"
        case .spherical: return "spherical"
        case .translation: return "reposition"
        }
    }

    /// The projection `name` spells (`projectionName`); nil for any other.
    init?(projectionName name: String) {
        guard let match = Self.projections.first(where: { $0.projectionName == name }) else {
            return nil
        }
        self = match
    }
}

extension Homography {
    /// The similarity — rotation, uniform scale and shift, no shear and no
    /// perspective — closest to this map over `rect`'s four corners: the
    /// least-squares `x' = a·x − b·y + tx, y' = b·x + a·y + ty` from the
    /// corners to their images. Collage's map: Collage keeps each
    /// layer's shape. nil when a corner has no image, the corners coincide,
    /// or the fit is not finite.
    ///
    /// The closed form: with both point sets centred on their means (u, v
    /// and u′, v′), a = Σ(u·u′ + v·v′) / Σ(u² + v²) and b = Σ(u·v′ − v·u′) /
    /// Σ(u² + v²); the shift carries the source mean onto the image mean.
    func similarityFit(over rect: CGRect) -> Homography? {
        guard let image = quad(of: rect) else { return nil }
        return Self.similarity(from: Self.corners(of: rect), to: image)
    }

    /// The least-squares similarity carrying `source`'s points onto
    /// `image`'s, pairwise (`similarityFit(over:)`'s closed form, its one
    /// implementation; the planar feature candidate,
    /// `AutoAlign.inlierReduction`). nil when the lists differ in length or
    /// are empty, the source points coincide, or the fit is not finite.
    static func similarity(from source: [CGPoint], to image: [CGPoint]) -> Homography? {
        guard !source.isEmpty, source.count == image.count else { return nil }
        let n = Double(source.count)
        let sx = source.map { Double($0.x) }.reduce(0, +) / n
        let sy = source.map { Double($0.y) }.reduce(0, +) / n
        let ix = image.map { Double($0.x) }.reduce(0, +) / n
        let iy = image.map { Double($0.y) }.reduce(0, +) / n
        var spread = 0.0, along = 0.0, across = 0.0
        for (p, q) in zip(source, image) {
            let u = Double(p.x) - sx, v = Double(p.y) - sy
            let u2 = Double(q.x) - ix, v2 = Double(q.y) - iy
            spread += u * u + v * v
            along += u * u2 + v * v2
            across += u * v2 - v * u2
        }
        guard spread > 0 else { return nil }
        let a = along / spread, b = across / spread
        let tx = ix - (a * sx - b * sy), ty = iy - (b * sx + a * sy)
        let fitted = Homography(m: [a, -b, tx, b, a, ty, 0, 0, 1])
        return fitted.m.allSatisfy(\.isFinite) ? fitted : nil
    }

    /// The translation closest to this map over `rect`'s four corners: the
    /// mean displacement of the corners to their images. Reposition's map
    /// when Geometric Distortion's fit refines it (AutoAlignPlanarLens.swift).
    /// nil when a corner has no image or the shift is not finite.
    func translationFit(over rect: CGRect) -> Homography? {
        guard let image = quad(of: rect) else { return nil }
        return Self.meanTranslation(from: Self.corners(of: rect), to: image)
    }

    /// The mean displacement of `source`'s points onto `image`'s, pairwise
    /// (`translationFit(over:)`'s, its one implementation; the planar
    /// feature candidate, `AutoAlign.inlierReduction`). nil
    /// when the lists differ in length or are empty, or the shift is not
    /// finite.
    static func meanTranslation(from source: [CGPoint], to image: [CGPoint]) -> Homography? {
        guard !source.isEmpty, source.count == image.count else { return nil }
        let n = Double(source.count)
        let dx = zip(source, image).map { Double($1.x - $0.x) }.reduce(0, +) / n
        let dy = zip(source, image).map { Double($1.y - $0.y) }.reduce(0, +) / n
        guard dx.isFinite, dy.isFinite else { return nil }
        return Homography.translation(dx: dx, dy: dy)
    }
}
