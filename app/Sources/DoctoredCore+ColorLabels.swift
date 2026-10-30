import AppKit

extension RasterDocument {
    // MARK: - Colour labels

    /// The two `dr_doc_*_label` calls, here because this file is the only home
    /// for `dr_*`. The vocabulary, the depth branch and the panel's filter
    /// live in `LayerLabel.swift`.
    func withLayerLabelHandle(_ idx: Int, _ label: UInt32) -> RasterDocument? {
        guard isValidIndex(idx) else { return nil }
        return wrap(dr_doc_with_layer_label(ptr, idx, label))
    }

    func layerLabelCode(_ idx: Int) -> UInt32 {
        guard isValidIndex(idx) else { return 0 }
        return dr_doc_layer_label(ptr, idx)
    }
}
