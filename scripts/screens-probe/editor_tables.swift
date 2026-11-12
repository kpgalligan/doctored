import AppKit

// The editor's sheets and views, one table each: `sheetOpeners` (the sheets `open_sheet` opens,
// each through the editor method its menu item calls) and `editorViews` (the views a `view`
// capture cuts from the editor window). A new sheet or view is one entry here. An opener that
// cannot reach its sheet's entry point throws, so the failure names it rather than timing out.

let sheetOpeners: [String: (EditorViewController) throws -> Void] = [
    "levels": { $0.showLevels(nil) },
    "curves": { $0.showCurves(nil) },
    // Image ▸ Image Size… and Image ▸ Canvas Size….
    "image_size": { $0.resizeImage(nil) },
    "canvas_size": { $0.showCanvasSize(nil) },
    // File ▸ Export… and File ▸ Contact Sheet….
    "export": { $0.showExportWorkspace(nil) },
    "contact_sheet": { $0.showContactSheet(nil) },
    // Layer ▸ Layer Content Options… (the active fill layer's dialog), Layer ▸ Mask ▸
    // Properties… and Edit ▸ Auto-Align Layers….
    "layer_content_options": { $0.layerContentOptions(nil) },
    "mask_properties": { $0.showMaskProperties(nil) },
    "auto_align_layers": { $0.autoAlignLayers(nil) },
    // Select ▸ Select and Mask… (needs a selection), Select ▸ Color Range…, Select ▸ Transform
    // Selection… (needs a selection) and Select ▸ Load Selection….
    "select_and_mask": { $0.refineSelection(nil) },
    "color_range": { $0.showColorRange(nil) },
    "transform_selection": { $0.transformSelection(nil) },
    "load_selection": { $0.loadSelectionSheet(nil) },
    // The Paths panel's Make Selection… (needs a current path: `set_current_path`).
    "make_selection_from_path": { $0.makeSelectionFromPath(nil) },
    // Image ▸ Channels ▸ Channel Options… (needs a channel target: `set_channel_target`) and
    // Image ▸ Apply Image….
    "channel_options": { $0.channelOptions(nil) },
    "apply_image": { $0.applyImageSheet(nil) },
    // Layer ▸ Warp…, the re-openable field of the active layer.
    "warp": { $0.showPersistedWarp(nil) },
    // Image ▸ Adjustments ▸ Posterize… and HDR Toning…, and six of the Adjustments items that
    // share one selector (`adjustmentSheet`).
    "posterize": { $0.showPosterize(nil) },
    "hdr_toning": { $0.showHdrToning(nil) },
    "hue_saturation": adjustmentSheet(.hueSaturation),
    "color_balance": adjustmentSheet(.colorBalance),
    "black_and_white": adjustmentSheet(.blackAndWhite),
    "channel_mixer": adjustmentSheet(.channelMixer),
    "selective_color": adjustmentSheet(.selectiveColor),
    "gradient_map": adjustmentSheet(.gradientMap),
    // Filters ▸ Blur ▸ Radial Blur… and Tilt-Shift…, Filters ▸ Lens Correction…, Develop
    // Filter…, Liquify… and Filters ▸ Sharpen ▸ Smart Sharpen….
    "radial_blur": { $0.showRadialBlur(nil) },
    "tilt_shift": { $0.showTiltShift(nil) },
    "lens_correction": { $0.showLensCorrection(nil) },
    "develop_filter": { $0.showDevelopFilter(nil) },
    "liquify": { $0.showLiquify(nil) },
    "smart_sharpen": { $0.showSmartSharpen(nil) },
    // Edit ▸ Content-Aware Fill… (needs a selection), Edit ▸ Fill…, the rail's foreground swatch
    // (the Color Picker), the Gradient tool bar's Edit… (the stops) and Layer ▸ Artwork Colors…
    // (the active text or shape layer).
    "content_aware_fill": { $0.contentAwareFill(nil) },
    "fill": { $0.fillSelection(nil) },
    "color_picker": { $0.pickForegroundColor() },
    "gradient_stops": { $0.showGradientStops() },
    "artwork_colors": { $0.editArtworkColors(nil) },
    // View ▸ Proof Setup ▸ Custom…, Image ▸ Mode ▸ Convert to Profile…, Duotone… (an 8- or
    // 16-bit Grayscale or a Duotone document), Indexed Color…, Color Table… (an Indexed document),
    // Bitmap… (an 8-bit Grayscale document) and Convert Precision….
    "proof_setup": { $0.showProofSetup(nil) },
    "convert_to_profile": { $0.convertToProfile(nil) },
    "duotone": { $0.convertToDuotoneMode(nil) },
    "indexed_color": { $0.convertToIndexedMode(nil) },
    "color_table": { $0.showColorTable(nil) },
    "bitmap": { $0.convertToBitmapMode(nil) },
    "convert_precision": { $0.convertDocumentPrecision(nil) },
    // Filters ▸ Neural Filters ▸ Depth Blur… and Super Zoom…, and Edit ▸ Sky Replacement… (each
    // runs its model first: the screen `needs` "models"), and Edit ▸ Generative Fill… (needs a
    // selection).
    "depth_blur": { $0.showDepthBlur(nil) },
    "super_zoom": { $0.showSuperZoom(nil) },
    "sky_replacement": { $0.showSkyReplacement(nil) },
    "generative_fill": { $0.showGenerativeFill(nil) },
    // The Assistant panel's Chat Settings…, through the panel's own action (the controller keeps
    // both the panel and the action private).
    "chat_settings": { editor in
        let action = NSSelectorFromString("showChatSettings:")
        guard let panel = editor.children.first(where: { $0 is AssistantPanelViewController }) else {
            throw ProbeFailure(description:
                "open_sheet chat_settings: the editor has no Assistant panel")
        }
        guard panel.responds(to: action) else {
            throw ProbeFailure(description:
                "open_sheet chat_settings: the Assistant panel has no showChatSettings: action")
        }
        _ = panel.perform(action, with: nil)
    },
]

