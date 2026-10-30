import AppKit
import Vision
import simd

// Edit ▸ Auto-Align Layers…: Vision registers each selected raster layer to a reference
// layer first, and the core's feature step (AutoAlignFeatures.swift) cross-checks
// that answer on its own matches and answers in its place when Vision finds nothing or disagrees;
// the core's existing `perspective_set` warps the layer into place, and the canvas then grows to
// the union of the aligned layers (measured, take1 R2) through Reveal All's `canvasResized`.
// The warp is the core's, which solves its own homography from `source_rect` and `quad`. Swift's
// `Homography` only converts Vision's matrix and projects four corners — it resamples nothing (the
// line `TransformAgain.swift` keeps for a remembered perspective).
//
// MEASURED VISION CONVENTION (macOS 26.5.1, `scratch/vision-convention-1/probe.swift` in the
// handoff: picture A is value noise, B(q) = A(K⁻¹q) for a known K, targeted image B, handler image
// A, eight readings of the matrix compared with K⁻¹):
//   - DIRECTION: `VNImageHomographicAlignmentObservation.warpTransform` maps a point of the
//     TARGETED (floating) image to its place in the handler's (reference) image.
//     `VNImageTranslationAlignmentObservation.alignmentTransform` maps the same way.
//   - Y AXIS: both are in pixels with the origin at the BOTTOM-left, y up. `Homography` is y down
//     from the top-left, so a matrix is conjugated by the flip F(y) = height − y, and a
//     translation's `ty` changes sign.
//   - ELEMENT ORDER: `warpTransform` is a `simd_float3x3` applied to a COLUMN vector,
//     p' = W · (x, y, 1), so row r, column c of the matrix is `warp[c][r]`. The transposed reading
//     is off by hundreds of pixels.
//
// MEASURED REACH (same machine; `scratch/vision-reach-6/reach.swift` and `vision-reach-10`, Vision
// only). The homographic request ALONE follows a shift of about 2% of the frame's width and is
// wrong by hundreds of pixels from about 5% (2048 × 1536 value noise: 0.25 px at 2%, 168 px at 5%,
// 289 px at 10%), and the wrong answer still looks like a mild transform. So `register` runs two
// passes — the translational request, then the homographic one on the frame pre-shifted by it —
// and warps on nothing the agreement score does not vouch for. With the feature step (phase 3a,
// `scripts/reach-probe` on real-photo views) 24 of 27 plain rows register within 2.0 px, to a 40°
// yaw at 25 % overlap, and none of the 138 unrelated pairs answers (0/138 false accepts). The
// tables are beside `register`.

/// A planar projective map, row-major 3×3, in pixels with the origin at the
/// top-left and y down: it maps a point of the floating layer to its place in
/// the reference. `(x, y) → (m0·x + m1·y + m2, m3·x + m4·y + m5) / (m6·x +
/// m7·y + m8)`.
struct Homography: Equatable {
    let m: [Double]

    static func translation(dx: Double, dy: Double) -> Homography {
        Homography(m: [1, 0, dx, 0, 1, dy, 0, 0, 1])
    }

    /// Vision's `warpTransform` over a frame `height` pixels tall, in this
    /// type's convention (the file header's three measured facts).
    init(visionWarp warp: simd_float3x3, height: Double) {
        let vision: [Double] = [
            Double(warp[0][0]), Double(warp[1][0]), Double(warp[2][0]),
            Double(warp[0][1]), Double(warp[1][1]), Double(warp[2][1]),
            Double(warp[0][2]), Double(warp[1][2]), Double(warp[2][2]),
        ]
        let flip: [Double] = [1, 0, 0, 0, -1, height, 0, 0, 1]
        m = Self.product(flip, Self.product(vision, flip))
    }

    init(m: [Double]) {
        self.m = m
    }

    /// The row-major product a · b of two 3×3 matrices.
    static func product(_ a: [Double], _ b: [Double]) -> [Double] {
        var out = [Double](repeating: 0, count: 9)
        for row in 0..<3 {
            for column in 0..<3 {
                var sum = 0.0
                for k in 0..<3 { sum += a[row * 3 + k] * b[k * 3 + column] }
                out[row * 3 + column] = sum
            }
        }
        return out
    }

    /// nil when the point is at or behind the map's horizon (w ≤ 0) or the
    /// result is not finite.
    func apply(_ point: CGPoint) -> CGPoint? {
        guard m.count == 9 else { return nil }
        let x = Double(point.x), y = Double(point.y)
        let w = m[6] * x + m[7] * y + m[8]
        guard w.isFinite, w > 0 else { return nil }
        let px = (m[0] * x + m[1] * y + m[2]) / w
        let py = (m[3] * x + m[4] * y + m[5]) / w
        guard px.isFinite, py.isFinite else { return nil }
        return CGPoint(x: px, y: py)
    }

    /// A map measured on frames drawn at `s` times the canvas, as a map of
    /// canvas pixels: D(1/s) · H · D(s). A canvas point is scaled into the
    /// frame, mapped, and scaled back, so the two translation terms divide by
    /// `s` and the two projective terms multiply by it.
    func rescaled(fromAnalysisScale s: Double) -> Homography {
        guard m.count == 9, s.isFinite, s > 0 else { return self }
        return Homography(m: [
            m[0], m[1], m[2] / s,
            m[3], m[4], m[5] / s,
            m[6] * s, m[7] * s, m[8],
        ])
    }

    /// This map after a shift by (`dx`, `dy`): a point is shifted first, then
    /// mapped (H · T).
    func preceded(byTranslation dx: Double, dy: Double) -> Homography {
        guard m.count == 9 else { return self }
        return Homography(m: [
            m[0], m[1], m[0] * dx + m[1] * dy + m[2],
            m[3], m[4], m[3] * dx + m[4] * dy + m[5],
            m[6], m[7], m[6] * dx + m[7] * dy + m[8],
        ])
    }

    /// `rect`'s corners under the map: TL, TR, BR, BL. nil when any corner
    /// has no image.
    func quad(of rect: CGRect) -> [CGPoint]? {
        let corners = Self.corners(of: rect).compactMap(apply)
        return corners.count == 4 ? corners : nil
    }

    static func corners(of rect: CGRect) -> [CGPoint] {
        [CGPoint(x: rect.minX, y: rect.minY), CGPoint(x: rect.maxX, y: rect.minY),
         CGPoint(x: rect.maxX, y: rect.maxY), CGPoint(x: rect.minX, y: rect.maxY)]
    }

    /// Whether the map is a plausible registration of `rect`: nine finite
    /// values, four finite corners in front of the horizon, a convex quad
    /// wound the way the rectangle is (no fold, no mirror), and every side
    /// between 1/4 and 4 times the side it came from.
    ///
    /// Why 1/4 to 4: two frames of one scene that overlap enough to register
    /// differ by a small rotation, a few percent of scale and a mild
    /// perspective, so a side that changes length fourfold is a mismatch the
    /// estimator fitted to noise, not a camera move. The band is wide on
    /// purpose — it only removes degenerate fits, and the translational
    /// request then answers instead.
    func isUsable(for rect: CGRect) -> Bool {
        guard m.count == 9, m.allSatisfy(\.isFinite), rect.width > 0, rect.height > 0,
              let quad = quad(of: rect) else { return false }
        let source = Self.corners(of: rect)
        for index in 0..<4 {
            let a = quad[index], b = quad[(index + 1) % 4], c = quad[(index + 2) % 4]
            // The rectangle TL → TR → BR → BL turns the same way at every
            // corner (a positive cross product with y down); so must its image.
            let cross = (b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x)
            guard cross.isFinite, cross > 0 else { return false }
            let side = hypot(b.x - a.x, b.y - a.y)
            let from = hypot(source[(index + 1) % 4].x - source[index].x,
                             source[(index + 1) % 4].y - source[index].y)
            guard from > 0, side >= from / 4, side <= from * 4 else { return false }
        }
        return true
    }

