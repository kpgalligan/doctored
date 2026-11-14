import AppKit

extension EditorViewController {
    @objc func showDocumentPrecision(_ sender: Any?) {
        guard let document, let doc = document.doc else { return }
        let alert = NSAlert()
        alert.messageText = doc.isHighDepth ? "16-bit Integer • Float Evaluation" : "8-bit Document"
        alert.informativeText = (doc.isHighDepth ? "The master contains up to 65,536 values per channel. Display is an explicit 8-bit preview.\n\n" : "The master contains 256 values per channel.\n\n")
            + DocumentPrecision.limitation
        if let window = view.window { alert.beginSheetModal(for: window) }
    }

    /// View-only items: they change window state, never document pixels. Their
    /// MCP twins: Rotate View / Reset View Rotation → `set_view`'s `rotation`;
    /// New View → `open_second_view`; Sync Pan & Zoom → `sync_pan_zoom`;
    /// Hide/Show Panels: none (window chrome). The grid-spacing,
    /// grid-subdivision and snap presets have none (the view-preference
    /// exemption).
    private static let viewOnlyActions: Set<String> = ["rotateView:", "resetViewRotation:", "newView:",
        "toggleSyncPanZoom:", "setGridSpacing:", "setGridSubdivisions:", "snapToAll:", "snapToNone:",
        "showViewExposure:",
        "resetViewExposure:",
        "togglePanels:"]

    /// Window items that read no pixels through a byte-only path.
    /// - Layer Comps: the `*_layer_comp` tools answer at both depths, and
    ///   `editingLayerComps`/`layerCompsJSON` route typed.
    /// - Workspaces: the `*_workspace` tools answer at both depths;
    ///   `applyWorkspace` already skips a tool the depth refuses.
    /// - Navigator: `get_view`/`set_view`.
    /// - Proof Setup ▸ Delete Broken File: a proof-preset library write
    ///   (`delete_proof_preset {file}`, document-free).
    /// - Edit ▸ Patterns: pattern-library writes (`delete_pattern`,
    ///   `import_patterns`, `export_patterns`, document-free).
    /// - Assistant: no MCP twin. Its tab is already reachable at every depth
    ///   through the panels' `onShowAssistant` closures, and every tool it
    ///   runs passes `execute`'s depth gate.
    private static let depthIndependentWindowActions: Set<String> = ["showLayerComps:",
        "saveWorkspace:", "restoreWorkspace:", "deleteWorkspace:", "deleteWorkspaceFile:",
        "showNavigator:", "showAssistant:", "deleteProofPresetFile:",
        "deleteLibraryPattern:", "importPatternLibrary:", "exportPatternLibrary:",
        // View ▸ Clear Color Samplers writes view state, never pixels.
        "clearColorSamplers:"]

    /// View preferences operate independently of document pixel storage.
    private static let viewOnlyActionPrefixes: [String] = ["toggleRuler", "toggleGrid", "toggleSnap",
        "setRulerUnit", "setGuideColor", "toggleGuides", "toggleLockGuides", "toggleColorSamplers",
        "toggleShowClipping"]

    /// `nil` from the precision gate for these hands the item to the existing
    /// rules exactly as at 8 bits (`validateViewChromeItem`, `validateViewsItem`,
    /// the session guard and the per-selector cases).
    private static func isDepthIndependentAction(_ action: String) -> Bool {
        viewOnlyActions.contains(action) || depthIndependentWindowActions.contains(action)
            || viewOnlyActionPrefixes.contains { action.hasPrefix($0) }
    }

    /// The four transform set-count checks: false when `action` is ⌘T, ⌥⌘T, Transform Again or
    /// its ⌥ form with no selection over a set the high-depth session cannot take as one entry.
    private func transformSetCountAllows(_ action: String, _ document: ImageDocument) -> Bool {
        if action == "freeTransform:", document.canvasSelection == nil {
            guard let doc = document.doc else { return false }
            let set = doc.movingSet(document.selectedLayerIndices)
            if set.count != 1 { return false }
        }
        // With no selection ⌥⌘T copies the originals' subtrees, which
        // the 16-bit session admits as exactly one entry, as ⌘T does.
        if action == "freeTransformCopy:", document.canvasSelection == nil {
            guard let doc = document.doc else { return false }
            if MoveDuplicateLayers.subtrees(of: document.selectedLayerIndices, in: doc).count != 1 {
                return false
            }
        }
        // Transform Again repeats over ⌘T's target, which the 16-bit
        // session admits as exactly one entry; with ⌥ over the originals'
        // subtrees, as ⌥⌘T does.
        if action == "transformAgain:", document.canvasSelection == nil {
            guard let doc = document.doc else { return false }
            if doc.movingSet(document.selectedLayerIndices).count != 1 { return false }
        }
        if action == "transformAgainWithCopy:", document.canvasSelection == nil {
            guard let doc = document.doc else { return false }
            if MoveDuplicateLayers.subtrees(of: document.selectedLayerIndices, in: doc).count != 1 {
                return false
            }
        }
        return true
    }