/// Image ▸ Adjustments ▸ <op> for the ops that share `showAdjustmentSheet`: the menu item's tag
/// is the op's index in `AdjustmentMenuOrder.newOps`, the list the menu builds itself from.
private func adjustmentSheet(_ op: AdjustmentLayerOp) -> (EditorViewController) throws -> Void {
    return { editor in
        guard let tag = AdjustmentMenuOrder.newOps.firstIndex(of: op) else {
            throw ProbeFailure(description: "open_sheet: \(op) is not in the Adjustments menu's order")
        }
        let item = NSMenuItem()
        item.tag = tag
        editor.showAdjustmentSheet(item)
    }
}

let editorViews: [String: (EditorViewController) -> NSView?] = [
    "layers_panel": { $0.layersPanel.view },
    "channels_panel": { $0.channelsPanel.view },
    "info_panel": { $0.infoPanel.view },
    "navigator_panel": { $0.navigatorPanel.view },
    "paths_panel": { $0.pathsPanel.view },
    // The controller keeps the Assistant panel private; it is one of its children.
    "assistant_panel": { editor in editor.children.first { $0 is AssistantPanelViewController }?.view },
    "options_bar": { $0.optionsBar },
    "tool_rail": { $0.toolRail },
    // The status bar is a local of loadView: the root's only BarView.
    "status_bar": { editor in
        let bars = editor.view.subviews.filter { $0 is BarView }
        return bars.count == 1 ? bars[0] : nil
    },
]

/// `open_sheet`: opens the sheet and waits until it is attached and drawn. A sheet taller than
/// the editor window hangs past its bottom, where a cut would clip it, so the window grows
/// downward (its top edge stays) until the sheet fits with a margin.
func openSheet(_ name: String, _ open: (EditorViewController) throws -> Void,
               _ context: Context) throws {
    let editor = try context.requireEditor()
    guard let window = context.window else { throw ProbeFailure(description: "no editor window") }
    try require(window.attachedSheet == nil, "open_sheet \(name): a sheet is already attached")
    try open(editor)
    do {
        try pumpUntil({ window.attachedSheet != nil }, seconds: 20)
    } catch {
        throw ProbeFailure(description: "open_sheet \(name): no sheet after 20 s")
    }
    settle(window.attachedSheet, seconds: 1)
    try fitSheet(window, context)
}

/// Grows `window` so its attached sheet ends at least `margin` above the window's bottom edge.
func fitSheet(_ window: NSWindow, _ context: Context) throws {
    let margin: CGFloat = 24
    guard let sheet = window.attachedSheet else { return }
    let overshoot = window.frame.minY + margin - sheet.frame.minY
    guard overshoot > 0 else { return }
    var frame = window.frame
    frame.origin.y -= overshoot
    frame.size.height += overshoot
    guard let screen = window.screen, screen.visibleFrame.contains(frame) else {
        throw ProbeFailure(description:
            "the sheet (\(sheet.frame.height) pt) is taller than the screen lets the window grow")
    }
    window.setFrame(frame, display: true)
    settle(window, seconds: 0.5)
    context.notes.append("window grown by \(Int(overshoot.rounded())) pt to hold the sheet")
}
