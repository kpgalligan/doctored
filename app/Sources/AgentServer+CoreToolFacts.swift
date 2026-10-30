import Foundation

extension AgentServer {
    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let coreToolFacts: [String: ToolFacts] = [
        // None of these handlers resolves a document.
        "list_documents": [.nonRecordable, .documentFree],
        "open_document": [.nonRecordable, .documentFree],
        "get_document": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
        "render": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
        "sample_color": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
        "set_active_layer": [.commonTool, .floatTool, .cmykTool],
        // Phase 12E-2b: the typed ADD (`precision_insert::add_layer_at`
        // through the core's `add_layer` structure op).
        // Phase 12E-2b: `new_layer` is the ONE layer-creating tool a float
        // document admits — the core's `add_layer` op writes f32 zeros, which
        // is a transparent Normal leaf and adds no operator. `place_image`
        // and the artwork tools stay refused BY NAME: the retained-source
        // pipeline has no float-aware half yet (`REFUSED_RETAINED`).
        "new_layer": [.typedTool, .floatTool, .cmykTool],
        // Phase 12E-2c: STAMP VISIBLE and FLATTEN IMAGE, each one request
        // through `precision_stamp_flatten`'s own projection render.
        // Phase 12E-2c: STAMP VISIBLE and FLATTEN IMAGE render that same
        // float projection into ONE canvas-sized leaf — no operator a float
        // document does not already evaluate (`layer_stamp`, `layer_flatten`).
        "flatten_image": [.typedTool, .floatTool, .cmykTool],
        // Phase 12F-1b: the EXACT canvas geometry. A zero-angle `crop` keeps
        // every raster and mask allocation; a quarter turn and a flip are a
        // Nearest PERMUTATION, so an extended-range component is only ever
        // copied. The FILTERED geometry is admitted too — a `crop`
        // with an angle (straighten), an arbitrary `rotate`, `image_size`,
        // the layer transforms and `transform_mask` — through the core's
        // premultiplied extended-range resampler, which clamps each RGB
        // component to the layer's own visible range (D3). `transform_again`
        // repeats a layer transform; with a selection it would transform the
        // selected PIXELS, which float does not admit, so
        // `DocumentPrecision.refusal` refuses that case by name.
        "transform_layer": [.typedTool, .floatTool, .cmykTool],
        "add_adjustment_layer": [.selectionSensitive, .typedTool, .floatTool, .cmykTool],
        "edit_adjustment_layer": [.typedTool, .floatTool, .cmykTool],
        // At float only the `FloatDocument.destructiveOps` filters
        // on a layer (`DocumentPrecision.refusal`); the core refuses the rest by name.
        // The twenty-three `apply_filter` filters among `FloatDocument.filterOps` too.
        "apply_filter": [.repeatable, .selectionSensitive, .typedTool, .rasterTool, .floatTool, .cmykTool],
        // At float the three paint a layer in linear light and a mask
        // or channel as at 16 bits (`FloatPaint.refusal` is their gate).
        "brush_stroke": [.typedTool, .rasterTool, .floatTool, .cmykTool],
        "eraser_stroke": [.typedTool, .rasterTool, .floatTool, .cmykTool],
        // Parity with the float menu/tool whitelist in
        // `EditorViewController+Precision.swift`: a selection is canvas
        // coverage, not document pixels, so everything the UI lets a user do
        // with one the agent answers too (including Save/Load Selection,
        // whose `channel_set` family is admitted at depth 32).
        "select_rect": [.commonTool, .floatTool, .cmykTool],
        "select_ellipse": [.commonTool, .floatTool, .cmykTool],
        "select_polygon": [.commonTool, .floatTool, .cmykTool],
        // Compares the R1 display copy in a float document.
        "select_magic_wand": [.commonTool, .floatTool, .cmykTool],
        "select_all": [.commonTool, .floatTool, .cmykTool],
        "deselect": [.commonTool, .floatTool, .cmykTool],
        "modify_selection": [.selectionSensitive, .commonTool, .floatTool, .cmykTool],
        // The float Paint Bucket (`FloatPaint`).
        "fill": [.selectionSensitive, .typedTool, .rasterTool, .floatTool, .cmykTool],
        "gradient": [.selectionSensitive, .typedTool, .rasterTool, .floatTool, .cmykTool],
        "clear_selection": [.selectionSensitive, .typedTool, .rasterTool],
        "image_size": [.typedTool, .floatTool, .cmykTool],
        // Phase 12F-1: CANVAS SIZE, one `GeometryEdit::CanvasSize` request —
        // a generalized Crop that reads no sample.
        // Phase 12F-1: CANVAS SIZE samples nothing — every raster, mask and
        // f32 buffer keeps its allocation and moves its offset — so it was the
        // first canvas-geometry op a float document admitted.
        "canvas_size": [.typedTool, .floatTool, .cmykTool],
        "undo": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
        "redo": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
        "save_copy": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
    ]
}
