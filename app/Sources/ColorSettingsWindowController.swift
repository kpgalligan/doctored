import AppKit

/// Edit ▸ Color Settings… (⇧⌘K, #1 phase 5): the one home of the working spaces, the
/// colour-management policies and the conversion options, persisted in the preferences
/// (`ColorSettings`). An app-modal card (`ModalCardWindowController`), because it works with no
/// document open, like File ▸ New….
///
/// * **Working Spaces** — RGB (the `WorkingSpace` cases; Image ▸ Mode ▸ Working Space is the same
///   key) and CMYK (Automatic — U.S. Web Coated (SWOP) v2 when installed, else Generic CMYK —
///   every installed CMYK profile both directions of which this build reads, and Load…) and Gray
///   (eight, Dot Gain 20 % by default: the curve Load Selection's luminosity takes at
///   8 and 16 bits on an RGB document, `ColorSettings.workingGray`).
/// * **Color Management Policies** — RGB and CMYK (Off, Preserve Embedded Profiles, Convert to
///   Working; what an open does, `ImageDocument`), and Ask When Pasting.
/// * **Conversion Options** — Engine (one item, this build's ICC engine), Intent and Use Black
///   Point Compensation: every conversion's defaults (Mode, Convert to Profile, the picker, the
///   CMYK proof and the exports' sheet).
///
/// OK stores every field and posts `ColorSettings.didChange`, on which `WorkingCMYK` reinstalls
/// the working CMYK profile in the core. There is no MCP setter: like Ask When Pasting, these are
/// app-wide preferences outside any document (`ColorSettings.askWhenPasting`'s exemption).
final class ColorSettingsWindowController: NSObject {
    private let rgbPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let cmykPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let grayPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let rgbPolicyPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let cmykPolicyPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let askWhenPastingBox = NSButton(checkboxWithTitle: "Ask When Pasting", target: nil, action: nil)
    private let enginePopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let intentPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let bpcBox = NSButton(checkboxWithTitle: "Use Black Point Compensation", target: nil, action: nil)
    private let message = NSTextField(wrappingLabelWithString: "")
    private var modal: ModalCardWindowController?
    /// The CMYK popup's choices in item order (Load… is the last item and has none).
    private var cmykChoices: [WorkingCMYKChoice] = []

    private override init() {
        super.init()
    }

    /// Runs the card; true when OK stored the settings.
    @discardableResult
    static func run() -> Bool {
        ColorSettingsWindowController().present()
    }

