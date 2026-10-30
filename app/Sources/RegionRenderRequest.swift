import Foundation

/// A canvas window of a layered document's projection (Phase 13A-1): the
/// region in canvas pixels and a preview divisor. Divisor 1 is authoritative;
/// a larger one is nearest-sampled at each cell's top-left pixel and is a
/// preview only. The core refuses by name every stack it does not evaluate
/// regionally — there is no full-frame fallback.
struct RegionRenderRequest {
    var x: Int
    var y: Int
    var width: Int
    var height: Int
    var previewDivisor = 1

    func validate() throws {
        guard x >= 0, y >= 0, width > 0, height > 0, previewDivisor > 0,
              [x, y, width, height, previewDivisor].allSatisfy({ $0 <= Int(UInt32.max) }) else {
            throw DoctoredCoreError(message: "Invalid render region or preview divisor.")
        }
    }
}