    /// The part of `rect` the map carries back into `rect`: the bounding box
    /// of the 33 × 33 grid points of `rect` (its edges included) whose image
    /// lies in `rect`. nil when that box has no width or no height.
    func overlap(in rect: CGRect) -> CGRect? {
        var minX = Double.infinity, minY = Double.infinity
        var maxX = -Double.infinity, maxY = -Double.infinity
        for j in 0...32 {
            for i in 0...32 {
                let point = CGPoint(x: rect.minX + Double(i) * rect.width / 32,
                                    y: rect.minY + Double(j) * rect.height / 32)
                // Closed on every side: an edge point carried onto an edge is in.
                guard let landed = apply(point),
                      (rect.minX...rect.maxX).contains(landed.x),
                      (rect.minY...rect.maxY).contains(landed.y) else { continue }
                minX = min(minX, point.x)
                maxX = max(maxX, point.x)
                minY = min(minY, point.y)
                maxY = max(maxY, point.y)
            }
        }
        guard maxX > minX, maxY > minY else { return nil }
        return CGRect(x: minX, y: minY, width: maxX - minX, height: maxY - minY)
    }
}

/// Eligibility, the analysis frames and Vision's registration. The job that
/// runs them is `AutoAlignWork`.
enum AutoAlign {
    /// How a layer was registered.
    enum Method: String { case homography, similarity, translation }
    /// What the caller asked for (the projection, AutoAlignProjection.swift):
    /// `auto` and `perspective` register in two passes (a translation, then a
    /// homography on the frame shifted by it) and fall back as `register`
    /// says — Auto is Perspective; `collage` reduces that answer
    /// to a similarity; `translation` (Reposition) asks only the translation;
    /// `cylindrical` and `spherical` register as `perspective` and reproject
    /// every layer (`AutoAlignWork.reprojectedDocument`, AutoAlignReprojection.swift).
    enum Registration: String { case auto, perspective, collage, cylindrical, spherical, translation }

    /// The longest side of the frames Vision registers. A registration needs
    /// the picture's structure, not its pixels: 2048 keeps the features of a
    /// 24–60 MP frame while the two Vision working copies stay near 100 MB
    /// (`registrationScratch`), and frames this size registered a shift of a
    /// fifth of their width within 0.4 px (`register`'s table). A canvas no
    /// larger than this is analysed at its own size, never enlarged.
    static let analysisMaxSide = 2048

    /// How far the two-pass homography's displacement at the frame centre may
    /// be from the translational estimate, as a share of the frame's shorter
    /// side, before the homography is set aside. Fixed at 2% BEFORE the reach
    /// was measured: the homographic request alone was accurate to about 2% of
    /// the frame, so a refinement that moves the centre farther than that
    /// after the pre-shift is outside where it was ever right. Measured
    /// afterwards, a correct two-pass result sat within 1.9% (a 5° rotation)
    /// and a 2° rotation within 1.4%.
    static let centreTolerance = 0.02

    /// The longest side of the grey copies the agreement score is measured
    /// on. 256 keeps the score a matter of milliseconds in an unoptimised
    /// build (at most 65,536 samples a copy — a bounded copy, never the
    /// canvas) while one of its pixels is 8 frame pixels at the 2048 analysis
    /// size, fine enough that an estimate tens of pixels off scores near zero.
    static let agreementSide = 256

    /// The least agreement score an estimate must reach to be used. Chosen
    /// FROM the measurement (`register`'s record), clear of every score it
    /// shows: on value noise and three photographs every estimate within 5 px
    /// of the truth scored 0.87 or more, and every estimate 15 px or more off
    /// scored 0.52 or less. The two highest of those, 0.50 and 0.52, are a
    /// translation standing in for a perspective move: right at the centre,
    /// 18 and 65 px off at the corners. That class is REFUSED — a layer left
    /// tens of pixels out at its corners is a wrong warp, and `auto` has the
    /// two-pass homography for such a frame — so the cut sits above both.
    /// 0.6 is midway between 0.52 and the next score above it, 0.68 (a
    /// two-pass estimate 7 px off, which step 2's centre check sets aside
    /// anyway), so no measured estimate is within 0.08 of the cut, the
    /// featureless-ramp trials aside, one of which scored 0.59.
    static let agreementThreshold = 0.6

    /// The least share of the copy's pixels the score must be measured over.
    /// Below an eighth of the frame the two layers share too little picture
    /// for a correlation to mean anything, so the score is 0.
    static let agreementMinimumShare = 0.125

    /// A layer whose four corners all land within this many canvas pixels of
    /// where they already are is reported `unchanged` and not resampled.
    /// A tenth of a pixel is below what a resample would visibly move, and
    /// resampling an already-aligned layer would only soften it.
    static let unchangedThreshold = 0.1

    /// How far, in ANALYSIS FRAME pixels, an estimate's corners may move and
    /// still give way to the identity (`register`). The agreement score is
    /// measured on copies 8 frame pixels to the copy pixel at the 2048 size,
    /// so it cannot tell a real shift of a frame pixel or two from none; the
    /// score alone would snap such a shift to the identity. Half a frame pixel
    /// sits above the 0.39 px Vision's two-pass map moved identical frames and
    /// below the 1 frame pixel of the smallest shift worth keeping (2 canvas
    /// pixels on a 4,096 px canvas; jobs-probe "auto-align action").
    static let identityRadius = 0.5

    struct Request {
        let reference: Int
        /// The layers to warp, ascending, the reference excluded.
        let floating: [Int]
        let registration: Registration
        let sampler: DrResizeFilter
        /// Whether the canvas grows to the union of the reference and the
        /// aligned layers (`alignedCanvas`). Auto-Align itself and Auto-Blend's
        /// Panorama ask for it; Reduce Noise, Stack Images and Merge to HDR
        /// keep the canvas.
        let growsCanvas: Bool
        /// Lens Correction ▸ Vignette Removal (AutoAlignVignette.swift);
        /// false when the projection ignores it (`Registration.lens`).
        let vignette: Bool
        /// Lens Correction ▸ Geometric Distortion (AutoAlignDistortion.swift);
        /// false when the projection ignores it.
        var distortion = false
        /// The surface the job reprojects onto, nil when it warps onto the
        /// reference's plane (`AutoAlign.surface(for:vignette:distortion:)`).
        var surface: Registration? {
            AutoAlign.surface(for: registration, vignette: vignette, distortion: distortion)
        }
    }

    struct Aligned {
        let layer: Int
        let name: String
        let method: Method
        /// Canvas pixels: the layer's points to their place in the reference.
        let homography: Homography
        /// Where the layer's rectangle went: TL, TR, BR, BL.
        let quad: [CGPoint]
        /// Cylindrical and Spherical only: row-major, a reference ray to this
        /// layer's ray (the core's `project_layer` rotation).
        var rotation: [Double]? = nil
    }

    struct Skipped {
        let layer: Int
        let name: String
        let reason: String
    }

    struct Outcome {
        let reference: Int
        let aligned: [Aligned]
        let unchanged: [Skipped]
        let unregistered: [Skipped]
        /// The registration's own plan (`registrationPlan`), admitted before
        /// anything is registered.
        let registrationBytes: UInt64
        /// The largest charge the job admitted: the registration plan or a
        /// warp's geometry plan.
        let planBytes: UInt64
        /// The new canvas in the OLD canvas's coordinates (`minX`, `minY` ≤
        /// 0); nil when the canvas did not change.
        let canvas: CGRect?
        /// Cylindrical and Spherical only: the focal length, canvas pixels.
        var focal: Double? = nil
        /// The surface the job reprojected onto, nil when it did not.
        var surface: Registration? = nil
        /// Vignette Removal only: the fitted falloff.
        var vignette: VignetteFit? = nil
        /// Geometric Distortion only: the fitted coefficient.
        var distortion: DistortionFit? = nil
    }

    /// Why a layer no estimate was vouched for is left where it is.
    static let unregisteredReason = "it does not match the reference closely enough to align"

    static let title = "Auto-Align Layers"

    static func emptyReferenceMessage(_ name: String) -> String {
        "“\(name)” is the reference layer and has no pixels to align to. Deselect it or "
            + "choose another reference."
    }