    /// Before the existing selector-specific enablement, so an unsupported
    /// method can never reach a nil byte handle through the responder chain.
    func validatePrecisionItem(_ item: NSValidatedUserInterfaceItem) -> Bool? {
        // Image ▸ Adjustments ▸ HDR Toning… at every depth.
        if let allowed = validateHdrToningItem(item) { return allowed }
        // #1: Image ▸ Mode ▸ RGB Color / CMYK Color answer at every depth.
        if let allowed = validateCmykModeItem(item) { return allowed }
        // #1 Indexed phase 1c: Image ▸ Mode ▸ Color Table…, on an Indexed document only.
        if let allowed = validateColorTableItem(item) { return allowed }
        // #1 Bitmap phase 1a: a Bitmap document's own closed whitelist (`BitmapDocument.selectors`),
        // asked before every other rule but the Mode items.
        if let document, document.doc?.isBitmap == true, let selector = item.action {
            let action = NSStringFromSelector(selector)
            if Self.isDepthIndependentAction(action) { return nil }
            return BitmapDocument.selectors.contains(action) ? nil : false
        }
        // #1 Indexed phase 1a: an Indexed Color document's own closed whitelist
        // (`IndexedDocument.selectors`), asked before every other rule but the Mode items.
        if let document, document.doc?.isIndexed == true, let selector = item.action {
            let action = NSStringFromSelector(selector)
            if Self.isDepthIndependentAction(action) { return nil }
            return IndexedDocument.selectors.contains(action) ? nil : false
        }
        // #1 Lab phase 1: a Lab Color document's own closed whitelist (`LabDocument.selectors`),
        // asked before every other rule but the Mode items.
        if let document, document.doc?.isLab == true, let selector = item.action {
            // Phase 5a: Image ▸ Mode ▸ 8/16 Bits/Channel, the declared depth checked (32 falls
            // through to the whitelist, which lacks it).
            if let allowed = validateBitDepthItem(item) { return allowed }
            let action = NSStringFromSelector(selector)
            if Self.isDepthIndependentAction(action) { return nil }
            // Phase 4a: the transform set-count checks (since phase 4b-i ⌘T, ⌥⌘T, Transform Again
            // and Puppet and Perspective Warp with a selection transform the selected pixels).
            if !transformSetCountAllows(action, document) { return false }
            // Phase 6b-ii: in Quick Mask, Fill…, its chords and Gaussian Blur edit the Quick
            // Mask's colour-free coverage (the high-depth rule below), as Gray's.
            if document.quickMaskActive, [
                "fillSelection:", "fillSelectionFromDeleteKey:", "fillWithForeground:",
                "fillWithBackground:", "fillWithForegroundPreserving:",
                "fillWithBackgroundPreserving:", "showBlur:",
            ].contains(action) { return nil }
            // Phase 2c: the Filter menu on the layer; phase 2d: on its mask or an alpha channel,
            // and phase 2e on the Lightness, a, b and alpha planes, where the luma filters run as
            // on RGB (`allowsScalarFilterAction`, MCP's `LabAdjust.toolRefusal` alike); and Repeat
            // Last Filter as the Gray branch.
            if FloatDocument.filterActions[action] != nil {
                guard !document.quickMaskActive else { return false }
                switch document.planeEditTarget {
                case .layer: return LabAdjust.allowsFilterAction(action) ? nil : false
                case .mask, .channel, .plane:
                    return LabAdjust.allowsScalarFilterAction(action) ? nil : false
                default: return false
                }
            }
            if action == "repeatLastFilter:" {
                guard let step = ActionRecorder.shared.lastRepeatable else { return nil }
                return DocumentPrecision.refusal(
                    tool: step.tool, arguments: step.arguments, document: document) == nil ? nil : false
            }
            // Phase 2b: the adjustment items by `LabAdjust`'s table.
            let adjusts = LabAdjust.allowsAction(action, tag: item.tag)
            return LabDocument.selectors.contains(action) || adjusts == true ? nil : false
        }
        // #1 phase 1a: a Grayscale document's own closed whitelist (`GrayDocument.selectors`),
        // asked before every other rule but the Mode items.
        if let document, document.doc?.isGray == true, let selector = item.action {
            // #1 Duotone: what a Duotone document disables beyond Gray's rules.
            if document.doc?.isDuotone == true,
               DuotoneDocument.refuses(selector: NSStringFromSelector(selector),
                                       proofColorsOn: proofDisplay.proofColors) {
                // A refused depth item still shows the declared depth's check.
                _ = validateBitDepthItem(item)
                return false
            }
            // Phase 5a: Image ▸ Mode ▸ 8/16/32 Bits/Channel, the declared depth checked.
            if let allowed = validateBitDepthItem(item) { return allowed }
            let action = NSStringFromSelector(selector)
            if Self.isDepthIndependentAction(action) { return nil }
            let depth = document.doc?.grayDepth ?? 16
            // Phase 4: the transform set-count checks, and in Quick Mask Fill…, its chords and
            // Gaussian Blur edit the Quick Mask's colour-free coverage (the high-depth rule below).
            if !transformSetCountAllows(action, document) { return false }
            if document.quickMaskActive, [
                "fillSelection:", "fillSelectionFromDeleteKey:", "fillWithForeground:",
                "fillWithBackground:", "fillWithForegroundPreserving:",
                "fillWithBackgroundPreserving:", "showBlur:",
            ].contains(action) { return nil }
            // Phase 2c: the Filter menu on the layer, or on its mask (at 32 bits only the plane
            // filters, as the float branch below), and Repeat Last Filter as the float branch;
            // phase 2d: an alpha channel answers as the mask (both colour-free u16 planes).
            if let filter = FloatDocument.filterActions[action] {
                guard !document.quickMaskActive, GrayAdjust.allowsFilterAction(action, depth: depth)
                else { return false }
                switch document.planeEditTarget {
                case .layer: return nil
                case .mask, .channel:
                    return depth != 32 || FloatDocument.planeFilterOps.contains(filter) ? nil : false
                default: return false
                }
            }
            if action == "repeatLastFilter:" {
                guard let step = ActionRecorder.shared.lastRepeatable else { return nil }
                return DocumentPrecision.refusal(
                    tool: step.tool, arguments: step.arguments, document: document) == nil ? nil : false
            }
            // Phase 2d: Invert Channel at 8 and 16 bits only.
            if action == "invertChannel:", depth == 32 { return false }
            // Phase 5b: Proof Setup and Proof Colors at 8 and 16 bits; a 32-bit document refuses
            // as a float document.
            if depth == 32, GrayDocument.proofSelectors.contains(action) { return false }
            // Phase 6b: Apply Image, Merge to HDR and the neural and generative filters refuse a
            // 32-bit document as a float document.
            if depth == 32, GrayDocument.floatRefused.contains(action) { return false }
            // The close: the tone map is a 32-bit document's.
            if depth != 32, GrayDocument.floatOnly.contains(action) { return false }
            let adjusts = GrayAdjust.allowsAction(action, tag: item.tag, depth: depth)
            return GrayDocument.selectors.contains(action) || adjusts == true ? nil : false
        }
        // #1 phase 5: Image ▸ Mode ▸ 8/16 Bits/Channel at every depth.
        if let allowed = validateBitDepthItem(item) { return allowed }
        guard let document, document.doc?.isHighDepth == true, let selector = item.action else { return nil }
        let action = NSStringFromSelector(selector)
        if Self.isDepthIndependentAction(action) { return nil }
        if !transformSetCountAllows(action, document) { return false }
        // In Quick Mask, Fill… and its five chords fill the Quick
        // Mask's native u16 coverage (`QuickMaskFill`), not document pixels,
        // so they answer through `validateFillItem` at 16 and 32 bits alike.
        // Gaussian Blur there softens the same coverage
        // (`QuickMaskBlur`), answered by `validateQuickMaskEditItem`. Asked
        // before the CMYK whitelist (#1): the coverage is colour-free.
        if document.quickMaskActive, [
            "fillSelection:", "fillSelectionFromDeleteKey:", "fillWithForeground:",
            "fillWithBackground:", "fillWithForegroundPreserving:", "fillWithBackgroundPreserving:",
            "showBlur:",
        ].contains(action) { return nil }
        // #1: a CMYK document's own closed whitelist (`CmykDocument.selectors`, and the tagged
        // adjustment items `CmykDocument.allowsTagged` admits), asked before every other 16-bit
        // and float rule; below the four transform set-count checks (phase 4b), which hold for
        // CMYK as they do at 16 bits, and Quick Mask's coverage edits.
        if document.doc?.isCmyk == true {
            let depth = CmykAdjustFilter.depth(of: document.doc)
            // The adjustments batch: the Filter menu by `CmykAdjustFilter`'s table on a layer's
            // own pixels, and Repeat Last Filter by the latched step's refusal (the float rule).
            // Liquify is phase 4b's (`CmykDocument.selectors`).
            // The remaining-gaps batch: with one ink plate or an alpha channel targeted, an item
            // is enabled when the CMYK table enables it AND the 16-bit rule does for a colour plane
            // or channel target (`sixteenBitActions`); the per-target validation then follows.
            let target = document.planeEditTarget
            let scalarTarget = target.isInk || target.isChannel
            if FloatDocument.filterActions[action] != nil, action != "showLiquify:" {
                return CmykAdjustFilter.allowsFilterAction(action, depth: depth)
                    && (target == .layer || (scalarTarget && Self.sixteenBitActions.contains(action)))
                    ? nil : false
            }
            if action == "repeatLastFilter:" {
                guard let step = ActionRecorder.shared.lastRepeatable else { return nil }
                return DocumentPrecision.refusal(tool: step.tool, arguments: step.arguments,
                                                 document: document) == nil ? nil : false
            }
            let cmyk = CmykDocument.selectors.contains(action)
                || CmykDocument.allowsTagged(action, tag: item.tag, depth: depth)
            let sixteenBit = Self.sixteenBitActions.contains(action) || action.hasPrefix("newAdjustmentLayer")
            if scalarTarget, Self.targetedActions.contains(action) {
                return cmyk && sixteenBit ? nil : false
            }
            return cmyk ? nil : false
        }
        // A 32-bit FLOAT document admits a far narrower set than the u16 one
        // (`FloatDocument.swift`), so its whitelist is separate and closed:
        // anything not named here is disabled rather than reaching an
        // operation the core would refuse.
        if document.doc?.isFloat == true {
            // Without a selection these four are layer transforms,
            // admitted with the set-count checks above. With one they
            // transform the selected pixels (`beginFloatingTransform`,
            // `transformAgainSelectedPixels`): a layer's, a mask or
            // channel's u16 plane, and one colour plane of the layer's f32
            // pixels.
            let float: Set<String> = [
                // v1: Edit ▸ Fade… (`validateFadeItem`), over the typed
                // `dr_precision_doc_fade` in the modes a float document admits.
                "showFade:",
                "showToneMap:", "showEvaluation:", "showDocumentPrecision:",
                "convertDocumentPrecision:", "showInfo:", "showHistory:", "showSnapshots:",
                "toggleLayersPanel:", "showChannels:", "undo:", "redo:",
                "zoomInAction:", "zoomOutAction:", "zoomActualAction:", "zoomFitAction:",
                "duplicateLayer:", "groupLayers:", "ungroupLayers:", "deleteLayer:",
                // One typed `duplicate_to_new_document` request.
                "duplicateToNewDocument:",
                "bringToFront:", "bringForward:", "sendBackward:", "sendToBack:",
                "toggleClippingMask:",
                "addAdjustmentLayer:", "addLayerMaskRevealAll:", "addLayerMaskHideAll:",
                "addLayerMaskFromSelection:", "deleteLayerMask:", "toggleLayerMaskEnabled:",
                "showMaskProperties:", "newChannel:", "duplicateChannel:", "renameChannel:",
                "channelOptions:", "deleteChannel:", "saveSelectionSheet:", "loadSelectionSheet:",
                "loadPortraitMatte:", "selectAll:", "deselect:", "reselect:", "showActions:",
                // A selection is canvas coverage, not document pixels, so the
                // selection work admitted here is exactly what
                // `FloatDocument.tools` answers over MCP.
                "invertSelection:", "featherSelection:", "growSelection:", "shrinkSelection:",
                "borderSelection:", "smoothSelection:", "toggleQuickMask:",
                // Select ▸ Subject, Objects…, People…, All People and Face Parts… read the tone-mapped display (Vision);
                // Select ▸ Sky and Person Parts… run their models on the same display.
                "selectSubject:", "showSubjectTargets:", "showPeopleTargets:", "selectPeople:", "showFaceParts:",
                "selectSky:", "showPersonParts:",
                // Color Range, Grow and Similar compare the R1 display copy
                // (`display_encoding`); MCP twins `select_color_range`, `select_similar`.
                "showColorRange:", "selectGrow:", "selectSimilar:",
                // Select ▸ Focus Area… reads the same R1 codes; MCP twin `select_focus_area`.
                "showFocusArea:",
                // Select ▸ Select and Mask… — the edge stage reads the same R1 codes (rule E),
                // the New Layer outputs keep the float composite; MCP twin `refine_selection`.
                "refineSelection:",
                // Phase 12E-1c: guides and the ruler origin are document
                // state at every depth now (`precision_guide`).
                "newGuide:", "clearGuides:",
                // Paths and vector masks are document state at every
                // depth (`precision_path`, `precision_vector_mask`). Stroke
                // Path runs the float Brush or Eraser along the path.
                "strokeCurrentPath:",
                // Edit ▸ Content-Aware Fill… on a float layer (`FloatRetouch`).
                "contentAwareFill:",
                // #1: Image ▸ Adjustments ▸ Equalize on the display encoding (`equalize_apply`).
                "equalize:",
                "makeSelectionFromPath:", "fillCurrentPath:", "makeWorkPath:",
                "duplicateCurrentPath:", "deleteCurrentPath:", "toggleClippingPath:",
                "editClippingPathFlatness:", "combinePaths:", "convertPathToShape:",
                "addVectorMaskFromCurrentPath:", "addVectorMaskRevealAll:",
                "deleteVectorMask:", "toggleVectorMaskEnabled:", "toggleVectorMaskLinked:",
                // Phase 12E-2b: a transparent canvas-sized leaf is the ONE
                // layer-creating op a float document admits, through the
                // core's own `add_layer` (`precision_insert::add_layer_at`).
                "newLayer:",
                // Phase 12E-2c. The first EIGHT are 12E-1b's: their core ops
                // (`layer_merge`, `layer_merge_visible`, `layer_lock`,
                // `layer_link`) have been in the float admitted set since
                // then and every `DoctoredCore` wrapper routes typed on its
                // first line, so the items were disabled for no reason. The
                // last two are this run's (`layer_stamp`, `layer_flatten`).
                // Merge Visible and Stamp Visible validate against
                // `contributingRasterLeaves()` at depth
                // (`EditorViewController+MergeStamp.swift`), so an enabled
                // item can never miss the CONTRIBUTING RASTER floor — that
                // floor only: a capacity refusal from
                // `preflight_raster_addition` or a `linear_light` (stale-transfer)
                // render refusal still lands as nil and beeps.
                "mergeDown:", "mergeVisible:", "linkLayers:", "unlinkLayers:",
                "lockTransparency:", "lockPixels:", "lockPosition:", "lockAll:",
                "stampVisible:", "flattenImage:",
                // #1 phase 4c: Layer ▸ Stamp Selected (`layer_stamp_selected`).
                "stampSelected:",
                // Layer ▸ Layer Mask ▸ Apply bakes alpha · coverage in f32 (typed `apply_mask`).
                "applyLayerMask:",
                // Layer ▸ New ▸ Layer Via Copy / Cut (typed `layer_via`'s f32 arm).
                "layerViaCopy:", "layerViaCut:",
                // Image ▸ Mode ▸ Assign Profile… relabels; Convert to Profile… is the linear matrix, no clamp.
                "assignProfile:", "convertToProfile:",
                // Calculations… and Add Luminosity Masks read planes and write a channel or selection.
                "calculationsSheet:", "addLuminosityMasks:",
                // File ▸ Contact Sheet… lays out each document's tone-mapped 8-bit rendering.
                "showContactSheet:",
                // Phase 12F-1: Image ▸ Canvas Size…, the first canvas-geometry
                // op admitted at 32-bit float (`geometry_canvas_size`).
                "showCanvasSize:",
                // Image ▸ Trim… is a crop to the kept rectangle
                // (`geometry_crop`) and Image ▸ Reveal All a canvas resize
                // (`geometry_canvas_size`); neither resamples. MCP twins
                // `trim` and `reveal_canvas`, `.floatTool`.
                "showTrim:", "revealAllCanvas:",
                // Phase 12F-1b: the rest of the EXACT canvas geometry —
                // Image ▸ Rotate/Flip and Image ▸ Crop to Selection. Each is
                // a Nearest permutation or a pure window move, so nothing is
                // resampled (`geometry_rotate` / `geometry_flip` /
                // `geometry_crop`).
                "rotateCW:", "rotateCCW:", "rotate180:", "flipH:", "flipV:",
                "cropToSelection:",
                // The FILTERED geometry — Image Size, Rotate Canvas ▸
                // Arbitrary, Free Transform (and a Copy, and Transform Again
                // with and without Copy) and Transform Mask — resamples
                // premultiplied extended-range f32 in the core. The four
                // transforms need no selection (checked above).
                "resizeImage:", "rotateArbitrary:", "freeTransform:", "transformMask:",
                "freeTransformCopy:", "transformAgain:", "transformAgainWithCopy:",
                // Phase 12G-1: the typed clipboard moves samples only
                // (`clipboard_copy` / `clipboard_copy_merged` /
                // `clipboard_paste`). Phase 12G-2 admits Cut: its float
                // erase is the core's own f32 alpha erase (`clipboard_cut`).
                "copy:", "copyMerged:", "paste:", "pasteAsNewLayer:", "pasteInPlace:", "cut:",
                // Edit ▸ Define Pattern…: the composite's tile, linear codes at float.
                "definePattern:",
                // Paste Into / Outside add the typed `set_mask`.
                "pasteInto:", "pasteOutside:",
                // File ▸ Export… and Quick Export. MCP twins
                // `export_image` / `quick_export`, which `FloatDocument.tools`
                // admits. File ▸ Export Layers… too, MCP twin
                // `export_layers` (canvas-framed or trimmed, exact 32-bit
                // TIFF by default). Contact Sheet… is admitted above:
                // its cells are each document's tone-mapped 8-bit rendering.
                "showExportWorkspace:", "quickExport:", "exportLayers:",
                // File ▸ Export as PDF… writes the tone-mapped
                // projection at 8 or 16 bits per component. MCP twin
                // `export_pdf`, which `FloatDocument.tools` admits.
                "exportAsPDF:",
                // File ▸ File Info… edits the metadata packets, which
                // every depth carries. MCP twins `get_file_info` /
                // `set_file_info`, `.floatTool`.
                "showFileInfo:",
                // Layer ▸ Align and Distribute, through the core's
                // typed `align_set` / `distribute_set` (integer translations,
                // nothing resampled). MCP twins `align_layers` /
                // `distribute_layers`, which `FloatDocument.tools` admits.
                "alignLeft:", "alignCenterX:", "alignRight:", "alignTop:", "alignCenterY:",
                "alignBottom:", "distributeHorizontally:", "distributeVertically:",
                // Layer ▸ Auto-Align Layers warps through the float
                // `perspective_set` (`geometry_perspective_set`). MCP twin
                // `auto_align_layers`, `.floatTool`.
                "autoAlignLayers:",
                // Layer ▸ Stack Mode and Reduce Noise from Stack… run
                // the float `stack_mode` (`layer_stack_mode`). MCP twins
                // `stack_mode` / `reduce_noise_from_stack`, `.floatTool`.
                "stackMode:", "showReduceNoiseFromStack:",
                // Edit ▸ Auto-Blend Layers… blends in the retouch
                // encoding (`dr_precision_doc_auto_blend`). MCP twin
                // `auto_blend_layers`, `.floatTool`.
                "showAutoBlendLayers:",
                // Edit ▸ Fill…, Stroke… and the ⌫ fills paint a layer
                // (the core's float `paint_blend`), its mask or an alpha
                // channel, and one colour plane (the same
                // `paint_blend`'s `plane`). MCP twins
                // `fill_selection` / `stroke_selection`, which
                // `FloatDocument.tools` admits.
                "fillSelection:", "fillSelectionFromDeleteKey:", "strokeSelection:",
                "fillWithForeground:", "fillWithBackground:", "fillWithForegroundPreserving:",
                "fillWithBackgroundPreserving:",
                // Layer ▸ Warp… resamples premultiplied extended-range
                // f32 in the core (`geometry_warp`, D3). A float warp is
                // applied at once and keeps no re-editable field. MCP twin
                // `warp_layer`, which `FloatDocument.tools` admits.
                "showPersistedWarp:",
                // Edit ▸ Puppet Warp resamples the float layer in the
                // core (`geometry_puppet`, D3); MCP twin `puppet_warp`.
                "showPuppetWarp:",
                // Edit ▸ Perspective Warp likewise
                // (`geometry_perspective_warp`); MCP twin `perspective_warp`.
                "showPerspectiveWarp:",
                // Layer ▸ Layer Style — the sheet (its popups
                // offer `FloatDocument.effectModes`), the nine effect items
                // and Copy / Paste / Clear. MCP twins `set_layer_style` and
                // `set_global_light`, which `FloatDocument.tools` admits.
                "layerStyle:", "layerStyleEffect:", "copyLayerStyle:", "pasteLayerStyle:",
                "clearLayerStyle:",
                // A placed image's 8/16/32-bit samples are
                // rendered into linear light in the core
                // (`retained_source_float`) and land as an ordinary layer; a
                // RAW layer keeps its source. MCP twins `place_image`,
                // `get_image_source` and `rasterize_image_source`, all
                // `.floatTool`.
                "placeEmbeddedImage:", "placeLinkedImage:", "rasterizeEmbeddedImage:",
                // Text and shape layers render into linear light
                // (`artwork_render::render_float`). MCP twins
                // `rasterize_artwork` and the text/shape tools, `.floatTool`.
                "rasterizeArtwork:", "editArtworkColors:",
                // Layer ▸ Replace Missing Fonts… re-renders text through the
                // same path. MCP twin `replace_missing_fonts`, `.floatTool`.
                "replaceMissingFonts:",
                // Layer ▸ New Fill Layer ▸ Solid Color… / Gradient…
                // render through the same float artwork path. MCP twins
                // `add_fill_layer` / `edit_fill_layer`, `.floatTool`.
                // Pattern… and Layer Content Options… render the same way.
                "newSolidColorFillLayer:", "newGradientFillLayer:",
                "newPatternFillLayer:", "layerContentOptions:",
                // Layer ▸ Place Live Photo… renders the still or movie
                // frame's 8/16-bit samples into linear light, as a placed
                // image is (`float-host-602/take2`). MCP twin
                // `add_live_photo_layer`, `.floatTool`.
                "placeLivePhoto:",
                // Layer ▸ Redevelop RAW…, RAW Source…,
                // Migrate RAW Decoder…, RAW Presets… and Synchronize RAW
                // Settings… keep the 16-bit development and render it into
                // linear light, as a placed image is (at 32 bits as its SDR
                // development, `float-raw-602/take2`). MCP twins
                // `redevelop_raw`, `get_raw_source`, `preview_raw_decoder_migration`,
                // `migrate_raw_decoder`, the RAW preset and settings tools
                // and the RAW local tools, `.floatTool`.
                "redevelopRaw:", "showRawSource:", "migrateRawDecoder:", "showRawPresets:",
                "synchronizeRawSettings:",
                // Filter ▸ Remove Red Eye corrects each found
                // pupil on the layer's display-encoded copy (the core's
                // `red_eye` row, `float_retouch`). MCP twin `red_eye_auto`,
                // `.floatTool`.
                "removeRedEye:",
            ]
            if float.contains(action) || action.hasPrefix("newAdjustmentLayer") { return nil }
            // Image ▸ Adjustments applies the core's `*_apply`
            // ops to a layer's own pixels (`FloatDocument.destructiveOps`);
            // MCP twin `apply_filter`, which `DocumentPrecision.refusal`
            // gates the same way.
            let appliedActions: [String: String] = [
                "showLevels:": "levels", "showCurves:": "curves", "showAdjustments:": "bcs",
                "applyDesaturate:": "hue_saturation",
                // The closed and display forms (`adjust_float`).
                "showThreshold:": "threshold", "showPosterize:": "posterize",
                "showHueRotate:": "hue_rotate", "applyGrayscale:": "grayscale",
                "applySepia:": "sepia",
            ]
            let applied: String? = action == "showAdjustmentSheet:"
                ? AdjustmentMenuOrder.op(for: item.tag)?.rawValue : appliedActions[action]
            if let applied {
                return FloatDocument.appliesDestructively(applied)
                    && document.planeEditTarget == .layer && !document.quickMaskActive ? nil : false
            }
            // The twenty-four filters of `FloatDocument.filterOps`
            // run on a layer's own pixels, each under its stated 32-bit rule
            // (linear light, or the display encoding for a display-referred
            // parameter). MCP twins `apply_filter` and `liquify`, gated the
            // same way. All but `FloatDocument.layerOnlyFilters` run on a
            // mask, an alpha channel or one colour channel too (`planeFilterOps`).
            if let filter = FloatDocument.filterActions[action] {
                guard !document.quickMaskActive else { return false }
                let target = document.planeEditTarget
                if target == .layer { return FloatDocument.filterOps.contains(filter) ? nil : false }
                return target.isPaintable && FloatDocument.planeFilterOps.contains(filter)
                    ? nil : false
            }
            // Repeat Last Filter repeats what the float gate
            // admits for the latched step, and nothing else.
            if action == "repeatLastFilter:" {
                guard let step = ActionRecorder.shared.lastRepeatable else { return nil }
                let refusal = DocumentPrecision.refusal(
                    tool: step.tool, arguments: step.arguments, document: document)
                return refusal == nil ? nil : false
            }
            return false
        }
        if action == "repeatLastFilter:", let step = ActionRecorder.shared.lastRepeatable,
           DocumentPrecision.refusal(tool: step.tool, arguments: step.arguments, document: document) != nil { return false }
        if Self.sixteenBitActions.contains(action) || action.hasPrefix("newAdjustmentLayer") { return nil }
        return false
    }

