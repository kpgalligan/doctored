import AppKit

/// One Contour row of the Layer Style sheet: the label, the picker
/// (the ten presets by title, then "Custom…") and the Anti-aliased
/// checkbox, on one line. "Custom…" opens a popover holding the Curves
/// dialog's `CurveEditorView`, seeded with the effect's custom points or
/// Linear's two; each edit writes a smooth custom curve through the
/// binding, so the sheet previews live. The picker shows titles only (no
/// thumbnails).
final class ContourRow: NSObject, PaneRow, NSPopoverDelegate {
    private let label: NSTextField
    private let popup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let antiAliased: NSButton
    private let get: () -> ContourValue?
    private let set: (ContourValue?) -> Void
    private let getAntiAliased: () -> Bool
    private let setAntiAliased: (Bool) -> Void
    private var popover: NSPopover?

    /// The picker item that stands for a custom curve.
    private static let custom = "custom"

    var views: [NSView] { [label, popup, antiAliased] }

    init(
        label: String, get: @escaping () -> ContourValue?,
        set: @escaping (ContourValue?) -> Void, antiAliased getAntiAliased: @escaping () -> Bool,
        setAntiAliased: @escaping (Bool) -> Void
    ) {
        self.label = fieldLabel(label)
        self.antiAliased = NSButton(checkboxWithTitle: "Anti-aliased", target: nil, action: nil)
        self.get = get
        self.set = set
        self.getAntiAliased = getAntiAliased
        self.setAntiAliased = setAntiAliased
        super.init()
        let menu = NSMenu()
        for (id, title) in zip(ContourValue.presets, ContourValue.titles) {
            let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
            item.representedObject = id
            menu.addItem(item)
        }
        menu.addItem(.separator())
        let custom = NSMenuItem(title: "Custom…", action: nil, keyEquivalent: "")
        custom.representedObject = Self.custom
        menu.addItem(custom)
        popup.menu = menu
        popup.font = DS.sans(13)
        popup.target = self
        popup.action = #selector(picked(_:))
        popup.widthAnchor.constraint(equalToConstant: PaneRows.sliderWidth).isActive = true
        antiAliased.font = DS.sans(13)
        antiAliased.target = self
        antiAliased.action = #selector(toggled(_:))
        reload()
    }

    func reload() {
        let contour = get()
        let id = contour?.isCustom == true ? Self.custom : ContourValue.presetID(contour)
        if let item = popup.itemArray.first(where: { ($0.representedObject as? String) == id }) {
            popup.select(item)
        }
        antiAliased.state = getAntiAliased() ? .on : .off
    }

    @objc private func picked(_ sender: Any?) {
        guard let id = popup.selectedItem?.representedObject as? String else { return }
        if id == Self.custom {
            showEditor()
        } else {
            set(ContourValue.picked(id))
        }
    }

    @objc private func toggled(_ sender: Any?) {
        setAntiAliased(antiAliased.state == .on)
    }

    private func showEditor() {
        let editor = CurveEditorView()
        let seed = get()?.points ?? ContourValue.linearPoints
        editor.points = seed.map { CurvePoint(x: $0.x, y: $0.y) }
        editor.onPointsChanged = { [weak self] points in self?.edited(points) }
        let side = editor.intrinsicContentSize
        editor.frame = NSRect(origin: .zero, size: side)
        let content = NSViewController()
        content.view = editor
        let popover = NSPopover()
        popover.contentViewController = content
        popover.contentSize = side
        popover.behavior = .transient
        popover.delegate = self
        self.popover = popover
        popover.show(relativeTo: popup.bounds, of: popup, preferredEdge: .maxX)
    }

    /// One editor change: the points as a smooth custom curve.
    private func edited(_ points: [CurvePoint]) {
        set(ContourValue(points: points.map { ContourPointValue(x: $0.x, y: $0.y) }))
    }

    func popoverDidClose(_ notification: Notification) {
        popover = nil
        // Back on what the effect holds (a preset when the editor was left
        // without an edit).
        reload()
    }
}

extension PaneRows {
    /// A Contour row bound to an effect's optional contour and Anti-aliased
    /// (nil / false write no key).
    static func contour(
        _ label: String = "Contour:", get: @escaping () -> ContourValue?,
        set: @escaping (ContourValue?) -> Void, antiAliased: @escaping () -> Bool?,
        setAntiAliased: @escaping (Bool?) -> Void
    ) -> ContourRow {
        ContourRow(
            label: label, get: get, set: set, antiAliased: { antiAliased() ?? false },
            setAntiAliased: { setAntiAliased($0 ? true : nil) })
    }

    /// The Range, 1…100 % in whole percents over the model's
    /// 0.01…1; 50 % (the default) is stored as nil, which writes no key.
    static func range(
        _ label: String = "Range:", get: @escaping () -> Double?,
        set: @escaping (Double?) -> Void
    ) -> PaneSliderRow {
        slider(
            label, min: 1, max: 100, integer: true, format: { String(format: "%.0f %%", $0) },
            get: { (get() ?? 0.5) * 100 }, set: { set($0 == 50 ? nil : $0 / 100) })
    }
}
