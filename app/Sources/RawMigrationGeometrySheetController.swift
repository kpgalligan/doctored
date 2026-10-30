import AppKit

/// Explicit inverse map for an unregistered decoder extent. A review always
/// displays this exact map and the two native dimensions before publication.
final class RawMigrationGeometrySheetController: NSObject {
    private var modal: ModalCardWindowController?
    private var fields: [NSTextField] = []
    private var filter = NSPopUpButton()
    private let destination: (Int, Int)
    private var resolved: RawSourceMap?

    private init(destination: (Int, Int)) { self.destination = destination }
    @available(*, unavailable) required init?(coder: NSCoder) {
        fatalError("RawMigrationGeometrySheetController does not support NSCoder")
    }
    static func run(saved: (Int, Int), proposed: (Int, Int)) -> RawSourceMap? {
        let controller = RawMigrationGeometrySheetController(destination: proposed)
        return controller.run(saved: saved)
    }
    private func run(saved: (Int, Int)) -> RawSourceMap? {
        let note = NSTextField(wrappingLabelWithString:
            "The saved RAW is \(saved.0)×\(saved.1); the proposed decoder is \(destination.0)×\(destination.1). Enter an inverse map from each proposed pixel-edge coordinate into the saved source. These coefficients are reviewed and recorded at full precision.")
        note.font = DS.sans(12)
        note.preferredMaxLayoutWidth = 430
        note.widthAnchor.constraint(equalToConstant: 430).isActive = true
        let rows = NSStackView()
        rows.orientation = .vertical
        rows.alignment = .leading
        rows.spacing = 8
        rows.addArrangedSubview(note)
        for (index, pair) in zip(["a · X scale", "b · Y shear", "c · X shear", "d · Y scale",
                                  "tx · X offset", "ty · Y offset"],
                                 ["1", "0", "0", "1", "0", "0"]).enumerated() {
            let row = NSStackView()
            row.orientation = .horizontal
            row.spacing = 8
            let label = NSTextField(labelWithString: pair.0)
            label.font = DS.sans(11)
            label.widthAnchor.constraint(equalToConstant: 110).isActive = true
            let field = NSTextField(string: pair.1)
            field.font = DS.sans(11)
            field.widthAnchor.constraint(equalToConstant: 170).isActive = true
            field.setAccessibilityIdentifier("raw-migration-map-\(index)")
            row.addArrangedSubview(label)
            row.addArrangedSubview(field)
            fields.append(field)
            rows.addArrangedSubview(row)
        }
        filter.addItems(withTitles: ["Bilinear", "Bicubic"])
        filter.setAccessibilityIdentifier("raw-migration-filter")
        rows.addArrangedSubview(filter)
        let cancel = NSButton(title: "Keep Saved Rendering", target: self, action: #selector(cancel(_:)))
        let review = StickerButton(title: "Review Geometry", style: .primary,
            target: self, action: #selector(review(_:)))
        let card = makeSheetView(title: "Register RAW Decoder Geometry", content: rows,
            buttonRow: makeButtonRow(cancel: cancel, apply: review), width: 480)
        let modal = ModalCardWindowController(title: "RAW Decoder Geometry", card: card)
        self.modal = modal
        let response = modal.runModal()
        self.modal = nil
        return response == .OK ? resolved : nil
    }
    @objc private func review(_ sender: Any?) {
        let values = fields.compactMap { Double($0.stringValue) }
        guard values.count == 6, values.allSatisfy(\.isFinite),
              let width = UInt32(exactly: destination.0),
              let height = UInt32(exactly: destination.1) else {
            NSSound.beep(); return
        }
        resolved = RawSourceMap(width: width, height: height, inverse: values,
            filter: filter.indexOfSelectedItem == 1 ? .bicubic : .bilinear)
        modal?.end(.OK)
    }
    @objc private func cancel(_ sender: Any?) { modal?.end(.cancel) }
}
