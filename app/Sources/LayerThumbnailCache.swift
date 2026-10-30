import Foundation

// The cache is keyed on `LayerInfo.id` and validated by `RasterDocument.layerStorageIdentity`
// (byte: `dr_doc_layer_storage_identity`; typed: `layerSource(idx)?.storageIdentity`), so a
// rename/label/lock/visibility/opacity edit reuses the fitted RGBA8 thumbnail and any pixel write
// misses. The identity is comparable only while the storage is owned, so every entry retains the
// `RasterDocument` it was read from; `reload()` is the ONE pruning point (`prune(to:rows:)` drops
// entries whose row is gone or whose storage changed and re-points survivors at the current
// document), and `releaseDisplayForWindowClose()` empties it through the panel's
// `releaseForWindowClose()`, which also removes the panel's document observer. A float layer's entry
// also carries a `FloatDisplay` key (the document's tone map and profile, memoized per document
// handle), so a tone-map change misses and a rename hits (since #547 Duotone phase 2a a Duotone
// document's key is its inks and profile, so an ink or profile change misses); MASK thumbnails
// have a second table on
// the same rules, validated by `RasterDocument.layerMaskStorageIdentity`
// (`dr_doc_layer_mask_storage_identity` / `dr_precision_doc_layer_mask_storage_identity`: the raw
// mask plane, so a mask paint or replacement misses), rendered by
// `RasterDocument.layerMaskThumbnail` (the panel's former fit, verbatim); groups and a 0 identity
// bypass both. A miss still renders synchronously on main, but the core fits a large layer before
// any whole-layer copy (`dr_doc_layer_thumbnail`'s box arm; at 16 bits
// `dr_precision_image_thumbnail_rgba8`, never upscaling, byte-identical below the box threshold).
// The typed misses are fitted in the core too: a float layer through
// `RasterPrecisionDocument.displayThumbnailRGBA8` (`dr_precision_doc_display_thumbnail_rgba8`, its
// tone map applied inside the box pass) and a 16-bit mask through `layerMaskThumbnailRGBA8`
// (`dr_precision_doc_layer_mask_thumbnail_rgba8`, the raw plane; a group's canvas view), both
// byte-identical to their former paths below the box threshold; the typed mask miss no longer goes
// through `PrecisionPlaneRead` or its `BackgroundWork.occupied` refusal (`layerMaskImage` /
// `nativePlaneDisplay` are unchanged for their other callers). `hits` / `misses` count LAYER
// thumbnails only (`maskHits` / `maskMisses` the masks) and are read by the 9H probe case
// `layer-thumbnail-cache-ui` (`scripts/native-paint-probe/thumbnails.swift`); the jobs probe's
// "layer thumbnail costs" measures the misses. An 8-bit mask miss is fitted in the core too
// (`RasterDocument.fittedLayerMaskImage` → `dr_doc_layer_mask_thumbnail`, the former Swift fit's
// dimensions and, below the box threshold, its bytes), and the 16-bit Channels panel's mask/alpha
// rows are (`planeImage(for:…)` → `nativePlaneThumbnail` → `channelThumbnailRGBA8` /
// `layerMaskCanvasThumbnailRGBA8`) instead of converting whole canvas planes in Swift.

/// The Layers panel's per-layer thumbnail cache.
///
/// Keyed on the layer's stable id (`LayerInfo.id`) and validated by its
/// pixel-storage identity (`RasterDocument.layerStorageIdentity`): a rename,
/// label, lock, visibility or opacity edit keeps the storage, so the row
/// reuses its fitted thumbnail instead of re-rendering the whole layer on the
/// main thread; any pixel write answers a new identity and misses.
///
/// The identity is comparable only while the storage is owned, so every
/// entry retains the `RasterDocument` it was read from (`owner`). At 16 bits
/// that document owns the storage the `layerSource` handle shares. `prune`
/// re-points every survivor at the current document after each reload, so
/// the cache never retains any other document.
///
/// A float layer's thumbnail also depends on the document's tone map and
/// profile curve, so its entries carry a `FloatDisplay` key beside the
/// identity: a tone-map or profile change misses, a rename still hits.
/// A Duotone layer's goes through the inks and the profile (#547 Duotone
/// phase 2a), which entering or leaving Duotone, `set_duotone` and Assign
/// change while every tile is shared, so its key carries both.
///
/// MASK thumbnails have a second table on the same rules, validated by
/// `RasterDocument.layerMaskStorageIdentity` — the raw mask plane's identity,
/// so a mask paint or replacement misses and a rename or a mask property
/// edit hits. A group's mask (identity 0) renders uncached.
///
/// Misses render synchronously on main, as before. Every miss is fitted in
/// the core before any whole-layer work: a float layer through
/// `displayThumbnailRGBA8`, a 16-bit mask through `layerMaskThumbnailRGBA8`
/// and an 8-bit mask through `fittedLayerMaskImage`, like the
/// 8- and 16-bit layer thumbnails.
struct LayerThumbnailCache {
    /// What a float document's layer thumbnail depends on besides the
    /// layer's pixels: the tone map and the profile it displays through; a
    /// Duotone document's (#547 Duotone phase 2a): its inks and profile.
    struct FloatDisplay: Equatable {
        let toneMap: ToneMap?
        let profile: Data
        let duotone: DuotoneOptions?
    }

    private struct Entry {
        let identity: UInt64
        let maxSide: Int
        /// nil for byte and 16-bit documents.
        let display: FloatDisplay?
        let thumbnail: RasterImage
        var owner: RasterDocument
    }

