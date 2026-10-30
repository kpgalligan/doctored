import AppKit

extension RasterDocument {
    /// Layer `index`'s Align with Layer box `[x, y, w, h]` in canvas pixels
    /// (FillLayer.swift's ALIGN): the core's one rule over the layer's
    /// mask (`dr_precision_doc_fill_align_box` when typed, CMYK included,
    /// else `dr_doc_fill_align_box`) — the bounds of its coverage clipped to
    /// the canvas, else the canvas. `[0, 0, width, height]` when the core
    /// answers false (a bad index). No Swift-side rule.
    func fillAlignBox(_ index: Int) -> [Int] {
        let whole = [0, 0, width, height]
        guard index >= 0 else { return whole }
        var xywh = [Int32](repeating: 0, count: 4)
        let found = xywh.withUnsafeMutableBufferPointer { out -> Bool in
            if let precision {
                return dr_precision_doc_fill_align_box(precision.ptr, index, out.baseAddress)
            }
            return dr_doc_fill_align_box(ptr, index, out.baseAddress)
        }
        return found ? xywh.map(Int.init) : whole
    }
}
