import AppKit

/// Spot channels in the editor (#1 phase 1a-2): Channels ▸ New Spot Channel… and Spot Channel
/// Options… (`SpotChannelSheetController`, whose Apply is the one undo step), their validation, and
/// the composite view's overprint — each visible spot channel's ink laid over the displayed
/// composite in list order, display only. Painting a spot channel is the alpha channel's path
/// (`commitCoverageOverlay`'s `.channel` arm, the typed scalar stroke): the brush adds ink.
extension EditorViewController {
    @objc func newSpotChannel(_ sender: Any?) {
        guard let document = document, let doc = document.doc,
              let sheet = SpotChannelSheetController(document: document, channel: nil)
        else {
            NSSound.beep()
            return
        }
        guard channelBudgetAllowsOneMore(doc) else { return }
        presentAsSheet(sheet)
    }

    @objc func spotChannelOptions(_ sender: Any?) {
        guard let document = document, case .channel(let index) = paintTarget,
              let sheet = SpotChannelSheetController(document: document, channel: index)
        else {
            NSSound.beep()
            return
        }
        presentAsSheet(sheet)
    }

    /// Channels ▸ Merge Spot Channel (#1 1c-1): the document flattened with the target spot
    /// channel's ink baked into the colour channels and the channel deleted, one undo step
    /// (`RasterDocument.mergingSpotChannel`). A Duotone document refuses by name. The paint target
    /// follows the removed row as Delete Channel's does.
    @objc func mergeSpotChannel(_ sender: Any?) {
        guard let document = document, case .channel(let index) = paintTarget,
              document.doc?.channelInfo(index)?.spot != nil
        else {
            NSSound.beep()
            return
        }
        document.applyEdit(
            "Merge Spot Channel",
            record: Self.channelStep(
                "merge_spot_channel", document.doc, index,
                note: "Channels panel: Merge Spot Channel")
        ) { try $0.mergingSpotChannel(index) }
    }

    /// New Spot Channel… needs a mode that has spot channels (not Indexed or Bitmap)
    /// outside Quick Mask; Spot Channel Options… and Merge Spot Channel need a spot
    /// channel as the target.
    func validateSpotChannelItem(_ item: NSValidatedUserInterfaceItem) -> Bool {
        guard let document = document, let doc = document.doc else { return false }
        switch item.action {
        case #selector(newSpotChannel(_:)):
            return !doc.isIndexed && !doc.isBitmap && !canvas.quickMaskActive
        case #selector(spotChannelOptions(_:)), #selector(mergeSpotChannel(_:)):
            guard case .channel(let index) = paintTarget else { return false }
            return doc.channelInfo(index)?.spot != nil
        default:
            return true
        }
    }

    /// The composite view with the spot channels `visible` overprinted, as the base the canvas
    /// draws instead of its own image; nil for none, or when the display projection or the
    /// overprint fails (a busy worker, say), so the caller keeps the canvas's own image.
    func spotOverprintBase(_ document: ImageDocument, _ doc: RasterDocument, visible: [Int]) -> CGImage? {
        guard !visible.isEmpty, let projection = try? document.currentDisplayProjection() else { return nil }
        let icc = doc.colorSpace.copyICCData().map { $0 as Data } ?? Data()
        guard let image = try? doc.spotOverprinted(projection, channels: visible, icc: icc) else { return nil }
        return image.makeCGImage(in: doc.colorSpace)
    }

    /// The display with the Channels panel unloaded: every spot channel overprinted, else none.
    func spotOnlyDisplay(_ document: ImageDocument, _ doc: RasterDocument) -> ChannelDisplay? {
        guard let base = spotOverprintBase(document, doc, visible: doc.spotIndices()) else { return nil }
        return ChannelDisplay(base: base, baseKind: .composite, replacesComposite: true, overlays: [])
    }
}
