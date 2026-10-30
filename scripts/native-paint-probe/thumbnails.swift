import AppKit

// The Layers panel's thumbnail cache, proven by its own hit/miss
// counters rather than by timing. A rename changes no pixel, so after it
// every raster row must be served from the cache (hits, no new miss); a pixel
// edit on ONE layer must miss exactly once, for that layer. At 16 bits this
// is the typed hit path: `layerSource(idx)?.storageIdentity` staying equal
// across a document change.

/// The panel's private cache, read the way the other probes read private
/// state (`member`, main.swift).
private func thumbnailCounters(_ panel: LayersPanelViewController) throws
    -> (hits: Int, misses: Int)
{
    guard let cache = member(panel, "thumbnails") as? LayerThumbnailCache else {
        throw DoctoredCoreError(message: "The Layers panel has no thumbnail cache")
    }
    return (cache.hits, cache.misses)
}

/// Every row's cell, through the panel's own delegate method — the call
/// AppKit makes for a row it shows — so the counts do not depend on when a
/// window ordered behind others gets laid out.
private func renderCells(_ panel: LayersPanelViewController) {
    for row in panel.rows.indices {
        _ = panel.tableView(panel.tableView, viewFor: nil, row: row)
    }
}

private func thumbnailCacheCase(_ document: ImageDocument, _ editor: EditorViewController,
                                depth: Int) throws -> [String: Any] {
    let panel = editor.layersPanel!
    _ = panel.view
    let added = try call("new_layer", ["name": "Upper"], document: document)
    try require(!(try responseIsError(added)), "new_layer refused at \(depth) bits: \(added)")
    pump()
    panel.reload(); pump()
    let rasterRows = panel.rows.filter { !$0.isGroup }.count
    try require(rasterRows == 2, "Expected two raster rows at \(depth) bits, got \(rasterRows)")

    // Warm: every row rendered once, then once more from the cache.
    renderCells(panel); pump()
    renderCells(panel)
    let warm = try thumbnailCounters(panel)
    try require(warm.misses >= rasterRows,
        "The first render stored fewer thumbnails than rows at \(depth) bits: \(warm)")
    try require(warm.hits >= rasterRows,
        "An unchanged document re-rendered its thumbnails at \(depth) bits: \(warm)")

    // Rename: no pixel changes, so no row may miss.
    let renamed = try call("set_layer_properties", ["index": 1, "name": "Renamed"],
                           document: document)
    try require(!(try responseIsError(renamed)), "rename refused at \(depth) bits: \(renamed)")
    pump()
    try require(document.doc?.layerInfo(1)?.name == "Renamed", "The rename did not land")
    renderCells(panel); pump()
    let afterRename = try thumbnailCounters(panel)
    try require(afterRename.misses == warm.misses,
        "A rename re-rendered thumbnails at \(depth) bits: \(warm) -> \(afterRename)")
    try require(afterRename.hits - warm.hits >= rasterRows,
        "A rename was not served from the cache at \(depth) bits: \(warm) -> \(afterRename)")

    // A pixel edit on layer 0: exactly one new miss; the other row still hits.
    let edited = try call("apply_filter", ["index": 0, "filter": "invert"], document: document)
    try require(!(try responseIsError(edited)), "invert refused at \(depth) bits: \(edited)")
    pump()
    renderCells(panel); pump()
    let afterEdit = try thumbnailCounters(panel)
    try require(afterEdit.misses - afterRename.misses == 1,
        "A one-layer pixel edit missed \(afterEdit.misses - afterRename.misses) times at "
            + "\(depth) bits: \(afterRename) -> \(afterEdit)")
    try require(afterEdit.hits > afterRename.hits,
        "The untouched layer was not served from the cache at \(depth) bits")
    return ["depth": depth, "raster_rows": rasterRows,
            "warm": ["hits": warm.hits, "misses": warm.misses],
            "after_rename": ["hits": afterRename.hits, "misses": afterRename.misses],
            "after_edit": ["hits": afterEdit.hits, "misses": afterEdit.misses]]
}

/// layer-thumbnail-cache-ui.
func layerThumbnailCacheUI() throws -> [String: Any] {
    var results: [[String: Any]] = []
    do {
        let (document, editor, _) = try openDocument()
        defer { document.close(); pump() }
        try require(document.doc?.isHighDepth == true, "Owned fixture is not a 16-bit document")
        results.append(try thumbnailCacheCase(document, editor, depth: 16))
    }
    do {
        let (document, editor, _) = try openByteDocument()
        defer { document.close(); pump() }
        results.append(try thumbnailCacheCase(document, editor, depth: 8))
    }
    return ["name": "layer-thumbnail-cache-ui", "passed": true, "depths": results]
}