    /// The scale the analysis frames are drawn at and their size, for a
    /// canvas: min(1, `analysisMaxSide` / longest side).
    static func analysis(canvasWidth: Int, canvasHeight: Int)
        -> (scale: Double, width: Int, height: Int)
    {
        let longest = max(canvasWidth, canvasHeight, 1)
        let scale = min(1, Double(analysisMaxSide) / Double(longest))
        return (scale, max(1, Int((scale * Double(canvasWidth)).rounded())),
                max(1, Int((scale * Double(canvasHeight)).rounded())))
    }

    /// Checks `layers` and picks the reference. Each layer must be a raster
    /// layer — not a group, not an adjustment, not a text or shape layer —
    /// that moves alone (`movingSet([i]) == [i]`, so it is not linked); a
    /// refusal names the layer. A layer is LOCKED when a Position lock blocks
    /// it. With no `reference`: none locked, the lowest index; one locked, it;
    /// more, a refusal. A given `reference` must be one of `layers`, and every
    /// other layer unlocked.
    static func request(in doc: RasterDocument, layers: [Int], reference: Int?,
                        registration: Registration, sampler: DrResizeFilter,
                        growsCanvas: Bool = false, vignette: Bool = false,
                        distortion: Bool = false) throws -> Request {
        let indices = Array(Set(layers)).sorted()
        guard indices.count >= 2 else {
            throw DoctoredCoreError(message: "\(title) needs at least two layers.")
        }
        // A lens the projection ignores is dropped (`Registration.lens`, take3).
        let vignette = vignette && registration.lens(.vignette) == .applies
        let distortion = distortion && registration.lens(.distortion) == .applies
        if doc.isCmyk,
           let reason = CmykDocument.autoAlignLensRefusal(vignette: vignette, distortion: distortion) {
            throw DoctoredCoreError(message: reason)
        }
        let surface = AutoAlign.surface(for: registration, vignette: vignette,
                                        distortion: distortion)
        for index in indices {
            guard let info = doc.layerInfo(index) else {
                throw DoctoredCoreError(message: "Layer \(index) is out of range.")
            }
            let name = "“\(info.name)”"
            guard !info.isGroup else {
                throw DoctoredCoreError(
                    message: "\(name) is a group. \(title) takes raster layers; select the "
                        + "layers inside it instead.")
            }
            guard !doc.layerIsAdjustment(index) else {
                throw DoctoredCoreError(
                    message: "\(name) is an adjustment layer. \(title) takes raster layers.")
            }
            guard doc.describedEntries(in: [index]).isEmpty else {
                throw DoctoredCoreError(
                    message: "\(name) is a text, shape or fill layer. Rasterize it before "
                        + "aligning it.")
            }
            guard doc.movingSet([index]) == [index] else {
                throw DoctoredCoreError(
                    message: "\(name) is linked to other layers, which would move with it. "
                        + "Unlink it before aligning it.")
            }
        }
        let locked = indices.filter { !doc.lockBlockingFlags($0, kind: DR_EDIT_POSITION).isEmpty }
        func names(_ set: [Int]) -> String {
            set.map { "“\(doc.layerInfo($0)?.name ?? "Layer \($0)")”" }.joined(separator: ", ")
        }
        // Cylindrical and Spherical move every layer, the reference too, and
        // Geometric Distortion corrects every layer (AutoAlignPlanarLens.swift).
        if surface != nil || distortion {
            let planar = surface == nil
            if !locked.isEmpty {
                throw DoctoredCoreError(message: reprojectionLockMessage(names(locked),
                                                                       planar: planar))
            }
            if let placed = indices.first(where: { doc.hasRetainedSource($0) }) {
                throw DoctoredCoreError(message: placedSourceMessage(names([placed]),
                                                                   planar: planar))
            }
        }
        let chosen: Int
        if let reference {
            guard indices.contains(reference) else {
                throw DoctoredCoreError(
                    message: "reference \(reference) is not one of the layers to align.")
            }
            let others = locked.filter { $0 != reference }
            guard others.isEmpty else {
                throw DoctoredCoreError(
                    message: "\(names(others)) cannot move because of a position lock. Only "
                        + "the reference layer may be locked.")
            }
            chosen = reference
        } else if locked.count > 1 {
            throw DoctoredCoreError(
                message: "\(names(locked)) are position-locked, and only the reference layer "
                    + "stays where it is. Unlock all but one of them.")
        } else {
            chosen = locked.first ?? indices[0]
        }
        return Request(reference: chosen, floating: indices.filter { $0 != chosen },
                       registration: registration, sampler: sampler, growsCanvas: growsCanvas,
                       vignette: vignette, distortion: distortion)
    }

    /// `layer` as Vision sees it: its thumbnail drawn at its scaled offset
    /// into an opaque mid-grey frame with the canvas's aspect, so every
    /// layer's frame shares one coordinate system and a layer smaller than
    /// the canvas keeps its place in it. Mid-grey because the surround must
    /// carry no edge of its own: black or white would add a high-contrast
    /// border Vision could lock onto. nil when the layer has nothing opaque.
    /// Reads only `doc`; safe on a worker.
    ///
    /// On a CMYK document the estimating plane is the layer's Display P3
    /// VIEW: its ink through the document's CMYK profile and view link
    /// (`PrecisionDocument::cmyk_layer_thumbnail_rgba8`, which `layerThumbnail`
    /// returns for CMYK; the document's `profile()` is the built-in Display P3,
    /// so `space` tags the frame correctly). Vision registers that frame and the
    /// agreement score and the feature step read its grey. It is what the user
    /// aligns by eye, and every ink (K and each of C, M and Y) darkens it, so
    /// structure on any plate reaches the estimate with no CMYK copy of the
    /// estimator; the warp then moves all four inks.
    static func analysisFrame(of doc: RasterDocument, layer: Int, scale: Double,
                              space: CGColorSpace) throws -> CGImage? {
        guard let info = doc.layerInfo(layer), info.width > 0, info.height > 0,
              doc.layerBounds(layer) != nil else { return nil }
        let side = max(1, Int((scale * Double(max(info.width, info.height))).rounded()))
        guard let thumbnail = doc.layerThumbnail(layer, maxSide: side),
              let image = thumbnail.makeCGImage(in: space) else {
            throw DoctoredCoreError(message: "“\(info.name)” could not be read for alignment.")
        }
        let width = max(1, Int((scale * Double(doc.width)).rounded()))
        let height = max(1, Int((scale * Double(doc.height)).rounded()))
        let bitmap = CGImageAlphaInfo.noneSkipLast.rawValue
        // A profile CoreGraphics cannot draw INTO (a LUT-only output profile)
        // falls back to sRGB: the frame only has to show the same structure
        // in both layers.
        guard let context = CGContext(data: nil, width: width, height: height,
                                      bitsPerComponent: 8, bytesPerRow: 0, space: space,
                                      bitmapInfo: bitmap)
            ?? CGContext(data: nil, width: width, height: height, bitsPerComponent: 8,
                         bytesPerRow: 0, space: ColorProfile.sRGB, bitmapInfo: bitmap) else {
            throw DoctoredCoreError(message: "An alignment frame could not be allocated.")
        }
        context.setFillColor(CGColor(gray: 0.5, alpha: 1))
        context.fill(CGRect(x: 0, y: 0, width: width, height: height))
        context.interpolationQuality = .high
        // CoreGraphics' origin is bottom-left; the layer's offset is from the top.
        let drawn = CGRect(
            x: scale * Double(info.offsetX),
            y: Double(height) - scale * Double(info.offsetY + info.height),
            width: scale * Double(info.width), height: scale * Double(info.height))
        context.draw(image, in: drawn)
        guard let frame = context.makeImage() else {
            throw DoctoredCoreError(message: "An alignment frame could not be drawn.")
        }
        return frame
    }

