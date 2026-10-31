import Foundation

/// Doctored's half of the assistant helper's `configure` message. The helper
/// (`agent/src/`) is shared byte for byte with Frame Job, so everything about the
/// prompt that names this app arrives here, through `AssistantHelperSession.open`.
enum AssistantHelperPrompt {
    /// Tools the model sees up front; every other catalog entry is deferred (tool search).
    static let coreTools: [String] = ["render", "get_document", "list_documents", "open_document"]

    /// Appended by the helper to `system` after a blank line: what tool search can find.
    static let searchGuide: String = """
        Most of your tools are loaded on demand. Search for them with tool_search_tool_bm25, in \
        plain words describing the operation ("gaussian blur", "select the sky", "add a text \
        layer"). The catalog covers: selections (rectangle, ellipse, lasso, magic wand, color \
        range, subject/people, refine, quick mask); layers and groups (new, duplicate, delete, \
        reorder, merge, flatten, properties, blend modes, styles, masks, clipping, locks, links, \
        comps); painting and retouching (brush, eraser, gradient, fill, clone stamp, healing, \
        spot heal, patch, dodge/burn, red eye, content-aware fill); adjustments and filters \
        (adjustment layers, auto tone/color/contrast, filters); \
        transforms and canvas (move, transform, warp, distort, crop, rotate, flip, canvas and \
        image size); text and shape layers; channels, color profiles and proofing; camera RAW \
        development; history, undo/redo and snapshots; documents, saving and export \
        (PNG/JPEG/PSD/PDF, presets); actions and batches; vector paths; colour modes \
        (Grayscale, CMYK, Lab, Indexed, Duotone, Bitmap); HDR merge, toning and tone mapping; \
        generative fill and expand; neural filters (colorize, super zoom, depth blur) and sky \
        replacement; stacks, panoramas and auto-align/blend; liquify and puppet warp; guides, \
        color samplers and file info.
        """

    /// The end-of-turn compaction summary's instructions: renders and images don't survive it.
    static let compactionInstructions: String = """
        Summarize this conversation between a user and the image-editing assistant built into \
        Doctored, so the assistant can continue it from the summary alone. Keep: what the user \
        asked for and every preference or constraint they stated; which documents and layers were \
        involved (ids and names); the edits made and how each turned out; anything still open or \
        promised. Canvas renders and other images will not survive, so describe what mattered in \
        them. Do not call tools; respond with the summary text only.
        """
}
