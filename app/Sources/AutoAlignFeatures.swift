import AppKit

/// `AutoAlign.register`'s feature step. The core's feature
/// registration (`FeatureRegistration`, `feature_register.rs`) runs on grey
/// copies of the two analysis frames. Its inliers then do these jobs:
/// - they CROSS-CHECK whichever homographic or similarity answer `register`
///   would take (`crossCheckError`; a similarity on the same reduction of
///   them, `judgedEstimate`);
/// - their own similarity or translation is the planar feature candidate
///   (`inlierReduction`, ruling 5);
/// - they decide whether the feature map itself may answer: the coverage and
///   residual gate (`coversOverlap`), then the same agreement gate Vision's
///   maps pass;
/// - they carry THE LENS TEST (rulings 2 to 4): how far
///   Geometric Distortion's one-coefficient model, fitted on them, departs
///   from the plain homography where they are (`carriesLens`).
/// Reads only its arguments; safe on a worker.
extension AutoAlign {
    /// The feature step's answer: the core's map, its RANSAC inliers as
    /// (floating point, reference point) pairs in frame pixels (top-left
    /// origin, y down), and the lens test's departure on them, frame pixels
    /// (nil when no lens model fits).
    struct FeatureEstimate {
        let map: Homography
        let pairs: [(floating: CGPoint, reference: CGPoint)]
        let departure: Double?
    }

    /// The least departure, frame pixels, at which the lens test is
    /// SIGNIFICANT (ruling 2's 1.0 px, declared in physical
    /// terms before the table it was judged on; ruling 3 measures it at the
    /// matches, ruling 4 at the worst one). The departure is the largest
    /// distance, over the feature inliers, between the lens model's
    /// prediction, D_k(P_k(D_k⁻¹(p))), and the plain homography's
    /// (`feature_register.rs`). A pixel where the
    /// matches are is a misfit a plain homography leaves behind where the eye
    /// sees edges meet. It is measured there, not extrapolated to the corner,
    /// because k alone is not pinned when the frames barely move (identical
    /// layers fit any k), while the departure then is near zero. Evidence,
    /// not its source: at most 0.2 px (worst point) on every lens-free pair
    /// measured, identical layers included, and 2.96–3.42 px (worst point) on
    /// the k −0.05 pairs. On a narrow overlap the inliers' strip can sit
    /// where the two models still agree; the reach table's 306 × 204 pair
    /// 20° apart departed 0.70 px there while its plain answer was 8.76 px
    /// off. Since a frame under `featureUpsampleBelow` is detected on its
    /// upsampled level, whose margin is half as wide, its inliers reach
    /// toward the edge and it departs 1.42 px (every k −0.05 reach row 1.24 px
    /// or more, every lens-free one 0.08 px or less). A departure over the
    /// whole overlap was tried and set aside: it extrapolates k past the
    /// inliers, over a canvas-sized frame's empty surround too, and called a
    /// lens-free pair of tiles a lens.
    static let lensSignificance = 1.0

    /// The cross-check's bound when Geometric Distortion is on and the lens
    /// test is significant, frame pixels: the distortion fit's pass-1 search,
    /// ±8 steps (`align_distortion.rs`'s `SEARCH`) at a step of one pixel,
    /// the spec's declared lens bound. A seed within it is one the fit can
    /// still match from; the fit, not the seed, is then the answer.
    static let crossCheckLensBound = 8.0

    /// The reason a layer whose frames fail the lens test is not aligned while
    /// Geometric Distortion is off (ruling 2's sentence).
    static let lensDistortionReason =
        "the frames carry lens distortion; turn on Geometric Distortion"

    /// Whether the estimate's lens test is significant: its departure is at
    /// least `lensSignificance`. false when no lens model fits.
    static func carriesLens(_ estimate: FeatureEstimate) -> Bool {
        guard let departure = estimate.departure else { return false }
        return departure >= lensSignificance
    }

    /// The longer frame side below which the core detects on a 2× upsampled
    /// level first: `feature_points.rs`'s `UPSAMPLE_BELOW` (the master).
    static let featureUpsampleBelow = 512

    /// How far from a frame's edge the core detects no feature, in frame
    /// pixels: `feature_points.rs`'s `MARGIN` (the master), 27 pixels of the
    /// finest level, so a descriptor's turned window fits. That is 27 frame
    /// pixels, or 13.5 when the longer side is under `featureUpsampleBelow`
    /// and the finest level is the upsampled one. Inside that border neither
    /// frame can hold a match, so the coverage gate does not count it.
    static func featureEdgeMargin(width: Int, height: Int) -> Double {
        max(width, height) < featureUpsampleBelow ? 13.5 : 27.0
    }