    /// Where `floating`'s points belong in `reference`, in frame pixels (top-
    /// left origin, y down), and how it was found; nil when no estimate is
    /// vouched for, and the layer is then left untouched. Both frames are the
    /// same size. Reads only its arguments; safe on a worker.
    ///
    /// An estimate is VOUCHED FOR when it `isUsable` and its agreement score
    /// (`Agreement.score`) reaches `agreementThreshold`: the two frames, one
    /// carried through the estimate onto the other, show the same edges. A
    /// translation is TRUSTWORTHY exactly when it is vouched for. In order:
    /// 1. The translational request, T. `translation` answers T when it is
    ///    vouched for and nil otherwise; the steps below are `auto`'s and
    ///    `perspective`'s, and `collage` reduces their answer (below).
    /// 2. The homographic request on the floating frame pre-shifted by T
    ///    rounded to whole pixels (a lossless copy over mid-grey), composed
    ///    with that shift. It is the answer when its displacement at the frame
    ///    centre is within `centreTolerance` of T and it is vouched for.
    /// 3. Otherwise T alone, when it is vouched for.
    /// 4. Otherwise the homographic request alone, when it is vouched for —
    ///    a rotation of about 5° with little shift defeats the translational
    ///    request (T 1,907 px off at 2048 × 1536) while the homographic request
    ///    alone follows it within 0.22 px.
    /// 5. Otherwise nil.
    ///
    /// `collage` takes that answer's `similarityFit` over the frame when it
    /// is vouched for (`similarity`), else T when it is vouched for, else nil:
    /// every projection warps only on a map the agreement score vouches for.
    ///
    /// THE FEATURE STEP (AutoAlignFeatures.swift) runs once a
    /// call: the core's corners, oriented patch descriptors and RANSAC
    /// homography F, with its inliers. When F exists, Vision's answer V (the
    /// steps above) is taken only when its `crossCheckError` on F's inliers is
    /// within `crossCheckBound`. Otherwise F is taken instead: it must pass
    /// the coverage and residual gate (`coversOverlap`), be usable over its
    /// overlap (`isUsable` on `overlap(in:)`), reach the agreement
    /// score, and pass the same cross-check. When it does not, the answer is nil. When
    /// the feature step finds no estimate, V stands on the agreement score
    /// alone.
    ///
    /// `translation` and `collage` (rulings 4 (b) and 5): no translation or
    /// similarity follows a keystone within 2 px of a homography's inliers.
    /// - A similarity V answers is judged like with like: the cross-check's
    ///   matches are F's inliers carried by the least-squares similarity of
    ///   those inliers (`judgedEstimate`), at 2.0 px as before. It answers
    ///   when it passes.
    /// - Otherwise the candidates are T and the planar feature candidate,
    ///   the inliers' own similarity (`collage`) or mean translation
    ///   (`translation`, `inlierReduction`) when F passes `coversOverlap`.
    ///   Their independent judge is the agreement score, which reads neither
    ///   path's matches: the higher-scoring one that is vouched for answers,
    ///   and with neither the answer is nil. On the ±10° yaw views T scored
    ///   0.67–0.71 and lay 2–4 px from the inliers' reduction on three pairs
    ///   and 300 px off (0.026) on the fourth; the inliers' similarity scored
    ///   0.64–0.76 on all four.
    ///
    /// THE LENS TEST (rulings 2 to 4) runs on F's inliers: how far Geometric
    /// Distortion's one-coefficient model departs from the plain homography
    /// there (`carriesLens`). When the departure is `lensSignificance` or
    /// more, the frames are not related by a plain homography. With `distortion` false (Geometric Distortion off) the
    /// answer is then nil, and `registered` gives `lensDistortionReason`. With
    /// it true the candidates are judged as above, the cross-check at
    /// `crossCheckLensBound`, and the caller's distortion fit follows.
    ///
    /// THE REACH WITH THE FEATURE STEP (`scripts/reach-probe`, real-photo views
    /// of `core/tests/fixtures/registration_photo/sky.jpg` at f 600; error the
    /// worst distance from the truth over a 33 × 33 grid; bounds 2.0 px plain,
    /// 8.0 px or the lens floor + 3.0 with k −0.05). Vision alone (bd000f44)
    /// registered 7 of 54 rows within bound and accepted 6 past it (16.02 px at
    /// worst). Now all 27 plain rows register within 2.0 px (1.65 px at
    /// worst): 600 × 400 views to a 40° yaw (25 % overlap), small views at
    /// 15 % overlap (a frame under 512 px is detected on its 2× upsampled
    /// level first), pure shifts and Collage turns to 15 %. Every lens
    /// row refuses with Geometric Distortion off, 26 of 27 register within
    /// 0.62 px of the truth after its fit, and none of the 138 unrelated
    /// pairs answers.
    ///
    /// An answer whose four corners all stay within `identityRadius` frame
    /// pixels of where they were, and that agrees no better than the identity
    /// while the identity is itself vouched for, gives way to the identity (as
    /// a translation), so a layer already in place is reported `unchanged` and
    /// not resampled: Vision's homography is never exact, and on two identical
    /// 800 × 600 value-noise frames the two-pass map moved the corners 0.12 to
    /// 0.39 px, past `unchangedThreshold` (jobs-probe "auto-align action").
    ///
    /// MEASURED (macOS 26.5.1, the standalone Vision probe
    /// `scratch/vision-reach-10/reach.swift` in the handoff, which
    /// imports no app code and restates these steps; error is the worst
    /// distance from the truth over the frame's corners and centre, in frame
    /// pixels; score is the agreement score).
    ///
    /// The translational request's reach on a pure shift, with the two-pass
    /// result beside it:
    ///   shift of width | 2048×1536 value noise        | 2048×1362 photograph
    ///                  | T error, two-pass error      | T error, two-pass error
    ///   2%             | 0.00, 0.14                   | 0.00, 0.03
    ///   5%             | 0.00, 0.05                   | 0.00, 0.15
    ///   10%            | 1.00, 0.12                   | 0.00, 0.07
    ///   20%            | 1.41, 0.36                   | 1.00, 0.84
    ///   35%            | 704 (score −0.02; nil)       | 2.00, 0.39
    /// So the reach is at least a fifth of the width, and past it (the noise
    /// at 35%, 50% and 65%; another picture; a flat frame) every estimate
    /// scored 0.03 or less and the answer was nil. A rotation shortens it: on
    /// the noise at 2048 × 1536, 2° with a 10% shift registered within 0.32 px
    /// and 1° with a 20% shift did not register (nil); a low-texture product
    /// photograph registered a 2° rotation only with no shift, and pure
    /// shifts through 30%. Across all 86 trials the answer was either within
    /// 2.5 px of the truth or nil.
    ///
    /// The score against the error, all 258 estimates (noise at 2048 × 1536
    /// and 640 × 480, three photographs; the flat frame, which has no truth,
    /// aside): error under 5 px scored 0.87 to
    /// 1.00; error of 15 px or more scored −0.02 to 0.40, and 0.50 and 0.52
    /// for a translation alone on a perspective move (right at the centre);
    /// in between, one 7 px two-pass estimate scored 0.68 with its centre
    /// 2.8% from T, so step 2 set it aside, and one 7 px single-pass estimate
    /// scored 0.91 where the two-pass answer (0.02 px) was taken first. The
    /// cut, `agreementThreshold` 0.6, refuses both of those translations;
    /// every answer the 86 trials gave scored 0.93 or more, so none of them
    /// depends on where between 0.52 and 0.68 the cut is.
    static func register(floating: CGImage, reference: CGImage, registration: Registration,
                         distortion: Bool = false) -> (Homography, Method)? {
        guard case let .aligned(map, method) = registered(
            floating: floating, reference: reference, registration: registration,
            distortion: distortion) else { return nil }
        return (map, method)
    }

    /// `register`'s answer, or why it is nil.
    enum Registered {
        case aligned(Homography, Method)
        case refused(String)
    }

