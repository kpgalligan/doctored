import AppKit

/// Image > Apply Image… / Calculations… and Select > Add Luminosity Masks:
/// the sheets, their live previews and the edit wrappers. The MATH is not
/// here — `ChannelMath` owns `applyImage`/`calculated`/`sourcePlane`,
/// including the per-plane fallthrough an RGB target needs, and the MCP
/// mirrors call the same functions, so the sheet and the agent can never
/// drift.
extension EditorViewController {
    /// Image > Apply Image…
    @objc func applyImageSheet(_ sender: Any?) {
        guard document?.doc != nil else {
            NSSound.beep()
            return
        }
        presentAsSheet(ApplyImageSheetController(editor: self))
    }

    /// Image > Calculations…
    @objc func calculationsSheet(_ sender: Any?) {
        guard document?.doc != nil else {
            NSSound.beep()
            return
        }
        presentAsSheet(CalculationsSheetController(editor: self))
    }

    /// Select > Add Luminosity Masks: the nine "Lights 1".."Midtones 3"
    /// channels from the composite's Rec. 709 luma, as one undo step.
    @objc func addLuminosityMasks(_ sender: Any?) {
        if addNativeLuminosityMasks() { return }
        guard let document = document, let doc = document.doc else {
            NSSound.beep()
            return
        }
        // The core refuses all nine past either cap (the channel count, and
        // the total pixel budget an .rz file can carry) and applyEdit would
        // turn that nil into a bare beep — on a big canvas the budget allows
        // only a handful of channels, and nothing on screen says so. Ask the
        // same question first and name the answer (ChannelBudget); the
        // agent's mirror already explains its own refusal in-band.
        if let reason = doc.channelBudgetRefusal(
            width: doc.width, height: doc.height,
            adding: RasterDocument.luminosityMaskCount)
        {
            presentChannelBudgetAlert(reason)
            return
        }
        document.applyEdit("Add Luminosity Masks", record: .addLuminosityMasks) {
            $0.addingLuminosityMasks()
        }
    }

    /// Commits `ChannelMath.applyImage` as ONE undo step.
    ///
    /// The math re-runs against the document's CURRENT handle inside the
    /// transform rather than the handle the sheet previewed, so an edit that
    /// slipped in while the dialog was open survives — the rule every
    /// live-preview sheet in `Sheets.swift` follows. A target that rewrites
    /// layer pixels (`.layer`, `.plane`) goes through `applyRasterizingEdit`
    /// because it contradicts a text/shape description exactly as
    /// a brush stroke does; a `.channel` target touches no layer, so it takes
    /// `applyEdit`. Either way a nil result — an unavailable source, or a
    /// blend that changed nothing — beeps inside `applyEdit` and registers no
    /// undo step.
    ///
    /// A Merged source is resolved first (`mergedComposite`): the retained
    /// projection when current, else a render whose error is presented, with
    /// no beep and nothing recorded.
    func applyImage(_ p: ApplyImageParameters) {
        guard let document = document, let doc = document.doc else {
            NSSound.beep()
            return
        }
        let record: [ActionStep] = .applyImage(p, in: doc)
        switch p.target {
        case .layer, .plane:
            // An adjustment layer's pixels are ignored by the compositor, so
            // blending into them would register a real undo step with nothing
            // to show for it. Refused with the same alert fill, gradient and
            // the brush already show, and the same rule the agent's
            // apply_image applies (AgentServer+Channels's
            // rejectAdjustmentPixelEdit). A CHANNEL target is document state
            // and is never refused.
            if refuseAdjustmentPixelEdit() { return }
            guard let merged = mergedComposite(p.readsMerged, document, doc) else { return }
            document.applyRasterizingEdit(
                "Apply Image", layer: p.targetLayer, record: record
            ) {
                ChannelMath.applyImage($0, p, merged: merged)
            }
        case .channel:
            guard let merged = mergedComposite(p.readsMerged, document, doc) else { return }
            document.applyEdit("Apply Image", record: record) {
                ChannelMath.applyImage($0, p, merged: merged)
            }
        case .mask, .ink:
            // A layer mask is not one of the planes this arithmetic is defined
            // over; the sheet disables Apply, so this is reachable only if the
            // target changed under an open dialog. One ink plate (#1) is a
            // CMYK document's, which is typed and commits through
            // `NativeChannelMathSession`, never this byte path.
            NSSound.beep()
        }
    }

