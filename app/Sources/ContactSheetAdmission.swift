import Foundation

/// The contact sheet compose's working-memory plan: what
/// `dr_contact_sheet_compose` allocates beside its cells, admitted on main by
/// `ContactSheet.compose` before the compose runs as a job.
///
/// Record: the Release app at `DR_WORKING_LIMIT_BYTES` 8 GiB,
/// `MallocLargeCache=0`, macOS 26.5.1, a fresh instance per case beside a
/// baseline twin (the same sources, then `resource_usage`: E 16,384–147,456),
/// 3 runs, `scripts/contact-sheet-admission/measure.py`. Sources are white
/// 8-bit `new_document`s: S12 4000×3000, S48 8000×6000, S96 12000×8000, P48
/// 8000×6000 Display P3, L48 S48 plus an empty layer. E =
/// `ri_interval_max_phys_footprint` − pre around `contact_sheet_from_sources
/// {documents}`, read once the footprint was still for 2 s, so the delivered
/// sheet document and its projection are inside it. E per run / plan / P÷E:
///   one-S12 (sheet 1104×312) 241,139,904 / 241,041,600 / 240,943,296  329,521,792  1.367
///   one-S48 962,527,904 / 962,740,896 / 962,609,824  1,229,521,792  1.277
///   one-S96 1,921,877,360 / 1,921,844,592 / 1,921,811,824  2,429,521,792  1.264
///   one-P48 962,855,608 / 962,806,456 / 962,609,848  1,229,521,792  1.277
///   one-L48 962,495,160 / 962,495,160 / 962,527,928  1,229,521,792  1.277
///   sheet-57 (S12 ×196, 14 columns, 512² cells: 7408×7744) 1,828,816,168 /
///     1,828,947,240 / 1,828,783,424  2,439,376,640  1.334
///   sheet-57-export (the same, PNG to a folder) 1,828,865,320 / 1,828,947,264 /
///     1,829,029,160  2,439,376,640  1.334
///   sheet-98 (680² cells: 9760×10096) 3,143,960,832 / 3,144,173,824 /
///     3,144,239,336  3,962,644,736  1.260
/// (≈ 20 B per cell source pixel from 48 to 96 MP, and ≈ 32 B per sheet pixel
/// from 57 to 98 MP: the composed sheet, the delivered document and its first
/// projection.) Rule: every case's plan ≥ ⌈1.25 × max E⌉, F rounded up to a
/// MiB. Result: F = 16 MiB, a = 37, b = 25; sheet-98 binds (32,345,566 to
/// spare), then one-S96 (27,175,092). The provisional constants (64 MiB, 12,
/// 40) measured the same E within 0.02 % and failed the sheet cases.
/// At 1 GiB, sheet-98 is refused "on its own" (gate R).
enum ContactSheetAdmission {
    static let composeFixed: UInt64 = 16 * 1024 * 1024
    static let composePerSheetPixel: UInt64 = 37
    static let composePerCellPixel: UInt64 = 25
    static let remedy = "Use fewer or smaller sources, or a smaller cell size."

    /// F + a·sheet pixels + b·the largest cell's source pixels,
    /// overflow-checked through PrecisionDocumentDescriptor.sum.
    static func composeScratch(sheetWidth: Int, sheetHeight: Int,
                               largestCellPixels: UInt64) throws -> UInt64 {
        let (sheet, overflow) = UInt64(max(sheetWidth, 0)).multipliedReportingOverflow(
            by: UInt64(max(sheetHeight, 0)))
        let (sheetTerm, sheetOverflow) = sheet.multipliedReportingOverflow(by: composePerSheetPixel)
        let (cellTerm, cellOverflow) = largestCellPixels.multipliedReportingOverflow(
            by: composePerCellPixel)
        guard sheetWidth >= 0, sheetHeight >= 0, !overflow, !sheetOverflow, !cellOverflow else {
            throw DoctoredCoreError(message: "Working-memory calculation overflow.")
        }
        return try PrecisionDocumentDescriptor.sum(
            PrecisionDocumentDescriptor.sum(composeFixed, sheetTerm), cellTerm)
    }
}
