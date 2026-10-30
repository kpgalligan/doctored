import AppKit
import ImageIO

// `PortraitMatte` maps each iPhone auxiliary matte to the channel `AuxiliaryMattes` named at open,
// `loadPortraitMatte(_:)` is only `loadSelection(from: .channel(i))`, and
// `validatePortraitMatteItem` enables an item when that channel exists and Quick Mask is off.

/// Select ▸ Portrait Matte: one item per iPhone auxiliary matte, each
/// loading the matte's alpha channel as the selection. The channels are the
/// ones `AuxiliaryMattes` attaches at open time, found by the name it gave
/// them, so this is a shortcut for Load Selection… with that channel and
/// nothing more: it goes through `loadSelection`, which owns both depths,
/// the Quick Mask refusal and the Actions recording.
///
/// Mirrored for the agent by `load_selection` with `from:"channel"` and the
/// matte's channel name (AgentServer+Channels.swift) — no separate tool.
enum PortraitMatte: Int, CaseIterable {
    case person, skin, hair, teeth, glasses, sky

    /// The menu title.
    var title: String {
        switch self {
        case .person: return "Portrait Matte"
        case .skin: return "Skin"
        case .hair: return "Hair"
        case .teeth: return "Teeth"
        case .glasses: return "Glasses"
        case .sky: return "Sky"
        }
    }

    /// The ImageIO auxiliary type the matte is read from.
    var auxiliaryType: CFString {
        switch self {
        case .person: return kCGImageAuxiliaryDataTypePortraitEffectsMatte
        case .skin: return kCGImageAuxiliaryDataTypeSemanticSegmentationSkinMatte
        case .hair: return kCGImageAuxiliaryDataTypeSemanticSegmentationHairMatte
        case .teeth: return kCGImageAuxiliaryDataTypeSemanticSegmentationTeethMatte
        case .glasses: return kCGImageAuxiliaryDataTypeSemanticSegmentationGlassesMatte
        case .sky: return kCGImageAuxiliaryDataTypeSemanticSegmentationSkyMatte
        }
    }

    /// The channel name `AuxiliaryMattes` gave this matte — looked up in its
    /// one table by type, so the two can never disagree.
    var channelName: String {
        AuxiliaryMattes.sources.first { $0.type == auxiliaryType }?.name ?? title
    }
}

extension EditorViewController {
    /// A Portrait Matte item: the item's tag is the matte's raw value. A
    /// document without that matte beeps (validation already greys it out).
    @objc func loadPortraitMatte(_ sender: Any?) {
        guard let tag = (sender as? NSMenuItem)?.tag, let matte = PortraitMatte(rawValue: tag),
              let index = document?.doc?.channelIndex(named: matte.channelName)
        else {
            NSSound.beep()
            return
        }
        loadSelection(from: .channel(index), mode: .replace, invert: false)
    }

    /// Enabled when the document carries the matte's channel and Quick Mask
    /// is off — Load Selection…'s own rule (`validateChannelItem`).
    func validatePortraitMatteItem(_ item: NSValidatedUserInterfaceItem) -> Bool {
        guard let doc = document?.doc, !canvas.quickMaskActive,
              let matte = PortraitMatte(rawValue: item.tag)
        else { return false }
        return doc.channelIndex(named: matte.channelName) != nil
    }
}