    /// Commits `ChannelMath.calculated` as a new channel, or sets it as the
    /// selection (transient undo). A Merged source is resolved first, as
    /// `applyImage` does.
    func calculations(_ p: CalculationsParameters, result: CalculationsResult) {
        guard let document = document, let doc = document.doc else {
            NSSound.beep()
            return
        }
        switch result {
        case .newChannel:
            // The same channel budget New Channel asks, answered with the
            // same alert rather than applyEdit's bare beep
            // (EditorViewController+Channels.channelBudgetAllowsOneMore).
            guard channelBudgetAllowsOneMore(doc) else { return }
            guard let merged = mergedComposite(p.readsMerged, document, doc) else { return }
            document.applyEdit(
                "Calculations", record: .calculations(p, result: "new_channel", in: doc)
            ) { current in
                guard let plane = ChannelMath.calculated(current, p, merged: merged) else {
                    return nil
                }
                return try current.addingChannel(
                    name: p.name, plane: plane, width: current.width, height: current.height)
            }
        case .selection:
            guard let merged = mergedComposite(p.readsMerged, document, doc) else { return }
            guard let plane = ChannelMath.calculated(doc, p, merged: merged) else {
                NSSound.beep()
                return
            }
            commitCanvasSelection(
                CanvasSelection(shape: .mask(plane), canvasWidth: doc.width, canvasHeight: doc.height),
                actionName: "Calculations Selection", record: .calculations(p, result: "selection", in: doc))
        }
    }

    /// `.some(nil)` when the parameters read no Merged plane, `.some(merged)`
    /// when they do, and nil when the render failed: the core's sentence is
    /// presented, with no beep and nothing recorded.
    private func mergedComposite(
        _ reads: Bool, _ document: ImageDocument, _ doc: RasterDocument
    ) -> MergedComposite?? {
        guard reads else { return .some(nil) }
        do {
            return .some(MergedComposite(doc: doc, image: try document.byteComposite(of: doc)))
        } catch {
            document.presentError(error)
            return nil
        }
    }
}

/// What Calculations does with the plane it computes.
enum CalculationsResult: Equatable {
    case newChannel
    case selection
}

/// The controls the two channel-math sheets share. `Sheets.swift`'s builders
/// cover the card, the 106 pt label column and the button row; these are the
/// rows Apply Image and Calculations need identically — the source, plane and
/// blend popups and the opacity pair — kept here rather than in either sheet
/// so neither dialog owns the other's controls.
///
/// Each `fill…` installs its popup's width constraint, so call it exactly
/// once per popup, from `loadView`.
enum ChannelMathControls {
    /// Popup width inside the 420 pt card: the inset takes 2 × 22, the label
    /// column 106 and the column spacing 12, leaving 258 pt — 220 keeps a
    /// margin for the longest blend-mode title ("Linear Dodge (Add)").
    private static let popupWidth: CGFloat = 220

    /// The tag a "Merged" item carries. Layer items carry their own index,
    /// which is why the sentinel has to be negative.
    private static let mergedTag = -1

    /// Merged plus every layer, TOP FIRST — the layers panel's order, so the
    /// two lists read the same way round.
    static func fillSources(_ popup: NSPopUpButton, _ doc: RasterDocument?) {
        let menu = NSMenu()
        let merged = NSMenuItem(title: "Merged", action: nil, keyEquivalent: "")
        merged.tag = mergedTag
        menu.addItem(merged)
        if let doc = doc, doc.layerCount > 0 {
            menu.addItem(.separator())
            for index in stride(from: doc.layerCount - 1, through: 0, by: -1) {
                let item = NSMenuItem(
                    title: doc.layerInfo(index)?.name ?? "Layer \(index + 1)",
                    action: nil, keyEquivalent: "")
                item.tag = index
                menu.addItem(item)
            }
        }
        popup.menu = menu
        popup.selectItem(at: 0)
        style(popup)
    }

    /// RGB, the colour planes and Luma, then every alpha channel — the plane
    /// vocabulary Apply Image and Calculations share. On a CMYK document
    /// (#1): CMYK (the composite), the four ink plates, Luma and Alpha; on a Grayscale document
    /// (phase 6b): Gray (the composite or layer) and Alpha.
    static func fillPlanes(_ popup: NSPopUpButton, _ doc: RasterDocument?) {
        let menu = NSMenu()
        if doc?.isGray == true {
            menu.addItem(planeItem("Gray", .rgb))
            menu.addItem(planeItem(RasterPlane.alpha.displayName, .plane(.alpha)))
        } else if doc?.isLab == true {
            // #1 Lab phase 6b-i: Lab (the composite or layer), Lightness, a and b (the stored
            // red, green and blue codes) and Alpha.
            menu.addItem(planeItem("Lab", .rgb))
            for plane: RasterPlane in [.red, .green, .blue] {
                menu.addItem(planeItem(LabDocument.channelName(plane), .plane(plane)))
            }
            menu.addItem(planeItem(RasterPlane.alpha.displayName, .plane(.alpha)))
        } else if doc?.isCmyk == true {
            menu.addItem(planeItem("CMYK", .rgb))
            for plate in CmykPlane.allCases {
                menu.addItem(planeItem(plate.name, .ink(plate)))
            }
            for plane: RasterPlane in [.luma, .alpha] {
                menu.addItem(planeItem(plane.displayName, .plane(plane)))
            }
        } else {
            menu.addItem(planeItem("RGB", .rgb))
            for plane: RasterPlane in [.red, .green, .blue, .luma, .alpha] {
                menu.addItem(planeItem(plane.displayName, .plane(plane)))
            }
        }
        let channels = doc?.channelCount ?? 0
        if channels > 0 {
            menu.addItem(.separator())
            for index in 0..<channels {
                menu.addItem(
                    planeItem(
                        doc?.channelInfo(index)?.name ?? "Alpha \(index + 1)", .channel(index)))
            }
        }
        popup.menu = menu
        popup.selectItem(at: 0)
        style(popup)
    }

