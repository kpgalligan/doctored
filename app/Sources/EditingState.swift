import AppKit

// A full editing state holds the immutable native document, current/previous selection, Quick Mask,
// layer selection, paint target and export preferences.
//
// Saved identity stays separate from transient editing state.

/// One complete editing state. The saved-file marker remains on ImageDocument:
/// restoring a selection or export preference must not invent a saved revision.
struct EditingState {
    let document: RasterDocument
    let selection: SelectionHistoryState
    let layerSelection: LayerSelection
    let paintTarget: PaintTarget
    let jpegExportQuality: Int
    let embedColorProfile: Bool
    let stripMetadata: Bool
    var exportBitDepth: Int = 16
    var exportDither: String = "none"
    var channelVisibility = ChannelVisibility()

    /// This state with its document swapped and every other field kept — the
    /// history's colour-sampler carry, which moves the live list onto
    /// the destination of a non-geometry undo/redo
    /// (`ImageDocument+History.swift`).
    func replacingDocument(_ document: RasterDocument) -> EditingState {
        EditingState(document: document, selection: selection, layerSelection: layerSelection,
                     paintTarget: paintTarget, jpegExportQuality: jpegExportQuality,
                     embedColorProfile: embedColorProfile, stripMetadata: stripMetadata,
                     exportBitDepth: exportBitDepth, exportDither: exportDither,
                     channelVisibility: channelVisibility)
    }

    func equivalent(to other: EditingState) -> Bool {
        document === other.document && selection == other.selection
            && layerSelection == other.layerSelection && paintTarget == other.paintTarget
            && jpegExportQuality == other.jpegExportQuality
            && embedColorProfile == other.embedColorProfile && stripMetadata == other.stripMetadata
            && exportBitDepth == other.exportBitDepth && exportDither == other.exportDither
            && channelVisibility == other.channelVisibility
    }

    /// Retained payload across ONE union, so snapshots/history share allocations.
    /// Core buffers use actual capacity; Swift arrays expose their payload count.
    /// CGImage backing is estimated by row bytes, contours by 64 bytes/element;
    /// platform object/allocator overhead remains outside this working estimate.
    static func retainedBytes(in states: [EditingState]) throws -> UInt64 {
        let report = try RasterDocument.resourceReport(
            documents: states.map(\.document), images: [], scratch: 0, limit: 0)
        guard let total = report["unique_retained_bytes"] as? NSNumber,
              let caches = report["cache_bytes"] as? NSNumber else {
            throw DoctoredCoreError(message: "History resource accounting returned no retained bytes.")
        }
        var bytes = total.uint64Value - min(total.uint64Value, caches.uint64Value)
        var buffers = Set<UInt>()
        var objects = Set<ObjectIdentifier>()
        func add(_ selection: CanvasSelection?) {
            guard let selection else { return }
            switch selection.shape {
            case .mask(let mask):
                mask.withUnsafeBufferPointer { buffer in
                    if let pointer = buffer.baseAddress, buffers.insert(UInt(bitPattern: pointer)).inserted {
                        bytes += UInt64(buffer.count)
                    }
                }
            case .mask16(let mask):
                mask.withUnsafeBufferPointer { buffer in
                    if let pointer = buffer.baseAddress, buffers.insert(UInt(bitPattern: pointer)).inserted {
                        bytes += UInt64(buffer.count) * 2
                    }
                }
            case .polygon(let points):
                points.withUnsafeBufferPointer { buffer in
                    if let pointer = buffer.baseAddress, buffers.insert(UInt(bitPattern: pointer)).inserted {
                        bytes += UInt64(buffer.count * MemoryLayout<CGPoint>.stride)
                    }
                }
            case .rect, .ellipse: break
            }
            for image in [selection.maskImage, selection.inverseMaskImage].compactMap({ $0 }) {
                if objects.insert(ObjectIdentifier(image)).inserted {
                    bytes += UInt64(image.bytesPerRow * image.height)
                }
            }
            if let contour = selection.maskContour, objects.insert(ObjectIdentifier(contour)).inserted {
                bytes += UInt64(contour.elementCount) * 64
            }
        }
        for state in states {
            add(state.selection.selection)
            add(state.selection.previous)
        }
        return bytes
    }
}
