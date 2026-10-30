import AppKit

// Phase 12C: the UI-SOURCE half of the layers panel's filter bar. The MCP
// drive (L2) can only ask `find_layers`, which shares `LayerFilter.matches`
// with the panel but NOT the two rules the panel alone has: the flat walk
// (`visibleRows(collapsedHidden: false)`) that gives a match inside a CLOSED
// group a row of its own, and the skipped `mappedToVisibleRows` remap that
// makes a click on such a row select the CHILD rather than its closed
// ancestor. Both are exercised here through the real controls of the real
// `LayersPanelViewController`, on the `replay.swift` precedent.

/// The one control in `view`'s subtree whose menu lists `title` — the filter
/// bar's popups are private to the panel, so they are found the way a user
/// finds them: by what they say.
private func popup(in view: NSView, listing title: String) throws -> NSPopUpButton {
    guard let control = descendants(view).compactMap({ $0 as? NSPopUpButton })
        .first(where: { $0.itemTitles.contains(title) }) else {
        throw DoctoredCoreError(message: "Actual filter popup listing \"\(title)\" missing")
    }
    return control
}

/// Picks `title` in `control` and fires the control's OWN action, which is
/// what AppKit does when the user picks a menu item.
private func choose(_ control: NSPopUpButton, _ title: String) throws {
    control.selectItem(withTitle: title)
    try require(control.titleOfSelectedItem == title,
        "Actual popup would not select \"\(title)\"")
    guard let action = control.action else {
        throw DoctoredCoreError(message: "Actual filter popup has no action")
    }
    _ = control.sendAction(action, to: control.target)
    pump()
}

private func index(named name: String, in doc: RasterDocument) throws -> Int {
    guard let idx = (0..<doc.layerCount).first(where: { doc.layerInfo($0)?.name == name }) else {
        throw DoctoredCoreError(message: "Owned layer \"\(name)\" missing")
    }
    return idx
}

/// label-filter-panel-ui.
///
/// A labelled CHILD of a CLOSED group: with no filter it has no row at all;
/// filtering by its label gives it a row at its OWN indent; clicking that row
/// selects the child and not the group; clearing the filter puts the
/// collapsed walk back exactly as it was.
func labelFilterPanelUI(output: URL) throws -> [String: Any] {
    let (document, editor, _) = try openByteDocument()
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    let panel = editor.layersPanel!
    _ = panel.view

    // --- A group holding one labelled child, closed.
    _ = try call("new_layer", ["name": "Sky"], document: document)
    pump()
    guard let doc0 = document.doc else { throw DoctoredCoreError(message: "Owned document empty") }
    let sky = try index(named: "Sky", in: doc0)
    let grouped = try call("group_layers", ["layers": [sky], "name": "Folder"], document: document)
    try require(!(try responseIsError(grouped)), "group_layers refused: \(grouped)")
    pump()
    guard let doc1 = document.doc else { throw DoctoredCoreError(message: "Owned document empty") }
    let child = try index(named: "Sky", in: doc1)
    let folder = try index(named: "Folder", in: doc1)
    try require(doc1.layerIsGroup(folder) && doc1.layerDepth(child) > doc1.layerDepth(folder),
        "Owned fixture is not a child inside a group")
    let labelled = try call("set_layer_label", ["layer": child, "label": "red"], document: document)
    try require(!(try responseIsError(labelled)), "set_layer_label refused: \(labelled)")
    let closed = try call("set_layer_properties", ["index": folder, "open": false],
                          document: document)
    try require(!(try responseIsError(closed)), "set_layer_properties refused: \(closed)")
    pump()
    panel.reload(); pump()

    // --- Unfiltered: the child has NO row; the closed folder does.
    try require(!panel.filter.isActive, "The panel opened with a filter already active")
    let collapsedRows = panel.rows.map(\.index)
    try require(panel.row(forLayerIndex: child) == nil,
        "A child of a CLOSED group already had a row: \(collapsedRows)")
    try require(collapsedRows.contains(folder), "The closed folder has no row: \(collapsedRows)")
    let childDepth = panel.tree.depth(of: child)
    try require(childDepth > panel.tree.depth(of: folder), "The child is not indented deeper")

    // --- Filter by that label, through the panel's own popup.
    let labelPopup = try popup(in: panel.view, listing: "All Labels")
    try choose(labelPopup, LayerLabel.red.displayName)
    panel.view.layoutSubtreeIfNeeded()
    try screenshot(panel.view, output.appendingPathComponent("label-filter-panel-ui.png"))
    try require(panel.filter.isActive && panel.filter.label == .red,
        "The popup did not put the panel into a label filter")
    let filteredRows = panel.rows.map(\.index)
    try require(filteredRows == [child],
        "The filtered walk is not exactly the labelled child: \(filteredRows) vs [\(child)]")
    guard let childRow = panel.row(forLayerIndex: child) else {
        throw DoctoredCoreError(message: "The labelled child got no row while filtering")
    }
    // Its OWN indent, not the folder's: the flat match list keeps depth.
    try require(panel.rows[childRow].depth == childDepth,
        "The filtered row lost the child's indent: \(panel.rows[childRow].depth) vs \(childDepth)")
    try require(panel.rows[childRow].label == .red, "The filtered row lost its label")
    // The document never changed: the filter is a panel narrowing only.
    try require(document.doc?.layerCount == doc1.layerCount && document.doc?.layerOpen(folder) == false,
        "The filter changed the document")

    // --- Click it: the CHILD is selected, not its closed ancestor.
    document.selectLayer(0); panel.reload(); pump()
    try require(document.activeLayerIndex == 0, "Owned pre-click selection did not take")
    try require(!panel.isReloading, "The panel is mid-reload before the click")
    panel.tableView.selectRowIndexes(IndexSet(integer: childRow), byExtendingSelection: false)
    pump()
    try require(panel.tableView.selectedRow == childRow,
        "The actual table did not take the clicked row: \(panel.tableView.selectedRow)")
    // AppKit delivers the table's selection notification on its own schedule,
    // which a pumped probe cannot depend on — so the panel's REAL handler is
    // called with the real table already holding the clicked row, which is
    // what a click does. (Idempotent: the handler early-outs when the
    // selection it computes is the one the document already has.)
    panel.tableViewSelectionDidChange(
        Notification(name: NSTableView.selectionDidChangeNotification, object: panel.tableView))
    pump()
    let afterClick = document.activeLayerIndex
    try require(afterClick == child,
        "Clicking the filtered row selected \(afterClick), not the child \(child)"
            + (afterClick == folder ? " (it selected the CLOSED ANCESTOR)" : ""))

    // --- Clear the filter: the collapsed walk comes back, remap and all.
    try choose(labelPopup, "All Labels")
    try require(!panel.filter.isActive, "Clearing the popup left the filter active")
    let restored = panel.rows.map(\.index)
    try require(restored == collapsedRows,
        "The cleared panel did not restore the collapsed walk: \(restored) vs \(collapsedRows)")
    try require(panel.row(forLayerIndex: child) == nil,
        "The child kept a row after the filter cleared")
    // …and the selection is remapped onto the row the user can see again.
    try require(document.activeLayerIndex == folder,
        "The cleared panel left the selection off-row: \(document.activeLayerIndex)")
    return ["name": "label-filter-panel-ui", "passed": true, "child": child, "folder": folder,
            "collapsed_rows": collapsedRows, "filtered_rows": filteredRows,
            "child_row_depth": childDepth, "selected_after_click": afterClick,
            "restored_rows": restored]
}