    /// The items whose edit lands on the TARGET — the adjustments, Auto, Fill and its chords,
    /// Apply Image, Fill Path and Fade — which a CMYK document answers by both its table and the
    /// 16-bit rule when one ink plate or an alpha channel is targeted (#1).
    private static let targetedActions: Set<String> = [
        "showAdjustments:", "showLevels:", "showCurves:", "showAdjustmentSheet:", "applyInvert:",
        "showPosterize:", "showThreshold:", "applyDesaturate:", "equalize:", "autoTone:",
        "autoContrast:", "autoColor:", "fillSelection:", "fillSelectionFromDeleteKey:",
        "fillWithForeground:", "fillWithBackground:", "fillWithForegroundPreserving:",
        "fillWithBackgroundPreserving:", "strokeSelection:", "applyImageSheet:",
        "fillCurrentPath:", "strokeCurrentPath:", "showFade:",
    ]

    /// The 16-bit document's whitelist (`validatePrecisionItem`'s last rule).
    private static let sixteenBitActions: Set<String> = [
        "showColorRange:", "showFocusArea:", "transformSelection:", "selectSubject:", "showSubjectTargets:", "showPeopleTargets:", "selectPeople:",
        "showFaceParts:", "selectSky:", "showPersonParts:", "applyImageSheet:", "calculationsSheet:", "addLuminosityMasks:", "applyLayerMask:",
        "loadSelectionSheet:", "loadPortraitMatte:", "newChannel:", "duplicateChannel:", "renameChannel:", "invertChannel:", "channelOptions:", "modifyNativePlane:",
        "contentAwareFill:", "autoTone:", "autoContrast:", "autoColor:", "showBlur:", "applySharpen:",
        "showUnsharpMask:", "showHighPass:", "showFrequencySeparation:", "showReduceNoise:", "showDustAndScratches:", "applyEdgeDetect:",
        "applyEmboss:", "showPixelate:", "showAddNoise:", "showBoxBlur:", "showSurfaceBlur:", "showAddGrain:",
        "showRadialBlur:", "showOffset:", "applyClouds:", "applyDifferenceClouds:",
        "showMotionBlur:", "showLensBlur:", "showTiltShift:", "showLiquify:", "showLensCorrection:",
        "showPresence:", "showDevelopFilter:", "showSmartSharpen:", "showDepthBlur:", "removeRedEye:",
        "showSuperZoom:", "showJpegArtifactsRemoval:", "showColorize:", // Neural Filters at 16 bits
        "rasterizeArtwork:", "editArtworkColors:", "replaceMissingFonts:", "placeLivePhoto:", "showDocumentPrecision:", "convertDocumentPrecision:", "showInfo:", "showHistory:", "showSnapshots:",
        "convertToFloatPrecision:", // Image ▸ Mode ▸ 32 Bits/Channel…
        // Layer ▸ New Fill Layer ▸ Solid Color… / Gradient…, Pattern… and
        // Layer Content Options….
        "newSolidColorFillLayer:", "newGradientFillLayer:", "newPatternFillLayer:",
        "layerContentOptions:",
        "placeEmbeddedImage:", "rasterizeEmbeddedImage:",
        "placeLinkedImage:", "redevelopRaw:", "showRawSource:", "migrateRawDecoder:", "showRawPresets:", "synchronizeRawSettings:",
        "toggleLayersPanel:", "showChannels:", "undo:", "redo:", "zoomInAction:", "zoomOutAction:",
        "zoomActualAction:", "zoomFitAction:", "selectAll:", "deselect:", "reselect:", "invertSelection:",
        "featherSelection:", "growSelection:", "shrinkSelection:", "borderSelection:", "smoothSelection:",
        "selectGrow:", "selectSimilar:",
        "toggleQuickMask:", "repeatLastFilter:", "showActions:", "startRecording:", "stopRecording:",
        "assignProfile:", "convertToProfile:", "showAdjustments:", "showLevels:", "showCurves:", "showHueRotate:",
        "showThreshold:", "showPosterize:", "showAdjustmentSheet:", "applyInvert:", "applyGrayscale:", "applySepia:",
        "applyDesaturate:",
        // #1: Image ▸ Adjustments ▸ Equalize (`equalize_apply`).
        "equalize:",
        "adjustmentOptions:", "addAdjustmentLayer:", "duplicateLayer:", "groupLayers:",
        // Layer ▸ Duplicate to New Document…
        "duplicateToNewDocument:",
        // Phase 12E-1: the typed structure ops. Each is one core request
        // that splices records and keeps every plane pointer-equal.
        "ungroupLayers:", "deleteLayer:",
        "bringToFront:", "bringForward:", "sendBackward:", "sendToBack:",
        "rotateCW:", "rotateCCW:", "rotate180:", "rotateArbitrary:",
        "flipH:", "flipV:", "resizeImage:",
        "cropToSelection:", "freeTransform:", "freeTransformCopy:", "transformAgain:",
        "transformAgainWithCopy:", "clearSelection:", "showMaskProperties:", "transformMask:",
        "toggleMaskContentLink:", "addLayerMaskRevealAll:", "addLayerMaskHideAll:", "addLayerMaskFromSelection:",
        "deleteLayerMask:", "toggleLayerMaskEnabled:", "deleteChannel:", "saveSelectionSheet:",
        "toggleClippingMask:", "layerStyle:", "copyLayerStyle:", "pasteLayerStyle:", "clearLayerStyle:",
        // Phase 12E-1c: View ▸ New Guide… and View ▸ Clear Guides edit
        // the DOCUMENT, and the typed core now owns both.
        "newGuide:", "clearGuides:",
        // The Paths panel's commands, Select ▸ Make Work Path… and
        // Layer ▸ Vector Mask, over the typed path and vector-mask ops.
        "makeSelectionFromPath:", "fillCurrentPath:", "strokeCurrentPath:", "makeWorkPath:",
        "duplicateCurrentPath:", "deleteCurrentPath:", "toggleClippingPath:",
        "combinePaths:", "convertPathToShape:",
        "editClippingPathFlatness:", "addVectorMaskFromCurrentPath:", "addVectorMaskRevealAll:",
        "deleteVectorMask:", "toggleVectorMaskEnabled:", "toggleVectorMaskLinked:",
        // Phase 12E-2b: Layer ▸ New Layer, through the core's typed
        // `add_layer` op (`precision_insert::add_layer_at`).
        "newLayer:",
        // Phase 12E-2c. The first EIGHT are 12E-1b's: their core ops
        // (`layer_merge`, `layer_merge_visible`, `layer_lock`,
        // `layer_link`) have answered at 16 bits since then and every
        // `DoctoredCore` wrapper routes typed on its first line, so the
        // items were disabled for no reason. The last two are this run's
        // (`layer_stamp`, `layer_flatten`). Merge Visible and Stamp
        // Visible validate against `contributingRasterLeaves()` at depth
        // (`EditorViewController+MergeStamp.swift`), so an enabled item
        // can never miss the CONTRIBUTING RASTER floor — that floor only:
        // a capacity refusal from `preflight_raster_addition` or a
        // `linear_light` (stale-transfer) render refusal still lands as nil
        // and beeps.
        "mergeDown:", "mergeVisible:", "linkLayers:", "unlinkLayers:",
        "lockTransparency:", "lockPixels:", "lockPosition:", "lockAll:",
        "stampVisible:", "flattenImage:",
        // #1 phase 4c: Layer ▸ Stamp Selected (`layer_stamp_selected`).
        "stampSelected:",
        // Layer ▸ Layer Via Copy (⌘J) / Via Cut (⇧⌘J), through the
        // core's typed `layer_via` (PrecisionLayerVia.swift); ⌘J still
        // duplicates where its routing rule says so. MCP twins
        // `layer_via_copy` / `layer_via_cut`.
        "layerViaCopy:", "layerViaCut:",
        // Phase 12F-1: Image ▸ Canvas Size…, through the core's typed
        // `canvas_size` geometry op.
        "showCanvasSize:", "showPersistedWarp:",
        // Edit ▸ Puppet Warp (`geometry_puppet`).
        "showPuppetWarp:",
        // Edit ▸ Perspective Warp (`geometry_perspective_warp`).
        "showPerspectiveWarp:",
        // Image ▸ Trim… (`cropped`) and Reveal All (`canvasResized`).
        "showTrim:", "revealAllCanvas:",
        // Phase 12G-1: the typed clipboard (`precision_clipboard`).
        "cut:", "copy:", "copyMerged:", "paste:", "pasteAsNewLayer:", "pasteInPlace:",
        "pasteInto:", "pasteOutside:",
        // Edit ▸ Define Pattern… (the composite's tile, `DefinePattern`).
        "definePattern:",
        // View ▸ Proof Setup…, Proof Colors and Gamut Warning. MCP
        // twins `get_proof_setup` / `set_proof_setup` / `render_proof`;
        // the canvas proofs the projection's display codes.
        "showProofSetup:", "toggleProofColors:", "toggleGamutWarning:",
        // View ▸ Proof Setup ▸ <preset> resolves into the same setup.
        "applyProofPresetFromMenu:",
        // File ▸ Export…, Quick Export, Export Layers… and
        // Contact Sheet…. MCP twins `export_image` / `quick_export` /
        // `export_layers` / `contact_sheet`, all in `commonTools`.
        // File ▸ Export as PDF… at 8 or 16 bits per component
        // (MCP twin `export_pdf`, in `commonTools`).
        "showExportWorkspace:", "quickExport:", "exportLayers:", "showContactSheet:",
        "exportAsPDF:",
        // File ▸ File Info…, the typed `set_file_info`. MCP twins
        // `get_file_info` / `set_file_info`, in `commonTools`.
        "showFileInfo:",
        // Edit ▸ Fill…, Stroke… and the ⌫ fills paint through
        // the typed `paint_blend` / scalar `paint` / `stroke_layer_edge`
        // (`EditorViewController+PrecisionFillStroke.swift`). MCP twins
        // `fill_selection` / `stroke_selection`. Float lists them in its
        // own whitelist above.
        "fillSelection:", "fillSelectionFromDeleteKey:", "strokeSelection:",
        "fillWithForeground:", "fillWithBackground:", "fillWithForegroundPreserving:",
        "fillWithBackgroundPreserving:",
        // Layer ▸ Align and Distribute, through the core's
        // typed `align_set` / `distribute_set`. MCP twins `align_layers` /
        // `distribute_layers`, in `commonTools`.
        "alignLeft:", "alignCenterX:", "alignRight:", "alignTop:", "alignCenterY:",
        "alignBottom:", "distributeHorizontally:", "distributeVertically:",
        // Layer ▸ Auto-Align Layers, through the typed
        // `perspective_set`. MCP twin `auto_align_layers`, in `typedTools`.
        "autoAlignLayers:",
        // Layer ▸ Stack Mode and Reduce Noise from Stack…, through
        // the typed `stack_mode`. MCP twins `stack_mode` /
        // `reduce_noise_from_stack`, in `typedTools`.
        "stackMode:", "showReduceNoiseFromStack:",
        // Layer ▸ Merge Layers to HDR… reads 16-bit layers into a new
        // float document. MCP twin `merge_layers_to_hdr`, in `commonTools`.
        "mergeLayersToHDR:",
        // Edit ▸ Auto-Blend Layers…, through the typed
        // `dr_precision_doc_auto_blend`. MCP twin `auto_blend_layers`, in
        // `typedTools`.
        "showAutoBlendLayers:",
        // v1: Edit ▸ Fade…, the typed `dr_precision_doc_fade`
        // (`validateFadeItem`). MCP twin `fade`, `.floatTool`.
        "showFade:",
        // Edit ▸ Sky Replacement…, the typed `dr_precision_doc_sky_replace` at 16 bits
        // only (take 1). MCP twin `sky_replacement`, in `typedTools`.
        "showSkyReplacement:",
        // Edit ▸ Generative Fill…, the typed `dr_precision_doc_gen_fill_*` at 16 bits only
        // (take 1). MCP twin `generative_fill`, in `typedTools`.
        "showGenerativeFill:",
        // Edit ▸ Generative Expand…, the typed `dr_precision_doc_gen_expand_coverage`
        // then Generative Fill's ops. MCP twin `generative_expand`, in `typedTools`.
        "showGenerativeExpand:",
        // Layer ▸ Generative Variations…, the typed `dr_precision_doc_gen_fill_*`
        // variation ops. MCP twins `generative_variations`, `select_generative_variation` and
        // `generate_variations`, in `typedTools`.
        "showGenerativeVariations:",
    ]

    func installPrecisionToolPolicy() {
        toolRail?.isToolAvailable = { [weak self] tool in
            DocumentPrecision.allows(tool, document: self?.document)
        }
        canvas.canBeginToolGesture = { [weak self] tool in
            DocumentPrecision.allows(tool, document: self?.document)
        }
    }
}