    /// `register`, with the reason a layer is not aligned:
    /// `lensDistortionReason` when the lens test refuses, else
    /// `unregisteredReason`.
    static func registered(floating: CGImage, reference: CGImage, registration: Registration,
                           distortion: Bool) -> Registered {
        guard floating.width == reference.width, floating.height == reference.height,
              let agreement = autoreleasepool(invoking: {
                  Agreement(floating: floating, reference: reference)
              }) else { return .refused(unregisteredReason) }
        let handler = VNImageRequestHandler(cgImage: reference, options: [:])
        let frame = CGRect(x: 0, y: 0, width: floating.width, height: floating.height)
        func vouched(_ map: Homography) -> Bool {
            map.isUsable(for: frame) && agreement.score(map) >= agreementThreshold
        }
        let identity = Homography.translation(dx: 0, dy: 0)
        let still = agreement.score(identity)
        func settled(_ map: Homography, _ method: Method) -> (Homography, Method) {
            guard still >= agreementThreshold, let quad = map.quad(of: frame),
                  zip(quad, Homography.corners(of: frame)).allSatisfy({
                      hypot($0.x - $1.x, $0.y - $1.y) <= identityRadius
                  }),
                  still >= agreement.score(map) else { return (map, method) }
            return (identity, .translation)
        }
        // The feature step, asked once a call, and its lens test
        // (AutoAlignFeatures.swift).
        let estimate = featureEstimate(floating: floating, reference: reference)
        let lens = estimate.map(carriesLens) ?? false
        if lens, !distortion { return .refused(lensDistortionReason) }
        let bound = lens ? crossCheckLensBound : crossCheckBound
        // Vision's answer, steps 1–4 and the projection's reduction, as before
        // phase 3a.
        let shift = translationalRequest(floating, on: handler)
        let trusted = shift.map { Homography.translation(dx: $0.dx, dy: $0.dy) }.flatMap {
            vouched($0) ? $0 : nil
        }
        func visionAnswer() -> (Homography, Method)? {
            guard registration != .translation else { return trusted.map { ($0, .translation) } }
            let answer: (Homography, Method)
            if let shift, let composed = twoPass(floating, shift: shift, frame: frame, on: handler),
               vouched(composed) {
                answer = (composed, .homography)
            } else if let trusted {
                answer = (trusted, .translation)
            } else if let single = homographicRequest(floating, on: handler,
                                                      height: reference.height),
                      vouched(single) {
                answer = (single, .homography)
            } else {
                return nil
            }
            guard registration == .collage else { return answer }
            // A translation is already a similarity; reported as what it is.
            if answer.1 == .translation { return answer }
            if let reduced = answer.0.similarityFit(over: frame), vouched(reduced) {
                return (reduced, .similarity)
            }
            return trusted.map { ($0, .translation) }
        }
        let vision = visionAnswer()
        let judge = estimate.map { judgedEstimate($0, registration: registration) }
        func confirmed(_ map: Homography) -> Bool {
            guard let judge else { return true }
            return crossCheckError(map, on: judge) <= bound
        }
        if registration == .translation || registration == .collage {
            // Ruling 5: a similarity Vision answers keeps the cross-check;
            // Vision's T and the inliers' own reduction are judged by the
            // agreement score, and the higher one that is vouched answers.
            if let vision, vision.1 == .similarity, confirmed(vision.0) {
                let (map, method) = settled(vision.0, vision.1)
                return .aligned(map, method)
            }
            let reduced = estimate.flatMap { estimate -> (Homography, Method)? in
                guard coversOverlap(estimate, frame: frame),
                      let map = inlierReduction(estimate, registration: registration) else {
                    return nil
                }
                return (map, registration == .collage ? .similarity : .translation)
            }
            var best: (map: Homography, method: Method, score: Double)?
            for candidate in [trusted.map { ($0, Method.translation) }, reduced].compactMap({ $0 })
            where candidate.0.isUsable(for: frame) {
                let score = agreement.score(candidate.0)
                guard score >= agreementThreshold, score > best?.score ?? -.infinity else {
                    continue
                }
                best = (candidate.0, candidate.1, score)
            }
            guard let best else { return .refused(unregisteredReason) }
            let (map, method) = settled(best.map, best.method)
            return .aligned(map, method)
        }
        if let vision, confirmed(vision.0) {
            let (map, method) = settled(vision.0, vision.1)
            return .aligned(map, method)
        }
        // The feature map is judged usable over the part of the frame it
        // carries into the frame (`Homography.overlap(in:)`): a real
        // 40° yaw at 50 % overlap stretches the far side 4.4×, past the
        // 1/4–4 band, but that side lies outside the reference, where no
        // inlier or coverage cell judges the map anyway.
        guard let estimate, coversOverlap(estimate, frame: frame),
              estimate.map.overlap(in: frame).map({ estimate.map.isUsable(for: $0) }) ?? false,
              agreement.score(estimate.map) >= agreementThreshold,
              confirmed(estimate.map) else {
            return .refused(unregisteredReason)
        }
        let (map, method) = settled(estimate.map, .homography)
        return .aligned(map, method)
    }

    /// Vision's translational registration of `floating` to the handler's
    /// image, y down. nil when the request throws, answers nothing or answers
    /// a non-finite value.
    private static func translationalRequest(_ floating: CGImage, on handler: VNImageRequestHandler)
        -> (dx: Double, dy: Double)?
    {
        // Each request runs in its own pool: a worker's pool drains only when
        // its job ends, and one request's working copies must be gone before
        // the next one's are made, or a registration costs their sum.
        autoreleasepool {
            let request = VNTranslationalImageRegistrationRequest(
                targetedCGImage: floating, options: [:])
            guard (try? handler.perform([request])) != nil,
                  let shift = request.results?.first?.alignmentTransform,
                  shift.tx.isFinite, shift.ty.isFinite else { return nil }
            // y up to y down: the vertical shift changes sign (the file header).
            return (Double(shift.tx), -Double(shift.ty))
        }
    }

    /// Vision's homographic registration of `floating` to the handler's
    /// image, `height` pixels tall. nil when the request throws or answers
    /// nothing.
    private static func homographicRequest(_ floating: CGImage, on handler: VNImageRequestHandler,
                                           height: Int) -> Homography? {
        autoreleasepool {
            let request = VNHomographicImageRegistrationRequest(
                targetedCGImage: floating, options: [:])
            guard (try? handler.perform([request])) != nil,
                  let warp = request.results?.first?.warpTransform else { return nil }
            return Homography(visionWarp: warp, height: Double(height))
        }
    }

    /// `register`'s step 2: the homography of `floating` shifted by `shift`
    /// rounded to whole pixels, composed with that shift. nil when the shift
    /// leaves the frame, the request fails, or the composed map's
    /// displacement at the frame centre is farther than `centreTolerance` of
    /// the shorter side from `shift`.
    private static func twoPass(_ floating: CGImage, shift: (dx: Double, dy: Double), frame: CGRect,
                                on handler: VNImageRequestHandler) -> Homography? {
        let dx = shift.dx.rounded(), dy = shift.dy.rounded()
        guard abs(dx) < frame.width, abs(dy) < frame.height else { return nil }
        let measured = autoreleasepool { () -> Homography? in
            guard let moved = shifted(floating, dx: Int(dx), dy: Int(dy)) else { return nil }
            return homographicRequest(moved, on: handler, height: floating.height)
        }
        guard let refined = measured else { return nil }
        let composed = refined.preceded(byTranslation: dx, dy: dy)
        let centre = CGPoint(x: frame.midX, y: frame.midY)
        guard let landed = composed.apply(centre) else { return nil }
        let apart = hypot(Double(landed.x - centre.x) - shift.dx, Double(landed.y - centre.y) - shift.dy)
        guard apart <= centreTolerance * Double(min(frame.width, frame.height)) else { return nil }
        return composed
    }

