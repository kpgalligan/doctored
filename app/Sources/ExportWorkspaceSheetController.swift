import AppKit

// The export sheet's preview is decoded from the bytes it is about to write and Save is disabled
// while a preview is pending or failed, so the picture on screen and the file on disk are the same
// bytes.

/// File ▸ Export… — the export workspace.
///
/// The preview is rendered from the ACTUAL ENCODED BYTES of the current
/// settings (`RasterExportResult.preview()` decodes what would be written),
/// and Save writes that very result, so the picture on screen and the file on
/// disk are the same bytes rather than two encodes that ought to agree.
/// Save is disabled whenever a preview is pending or failed, because there is
/// then no result to write.
final class ExportWorkspaceSheetController: NSViewController {
    private weak var document: ImageDocument?
    private let capture: ExportCapture
    private var settings: ExportSettings

    private let formatPopup = NSPopUpButton()
    private let qualityField = NSTextField()
    private let depthPopup = NSPopUpButton()
    /// The sample width of a 32-bit float deliverable; shown on a
    /// float document only.
    private let samplesPopup = NSPopUpButton()
    /// The HDR encoding of a 32-bit HEIC or AVIF (v1.x): gain map or
    /// PQ; shown on a float document only.
    private let hdrPopup = NSPopUpButton()
    private let ditherPopup = NSPopUpButton()
    private let sizePopup = NSPopUpButton()
    private let sizeField = NSTextField()
    private let filterPopup = NSPopUpButton()
    private let profilePopup = NSPopUpButton()
    /// The rows `profilePopup` shows, in order: the three built-ins then the
    /// installed RGB profiles an export can convert into at every depth
    /// (`InstalledProfiles.exportDestinations`), plus an installed
    /// file whose bytes are the document's own profile, so it stays selectable.
    private var profileChoices: [OutputProfile] = OutputProfile.builtins
    private let intentPopup = NSPopUpButton()
    private let bpcBox = NSButton(checkboxWithTitle: "Black point compensation",
                                  target: nil, action: nil)
    private let sharpenBox = NSButton(checkboxWithTitle: "Output sharpening", target: nil, action: nil)
    /// Unsharp Mask or Smart Sharpen. Radius and Amount are both
    /// methods' fields (Amount in percent under Smart Sharpen); Threshold is
    /// Unsharp Mask's; the rows in `smartRows` are Smart Sharpen's.
    private let sharpenMethodPopup = NSPopUpButton()
    private let radiusField = NSTextField()
    private let amountField = NSTextField()
    private let amountLabel = fieldLabel("Amount")
    private let thresholdField = NSTextField()
    private let noiseField = NSTextField()
    private let removePopup = NSPopUpButton()
    private let angleField = NSTextField()
    /// Fade, tonal width and radius of the Shadows, then of the Highlights.
    private let shadowFields = [NSTextField(), NSTextField(), NSTextField()]
    private let highlightFields = [NSTextField(), NSTextField(), NSTextField()]
    private var smartRows: [NSView] = []
    private let embedBox = NSButton(checkboxWithTitle: "Embed colour profile", target: nil, action: nil)
    private let stripBox = NSButton(checkboxWithTitle: "Strip capture metadata", target: nil, action: nil)
    private let presetPopup = NSPopUpButton()
    private let metadataPopup = NSPopUpButton()
    private let previewView = NSImageView()
    private let status = NSTextField(wrappingLabelWithString: "")
    private var saveButton: NSButton?

    private let renderer = PreviewRenderer()
    private let mailbox = ExportPreviewMailbox()
    private var serial = 0
    private var encoded: RasterExportResult?
    private var presets: [ExportPreset] = []
    private var policies: [NamedMetadataPolicy] = []

    private let isTyped: Bool
    /// True for a 32-bit FLOAT document, which is the only one that can ask
    /// for the exact 32-bit deliverable (Phase 11F).
    private let isFloat: Bool
    /// A 32-bit AVIF of a float document: an HDR AVIF, which this system
    /// cannot encode (`HostImageEncoder.hdrAVIFUnavailable`).
    /// A CMYK document (#1): Document profile keeps CMYK for JPEG and TIFF (`cmyk_export`), a
    /// TIFF at 8 or 16 bits.
    private let isCmyk: Bool

