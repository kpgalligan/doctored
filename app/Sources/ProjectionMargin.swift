import Foundation

// `RasterDocument.projectionScratchEstimate()` returns its structural estimate through
// `ProjectionMargin.charged` (+ m/16, fitted per depth, record beside the constants), so every
// caller takes the margin.

/// The display projection's charge: the structural estimate plus a fitted margin, per depth.
/// `RasterDocument.projectionScratchEstimate()` returns every arm through `charged`, so each caller
/// (the publication preflight, `PreparedDocumentEdit`, `ByteFilterAdmission`, masks, opens) takes it.
/// An 8-bit publication charges the margined projection inside
/// `ByteFilterAdmission.commitStageBytes` (plus the n×4 result and written layer).
///
/// Record (batch 165; Release app at `DR_WORKING_LIMIT_BYTES` 8 GiB, a fresh
/// instance per case, `MallocLargeCache=0`, E = `ri_interval_max_phys_footprint`
/// − pre around the one MCP call, 3 runs plus a baseline twin at E 0 ×3; R =
/// `unique_retained − cache`). Rule: the smallest m ≥ 4 per depth for which every
/// row's charge P is ≥ ⌈1.25 × max E⌉, P being the largest per-gate charge plus
/// that gate's retained growth.
/// - C1, photo45.png (8256×5504) `apply_filter` invert: E 1,631,257,752 /
///   1,631,274,160 / 1,631,257,752; structural 24n 1,090,584,576; R
///   363,550,219 → 545,320,845. P = `commitStageBytes(updated)` + ΔR.
/// - C2, the same Box Blur r5: E 1,632,961,688 / 1,633,109,144 /
///   1,633,174,680; the up-front gate (plan 1,372,168,320) is below the
///   publication's.
/// - C3, png8-48 (8064×6048) invert: E 1,752,892,664 / 1,752,827,128 /
///   1,752,810,744; structural 1,170,505,728; R 390,191,823 → 585,282,977.
/// - PB-8 (RAW redevelop publish, `scripts/raw-decode-probe`, D24 / D26): E
///   480,248,144 / 519,438,720; structural 576,000,000 / 623,001,600.
/// - H1, the twice-turned two-layer 24 MP 16-bit `.rz` reopened: E
///   2,517,436,168 / 2,521,712,392 / 2,521,384,712; structural 1,458,127,550
///   (render-bound); R 0 → 1,790,050,327; P = max(the reader's 2,663,682,924,
///   R + charge).
/// - H2, P16.png (6000×4000, 16-bit) invert: E 764,248,592 / 765,395,496 /
///   764,166,672; structural 578,608,290; R 481,086,365 → 674,153,917; the
///   edit's own plan 442,500,718 is below.
/// - PB-16 (D24 / D26): E 576,291,224 / 623,329,760; structural 578,608,290 /
///   625,698,376.
/// Fit: m8 = 7 (the maintainer's decision; the rule alone gave 6) — C1
/// charge 1,567,715,328, `commitStageBytes` 1,931,243,520, P 2,113,014,146
/// (1.295×), C2 1.294×, C3 1,682,601,984 / 2,072,770,560 / P 2,267,861,714
/// (1.294×), PB-8 828,000,000 / 895,564,800 (1.724×); m = 6 gave C1 1.254×,
/// m = 5 1.212×. m16 = 6 — H2
/// 988,653,951 (1.292×; m = 5 gives 1.244×), H1 3,794,975,709 (1.505×), PB-16
/// 1.381×. Before the margin the structural estimates were 1.0× (PB-16
/// 1.004×, H2's ΔR + structural 1.008×).
///
/// Record (batch 168; the same harness at 60ebea3): 45 MP 8-bit
/// `apply_filter` on photo45.png and on noise (p8-45.png), 3 runs each, flat
/// gray (flat45.png) for invert; twin E 0 ×3. Every row has n 45,441,024,
/// structural 1,090,584,576, R 363,550,219 → 545,320,845, so P =
/// `commitStageBytes(updated)` + ΔR = 2,044,852,610 for all at m8 = 6, the
/// P/E below (the byte filters' up-front max(plan, `commitStageBytes`) was
/// 1,863,081,984, below). At m8 = 7 P = 2,113,014,146 for all
/// (up-front 1,931,243,520), lowest 1.292× (noise Invert).
/// Max E photo45 / noise and P/E at m8 = 6:
/// - Byte filters: Box Blur r5 1,633,305,752 / 1,633,256,600 (1.252×);
///   Unsharp Mask 1,632,584,880 / 1,632,371,936 (1.253×); Add Grain
///   1,633,191,064 / 1,633,207,448 (1.252×); Dust & Scratches r1
///   1,634,911,456 / 1,634,796,768 (1.251×); Reduce Noise 1,613,874,400 /
///   1,613,808,864 (1.267×); Clouds 1,633,453,232 / 1,633,404,104,
///   Difference Clouds 1,633,453,256 / 1,633,486,024, Offset 1,633,453,232 /
///   1,633,240,240 (1.252×). Surface Blur T15 r5–r1, Dust & Scratches r2 and
///   Radial Blur Draft were refused by the work limit, so had no row; that limit
///   was later lifted for jobs, and their rows are in
///   `designs/working-limit-ram-scaled.md` (with 45 MP runs at both
///   depths).
/// - Publication only: Gaussian σ4 1,632,158,896 / 1,631,896,752 (1.253×);
///   Add Noise 1,632,519,344 / 1,632,502,960, Levels 1,632,519,344 /
///   1,632,568,496 (1.253×); Pixelate 8 1,635,026,072 / 1,635,026,072, Invert
///   1,635,042,456 / 1,635,075,272 (1.251×); flat Invert 1,634,993,328
///   (1.251×).
/// - 16-bit: H2 (P16) invert 765,035,072, P 988,653,951 (1.292×); H2n (noise
///   p16-24) 765,035,096, P 1,084,653,991 (1.418×); H1n (the H1 build with
///   p16-24, reopened) 2,520,581,872, P = R 1,790,050,378 + charge =
///   3,794,975,760 (1.506×).
/// Cause: none by content. Noise and photo45 peak within 82 KB of flat
/// (MALLOC_LARGE pixel buffers at the peak sample); batch 167's 3.8–6.6 MB
/// higher noise rows were not reproduced, with or without its extra argv.
/// Outcome C: every row passes at m8 = 6 and m16 = 6, unchanged.
/// Outcome: m8 = 7 by the maintainer's decision, m16 = 6.
enum ProjectionMargin {
    static let byteSixteenths: UInt64 = 7
    static let precisionSixteenths: UInt64 = 6

    /// structural + ⌈structural · m / 16⌉ (m by depth); throws DoctoredCoreError on overflow.
    static func charged(_ structural: UInt64, highDepth: Bool) throws -> UInt64 {
        let margin = highDepth ? precisionSixteenths : byteSixteenths
        let (scaled, overflow) = structural.multipliedReportingOverflow(by: margin)
        guard !overflow else {
            throw DoctoredCoreError(message: "Working-memory calculation overflow.")
        }
        let (rounded, roundOverflow) = scaled.addingReportingOverflow(15)
        guard !roundOverflow else {
            throw DoctoredCoreError(message: "Working-memory calculation overflow.")
        }
        return try PrecisionDocumentDescriptor.sum(structural, rounded / 16)
    }
}