    /// The coverage gate's grid: 4 × 4 cells over the overlap, so that each
    /// quadrant of the overlap is 2 × 2 cells.
    static let featureCoverageGrid = 4

    /// The share of the overlap's eligible cells the inliers must hold, and
    /// the count must be strictly MORE than it. Fixed before the after table
    /// (ruling 1). A homography's eight parameters are pinned
    /// only where its inliers are. A map whose inliers hold more than half the
    /// overlap interpolates over most of it, and is not extrapolated from
    /// one part. A fit to one corner of a field no homography follows (a
    /// lens-distorted pair) holds at most a quadrant, 4 of 16 cells, and a
    /// fit to one side at most 8; both fail. A fit over the whole overlap
    /// that misses a few flat cells (a clipped sun, a black roof) passes.
    static let featureCoverageShare = 0.5

    /// The largest RMS distance, frame pixels, of the inliers from the
    /// feature map. It is half the core's inlier distance (`INLIER_DISTANCE`,
    /// 3.0 px), and was fixed before the after table (ruling 1).
    /// - A correct fit's residual is the matches' own position error. The
    ///   core states that as half a pixel at level 0 and a pixel or two on
    ///   coarser levels; three quarters of the features are at level 0, and
    ///   a pair carries two positions. That gives about 1.0 px RMS.
    /// - A map that cannot follow the field keeps every match within 3.0 px,
    ///   so its inliers fill that disc, and a uniform disc of radius r has an
    ///   RMS of r/√2 = 2.1 px.
    /// - 1.5 px lies between the two.
    static let featureResidualBound = 1.5

    /// The worst cell-mean reprojection error, frame pixels, that a candidate
    /// may show on the feature inliers (`crossCheckError`). This is the reach
    /// table's plain bound, 2.0 px, and was fixed before the after table
    /// (ruling 1). Each inlier's reference point is the truth plus the
    /// match's zero-mean position error, so a cell's mean error is the
    /// candidate's own displacement error over that cell. The worst cell is
    /// then the worst error where the matches can see, which is the table's
    /// measure. With Geometric Distortion on and the lens test significant,
    /// `crossCheckLensBound` applies instead.
    static let crossCheckBound = 2.0

    /// The fewest inliers a cross-check cell is judged on. One match's
    /// position error is about 1.0 px RMS (`featureResidualBound`), so the
    /// mean of four is 0.5 px, a quarter of `crossCheckBound`: noise alone
    /// cannot fail a correct candidate's cell.
    static let crossCheckCellInliers = 4

    /// The feature inliers' own reduction for a planar registration (ruling
    /// 5): the least-squares similarity (`collage`) or the mean translation
    /// (`translation`) from their floating points to their reference points
    /// (`Homography.similarity`, `meanTranslation`: the closed forms
    /// `similarityFit` and `translationFit` use). It is the planar feature
    /// candidate, and Collage's similarity answers are cross-checked against
    /// it (`judgedEstimate`). nil for the homographic registrations, or when
    /// the reduction does not exist.
    static func inlierReduction(_ estimate: FeatureEstimate, registration: Registration)
        -> Homography?
    {
        let source = estimate.pairs.map(\.floating), image = estimate.pairs.map(\.reference)
        switch registration {
        case .translation: return Homography.meanTranslation(from: source, to: image)
        case .collage: return Homography.similarity(from: source, to: image)
        case .auto, .perspective, .cylindrical, .spherical: return nil
        }
    }

    /// The matches `registration`'s answers are cross-checked on (ruling 4
    /// (b), kept by ruling 5 for similarity answers): for `collage`, each
    /// inlier's floating point paired with where `inlierReduction` carries
    /// it, so a similarity answer is judged against the reduced registration;
    /// for every other registration, the estimate itself (`translation`'s
    /// answers are judged by the agreement score instead, ruling 5). The
    /// estimate itself when the reduction or a point under it does not exist.
    static func judgedEstimate(_ estimate: FeatureEstimate, registration: Registration)
        -> FeatureEstimate
    {
        guard registration == .collage,
              let reduced = inlierReduction(estimate, registration: registration) else {
            return estimate
        }
        var pairs: [(floating: CGPoint, reference: CGPoint)] = []
        pairs.reserveCapacity(estimate.pairs.count)
        for pair in estimate.pairs {
            guard let landed = reduced.apply(pair.floating) else { return estimate }
            pairs.append((floating: pair.floating, reference: landed))
        }
        return FeatureEstimate(map: reduced, pairs: pairs, departure: estimate.departure)
    }

