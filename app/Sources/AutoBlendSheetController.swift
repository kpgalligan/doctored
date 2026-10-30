import AppKit

/// Edit ▸ Auto-Blend Layers… (AutoBlend.swift): the method (Panorama,
/// preselected, or Stack Images), Seamless Tones
/// and Colours, whether the layers are registered first, and Panorama's
/// Content-Aware Fill Transparent Areas (enabled only while Panorama is
/// selected; AutoBlendFill.swift). Apply hands the method and the three
/// switches to `onApply` (`EditorViewController.commitAutoBlend`).
final class AutoBlendSheetController: NSViewController {
    private let onApply: (_ method: AutoBlend.Method, _ seamless: Bool, _ align: Bool,
                          _ fill: Bool) -> Void
    private let methodPopUp = NSPopUpButton(frame: .zero, pullsDown: false)
    private let seamlessButton = NSButton(
        checkboxWithTitle: "Seamless Tones and Colours", target: nil, action: nil)
    private let alignButton = NSButton(checkboxWithTitle: "Align first", target: nil, action: nil)
    private let fillButton = NSButton(
        checkboxWithTitle: "Content-Aware Fill Transparent Areas", target: nil, action: nil)

    init(onApply: @escaping (_ method: AutoBlend.Method, _ seamless: Bool, _ align: Bool,
                             _ fill: Bool) -> Void) {
        self.onApply = onApply
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("AutoBlendSheetController does not support NSCoder")
    }

    override func loadView() {
        methodPopUp.addItems(withTitles: AutoBlend.Method.allCases.map(\.title))
        methodPopUp.selectItem(at: 0)
        methodPopUp.font = DS.sans(13)
        methodPopUp.identifier = NSUserInterfaceItemIdentifier("auto-blend-method")
        methodPopUp.target = self
        methodPopUp.action = #selector(methodChanged(_:))
        seamlessButton.state = .on
        seamlessButton.font = DS.sans(13)
        seamlessButton.identifier = NSUserInterfaceItemIdentifier("auto-blend-seamless")
        alignButton.state = .off
        alignButton.font = DS.sans(13)
        alignButton.identifier = NSUserInterfaceItemIdentifier("auto-blend-align")
        fillButton.state = .off
        fillButton.font = DS.sans(13)
        fillButton.identifier = NSUserInterfaceItemIdentifier("auto-blend-fill")
        updateFillEnabled()
        let options = NSStackView(views: [seamlessButton, alignButton, fillButton])
        options.orientation = .vertical
        options.alignment = .leading
        options.spacing = 6

        let grid = NSGridView(views: [
            [fieldLabel("Method:"), methodPopUp],
            [fieldLabel("Options:"), options],
        ])
        grid.rowSpacing = 14
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106
        grid.cell(for: options)?.yPlacement = .top
        grid.cell(atColumnIndex: 0, rowIndex: 1).yPlacement = .top

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        applyButton.identifier = NSUserInterfaceItemIdentifier("auto-blend-apply")
        view = makeSheetView(
            title: AutoBlend.title,
            hint: "Panorama gives each selected layer a mask revealing its side of a seam "
                + "through each overlap; Stack Images, where it is sharpest. Seamless Tones and "
                + "Colours matches the layers' tones and blends across the seams (Panorama) or "
                + "adds a merged layer (Stack Images); Align first registers the layers to one "
                + "another before they are compared. Content-Aware Fill Transparent Areas "
                + "(Panorama) adds the picture as one layer with its empty corners filled.",
            content: grid,
            buttonRow: makeButtonRow(cancel: cancelButton, apply: applyButton))
    }

    private var selectedMethod: AutoBlend.Method {
        let methods = AutoBlend.Method.allCases
        let index = methodPopUp.indexOfSelectedItem
        return methods.indices.contains(index) ? methods[index] : .panorama
    }

    /// The fill is Panorama's alone (`AutoBlend.fillNeedsPanorama`).
    private func updateFillEnabled() {
        fillButton.isEnabled = selectedMethod == .panorama
    }

    @objc private func methodChanged(_ sender: Any?) {
        updateFillEnabled()
    }

    @objc private func applyClicked(_ sender: Any?) {
        let method = selectedMethod
        let seamless = seamlessButton.state == .on
        let align = alignButton.state == .on
        let fill = method == .panorama && fillButton.state == .on
        dismiss(self)
        onApply(method, seamless, align, fill)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}