    /// `frame` moved by whole pixels (`dx` right, `dy` down) over the mid-grey
    /// the analysis frames use: a copy, nothing resampled.
    private static func shifted(_ frame: CGImage, dx: Int, dy: Int) -> CGImage? {
        let bitmap = CGImageAlphaInfo.noneSkipLast.rawValue
        let space = frame.colorSpace ?? ColorProfile.sRGB
        guard let context = CGContext(data: nil, width: frame.width, height: frame.height,
                                      bitsPerComponent: 8, bytesPerRow: 0, space: space,
                                      bitmapInfo: bitmap)
            ?? CGContext(data: nil, width: frame.width, height: frame.height, bitsPerComponent: 8,
                         bytesPerRow: 0, space: ColorProfile.sRGB, bitmapInfo: bitmap) else {
            return nil
        }
        context.setFillColor(CGColor(gray: 0.5, alpha: 1))
        context.fill(CGRect(x: 0, y: 0, width: frame.width, height: frame.height))
        context.interpolationQuality = .none
        // CoreGraphics' y runs up, so a shift down is a negative offset.
        context.draw(frame, in: CGRect(x: dx, y: -dy, width: frame.width, height: frame.height))
        return context.makeImage()
    }

    /// One registration's working copies: Vision's two `width × height`
    /// frames, each charged as `HorizonDetection.scratchBytes` charges the
    /// one frame its request reads, plus the feature step's
    /// (`featureScratch`: its grey copies and the core's peak).
    static func registrationScratch(width: Int, height: Int) -> UInt64 {
        let vision = 2 * HorizonDetection.scratchBytes(width: width, height: height)
        let (sum, overflow) = vision.addingReportingOverflow(
            featureScratch(width: width, height: height))
        return overflow ? UInt64.max : sum
    }

    /// What reading a typed layer's analysis thumbnail is charged per pixel of
    /// the layer: the thumbnail path takes a transient working set of about
    /// 8 bytes a layer pixel at 16 bits and at float (it quantizes the whole
    /// layer before fitting it; a byte layer's takes none). A float layer is
    /// charged its own bytes, 16 a pixel, twice that. A 16-bit layer's own
    /// bytes are only 8, which leaves no margin on the term — at 48 MP the
    /// plan came to 1.249 × the peak — so it is charged the existing 16-bit
    /// place figure, `ImageSourceFile.scratchPerPixel16`.
    static func thumbnailChargePerPixel(float: Bool, typed: Bool) -> UInt64 {
        float ? 16 : typed ? ImageSourceFile.scratchPerPixel16 : 0
    }

    /// The registration's whole plan: `registrationScratch` for the analysis
    /// frame, plus — for a 16-bit or float document — the thumbnail's
    /// transient for the largest layer analysed
    /// (`thumbnailChargePerPixel`; the layers are read one after the other).
    ///
    /// MEASUREMENT (`scripts/stack-admission/measure.py`, a Release
    /// app, a fresh process per case at an 8 GiB working limit,
    /// `MallocLargeCache=0`, three runs; E is the interval's peak
    /// phys_footprint over its start around one `auto_align_layers` call on
    /// two identical layers, which answers `unchanged`, so the interval is
    /// the registration alone; every baseline twin read E ≤ 81,920):
    ///   8-bit, 4032 × 3024: plan 117,440,512; E 63,275,104 / 63,242,336 /
    ///     63,291,512; 1.856 ×
    ///   16-bit, 4032 × 3024: plan 349,103,104; E 160,841,872 /
    ///     160,907,408 / 160,841,920; 2.170 ×
    ///   float, 4032 × 3024: plan 312,524,800; E 160,661,648 / 162,185,360 /
    ///     160,678,056; 1.927 ×
    ///   8-bit, 8064 × 6048: plan 117,440,512; E 16,072,776 / 16,007,216 /
    ///     16,105,568; 7.292 ×
    ///   16-bit, 8064 × 6048: plan 1,044,090,880; E 406,274,360 /
    ///     406,389,024 / 406,241,568; 2.569 ×
    /// A typed layer adds about 8 bytes a layer pixel to the 8-bit case's
    /// peak (97.6 MB at 12 MP, 390 MB at 48 MP). Charged its own 8 bytes a
    /// pixel, the 16-bit 48 MP plan would have been 507,609,088 against a
    /// need of 507,986,280: 1.249 ×, a miss. The two-pass registration's peak
    /// is the single request's: 8-bit E was 63,209,592 with one request.
    /// With warps and the union canvas (2026-10-16, case
    /// `register-4-jpeg8-24mp`: four 6000 × 4000 plane frames, three warped,
    /// the canvas grown to 6057 × 4045), the job's largest charge, a warp's
    /// geometry plan, was 3,077,490,544 (`working_memory_plan_bytes`) against
    /// E 384,024,888 / 382,943,520 / 383,877,432: 8.014 ×; this plan 111,837,184.
    /// On CMYK (2026-10-23; jpeg8 stacks converted with `convert_mode cmyk`, 8-bit, the
    /// thumbnail charged `scratchPerPixel16`; every baseline twin read E ≤ 65,536):
    ///   CMYK, 4032 × 3024 (`register-cmyk`): plan 451,318,400; E 151,257,184 /
    ///     29,147,160 / 151,289,952; 2.983 ×
    ///   CMYK warps (`register-4-cmyk-24mp`): the job's largest charge 905,785,500 against
    ///     E 662,684,200 / 675,856,912 / 663,814,672: 1.340 ×; this plan 658,845,824
    ///   CMYK Cylindrical (`project-3-cmyk-24mp`): 1,821,616,707 against E 765,805,144 /
    ///     766,984,792 / 771,015,256: 2.363 ×
    ///   CMYK Reduce Noise, median (`denoise-3-cmyk-24mp`): 1,713,934,972 against
    ///     E 802,603,656 / 802,275,976 / 804,274,824: 2.131 ×
    static func registrationPlan(frame: (width: Int, height: Int), largestLayerPixels: UInt64,
                                 chargePerPixel: UInt64) throws -> UInt64 {
        let (transient, overflow) = largestLayerPixels.multipliedReportingOverflow(by: chargePerPixel)
        guard !overflow else {
            throw DoctoredCoreError(message: "Working-memory calculation overflow.")
        }
        return try PrecisionDocumentDescriptor.sum(
            registrationScratch(width: frame.width, height: frame.height), transient)
    }
}

extension AutoAlign {
    /// The canvas united with the content bounds (`layerBounds`) of each of
    /// `layers`, in the canvas's own coordinates; nil when that is the canvas.
    /// Reveal All's union (`revealAllRect`) over the named layers only.
    static func alignedCanvas(of doc: RasterDocument, layers: [Int]) -> CGRect? {
        let canvas = CGRect(x: 0, y: 0, width: doc.width, height: doc.height)
        var union = canvas
        for index in layers {
            guard let bounds = doc.layerBounds(index), bounds.width > 0, bounds.height > 0 else {
                continue
            }
            union = union.union(CGRect(
                x: bounds.x, y: bounds.y, width: bounds.width, height: bounds.height))
        }
        return union == canvas ? nil : union
    }

    /// The refusal when the union canvas would pass the 100 MP canvas limit
    /// (`RasterImage.maxResizePixels`): "past the 100-megapixel limit", with
    /// the canvas it would need.
    static func tooBigMessage(width: Int, height: Int) -> String {
        "The aligned layers would need a \(width) × \(height)-pixel canvas, past the "
            + "100-megapixel limit. Nothing was changed."
    }
}

/// The agreement score `AutoAlign.register` vouches an estimate with: grey
/// copies of the two frames, at most `AutoAlign.agreementSide` on the longest
/// side, built once per registration.
private struct Agreement {
    let width: Int
    let height: Int
    /// Copy pixels per frame pixel.
    let scale: Double
    let floating: [Float]
    let reference: [Float]

    init?(floating: CGImage, reference: CGImage) {
        let longest = max(floating.width, floating.height, 1)
        let scale = min(1, Double(AutoAlign.agreementSide) / Double(longest))
        let width = max(1, Int((scale * Double(floating.width)).rounded()))
        let height = max(1, Int((scale * Double(floating.height)).rounded()))
        guard width >= 3, height >= 3,
              let first = Self.greys(floating, width: width, height: height),
              let second = Self.greys(reference, width: width, height: height) else { return nil }
        self.width = width
        self.height = height
        self.scale = Double(width) / Double(floating.width)
        self.floating = first
        self.reference = second
    }