    /// The core's map from `floating` to `reference` and its inliers (frame
    /// pixels, top-left origin, y down — the core's convention is
    /// `Homography`'s, so nothing is flipped). nil when the frames differ in
    /// size, a grey copy cannot be drawn, or the core finds no estimate or
    /// refuses the size.
    static func featureEstimate(floating: CGImage, reference: CGImage) -> FeatureEstimate? {
        guard floating.width == reference.width, floating.height == reference.height else {
            return nil
        }
        return autoreleasepool { () -> FeatureEstimate? in
            guard let first = greyFrame(floating), let second = greyFrame(reference),
                  let fit = try? FeatureRegistration.register(
                      floating: first, reference: second, width: floating.width,
                      height: floating.height) else { return nil }
            let values = fit.pairs
            let pairs = stride(from: 0, to: values.count - 3, by: 4).map { i in
                (floating: CGPoint(x: values[i], y: values[i + 1]),
                 reference: CGPoint(x: values[i + 2], y: values[i + 3]))
            }
            return FeatureEstimate(map: Homography(m: fit.map), pairs: pairs,
                                   departure: fit.departure)
        }
    }

    /// The coverage and residual gate for one-region fits. It decides whether
    /// the feature map may answer at all, before the agreement score is asked.
    /// - Residual: the inliers' RMS distance from the map is within
    ///   `featureResidualBound`.
    /// - The overlap: the floating points inside the frame inset by
    ///   `featureEdgeMargin` whose image under the map is also inside it.
    ///   Its rectangle is the bounding box of a 33 × 33 sample grid's points
    ///   that fall in it.
    /// - Coverage: `featureCoverageGrid` cells a side over that rectangle. A
    ///   cell is eligible when its centre lies in the overlap. The eligible
    ///   cells holding an inlier must be more than `featureCoverageShare` of
    ///   the eligible ones.
    static func coversOverlap(_ estimate: FeatureEstimate, frame: CGRect) -> Bool {
        guard let measured = featureCoverage(estimate, frame: frame) else { return false }
        return measured.rms <= featureResidualBound && measured.eligible > 0
            && Double(measured.covered) > featureCoverageShare * Double(measured.eligible)
    }

    /// `coversOverlap`'s measurements: the inliers' RMS distance from the map,
    /// and the eligible and covered cells. nil when the map cannot carry an
    /// inlier, or the overlap is empty.
    static func featureCoverage(_ estimate: FeatureEstimate, frame: CGRect)
        -> (rms: Double, covered: Int, eligible: Int)?
    {
        let map = estimate.map
        guard !estimate.pairs.isEmpty else { return nil }
        var squared = 0.0
        for pair in estimate.pairs {
            guard let landed = map.apply(pair.floating) else { return nil }
            squared += pow(landed.x - pair.reference.x, 2) + pow(landed.y - pair.reference.y, 2)
        }
        let rms = (squared / Double(estimate.pairs.count)).squareRoot()
        let margin = featureEdgeMargin(width: Int(frame.width), height: Int(frame.height))
        let inner = frame.insetBy(dx: margin, dy: margin)
        guard inner.width > 0, inner.height > 0 else { return nil }
        func inOverlap(_ point: CGPoint) -> Bool {
            guard inner.contains(point), let landed = map.apply(point) else { return false }
            return inner.contains(landed)
        }
        var minX = Double.infinity, minY = Double.infinity
        var maxX = -Double.infinity, maxY = -Double.infinity
        for j in 0...32 {
            for i in 0...32 {
                let point = CGPoint(x: inner.minX + Double(i) * inner.width / 32,
                                    y: inner.minY + Double(j) * inner.height / 32)
                guard inOverlap(point) else { continue }
                minX = min(minX, point.x)
                maxX = max(maxX, point.x)
                minY = min(minY, point.y)
                maxY = max(maxY, point.y)
            }
        }
        guard maxX > minX, maxY > minY else { return nil }
        let side = featureCoverageGrid
        let box = CGRect(x: minX, y: minY, width: maxX - minX, height: maxY - minY)
        var held = [Bool](repeating: false, count: side * side)
        for pair in estimate.pairs {
            held[cell(of: pair.floating, in: box, side: side)] = true
        }
        var eligible = 0, covered = 0
        for row in 0..<side {
            for column in 0..<side {
                let centre = CGPoint(x: box.minX + (Double(column) + 0.5) * box.width / Double(side),
                                     y: box.minY + (Double(row) + 0.5) * box.height / Double(side))
                guard inOverlap(centre) else { continue }
                eligible += 1
                if held[row * side + column] { covered += 1 }
            }
        }
        return (rms, covered, eligible)
    }