    /// True when these settings keep CMYK: the core resizes, sharpens, changes the TIFF depth and
    /// applies the metadata policy on ink, and its 16 → 8 step never dithers.
    private func keepsCmyk(_ settings: ExportSettings) -> Bool {
        isCmyk && settings.profile == .document && (settings.format == .jpeg || settings.format == .tiff)
    }

    private var isHDRAVIF: Bool {
        isFloat && settings.bitDepth == 32 && settings.format == .avif
    }

    init(document: ImageDocument) throws {
        self.document = document
        capture = try ExportCapture(document)
        settings = ExportSettings.defaults(for: document)
        isTyped = capture.source.precision != nil
        isFloat = capture.source.isFloat
        isCmyk = capture.source.isCmyk
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("ExportWorkspaceSheetController does not support NSCoder")
    }

    override func loadView() {
        for field in [qualityField, sizeField, radiusField, amountField, thresholdField,
                      noiseField, angleField] + shadowFields + highlightFields {
            DSField.style(field)
            field.target = self
            field.action = #selector(controlChanged(_:))
            let small = shadowFields.contains(field) || highlightFields.contains(field)
            field.widthAnchor.constraint(equalToConstant: small ? 44 : 72).isActive = true
        }
        sharpenMethodPopup.addItems(withTitles: ["Unsharp Mask", "Smart Sharpen"])
        removePopup.addItems(withTitles: SmartSharpenRemove.allCases.map(\.title))
        formatPopup.addItems(withTitles: ExportFormat.allCases.map(\.displayName))
        // OpenEXR and Radiance are float deliverables: offered on a
        // float document and hidden on every other.
        for (index, format) in ExportFormat.allCases.enumerated() where format.isFloatOnly {
            formatPopup.item(at: index)?.isHidden = !isFloat
        }
        // 32-bit is the float document's exact deliverable and is offered
        // only there; the sync below hides it on every other document.
        depthPopup.addItems(withTitles: ["8-bit", "16-bit", "32-bit float"])
        samplesPopup.addItems(withTitles: ["32-bit float", "16-bit half"])
        hdrPopup.addItems(withTitles: [ExportSettings.HDREncoding.gainMap.displayName,
                                       ExportSettings.HDREncoding.pq.displayName])
        hdrPopup.setAccessibilityIdentifier("export.hdrEncoding")
        ditherPopup.addItems(withTitles: ["No dither", "Ordered dither"])
        sizePopup.addItems(withTitles: ["Original size", "Width", "Height", "Long edge", "Scale %"])
        filterPopup.addItems(withTitles: ExportSettings.filterNames.map(\.2))
        let ownBytes = capture.source.iccProfile
        profileChoices = OutputProfile.builtins + InstalledProfiles.rgb.filter {
            $0.description.exportDestinationRefusal == nil
                || (ownBytes != nil
                    && (try? Data(contentsOf: $0.url)) == ownBytes)
        }.map {
            OutputProfile.file(path: $0.path, name: $0.name)
        }
        profilePopup.addItems(withTitles: profileChoices.map(\.displayName))
        for intent in RenderingIntent.allCases {
            intentPopup.addItem(withTitle: intent.displayName)
            intentPopup.lastItem?.tag = Int(intent.code)
        }
        for popup in [formatPopup, depthPopup, samplesPopup, hdrPopup, ditherPopup, sizePopup,
                      filterPopup, profilePopup, intentPopup, sharpenMethodPopup, removePopup] {
            popup.target = self
            popup.action = #selector(controlChanged(_:))
            popup.widthAnchor.constraint(equalToConstant: 150).isActive = true
        }
        for box in [sharpenBox, embedBox, stripBox, bpcBox] {
            box.font = DS.sans(12)
            box.target = self
            box.action = #selector(controlChanged(_:))
        }
        formatPopup.setAccessibilityIdentifier("export.format")
        sizePopup.setAccessibilityIdentifier("export.size")
        profilePopup.setAccessibilityIdentifier("export.profile")
        sharpenBox.setAccessibilityIdentifier("export.sharpen")
        sharpenMethodPopup.setAccessibilityIdentifier("export.sharpenMethod")

        metadataPopup.widthAnchor.constraint(equalToConstant: 150).isActive = true
        metadataPopup.target = self
        metadataPopup.action = #selector(metadataPolicyChosen(_:))
        metadataPopup.setAccessibilityIdentifier("export.metadata")

        presetPopup.widthAnchor.constraint(equalToConstant: 150).isActive = true
        presetPopup.target = self
        presetPopup.action = #selector(presetChosen(_:))
        presetPopup.setAccessibilityIdentifier("export.presets")
        let savePreset = StickerButton(title: "Save Preset…", style: .secondary,
                                       target: self, action: #selector(savePreset(_:)))
        let managePresets = StickerButton(title: "Manage…", style: .secondary,
                                          target: self, action: #selector(managePresets(_:)))

        previewView.imageScaling = .scaleProportionallyUpOrDown
        previewView.widthAnchor.constraint(equalToConstant: 300).isActive = true
        previewView.heightAnchor.constraint(equalToConstant: 260).isActive = true
        previewView.setAccessibilityIdentifier("export.preview")
        status.font = DS.mono(11)
        status.textColor = DS.textMuted
        status.preferredMaxLayoutWidth = 300
        let preview = NSStackView(views: [previewView, status])
        preview.orientation = .vertical
        preview.alignment = .leading
        preview.spacing = 8

        var rows: [NSView] = [row("Format", formatPopup), row("JPEG quality", qualityField)]
        if isTyped {
            rows.append(row("Bit depth", depthPopup))
            if isFloat {
                rows.append(row("Float samples", samplesPopup))
                rows.append(row("HDR encoding", hdrPopup))
            }
            rows.append(row("Dither", ditherPopup))
        }
        rows.append(row("Size", sizePopup))
        rows.append(row("Value", sizeField))
        rows.append(row("Resample", filterPopup))
        rows.append(row("Profile", profilePopup))
        if isCmyk {
            let note = NSTextField(wrappingLabelWithString:
                "Document profile keeps CMYK for JPEG and TIFF (8 or 16 bits, never dithered); "
                    + "other formats convert to sRGB.")
            note.font = DS.sans(11)
            note.textColor = DS.textMuted
            note.preferredMaxLayoutWidth = 250
            note.setAccessibilityIdentifier("export.cmykNote")
            rows.append(note)
        }
        rows.append(row("Intent", intentPopup))
        rows.append(bpcBox)
        rows.append(sharpenBox)
        rows.append(row("Method", sharpenMethodPopup))
        rows.append(row("Radius", radiusField))
        let amountRow = NSStackView(views: [amountLabel, amountField])
        amountRow.spacing = 8
        rows.append(amountRow)
        rows.append(row("Threshold", thresholdField))
        let triple: ([NSTextField]) -> NSStackView = { fields in
            let stack = NSStackView(views: fields)
            stack.spacing = 4
            return stack
        }
        smartRows = [row("Reduce Noise", noiseField), row("Remove", removePopup),
                     row("Angle", angleField), row("Shadows", triple(shadowFields)),
                     row("Highlights", triple(highlightFields))]
        rows.append(contentsOf: smartRows)
        rows.append(embedBox)
        rows.append(stripBox)
        rows.append(row("Metadata policy", metadataPopup))
        rows.append(row("Preset", presetPopup))
        let presetRow = NSStackView(views: [savePreset, managePresets])
        presetRow.spacing = 8
        rows.append(presetRow)
        let options = NSStackView(views: rows)
        options.orientation = .vertical
        options.alignment = .leading
        options.spacing = 8
        options.widthAnchor.constraint(equalToConstant: 260).isActive = true

        let columns = NSStackView(views: [preview, options])
        columns.orientation = .horizontal
        columns.alignment = .top
        columns.spacing = 24

        let save = StickerButton(title: "Export…", style: .primary,
                                 target: self, action: #selector(exportClicked(_:)))
        save.setAccessibilityIdentifier("export.save")
        save.isEnabled = false
        saveButton = save
        view = makeSheetView(
            title: "Export",
            hint: "The preview is decoded from the bytes that will be written, so the file is "
                + "exactly what you see. The document is not changed.",
            content: columns,
            buttonRow: makeButtonRow(
                cancel: sheetCancelButton(target: self, action: #selector(cancelClicked(_:))),
                apply: save),
            width: 640)
        applySettingsToControls()
        reloadPolicies()
        reloadPresets()
    }

    override func viewDidAppear() {
        super.viewDidAppear()
        requestPreview()
    }

    private func row(_ label: String, _ control: NSView) -> NSStackView {
        let row = NSStackView(views: [fieldLabel(label), control])
        row.orientation = .horizontal
        row.spacing = 8
        return row
    }

    // MARK: - Controls <-> settings

    private func applySettingsToControls() {
        formatPopup.selectItem(at: ExportFormat.allCases.firstIndex(of: settings.format) ?? 0)
        qualityField.stringValue = "\(settings.jpegQuality)"
        depthPopup.item(at: 2)?.isHidden = !isFloat
        depthPopup.selectItem(at: settings.bitDepth == 32 && isFloat
            ? 2 : settings.bitDepth == 16 ? 1 : 0)
        samplesPopup.selectItem(at: settings.floatSamples == .f16 ? 1 : 0)
        hdrPopup.selectItem(at: settings.hdrEncoding == .pq ? 1 : 0)
        ditherPopup.selectItem(at: settings.dither == "ordered" ? 1 : 0)
        switch settings.size {
        case .none: sizePopup.selectItem(at: 0); sizeField.stringValue = ""
        case .width(let value): sizePopup.selectItem(at: 1); sizeField.stringValue = "\(value)"
        case .height(let value): sizePopup.selectItem(at: 2); sizeField.stringValue = "\(value)"
        case .longEdge(let value): sizePopup.selectItem(at: 3); sizeField.stringValue = "\(value)"
        case .scale(let value): sizePopup.selectItem(at: 4)
            sizeField.stringValue = String(format: "%g", value * 100)
        }
        filterPopup.selectItem(
            at: ExportSettings.filterNames.firstIndex { $0.0 == settings.filter } ?? 3)
        profilePopup.selectItem(
            at: profileChoices.firstIndex(of: settings.profile) ?? 0)
        intentPopup.selectItem(withTag: Int(settings.intent.code))
        bpcBox.state = settings.blackPointCompensation ? .on : .off
        sharpenBox.state = settings.sharpen != nil ? .on : .off
        loadSharpenFields(settings.sharpen ?? .unsharpDefault)
        embedBox.state = settings.embedProfile ? .on : .off
        stripBox.state = settings.stripMetadata ? .on : .off
        updateEnablement()
    }

    /// Shows `sharpen`'s method and values; the other method's own fields
    /// keep their defaults, so switching to it starts there.
    private func loadSharpenFields(_ sharpen: OutputSharpen) {
        let format = { (value: Double) in String(format: "%g", value) }
        var smart = SmartSharpenSettings()
        var threshold = 0.02
        switch sharpen {
        case let .unsharpMask(radius, amount, unsharpThreshold):
            sharpenMethodPopup.selectItem(at: 0)
            radiusField.stringValue = format(radius)
            amountField.stringValue = format(amount)
            threshold = unsharpThreshold
        case .smartSharpen(let settings):
            sharpenMethodPopup.selectItem(at: 1)
            smart = settings
            radiusField.stringValue = format(settings.radius)
            amountField.stringValue = format(settings.amount)
        }
        thresholdField.stringValue = format(threshold)
        noiseField.stringValue = format(smart.noise)
        removePopup.selectItem(at: SmartSharpenRemove.allCases.firstIndex(of: smart.remove) ?? 1)
        angleField.stringValue = format(smart.angle)
        for (field, value) in zip(shadowFields, [smart.shadowFade, smart.shadowTonalWidth,
                                                 Double(smart.shadowRadius)]) {
            field.stringValue = format(value)
        }
        for (field, value) in zip(highlightFields, [smart.highlightFade, smart.highlightTonalWidth,
                                                    Double(smart.highlightRadius)]) {
            field.stringValue = format(value)
        }
    }

    /// The sharpen the fields describe; an unreadable number falls back to
    /// the method's default and the core refuses an out-of-range one.
    private func readSharpen() -> OutputSharpen {
        guard sharpenMethodPopup.indexOfSelectedItem == 1 else {
            return .unsharpMask(radius: Double(radiusField.stringValue) ?? 0,
                                amount: Double(amountField.stringValue) ?? 0,
                                threshold: Double(thresholdField.stringValue) ?? 0)
        }
        var smart = SmartSharpenSettings()
        let read = { (field: NSTextField, fallback: Double) -> Double in
            guard let value = Double(field.stringValue), value.isFinite else { return fallback }
            return value
        }
        // A whole-pixel radius; the core refuses one out of range.
        let whole = { (field: NSTextField, fallback: Int) -> Int in
            Int(min(max(read(field, Double(fallback)).rounded(), -1e6), 1e6))
        }
        smart.radius = read(radiusField, smart.radius)
        smart.amount = read(amountField, smart.amount)
        smart.noise = read(noiseField, smart.noise)
        smart.remove = SmartSharpenRemove.allCases[
            min(max(removePopup.indexOfSelectedItem, 0), SmartSharpenRemove.allCases.count - 1)]
        smart.angle = read(angleField, smart.angle)
        smart.shadowFade = read(shadowFields[0], smart.shadowFade)
        smart.shadowTonalWidth = read(shadowFields[1], smart.shadowTonalWidth)
        smart.shadowRadius = whole(shadowFields[2], smart.shadowRadius)
        smart.highlightFade = read(highlightFields[0], smart.highlightFade)
        smart.highlightTonalWidth = read(highlightFields[1], smart.highlightTonalWidth)
        smart.highlightRadius = whole(highlightFields[2], smart.highlightRadius)
        return .smartSharpen(smart)
    }

    private func updateEnablement() {
        // HEIC and AVIF are lossy and high-depth-capable (Phase 11D):
        // they take the quality dial like JPEG, and 16 bits may be ASKED
        // for — what the host's encoder actually wrote is reported by the
        // result. JPEG XL here is lossless and 8-bit, so it takes neither.
        let hostEncoded = settings.format == .heic || settings.format == .avif
        qualityField.isEnabled = settings.format == .jpeg || hostEncoded
        // OpenEXR and Radiance exist only at 32 bits, so the depth popup
        // shows that pin and is not a choice there.
        depthPopup.isEnabled = isTyped
            && (settings.format == .png || settings.format == .tiff || hostEncoded)
        // Half samples are a TIFF's or an OpenEXR's; Radiance has one form.
        samplesPopup.isEnabled = isFloat && settings.bitDepth == 32
            && (settings.format == .tiff || settings.format == .exr)
        // A 32-bit HEIC is the HDR photo deliverable (v1.x). A 32-bit
        // AVIF can only fail (no HDR AVIF encoder), so its row is disabled and
        // the tooltip says why.
        hdrPopup.isEnabled = isFloat && settings.bitDepth == 32 && settings.format == .heic
        hdrPopup.toolTip = isHDRAVIF ? HostImageEncoder.hdrAVIFUnavailable : nil
        ditherPopup.isEnabled = isTyped && settings.bitDepth == 8
        // Every control the kept-CMYK and float pins below disable is set on every pass, so
        // leaving the pinned case (another format or profile) turns it back on.
        let keeps = keepsCmyk(settings)
        sharpenBox.isEnabled = true
        profilePopup.isEnabled = true
        sizeField.isEnabled = settings.size != .none
        filterPopup.isEnabled = settings.size != .none
        let smart = sharpenMethodPopup.indexOfSelectedItem == 1
        var sharpenControls: [NSControl] = [sharpenMethodPopup, radiusField, amountField,
                                            thresholdField, noiseField, removePopup, angleField]
        sharpenControls += shadowFields as [NSControl]
        sharpenControls += highlightFields as [NSControl]
        for control in sharpenControls {
            control.isEnabled = sharpenBox.state == .on
        }
        thresholdField.isEnabled = sharpenBox.state == .on && !smart
        amountLabel.stringValue = smart ? "Amount (%)" : "Amount"
        for row in smartRows { row.isHidden = !smart }
        embedBox.isEnabled = ExportCapabilities.canEmbedProfile(settings.format)
        stripBox.isEnabled = ExportCapabilities.canCarryMetadata(settings.format)
        // "Document profile" never converts, so the core ignores intent and
        // BPC there; their values stay in `settings`.
        let converts = settings.profile != .document || (isCmyk && !keeps)
        intentPopup.isEnabled = converts
        bpcBox.isEnabled = converts
        // A kept-CMYK JPEG or TIFF (#1) resizes, sharpens, takes either TIFF depth and follows
        // the metadata policy, but its 16 → 8 step is the save's rounding: the core refuses a
        // dithered 8-bit TIFF of a 16-bit document, so dither shows its pin.
        if keeps {
            ditherPopup.isEnabled = false
        }
        // A 32-bit float export writes the document profile unsharpened
        // (`readControls` pins both), so those controls show the pins.
        if isFloat && settings.bitDepth == 32 {
            let pinned: [NSControl] = [profilePopup, intentPopup, bpcBox, sharpenBox]
            for control in pinned + sharpenControls {
                control.isEnabled = false
            }
        }
    }

    /// The controls' current value as one settings object. Ranges belong to
    /// the core, so a nonsense number simply becomes the refusal the encode
    /// reports in the status line.
    private func readControls() -> ExportSettings {
        var next = settings
        next.format = ExportFormat.allCases[
            min(max(formatPopup.indexOfSelectedItem, 0), ExportFormat.allCases.count - 1)]
        next.jpegQuality = Int(qualityField.stringValue) ?? settings.jpegQuality
        if isFloat, depthPopup.indexOfSelectedItem == 2 {
            next.bitDepth = 32
        } else {
            next.bitDepth = isTyped && depthPopup.indexOfSelectedItem == 1 ? 16 : 8
        }
        // OpenEXR and Radiance exist only at 32 bits, so choosing one
        // on a float document IS choosing the depth.
        if isFloat, next.format.isFloatOnly { next.bitDepth = 32 }
        // The formats `updateEnablement` enables the depth popup for, which
        // are the ones the core's request validation takes at 16 bits.
        if next.bitDepth == 16, ![.png, .tiff, .heic, .avif].contains(next.format) {
            next.bitDepth = 8
        }
        next.dither = next.bitDepth == 8 && ditherPopup.indexOfSelectedItem == 1 ? "ordered" : "none"
        let value = Double(sizeField.stringValue) ?? 0
        switch sizePopup.indexOfSelectedItem {
        case 1: next.size = .width(Int(value))
        case 2: next.size = .height(Int(value))
        case 3: next.size = .longEdge(Int(value))
        case 4: next.size = .scale(value / 100)
        default: next.size = .none
        }
        next.filter = ExportSettings.filterNames[
            min(max(filterPopup.indexOfSelectedItem, 0), ExportSettings.filterNames.count - 1)].0
        next.profile = profileChoices[
            min(max(profilePopup.indexOfSelectedItem, 0), profileChoices.count - 1)]
        next.intent = RenderingIntent.allCases
            .first { Int($0.code) == intentPopup.selectedTag() } ?? .relative
        next.blackPointCompensation = bpcBox.state == .on
        next.sharpen = sharpenBox.state == .on ? readSharpen() : nil
        next.embedProfile = embedBox.state == .on
        next.stripMetadata = stripBox.state == .on
        // Every stage but the resample that would move a sample is
        // refused by name at 32 bits, so the sheet pins them AFTER every
        // popup read and cannot offer a combination the core would only
        // reject (Phase 11F).
        if next.bitDepth == 32 {
            // Three containers hold float samples: TIFF, OpenEXR, Radiance;
            // HEIC and AVIF hold the HDR photo encodings (v1.x).
            if next.format != .tiff && !next.format.isFloatOnly
                && next.format != .heic && next.format != .avif { next.format = .tiff }
            next.profile = .document
            next.sharpen = nil
        }
        next.floatSamples = next.bitDepth == 32 && (next.format == .tiff || next.format == .exr)
            && samplesPopup.indexOfSelectedItem == 1 ? .f16 : .f32
        next.hdrEncoding = next.bitDepth == 32 && (next.format == .heic || next.format == .avif)
            ? (hdrPopup.indexOfSelectedItem == 1 ? .pq : .gainMap) : nil
        // A kept-CMYK JPEG or TIFF (#1) never dithers (the dither control is disabled).
        if keepsCmyk(next) {
            next.dither = "none"
        }
        // The popup only CHOOSES; `settings.metadata` carries the resolved
        // policy, so the recorded step and the request never name a library
        // entry that could later be edited or deleted.
        return next
    }

    // MARK: - Metadata policies

    private func reloadPolicies() {
        let chosen = metadataPopup.titleOfSelectedItem
        metadataPopup.removeAllItems()
        metadataPopup.addItem(withTitle: "Custom")
        do {
            policies = try MetadataPolicyLibrary.entries()
            metadataPopup.addItems(withTitles: policies.map(\.name))
        } catch {
            policies = []
            status.stringValue = error.localizedDescription
        }
        metadataPopup.addItem(withTitle: "Manage…")
        if let chosen, metadataPopup.itemTitles.contains(chosen), chosen != "Manage…" {
            metadataPopup.selectItem(withTitle: chosen)
        } else {
            metadataPopup.selectItem(at: 0)
        }
    }

    @objc private func metadataPolicyChosen(_ sender: Any?) {
        let index = metadataPopup.indexOfSelectedItem
        if metadataPopup.titleOfSelectedItem == "Manage…" {
            let current = settings.metadata ?? .defaults
            if let chosen = MetadataPolicyManageSheet.run(current: current) {
                settings.metadata = chosen.isDefault ? nil : chosen
            }
            reloadPolicies()
            requestPreview()
            return
        }
        settings.metadata = policies.indices.contains(index - 1)
            ? policies[index - 1].policy : nil
        requestPreview()
    }

    @objc private func controlChanged(_ sender: Any?) {
        // Choosing the other method starts from that method's defaults.
        if let popup = sender as? NSPopUpButton, popup === sharpenMethodPopup {
            loadSharpenFields(popup.indexOfSelectedItem == 1
                ? .smartSharpen(SmartSharpenSettings()) : .unsharpDefault)
        }
        settings = readControls()
        // At 32 bits the pinned format, profile and sharpen are set back in
        // their controls (a format with no float form snaps back to TIFF, and
        // choosing OpenEXR or Radiance shows the 32-bit depth it implies);
        // every other control keeps what was typed, as at 8/16 bits.
        if settings.bitDepth == 32 {
            depthPopup.selectItem(at: 2)
            samplesPopup.selectItem(at: settings.floatSamples == .f16 ? 1 : 0)
            formatPopup.selectItem(at: ExportFormat.allCases.firstIndex(of: settings.format) ?? 0)
            profilePopup.selectItem(at: profileChoices.firstIndex(of: settings.profile) ?? 0)
            sharpenBox.state = settings.sharpen != nil ? .on : .off
        }
        updateEnablement()
        requestPreview()
    }

    // MARK: - Presets

    private func reloadPresets() {
        presetPopup.removeAllItems()
        presetPopup.addItem(withTitle: "Custom")
        do {
            presets = try ExportPresetLibrary.entries()
            presetPopup.addItems(withTitles: presets.map(\.name))
        } catch {
            presets = []
            status.stringValue = error.localizedDescription
        }
        presetPopup.selectItem(at: 0)
    }

    @objc private func presetChosen(_ sender: Any?) {
        let index = presetPopup.indexOfSelectedItem - 1
        guard presets.indices.contains(index) else { return }
        settings = presets[index].settings
        applySettingsToControls()
        requestPreview()
    }

    @objc private func savePreset(_ sender: Any?) {
        ExportPresetSaveSheet.run(settings: readControls())
        reloadPresets()
    }

    @objc private func managePresets(_ sender: Any?) {
        if let chosen = ExportPresetManagerSheet.run(current: readControls()) {
            settings = chosen.settings
            applySettingsToControls()
            requestPreview()
        }
        reloadPresets()
    }

    // MARK: - The encoded preview

    private func requestPreview() {
        serial += 1
        encoded = nil
        saveButton?.isEnabled = false
        // An HDR AVIF can only fail, so no preview is encoded. The return
        // follows the serial bump, so an in-flight preview is dropped as stale.
        if isHDRAVIF {
            status.stringValue = HostImageEncoder.hdrAVIFUnavailable
            previewView.image = nil
            return
        }
        renderer.onRender = { [weak self] image in self?.showPreview(image) }
        let settings = self.settings
        let capture = self.capture
        let mailbox = self.mailbox
        let version = serial
        let space = ColorProfile.space(for: settings.profile.profileData(for: capture.source))
        do {
            _ = try RasterResources.shared.admit(scratch: try capture.scratchEstimate(settings),
                                                 own: capture.ownRetainedBytes,
                                                 remedy: capture.refusalRemedy(for: settings))
        } catch {
            status.stringValue = error.localizedDescription
            previewView.image = nil
            return
        }
        status.stringValue = "Encoding preview…"
        renderer.request {
            do {
                let result = try capture.encode(settings, control: nil)
                let image = try result.preview()
                mailbox.put(version, result)
                return image.makeCGImage(in: space)
            } catch {
                mailbox.fail(version, error.localizedDescription)
                return nil
            }
        }
    }

    private func showPreview(_ image: CGImage?) {
        guard let taken = mailbox.take(serial) else { return }
        switch taken {
        case .failed(let message):
            encoded = nil
            previewView.image = nil
            saveButton?.isEnabled = false
            status.stringValue = message
        case .encoded(let result):
            encoded = result
            saveButton?.isEnabled = true
            if let image {
                previewView.image = NSImage(cgImage: image,
                                            size: NSSize(width: image.width, height: image.height))
            }
            let info = (try? result.info()) ?? [:]
            let width = (info["width"] as? NSNumber)?.intValue ?? 0
            let height = (info["height"] as? NSNumber)?.intValue ?? 0
            let bytes = (info["bytes"] as? NSNumber)?.intValue ?? 0
            let depth = (info["bit_depth"] as? NSNumber)?.intValue ?? 8
            status.stringValue = "\(width)×\(height) px · \(depth)-bit · "
                + ByteCountFormatter.string(fromByteCount: Int64(bytes), countStyle: .file)
                + " encoded"
        }
    }

    // MARK: - Writing

    @objc private func exportClicked(_ sender: Any?) {
        guard let document, let result = encoded, let window = view.window else {
            NSSound.beep()
            return
        }
        let settings = self.settings
        let capture = self.capture
        let panel = NSSavePanel()
        panel.canCreateDirectories = true
        panel.allowedContentTypes = [settings.format.utType]
        panel.nameFieldStringValue = ExportNaming.baseName(of: document) + "-export"
        panel.beginSheetModal(for: window) { [weak self] response in
            guard let self else { return }
            guard response == .OK, let url = panel.url else { return }
            do {
                // The panel already confirmed any replacement, so this one
                // caller — and only this one — writes over an existing file.
                try result.write(to: url, overwrite: true)
                // The two deleted export commands were the only UI writers of
                // the five per-document save preferences `CapturedSave` reads
                // for every Save/Save As, so a successful export writes them
                // back exactly as they did. No `updateChangeCount`: an export
                // is not an edit, and `refreshCurrentViewState` keeps Undo
                // from reverting the preference.
                document.jpegExportQuality = settings.jpegQuality
                document.embedColorProfile = settings.embedProfile
                document.stripMetadata = settings.stripMetadata
                if capture.source.precision != nil {
                    document.exportBitDepth = settings.bitDepth
                    document.exportDither = settings.dither
                }
                document.history.refreshCurrentViewState()
                if let message = ExportCapabilities.droppedMessage(
                    capture.source, report: try result.saveReport(), format: settings.format,
                    embedProfile: settings.embedProfile, stripMetadata: settings.stripMetadata,
                    metadataNotCaptured: capture.metadataNotCaptured,
                    metadataImportReport: capture.metadataImportReport) {
                    document.presentExportNotice(message)
                }
            } catch {
                document.presentError(error)
            }
            self.renderer.cancel()
            self.dismiss(self)
        }
    }

    @objc private func cancelClicked(_ sender: Any?) {
        renderer.cancel()
        dismiss(self)
    }
}

/// The preview worker's result, handed to the main thread beside the CGImage
/// `PreviewRenderer` already carries. A locked mailbox rather than a shared
/// property: the compute closure runs on the renderer's own queue.
private final class ExportPreviewMailbox {
    enum Outcome {
        case encoded(RasterExportResult)
        case failed(String)
    }

    private let lock = NSLock()
    private var value: (Int, Outcome)?

    func put(_ version: Int, _ result: RasterExportResult) {
        lock.lock()
        value = (version, .encoded(result))
        lock.unlock()
    }

    func fail(_ version: Int, _ message: String) {
        lock.lock()
        value = (version, .failed(message))
        lock.unlock()
    }

    /// Only the CURRENT request's outcome is delivered; a stale one is
    /// dropped, exactly as the preview stamps elsewhere do.
    func take(_ version: Int) -> Outcome? {
        lock.lock()
        defer { lock.unlock() }
        guard let current = value, current.0 == version else { return nil }
        value = nil
        return current.1
    }
}