    private func present() -> Bool {
        for space in WorkingSpace.allCases { rgbPopup.addItem(withTitle: space.displayName) }
        rgbPopup.selectItem(at: WorkingSpace.allCases.firstIndex(of: ColorSettings.workingSpace) ?? 0)
        rgbPopup.setAccessibilityIdentifier("color-settings-rgb")
        fillCMYK(selecting: ColorSettings.workingCMYK)
        cmykPopup.target = self
        cmykPopup.action = #selector(cmykChanged(_:))
        cmykPopup.setAccessibilityIdentifier("color-settings-cmyk")
        for space in GrayWorkingSpace.allCases { grayPopup.addItem(withTitle: space.displayName) }
        grayPopup.selectItem(at: GrayWorkingSpace.allCases.firstIndex(of: ColorSettings.workingGray) ?? 0)
        grayPopup.setAccessibilityIdentifier("color-settings-gray")
        for (popup, policy, id) in [(rgbPolicyPopup, ColorSettings.rgbPolicy, "color-settings-rgb-policy"),
                                    (cmykPolicyPopup, ColorSettings.cmykPolicy, "color-settings-cmyk-policy")] {
            for item in ColorPolicy.allCases { popup.addItem(withTitle: item.displayName) }
            popup.selectItem(at: ColorPolicy.allCases.firstIndex(of: policy) ?? 0)
            popup.setAccessibilityIdentifier(id)
        }
        askWhenPastingBox.state = ColorSettings.askWhenPasting ? .on : .off
        askWhenPastingBox.setAccessibilityIdentifier("color-settings-ask-pasting")
        enginePopup.addItem(withTitle: "Doctored ICC")
        enginePopup.setAccessibilityIdentifier("color-settings-engine")
        for intent in RenderingIntent.allCases { intentPopup.addItem(withTitle: intent.displayName) }
        intentPopup.selectItem(at: RenderingIntent.allCases.firstIndex(of: ColorSettings.intent) ?? 0)
        intentPopup.setAccessibilityIdentifier("color-settings-intent")
        bpcBox.state = ColorSettings.blackPointCompensation ? .on : .off
        bpcBox.setAccessibilityIdentifier("color-settings-bpc")
        for control in [rgbPopup, cmykPopup, grayPopup, rgbPolicyPopup, cmykPolicyPopup, enginePopup,
                        intentPopup] {
            control.font = DS.sans(13)
        }
        for box in [askWhenPastingBox, bpcBox] { box.font = DS.sans(13) }
        message.font = DS.sans(11)
        message.textColor = DS.textMuted
        message.preferredMaxLayoutWidth = 420
        message.setAccessibilityIdentifier("color-settings-message")

        let grid = NSGridView(views: [
            [section("Working Spaces"), NSGridCell.emptyContentView],
            [fieldLabel("RGB:"), rgbPopup],
            [fieldLabel("CMYK:"), cmykPopup],
            [fieldLabel("Gray:"), grayPopup],
            [section("Color Management Policies"), NSGridCell.emptyContentView],
            [fieldLabel("RGB:"), rgbPolicyPopup],
            [fieldLabel("CMYK:"), cmykPolicyPopup],
            [NSGridCell.emptyContentView, askWhenPastingBox],
            [section("Conversion Options"), NSGridCell.emptyContentView],
            [fieldLabel("Engine:"), enginePopup],
            [fieldLabel("Intent:"), intentPopup],
            [NSGridCell.emptyContentView, bpcBox],
        ])
        grid.rowSpacing = 9
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 110
        let content = NSStackView(views: [grid, message])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 12

        let cancel = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        cancel.setAccessibilityIdentifier("color-settings-cancel")
        let ok = StickerButton(title: "OK", style: .primary, target: self, action: #selector(okClicked(_:)))
        ok.setAccessibilityIdentifier("color-settings-ok")
        modal = ModalCardWindowController(
            title: "Color Settings",
            card: makeSheetView(
                title: "Color Settings",
                hint: "The working spaces new documents and conversions use, what opening a file does "
                    + "with its profile, and every conversion's default intent and black point "
                    + "compensation. Changes affect future opens and conversions, never an open "
                    + "document's pixels.",
                content: content, buttonRow: makeButtonRow(cancel: cancel, apply: ok), width: 480))
        return modal?.runModal() == .OK
    }

    private func section(_ title: String) -> NSTextField {
        let label = NSTextField(labelWithString: title)
        label.font = DS.sans(12, weight: .semibold)
        return label
    }

    /// The CMYK popup: Automatic, every installed CMYK profile, a chosen file not among them, and
    /// Load….
    private func fillCMYK(selecting choice: WorkingCMYKChoice) {
        cmykPopup.removeAllItems()
        cmykChoices = []
        cmykPopup.addItem(withTitle: WorkingCMYK.automaticName.map { "Automatic — \($0)" } ?? "Automatic")
        cmykChoices.append(.automatic)
        for profile in InstalledProfiles.proofable where profile.description.space == "cmyk" {
            cmykPopup.addItem(withTitle: profile.name)
            cmykChoices.append(.file(path: profile.url.path))
        }
        if case .file(let path) = choice, !cmykChoices.contains(choice) {
            cmykPopup.addItem(withTitle: URL(fileURLWithPath: path).lastPathComponent)
            cmykChoices.append(choice)
        }
        cmykPopup.addItem(withTitle: "Load…")
        cmykPopup.selectItem(at: cmykChoices.firstIndex(of: choice) ?? 0)
    }

    /// Load… picks a CMYK profile file; anything else is refused by name and the selection
    /// returns to Automatic.
    @objc private func cmykChanged(_ sender: Any?) {
        guard cmykPopup.indexOfSelectedItem == cmykChoices.count else { return }
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.init(filenameExtension: "icc"), .init(filenameExtension: "icm")]
            .compactMap { $0 }
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        guard panel.runModal() == .OK, let url = panel.url else {
            cmykPopup.selectItem(at: 0)
            return
        }
        guard let data = try? Data(contentsOf: url), RasterProfile.describe(data)?.space == "cmyk" else {
            message.stringValue = "\(url.lastPathComponent) is not a CMYK profile."
            cmykPopup.selectItem(at: 0)
            return
        }
        message.stringValue = ""
        fillCMYK(selecting: .file(path: url.path))
    }

    @objc private func cancelClicked(_ sender: Any?) { modal?.end(.cancel) }

    @objc private func okClicked(_ sender: Any?) {
        let spaces = WorkingSpace.allCases
        ColorSettings.workingSpace = spaces[max(0, min(spaces.count - 1, rgbPopup.indexOfSelectedItem))]
        let index = cmykPopup.indexOfSelectedItem
        ColorSettings.workingCMYK = cmykChoices.indices.contains(index) ? cmykChoices[index] : .automatic
        let grays = GrayWorkingSpace.allCases
        ColorSettings.workingGray = grays[max(0, min(grays.count - 1, grayPopup.indexOfSelectedItem))]
        let policies = ColorPolicy.allCases
        ColorSettings.rgbPolicy = policies[max(0, min(policies.count - 1, rgbPolicyPopup.indexOfSelectedItem))]
        ColorSettings.cmykPolicy = policies[max(0, min(policies.count - 1, cmykPolicyPopup.indexOfSelectedItem))]
        ColorSettings.askWhenPasting = askWhenPastingBox.state == .on
        let intents = RenderingIntent.allCases
        ColorSettings.intent = intents[max(0, min(intents.count - 1, intentPopup.indexOfSelectedItem))]
        ColorSettings.blackPointCompensation = bpcBox.state == .on
        NotificationCenter.default.post(name: ColorSettings.didChange, object: nil)
        modal?.end(.OK)
    }
}