    /// The candidate's error on the feature inliers, frame pixels: over
    /// `featureCoverageGrid` cells a side laid on the inliers' bounding box,
    /// the length of the worst cell's mean reprojection vector,
    /// mean(map(p) − q), among the cells holding at least
    /// `crossCheckCellInliers` inliers. 0 when no cell holds that many (the
    /// matches cannot judge); infinite when the map cannot carry an inlier.
    static func crossCheckError(_ map: Homography, on estimate: FeatureEstimate) -> Double {
        guard let first = estimate.pairs.first?.floating else { return 0 }
        var minX = first.x, minY = first.y, maxX = first.x, maxY = first.y
        for pair in estimate.pairs {
            minX = min(minX, pair.floating.x)
            maxX = max(maxX, pair.floating.x)
            minY = min(minY, pair.floating.y)
            maxY = max(maxY, pair.floating.y)
        }
        let side = featureCoverageGrid
        let box = CGRect(x: minX, y: minY, width: maxX - minX, height: maxY - minY)
        var sums = [(dx: Double, dy: Double, count: Int)](repeating: (0, 0, 0), count: side * side)
        for pair in estimate.pairs {
            guard let landed = map.apply(pair.floating) else { return .infinity }
            let index = cell(of: pair.floating, in: box, side: side)
            sums[index].dx += landed.x - pair.reference.x
            sums[index].dy += landed.y - pair.reference.y
            sums[index].count += 1
        }
        var worst = 0.0
        for sum in sums where sum.count >= crossCheckCellInliers {
            worst = max(worst, hypot(sum.dx / Double(sum.count), sum.dy / Double(sum.count)))
        }
        return worst
    }

    /// The row-major index of the `side × side` cell of `box` holding
    /// `point`; a point outside the box (or a box of no extent) clamps to the
    /// nearest cell.
    private static func cell(of point: CGPoint, in box: CGRect, side: Int) -> Int {
        func index(_ value: Double, _ start: Double, _ length: Double) -> Int {
            guard length > 0 else { return 0 }
            let raw = ((value - start) / length * Double(side)).rounded(.down)
            guard raw.isFinite else { return 0 }
            return Int(min(max(raw, 0), Double(side - 1)))
        }
        return index(point.y, box.minY, box.height) * side + index(point.x, box.minX, box.width)
    }

    /// `frame` drawn at its own size into 8-bit DeviceGray, row 0 at the
    /// top, unpadded; nil when the context cannot be made.
    static func greyFrame(_ frame: CGImage) -> [UInt8]? {
        let width = frame.width, height = frame.height
        guard width > 0, height > 0 else { return nil }
        var bytes = [UInt8](repeating: 0, count: width * height)
        let drawn = bytes.withUnsafeMutableBytes { buffer -> Bool in
            guard let context = CGContext(
                data: buffer.baseAddress, width: width, height: height, bitsPerComponent: 8,
                bytesPerRow: width, space: CGColorSpaceCreateDeviceGray(),
                bitmapInfo: CGImageAlphaInfo.none.rawValue) else { return false }
            context.interpolationQuality = .none
            context.draw(frame, in: CGRect(x: 0, y: 0, width: width, height: height))
            return true
        }
        return drawn ? bytes : nil
    }

    /// The feature step's working bytes at a `width × height` analysis frame:
    /// - the two grey copies;
    /// - the inlier buffer (`FeatureRegistration.maxPairs` pairs of four
    ///   doubles);
    /// - the core's own peak (`FeatureRegistration.scratchBytes`).
    static func featureScratch(width: Int, height: Int) -> UInt64 {
        let pixels = UInt64(max(0, width)).multipliedReportingOverflow(by: UInt64(max(0, height)))
        let copies = pixels.partialValue.multipliedReportingOverflow(by: 2)
        guard !pixels.overflow, !copies.overflow else { return UInt64.max }
        let buffer = UInt64(4 * FeatureRegistration.maxPairs * MemoryLayout<Double>.size)
        let (sum, overflow) = copies.partialValue.addingReportingOverflow(
            FeatureRegistration.scratchBytes(width: width, height: height))
        let (total, more) = sum.addingReportingOverflow(buffer)
        return overflow || more ? UInt64.max : total
    }
}
