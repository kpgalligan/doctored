import AppKit

/// The page-and-resolution prompt a PDF or SVG open shows (Phase 11D).
///
/// It runs as a MODAL alert with an accessory view rather than as a document
/// sheet, for the same reason `PSDImport`'s compatibility prompt does: at the
/// point the choice is needed there is no document and no window to attach a
/// sheet to — `NSDocumentController` is still deciding what to read.
final class VectorImportSheetController: NSObject {
    private let kind: VectorKind
    private let pagePopup = NSPopUpButton()
    private let ppiField = NSTextField()
    private let ppiMenu = NSPopUpButton()
    private let readout = NSTextField(wrappingLabelWithString: "")

    private init(kind: VectorKind) {
        self.kind = kind
        super.init()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("VectorImportSheetController does not support NSCoder")
    }

    /// Asks for the page and the resolution. nil when the user cancelled.
    static func runModal(url: URL, kind: VectorKind) -> VectorImportChoice? {
        let controller = VectorImportSheetController(kind: kind)
        let alert = NSAlert()
        alert.messageText = "Import “\(url.lastPathComponent)”"
        alert.informativeText = kind.name == "pdf"
            ? "The page is rasterized at the resolution you choose. Nothing is retained for "
                + "re-rasterization: the document holds pixels."
            : "The drawing is rasterized at the resolution you choose by the system's SVG "
                + "renderer. Nothing is retained for re-rasterization."
        alert.addButton(withTitle: "Import")
        alert.addButton(withTitle: "Cancel")
        alert.accessoryView = controller.accessory()
        alert.window.initialFirstResponder = controller.ppiField
        guard alert.runModal() == .alertFirstButtonReturn else { return nil }
        return controller.choice
    }

    private func accessory() -> NSView {
        for index in 1...kind.pages {
            let size = kind.pageSizes[index - 1]
            pagePopup.addItem(withTitle: String(
                format: "Page %d — %.0f × %.0f pt", index, size.width, size.height))
        }
        pagePopup.isEnabled = kind.pages > 1
        pagePopup.target = self
        pagePopup.action = #selector(controlChanged(_:))
        pagePopup.setAccessibilityIdentifier("vector-import.page")

        DSField.style(ppiField)
        ppiField.stringValue = String(format: "%g", VectorImport.defaultPPI)
        ppiField.target = self
        ppiField.action = #selector(controlChanged(_:))
        ppiField.widthAnchor.constraint(equalToConstant: 80).isActive = true
        ppiField.setAccessibilityIdentifier("vector-import.ppi")

        ppiMenu.addItem(withTitle: "Presets")
        for value in VectorImport.quickPPI {
            ppiMenu.addItem(withTitle: String(format: "%g ppi", value))
        }
        ppiMenu.target = self
        ppiMenu.action = #selector(presetChosen(_:))
        ppiMenu.setAccessibilityIdentifier("vector-import.ppi-presets")

        readout.font = DS.sans(11)
        readout.textColor = DS.textMuted
        readout.preferredMaxLayoutWidth = 360
        readout.setAccessibilityIdentifier("vector-import.readout")

        let ppiRow = NSStackView(views: [label("Resolution"), ppiField, ppiMenu])
        ppiRow.orientation = .horizontal
        ppiRow.spacing = 8
        let pageRow = NSStackView(views: [label("Page"), pagePopup])
        pageRow.orientation = .horizontal
        pageRow.spacing = 8

        let stack = NSStackView(views: kind.pages > 1 ? [pageRow, ppiRow, readout]
                                                      : [ppiRow, readout])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 10
        stack.translatesAutoresizingMaskIntoConstraints = true
        stack.frame = NSRect(x: 0, y: 0, width: 400, height: kind.pages > 1 ? 110 : 80)
        refreshReadout()
        return stack
    }

    private func label(_ text: String) -> NSTextField {
        let field = NSTextField(labelWithString: text)
        field.font = DS.sans(12)
        field.widthAnchor.constraint(equalToConstant: 80).isActive = true
        return field
    }

    private var choice: VectorImportChoice {
        var value = VectorImportChoice()
        value.page = max(1, min(kind.pages, pagePopup.indexOfSelectedItem + 1))
        value.ppi = Double(ppiField.stringValue) ?? VectorImport.defaultPPI
        return value
    }

    @objc private func presetChosen(_ sender: Any?) {
        let index = ppiMenu.indexOfSelectedItem - 1
        guard VectorImport.quickPPI.indices.contains(index) else { return }
        ppiField.stringValue = String(format: "%g", VectorImport.quickPPI[index])
        ppiMenu.selectItem(at: 0)
        refreshReadout()
    }

    @objc private func controlChanged(_ sender: Any?) {
        refreshReadout()
    }

    /// The pixel size the current choice produces, or the refusal that choice
    /// earns — asked of `VectorImport` so the sheet cannot show a size the
    /// import would then refuse.
    private func refreshReadout() {
        do {
            let (width, height) = try VectorImport.pixelSize(kind, choice: choice)
            readout.stringValue = "\(width) × \(height) pixels"
            readout.textColor = DS.textMuted
        } catch {
            readout.stringValue = (error as? DoctoredCoreError)?.message
                ?? error.localizedDescription
            readout.textColor = .systemRed
        }
    }
}
