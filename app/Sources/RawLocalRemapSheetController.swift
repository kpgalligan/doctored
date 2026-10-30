import AppKit

/// One authored canonical u16 mask remap; the canvas dimensions stay fixed.
final class RawLocalRemapSheetController: NSObject {
    private let dimensions: (Int, Int)
    private var modal: ModalCardWindowController?
    private var fields: [NSTextField] = []
    private var filter = NSPopUpButton()
    private var result: RawSourceMap?

    private init(dimensions: (Int, Int)) { self.dimensions = dimensions }
    @available(*, unavailable) required init?(coder: NSCoder) {
        fatalError("RawLocalRemapSheetController does not support NSCoder")
    }
    static func run(width: Int, height: Int) -> RawSourceMap? {
        RawLocalRemapSheetController(dimensions: (width, height)).run()
    }
    private func run() -> RawSourceMap? {
        let body = NSStackView()
        body.orientation = .vertical; body.alignment = .leading; body.spacing = 7
        let note = NSTextField(wrappingLabelWithString:
            "Remap this canonical \(dimensions.0)×\(dimensions.1) mask. The inverse matrix samples the existing mask at each new pixel edge. Coverage stays native u16; the exact matrix is recorded.")
        note.font = DS.sans(11); note.preferredMaxLayoutWidth = 400
        note.widthAnchor.constraint(equalToConstant: 400).isActive = true
        body.addArrangedSubview(note)
        for (index, pair) in zip(["a", "b", "c", "d", "tx", "ty"],
                                 ["1", "0", "0", "1", "0", "0"]).enumerated() {
            let row = NSStackView()
            row.orientation = .horizontal; row.spacing = 8
            let label = NSTextField(labelWithString: pair.0)
            label.widthAnchor.constraint(equalToConstant: 40).isActive = true
            let field = NSTextField(string: pair.1)
            field.widthAnchor.constraint(equalToConstant: 170).isActive = true
            field.setAccessibilityIdentifier("raw-local-remap-\(index)")
            row.addArrangedSubview(label); row.addArrangedSubview(field)
            fields.append(field); body.addArrangedSubview(row)
        }
        filter.addItems(withTitles: ["Bilinear", "Bicubic"])
        filter.setAccessibilityIdentifier("raw-local-remap-filter")
        body.addArrangedSubview(filter)
        let cancel = NSButton(title: "Cancel", target: self, action: #selector(cancel(_:)))
        let apply = StickerButton(title: "Remap Mask", style: .primary,
            target: self, action: #selector(apply(_:)))
        let card = makeSheetView(title: "Remap RAW Local Mask", content: body,
            buttonRow: makeButtonRow(cancel: cancel, apply: apply), width: 450)
        let modal = ModalCardWindowController(title: "Remap RAW Local Mask", card: card)
        self.modal = modal
        let response = modal.runModal()
        self.modal = nil
        return response == .OK ? result : nil
    }
    @objc private func apply(_ sender: Any?) {
        let coefficients = fields.compactMap { Double($0.stringValue) }
        guard coefficients.count == 6, coefficients.allSatisfy(\.isFinite),
              let width = UInt32(exactly: dimensions.0),
              let height = UInt32(exactly: dimensions.1) else {
            NSSound.beep(); return
        }
        result = RawSourceMap(width: width, height: height, inverse: coefficients,
            filter: filter.indexOfSelectedItem == 1 ? .bicubic : .bilinear)
        modal?.end(.OK)
    }
    @objc private func cancel(_ sender: Any?) { modal?.end(.cancel) }
}
