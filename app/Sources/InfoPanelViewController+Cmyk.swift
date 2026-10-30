import AppKit

// The Info panel's CMYK readout (#1 phases 2 and 5): the CMYK and Total ink rows. In a CMYK
// document they show the composite's ink under the cursor — the core's own sample (`cmyk_render`'s
// `cmyk_sample`, the same numbers `sample_color` reports), never a conversion of the view's RGB —
// as C, M, Y and K percent and their sum. The sample prepares the stack's masks (a feathered one
// blurs: ~270 ms at 24 MP), so it runs on the panel's debounced, coalescing `cmykLoader`, never in
// `mouseMoved`. On an RGB document (phase 5) they show the colour under the cursor separated
// through the working CMYK (`dr_cmyk_separate`, Color Settings' intent and BPC), with
// a "!" after each ink when the colour is out of the press's gamut (the proof's rule,
// CIE76 ΔE above 5 against its own round trip).

extension InfoPanelViewController {
    /// The two CMYK rows for the cursor at `point` (nil: "—"). `rgb` is an RGB document's colour
    /// there (encoded 0…1 in its profile); a CMYK document reads its own ink.
    func showCmykInk(at point: (x: Int, y: Int)?, rgb: [Double]? = nil) {
        guard isViewLoaded else { return }
        if let rgb, let doc = document?.doc, !doc.isCmyk {
            let profile = doc.iccProfile
            cmykLoader.onReport = { [weak self] in self?.showCmykRows($0) }
            cmykLoader.onFailure = { [weak self] _ in self?.showCmykRows(nil) }
            cmykLoader.requestReport {
                let separation = try RasterPrecisionDocument.separate(rgb: Array(rgb.prefix(3)),
                                                                      rgbProfile: profile, document: nil)
                let ink = [separation.ink.c, separation.ink.m, separation.ink.y, separation.ink.k]
                return ["ink_percent": ink, "total_ink_percent": separation.ink.total,
                        "out_of_gamut": separation.outOfGamut]
            }
            return
        }
        guard let point, let doc = document?.doc, doc.isCmyk, let precision = doc.precision,
              let context = try? PrecisionWorkContext(source: precision) else {
            cmykLoader.supersede()
            showCmykRows(nil)
            return
        }
        cmykLoader.onReport = { [weak self] in self?.showCmykRows($0) }
        cmykLoader.onFailure = { [weak self] _ in self?.showCmykRows(nil) }
        cmykLoader.requestReport {
            try precision.cmykSample(x: point.x, y: point.y, reach: 0, workingLimit: context.limit,
                                     otherRetained: context.otherRetained)
        }
    }

    /// One sample's `ink_percent` and `total_ink_percent` onto the rows; nil shows "—". An RGB
    /// colour outside the working CMYK's gamut marks each ink "!".
    private func showCmykRows(_ sample: [String: Any]?) {
        guard isViewLoaded else { return }
        var ink = "—", total = "—"
        if let percent = sample?["ink_percent"] as? [NSNumber], percent.count == 4 {
            let mark = sample?["out_of_gamut"] as? Bool == true ? "!" : ""
            ink = percent.map { String(format: "%.0f%%", $0.doubleValue) + mark }.joined(separator: ", ")
            if let sum = sample?["total_ink_percent"] as? NSNumber {
                total = String(format: "%.0f%%", sum.doubleValue)
            }
        }
        valueLabels[.cmyk]?.stringValue = ink
        valueLabels[.totalInk]?.stringValue = total
    }
}