    /// Keyed on `LayerInfo.id`.
    private var entries: [UInt64: Entry] = [:]
    /// Mask thumbnails, keyed on `LayerInfo.id`; `display` is always nil.
    private var maskEntries: [UInt64: Entry] = [:]
    /// The float display of the last document asked, so a reload reads the
    /// tone map and profile once rather than once per row.
    private var displayMemo: (owner: RasterDocument, display: FloatDisplay?)?

    /// LAYER thumbnail lookups answered from the cache / rendered and stored.
    /// Read by the native-paint probe's thumbnail-cache case.
    private(set) var hits = 0
    private(set) var misses = 0
    /// The same counts for mask thumbnails.
    private(set) var maskHits = 0
    private(set) var maskMisses = 0

    /// Layer `index`'s fitted thumbnail, from the cache when its storage (and
    /// at float, its display) is unchanged; otherwise rendered through
    /// `layerThumbnail` and stored when it rendered.
    mutating func thumbnail(in doc: RasterDocument, index: Int, layerID: UInt64,
                            maxSide: Int) -> RasterImage? {
        let identity = layerID == 0 ? 0 : doc.layerStorageIdentity(index)
        guard identity != 0 else { return doc.layerThumbnail(index, maxSide: maxSide) }
        let display = floatDisplay(of: doc)
        if var entry = entries[layerID], entry.identity == identity, entry.maxSide == maxSide,
           entry.display == display {
            entry.owner = doc
            entries[layerID] = entry
            hits += 1
            return entry.thumbnail
        }
        misses += 1
        guard let rendered = doc.layerThumbnail(index, maxSide: maxSide) else { return nil }
        entries[layerID] = Entry(identity: identity, maxSide: maxSide, display: display,
                                 thumbnail: rendered, owner: doc)
        return rendered
    }

    /// Layer `index`'s fitted mask thumbnail, from the cache when its mask
    /// plane is unchanged; otherwise rendered through `layerMaskThumbnail`
    /// and stored when it rendered. A 0 identity (no mask, a group) renders
    /// uncached.
    mutating func maskThumbnail(in doc: RasterDocument, index: Int, layerID: UInt64,
                                maxSide: Int) -> RasterImage? {
        let identity = layerID == 0 ? 0 : doc.layerMaskStorageIdentity(index)
        guard identity != 0 else { return doc.layerMaskThumbnail(index, maxSide: maxSide) }
        if var entry = maskEntries[layerID], entry.identity == identity, entry.maxSide == maxSide {
            entry.owner = doc
            maskEntries[layerID] = entry
            maskHits += 1
            return entry.thumbnail
        }
        maskMisses += 1
        guard let rendered = doc.layerMaskThumbnail(index, maxSide: maxSide) else { return nil }
        maskEntries[layerID] = Entry(identity: identity, maxSide: maxSide, display: nil,
                                     thumbnail: rendered, owner: doc)
        return rendered
    }

    /// Keeps only entries for the non-group rows of `rows` whose storage (and
    /// display) is still `doc`'s — mask entries only for rows with a mask
    /// whose plane is still `doc`'s — and re-points each survivor at `doc`.
    /// A nil `doc` empties the cache.
    mutating func prune(to doc: RasterDocument?, rows: [LayerRowModel]) {
        guard let doc else {
            removeAll()
            return
        }
        let display = floatDisplay(of: doc)
        var kept: [UInt64: Entry] = [:]
        var keptMasks: [UInt64: Entry] = [:]
        for row in rows where !row.isGroup {
            if var entry = entries[row.info.id],
               entry.identity == doc.layerStorageIdentity(row.index), entry.display == display {
                entry.owner = doc
                kept[row.info.id] = entry
            }
            if row.hasMask, var entry = maskEntries[row.info.id],
               entry.identity == doc.layerMaskStorageIdentity(row.index) {
                entry.owner = doc
                keptMasks[row.info.id] = entry
            }
        }
        entries = kept
        maskEntries = keptMasks
    }

    mutating func removeAll() {
        entries.removeAll()
        maskEntries.removeAll()
        displayMemo = nil
    }

    /// `doc`'s display key (nil unless it is a float or a Duotone
    /// document), memoized per document handle.
    private mutating func floatDisplay(of doc: RasterDocument) -> FloatDisplay? {
        if let memo = displayMemo, memo.owner === doc { return memo.display }
        let display: FloatDisplay?
        if doc.isFloat {
            display = FloatDisplay(toneMap: doc.toneMap, profile: doc.precision?.profileData ?? Data(),
                                   duotone: nil)
        } else if doc.isDuotone {
            // #547 Duotone phase 2a: the inks and the profile the view goes through.
            let options = (try? doc.precision?.duotoneOptions()).flatMap { $0 }
            display = FloatDisplay(toneMap: nil, profile: doc.precision?.profileData ?? Data(),
                                   duotone: options.flatMap { try? DuotoneOptions(json: $0) })
        } else {
            display = nil
        }
        displayMemo = (doc, display)
        return display
    }
}

extension RasterDocument {
    /// The mask's grayscale image scaled down for its thumbnail well, never
    /// up; nil without a mask. Both depths fit in the core before any
    /// whole-plane image: a typed mask through `layerMaskThumbnailRGBA8`,
    /// a byte mask through `fittedLayerMaskImage` (the former
    /// Swift fit's dimensions and, below the box threshold, its bytes).
    func layerMaskThumbnail(_ idx: Int, maxSide: Int) -> RasterImage? {
        if let precision {
            guard let context = try? PrecisionWorkContext(source: precision) else { return nil }
            return try? precision.layerMaskThumbnailRGBA8(
                idx, maxSide: maxSide, workingLimit: context.limit,
                otherRetained: context.otherRetained)
        }
        return fittedLayerMaskImage(idx, maxSide: maxSide)
    }
}