    private static func greys(_ frame: CGImage, width: Int, height: Int) -> [Float]? {
        var bytes = [UInt8](repeating: 0, count: width * height)
        let drawn = bytes.withUnsafeMutableBytes { buffer -> Bool in
            guard let context = CGContext(
                data: buffer.baseAddress, width: width, height: height, bitsPerComponent: 8,
                bytesPerRow: width, space: CGColorSpaceCreateDeviceGray(),
                bitmapInfo: CGImageAlphaInfo.none.rawValue) else { return false }
            context.interpolationQuality = .high
            context.draw(frame, in: CGRect(x: 0, y: 0, width: width, height: height))
            return true
        }
        return drawn ? bytes.map(Float.init) : nil
    }

    /// How well `map` (floating frame pixels to reference frame pixels)
    /// carries one frame onto the other, −1…1: the zero-mean normalized
    /// correlation of the two copies' central-difference gradients, the
    /// reference resampled through the map onto the floating copy's grid,
    /// over the pixels whose whole stencil lands inside the reference. 0 when
    /// fewer than `AutoAlign.agreementMinimumShare` of the pixels do, or
    /// either copy's gradients are constant there.
    ///
    /// Why gradients and not the grey levels themselves: two misaligned
    /// frames of one scene still share their broad light and dark areas, so
    /// their levels correlate; their edges coincide only when the frames are
    /// aligned to about a copy pixel. A gain or offset between the frames (an
    /// exposure bracket) scales or cancels in a gradient and leaves the
    /// score alone. Why zero-mean: a smooth ramp (a clear sky, the admission
    /// fixtures' scene) has one gradient everywhere, which equals itself
    /// under ANY shift — without the means removed, a ramp shifted 1,293 px
    /// scored 0.74; with them, 0.07 (`scratch/vision-reach-10`, `ramp`).
    func score(_ map: Homography) -> Double {
        // Copy pixels → frame pixels → map → copy pixels: D(scale)·H·D(1/scale).
        let small = map.rescaled(fromAnalysisScale: 1 / scale)
        var moved = [Float](repeating: .nan, count: width * height)
        for y in 0..<height {
            for x in 0..<width {
                guard let q = small.apply(CGPoint(x: Double(x) + 0.5, y: Double(y) + 0.5)) else {
                    continue
                }
                // Pixel centres are at half-integers.
                let fx = Double(q.x) - 0.5, fy = Double(q.y) - 0.5
                guard fx >= 0, fy >= 0, fx <= Double(width - 1), fy <= Double(height - 1) else {
                    continue
                }
                let x0 = min(Int(fx), width - 2), y0 = min(Int(fy), height - 2)
                let tx = Float(fx - Double(x0)), ty = Float(fy - Double(y0))
                let i = y0 * width + x0
                let top = reference[i] + (reference[i + 1] - reference[i]) * tx
                let bottom = reference[i + width]
                    + (reference[i + width + 1] - reference[i + width]) * tx
                moved[y * width + x] = top + (bottom - top) * ty
            }
        }
        var count = 0
        var own = 0.0, other = 0.0, both = 0.0
        var ownX = 0.0, ownY = 0.0, otherX = 0.0, otherY = 0.0
        for y in 1..<(height - 1) {
            for x in 1..<(width - 1) {
                let i = y * width + x
                let left = moved[i - 1], right = moved[i + 1]
                let above = moved[i - width], below = moved[i + width]
                guard !left.isNaN, !right.isNaN, !above.isNaN, !below.isNaN else { continue }
                let mx = Double(right - left), my = Double(below - above)
                let gx = Double(floating[i + 1] - floating[i - 1])
                let gy = Double(floating[i + width] - floating[i - width])
                own += gx * gx + gy * gy
                other += mx * mx + my * my
                both += gx * mx + gy * my
                ownX += gx
                ownY += gy
                otherX += mx
                otherY += my
                count += 1
            }
        }
        guard Double(count) >= AutoAlign.agreementMinimumShare * Double(width * height),
              own > 0, other > 0 else { return 0 }
        let n = Double(count)
        let ownVariance = own - (ownX * ownX + ownY * ownY) / n
        let otherVariance = other - (otherX * otherX + otherY * otherY) / n
        let covariance = both - (ownX * otherX + ownY * otherY) / n
        // Gradients that are one constant vector (to rounding) leave nothing
        // to correlate.
        guard ownVariance > 1e-9 * max(own, 1), otherVariance > 1e-9 * max(other, 1) else {
            return 0
        }
        return covariance / (ownVariance * otherVariance).squareRoot()
    }
}

/// The ONE compound Auto-Align job: every registration, every warp and the
/// display preparation under one `RasterJobControl`, so Cancel/Esc stops it
/// between layers or inside a warp and a cancel publishes nothing. The
/// capture and staleness guards are `TransformWork.wait`'s; the compound shape
/// is `CanvasGeometryJob.swift`'s. The worker captures values only.
struct AutoAlignWork {
    let source: RasterDocument
    let request: AutoAlign.Request
    /// Each named layer's name and pixel rectangle, read on main.
    let layers: [Int: (name: String, rect: CGRect)]
    /// The colour space the analysis frames are drawn in, read on main.
    let space: CGColorSpace
    /// Cylindrical, Spherical and Geometric Distortion only: the core's
    /// working-memory context, read on main (`meshWarpBudget`).
    let projectionBudget: (limit: UInt64, other: UInt64)?

    /// Main thread: reads the layers' names and rectangles, and the colour
    /// space, from `source`.
    init(source: RasterDocument, request: AutoAlign.Request) throws {
        guard Thread.isMainThread else {
            throw DoctoredCoreError(message: "Capture the layers before alignment work starts.")
        }
        self.source = source
        self.request = request
        self.space = source.colorSpace
        self.projectionBudget = request.surface != nil || request.distortion
            ? try source.meshWarpBudget() : nil
        var layers: [Int: (name: String, rect: CGRect)] = [:]
        for index in [request.reference] + request.floating {
            guard let info = source.layerInfo(index) else {
                throw DoctoredCoreError(message: "Layer \(index) is out of range.")
            }
            layers[index] = (info.name, CGRect(x: info.offsetX, y: info.offsetY,
                                               width: info.width, height: info.height))
        }
        self.layers = layers
    }

    /// The registration's plan for this work's layers
    /// (`AutoAlign.registrationPlan` over the analysis frame and the largest
    /// layer named). Reads only values captured on main. The caller admits
    /// it before the worker starts.
    func registrationPlan() throws -> UInt64 {
        let analysis = AutoAlign.analysis(canvasWidth: source.width, canvasHeight: source.height)
        let largestLayer = layers.values.map { UInt64(max($0.rect.width * $0.rect.height, 0)) }
            .max() ?? 0
        return try AutoAlign.registrationPlan(
            frame: (analysis.width, analysis.height), largestLayerPixels: largestLayer,
            chargePerPixel: AutoAlign.thumbnailChargePerPixel(
                float: source.isFloat, typed: source.isHighDepth))
    }

