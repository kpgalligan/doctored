import Foundation

// The 32-bit float (linear) RAW development's opacity rule on hand-made buffers, with no camera
// file: `RetainedRawDecoder.normalizeOpaqueAlpha`, the twin of the 16-bit development's
// `UInt16.max` check. `scripts/verify_raw_float_cameras.py` drives it on real files.

/// "raw float alpha rule": an alpha within half a 16-bit code of 1, above or below, is stored as
/// exactly 1 with its RGB unchanged; any other alpha, NaN and infinity included, refuses. Each
/// row is one pixel. Up to 1 the accepted rows are exactly those the 16-bit development stores
/// as 65535 (the case's `code`: rounded to nearest, clamped); above 1, where that development
/// clamps every alpha to 65535, the rule still refuses one past half a code. Every row is
/// checked before the case fails.
func verifyRawFloatAlphaRule() throws {
    let tag = "[raw float alpha rule]"
    let half = Float(0.5 / 65535)
    let accepted: [(String, Float)] = [
        ("1", 1),
        ("1 - 2^-24 (one ulp under)", 1 - 0x1p-24),
        ("1 + 2^-23 (one ulp over)", 1 + 0x1p-23),
        ("0.9999996 (the measured Z 6 low)", 0.9999996),
        ("1.0000004 (the measured Z 6 high)", 1.0000004),
        ("1 - 0.5/65535", 1 - half),
        ("1 + 0.5/65535", 1 + half),
    ]
    let refused: [(String, Float)] = [
        ("1 - 2^-16 (code 65534)", 1 - 0x1p-16),
        ("0.9999", 0.9999),
        ("0.5", 0.5),
        ("0", 0),
        ("1.5", 1.5),
        ("NaN", .nan),
        ("infinity", .infinity),
    ]
    func code(_ alpha: Float) -> Int {
        alpha.isNaN ? -1 : Int((min(max(Double(alpha), 0), 1) * 65535).rounded())
    }
    var failures: [String] = []
    for (name, alpha) in accepted {
        let rgb: [Float] = [0.25, 1.75, -0.125]
        var samples = rgb + [alpha]
        let answer = RetainedRawDecoder.normalizeOpaqueAlpha(&samples)
        if !answer || samples != rgb + [1] {
            failures.append("\(name): accepted \(answer), now \(samples)")
        }
        if code(alpha) != 65535 { failures.append("\(name): the 16-bit code is \(code(alpha)), not 65535") }
    }
    for (name, alpha) in refused {
        var samples: [Float] = [0.5, 0.5, 0.5, 1, 0.5, 0.5, 0.5, alpha]
        if RetainedRawDecoder.normalizeOpaqueAlpha(&samples) {
            failures.append("\(name): accepted")
        }
        if alpha <= 1, code(alpha) == 65535 { failures.append("\(name): the 16-bit code is 65535") }
    }
    try require(failures.isEmpty, "\(tag) " + failures.joined(separator: "; "))
    print("PASS: raw float alpha rule — \(accepted.count) alphas within half a 16-bit code of 1 "
        + "(one ulp either side, the measured Z 6 extremes, the bound either side) become exactly 1 "
        + "with RGB unchanged; \(refused.count) others (code 65534, 0.9999, 0.5, 0, 1.5, NaN, "
        + "infinity) refuse; up to 1 the accepted alphas are exactly those the 16-bit development "
        + "stores as 65535")
}
