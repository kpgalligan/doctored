import AppKit

// The selected layers are duplicated into a handle the document never holds
// (`DuplicateTransform.source`) and ⌘T's own `TransformSession` opens over the copies with
// `duplicate` set, so its `layers`, `sourceRect`, plates and 8-bit preview index `duplicate.source`
// while the document, locks, snap fold and the 16-bit gesture's capture are `duplicate.base`'s.

/// Layer ▸ Free Transform a Copy (⌥⌘T) with NO pixel selection:
/// the target layers are duplicated and Free Transform opens on the
/// copies, and Return commits ONE "Transform Copy" step. `transform_layers`
/// with `duplicate` is the MCP twin. The controller half is
/// `EditorViewController+TransformCopy.swift`.
///
/// Decisions stated here and in the feature reference:
/// - the session never edits the document. It is an ordinary
///   `TransformSession` built over `source` — the base with the originals
///   duplicated — so its `layers`, `sourceRect`, plates and 8-bit preview
///   belong to `source`, and the original shows under the transformed copy.
///   The document, the locks, the snap fold and the typed gesture's capture
///   belong to `base`; no duplicate path reads `document.doc` with a session
///   index;
/// - the copies are `duplicateLayers`' own (the Move tool's ⌥ duplicate):
///   unlinked, each directly above its source's subtree, so the lock rule is
///   the originals' SUBTREE Position block, checked once at open, never
///   `movingSet`;
/// - a text, shape or artwork copy takes ⌘T's described rules
///   over `source`: at 8 bits a lone copy under an affine composes
///   into its own description (`describedPrimaryGate`), a true perspective
///   or a set asks the rasterize prompt; at 16 bits the copies take ⌘T's SET
///   path (`.duplicateTransform` captures their artwork like `.transformSet`,
///   and a true perspective prompts first);
/// - Esc and an identity Return leave the document untouched and add no
///   step; the copies become the layer selection only after the edit
///   publishes;
/// - at 16 bits the originals' subtree set must be exactly one entry, as
///   ⌘T's 16-bit gate requires.
struct DuplicateTransform {
    static let actionName = TransformSelectedPixels.duplicateActionName

    /// `document.doc` at open; the commit publishes only over it.
    let base: RasterDocument
    /// `base.duplicateLayers(originals)`: the session's box, preview and
    /// 8-bit input.
    let source: RasterDocument
    /// `document.selectedLayerIndices` at open.
    let originals: [Int]
    /// Where the copies landed in `source`.
    let copies: [Int]
    /// `document.activeLayerIndex` at open; its copy becomes the primary.
    let primary: Int

    /// The copies' subtrees in `source`: the session's `layers`.
    var set: [Int] { MoveDuplicateLayers.subtrees(of: copies, in: source) }

    /// The layer selection after the commit publishes.
    var selection: LayerSelection {
        MoveDuplicateLayers.selection(
            copies: copies, primary: primary, originals: originals, in: base)
    }

    /// nil when `duplicateLayers` answered nil.
    init?(base: RasterDocument, originals: [Int], primary: Int) throws {
        let copies = MoveDuplicateLayers.copies(of: originals, in: base)
        guard let source = try base.duplicateLayers(originals) else { return nil }
        self.base = base
        self.source = source
        self.originals = originals
        self.copies = copies
        self.primary = primary
    }
}