    /// The job's worker loop, for a caller that owns the compound `control`
    /// (`wait` below; Reduce Noise from Stack's `StackModeWork`,
    /// StackMode.swift; Merge to HDR's one job, MergeHDR.swift; Auto-Blend's
    /// "Align first", `AutoBlendWork`, AutoBlend.swift): registers every
    /// floating layer to the reference and warps each vouched one, and
    /// answers the document with the warped layers — `source` itself when no
    /// layer moved — and what happened to each. Prepares no display. Runs on
    /// the worker, after `control.beginCompoundOperation()`.
    ///
    /// `registration` is `registrationPlan()`, already admitted. `budget` is
    /// the byte working-memory context read on main (nil for a typed
    /// document). The layers share `share` of the control's progress evenly.
    /// Each warp's geometry plan is admitted just before it runs. Throws
    /// `JobCancelled` on a cancel and `AutoAlign.emptyReferenceMessage` when
    /// the reference has nothing opaque (before any registration). Reads only
    /// captured values: safe on a worker.
    func alignedDocument(control: RasterJobControl, registration: UInt64,
                         budget: (limit: UInt64, other: UInt64)?,
                         share: ClosedRange<Double>) throws
        -> (document: RasterDocument, outcome: AutoAlign.Outcome)
    {
        if request.surface != nil {
            return try reprojectedDocument(control: control, registration: registration, share: share)
        }
        if request.distortion {
            return try planarLensDocument(control: control, registration: registration, share: share)
        }
        let source = self.source
        let request = self.request
        let layers = self.layers
        let space = self.space
        let analysis = AutoAlign.analysis(canvasWidth: source.width, canvasHeight: source.height)
        let referenceName = layers[request.reference]?.name ?? "The reference"
        // Every frame and registration runs in its own pool, so one
        // layer's working copies are gone before the next layer's.
        guard let referenceFrame = try autoreleasepool(invoking: {
            try AutoAlign.analysisFrame(
                of: source, layer: request.reference, scale: analysis.scale, space: space)
        }) else {
            throw DoctoredCoreError(message: AutoAlign.emptyReferenceMessage(referenceName))
        }
        var running = source
        var aligned: [AutoAlign.Aligned] = []
        var unchanged: [AutoAlign.Skipped] = []
        var unregistered: [AutoAlign.Skipped] = []
        var largest = registration
        let count = Double(max(request.floating.count, 1))
        let span = share.upperBound - share.lowerBound
        for (position, layer) in request.floating.enumerated() {
            guard !control.isCancelled else { throw JobCancelled() }
            guard let (name, rect) = layers[layer] else { continue }
            func skip(_ reason: String) {
                unregistered.append(.init(layer: layer, name: name, reason: reason))
            }
            let found = try autoreleasepool {
                () throws -> (hasPixels: Bool, registered: AutoAlign.Registered?) in
                guard let frame = try AutoAlign.analysisFrame(
                    of: source, layer: layer, scale: analysis.scale, space: space) else {
                    return (false, nil)
                }
                return (true, AutoAlign.registered(
                    floating: frame, reference: referenceFrame,
                    registration: request.registration, distortion: false))
            }
            guard found.hasPixels else {
                skip("the layer has no pixels")
                continue
            }
            if case let .refused(reason)? = found.registered {
                skip(reason)
                continue
            }
            guard case let .aligned(measured, method)? = found.registered else {
                skip(AutoAlign.unregisteredReason)
                continue
            }
            let homography = measured.rescaled(fromAnalysisScale: analysis.scale)
            guard let quad = homography.quad(of: rect) else {
                skip(AutoAlign.unregisteredReason)
                continue
            }
            let moved = zip(quad, Homography.corners(of: rect)).contains {
                hypot($0.x - $1.x, $0.y - $1.y) > AutoAlign.unchangedThreshold
            }
            guard moved else {
                unchanged.append(.init(layer: layer, name: name, reason: "already aligned"))
                continue
            }
            do {
                let command = try GeometrySetRequest.perspective(
                    layers: [layer], sourceRect: rect, quad: quad, sampler: request.sampler
                ).command().request
                let scratch = try PrecisionDocumentDescriptor.integer(
                    running.geometryEditPlan(request: command), "scratch_bytes")
                _ = try RasterResources.shared.admit(scratch: scratch)
                largest = max(largest, scratch)
                let input = running
                let warped = try control.withChildOperation(
                    from: share.lowerBound + span * Double(position) / count,
                    to: share.lowerBound + span * Double(position + 1) / count
                ) { child -> RasterDocument? in
                    guard child.setScratchLimit(scratch) else {
                        throw DoctoredCoreError(message: "Could not bound alignment work.")
                    }
                    return try input.editingGeometry(request: command, budget: budget,
                                                     control: child)
                }
                if let warped {
                    running = warped
                    aligned.append(.init(layer: layer, name: name, method: method,
                                         homography: homography, quad: quad))
                } else {
                    unchanged.append(.init(layer: layer, name: name,
                                           reason: "already aligned"))
                }
            } catch let cancel as JobCancelled {
                throw cancel
            } catch {
                // A cancel is never a per-layer refusal.
                guard !control.isCancelled else { throw JobCancelled() }
                skip(error.localizedDescription)
            }
        }
        // The union canvas (take1 R2): grown, as Reveal All grows it, to hold
        // the reference and every layer that moved. Bounded once for every
        // projection: at most `RasterImage.maxResizePixels` (100 MP) and
        // within the channel budget, else a refusal and nothing changes.
        var canvas: CGRect?
        if request.growsCanvas, !aligned.isEmpty,
           let grown = AutoAlign.alignedCanvas(
               of: running, layers: [request.reference] + aligned.map(\.layer)) {
            let width = Int(grown.width), height = Int(grown.height)
            guard width <= RasterImage.maxResizePixels / max(height, 1) else {
                throw DoctoredCoreError(
                    message: AutoAlign.tooBigMessage(width: width, height: height))
            }
            if let reason = running.channelBudgetRefusal(width: width, height: height) {
                throw DoctoredCoreError(message: reason)
            }
            running = try running.canvasResized(
                w: width, h: height, originX: Int(-grown.minX), originY: Int(-grown.minY))
                ?? running
            canvas = grown
        }
        guard !control.isCancelled else { throw JobCancelled() }
        let outcome = AutoAlign.Outcome(
            reference: request.reference, aligned: aligned, unchanged: unchanged,
            unregistered: unregistered, registrationBytes: registration,
            planBytes: largest, canvas: canvas)
        return (running, outcome)
    }

    /// Runs the job and answers the prepared edit (nil when no layer moved)
    /// with what happened to each layer. Throws `JobCancelled` on a cancel,
    /// `AutoAlign.emptyReferenceMessage` when the reference has nothing
    /// opaque (before any registration), and a staleness sentence when the
    /// document changed underneath; none of them publishes anything.
    ///
    /// Progress: the layers share 0…0.8 evenly, the display preparation is
    /// 0.8…1. Admission is per stage, as `CanvasGeometryJob` admits: the
    /// registration scratch on main, each warp's geometry plan on the worker
    /// just before it runs, and `PreparedDocumentEdit` its own display.
    func wait(in document: ImageDocument, title: String) throws
        -> (prepared: PreparedDocumentEdit?, outcome: AutoAlign.Outcome)
    {
        assert(Thread.isMainThread)
        guard document.doc === source, !document.isLiveEditing else {
            throw DoctoredCoreError(message: "The document changed before the alignment started.")
        }
        let revision = document.revision
        let state = document.captureEditingState()
        let source = self.source
        let work = self
        let budget: (limit: UInt64, other: UInt64)? =
            source.isHighDepth ? nil : try source.legacyPrecisionWorkContext()
        let registration = try registrationPlan()
        _ = try RasterResources.shared.admit(scratch: registration)
        let control = try RasterJobControl()
        guard control.setScratchLimit(registration) else {
            throw DoctoredCoreError(message: "Could not bound alignment work.")
        }
        let result = try BackgroundWork.wait(
            title: title, control: control, revealAfter: DocumentJobs.panelDelay
        ) {
            try WorkerTask(scratchEstimate: registration) {
                () -> (prepared: PreparedDocumentEdit?, outcome: AutoAlign.Outcome) in
                try control.beginCompoundOperation()
                let (finished, outcome) = try work.alignedDocument(
                    control: control, registration: registration, budget: budget, share: 0...0.8)
                guard finished !== source else { return (prepared: nil, outcome: outcome) }
                let prepared = try control.withChildOperation(from: 0.8, to: 1) { child in
                    PreparedDocumentEdit(finished, scratch: BackgroundWork.scratch, control: child)
                }
                return (prepared: prepared, outcome: outcome)
            }
        }
        guard !control.isCancelled else { throw JobCancelled() }
        guard document.revision == revision, document.doc === source, !document.isLiveEditing,
              (!source.isHighDepth && source.retainedSourceIndices.isEmpty)
                || document.isCurrentEditingState(state) else {
            throw DoctoredCoreError(
                message: "The document changed while the alignment ran. Run \(title) again.")
        }
        return result
    }
}