    /// The blend modes grouped as in the layers panel; tags are the raw mode
    /// values, the one encoding `DrBlendMode` survives in a menu item.
    ///
    /// `singlePlane` leaves out the four HSL modes
    /// (`DrBlendMode.grayDegenerateModes`): a result that is ONE 8-bit plane
    /// — Calculations always, Apply Image onto a colour plane or an alpha
    /// channel — has no triple for them to work on, and the core refuses
    /// them there, so offering them would advertise four modes that answer
    /// the base unchanged. `allowed`, when given, keeps only those modes (a Grayscale document's
    /// list, #1 phase 6b).
    static func fillBlendModes(_ popup: NSPopUpButton, singlePlane: Bool, allowed: [DrBlendMode]? = nil) {
        let menu = NSMenu()
        for group in DrBlendMode.blendModeGroups {
            let offered = allowed.map { list in group.filter { list.contains($0.0) } } ?? group
            let usable = singlePlane
                ? offered.filter { !DrBlendMode.degeneratesOnGray($0.0) }
                : offered
            guard !usable.isEmpty else { continue }
            if menu.numberOfItems > 0 {
                menu.addItem(.separator())
            }
            for (mode, title) in usable {
                let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
                item.tag = Int(mode.rawValue)
                menu.addItem(item)
            }
        }
        popup.menu = menu
        popup.selectItem(withTag: Int(DR_BLEND_NORMAL.rawValue))
        style(popup)
    }

    /// The source layer a popup names; nil = Merged.
    static func sourceLayer(_ popup: NSPopUpButton) -> Int? {
        let tag = popup.selectedItem?.tag ?? mergedTag
        return tag >= 0 ? tag : nil
    }

    /// The plane a popup names. An item with no choice on it (a separator,
    /// or an empty menu) reads as RGB, the default the parameters carry.
    static func plane(_ popup: NSPopUpButton) -> PlaneChoice {
        popup.selectedItem?.representedObject as? PlaneChoice ?? .rgb
    }

    static func blendMode(_ popup: NSPopUpButton) -> DrBlendMode {
        let tag = popup.selectedItem?.tag ?? Int(DR_BLEND_NORMAL.rawValue)
        return DrBlendMode.allBlendModes.first { Int($0.0.rawValue) == tag }?.0 ?? DR_BLEND_NORMAL
    }

    /// 0–100 slider plus its whole-number field, styled and laid out as one
    /// row. The caller owns both controls and wires their actions.
    static func opacityRow(slider: NSSlider, field: NSTextField) -> NSStackView {
        slider.isContinuous = true
        slider.widthAnchor.constraint(equalToConstant: 170).isActive = true

        let formatter = NumberFormatter()
        formatter.numberStyle = .none
        formatter.allowsFloats = false
        formatter.minimum = 0
        formatter.maximum = 100
        field.formatter = formatter
        field.widthAnchor.constraint(equalToConstant: 56).isActive = true
        DSField.style(field)

        let row = NSStackView(views: [slider, field])
        row.orientation = .horizontal
        row.spacing = 10
        return row
    }

    /// The field caught up to the slider — whole percent, the granularity
    /// the Channel Options sheet already uses for the same kind of value.
    static func syncField(_ field: NSTextField, to slider: NSSlider) {
        field.stringValue = String(Int(slider.doubleValue.rounded()))
    }

    /// The slider caught up to a typed value, clamped into range and echoed
    /// back so the field can never show a number the slider does not hold.
    static func syncSlider(_ slider: NSSlider, to field: NSTextField) {
        slider.doubleValue = min(max(Double(field.integerValue), 0), 100)
        field.stringValue = String(Int(slider.doubleValue))
    }

    /// The slider's percent as the 0…1 opacity the parameters carry.
    static func opacity(_ slider: NSSlider) -> Double {
        min(max(slider.doubleValue, 0), 100) / 100
    }

    private static func planeItem(_ title: String, _ choice: PlaneChoice) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        // The choice rides on the item itself, so a separator simply carries
        // none and the menu needs no index arithmetic to stay in step.
        item.representedObject = choice
        return item
    }

    /// The sheets' popup look: the 13 pt sans of every other dialog control
    /// and one shared width so the rows line up. Installs a constraint, so
    /// call it once per popup (each `fill…` already does).
    static func style(_ popup: NSPopUpButton) {
        popup.font = DS.sans(13)
        popup.widthAnchor.constraint(equalToConstant: popupWidth).isActive = true
    }
}
