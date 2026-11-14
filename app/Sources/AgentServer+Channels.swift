import AppKit

// `add_channel`, the new-channel `save_selection` and `calculations` decide a full channel list up
// front like `duplicate_channel`.

/// The channel tools, mirroring the Channels panel and its menus:
///
/// | Handler | UI path it mirrors |
/// |---|---|
/// | `listChannels` | the Channels panel's row list |
/// | `addChannel` | the panel footer's New Channel / Save Selection as Channel |
/// | `duplicateChannel` / `deleteChannel` / `invertChannel` | the row menu's
///   Duplicate, Delete and Invert |
/// | `renameChannel` / `setChannelOptions` | a row double-click and the row
///   menu's Channel Options… |
/// | `loadSelection` | Select > Load Selection… and ⌘-click on a row or thumbnail |
/// | `saveSelection` | Select > Save Selection… |
/// | `applyImage` / `calculations` | Image > Apply Image… / Calculations… |
/// | `addLuminosityMasks` | Select > Add Luminosity Masks |
///
/// The shared pieces (target parsing, the coverage commit, plane filters,
/// plane selections, the channel argument) live in
/// AgentServer+ChannelTargets.swift; the math behind apply_image and
/// calculations is `ChannelMath`, shared with the sheets. Catalog entries
/// live in AgentCatalog+GuideChannel.swift and AgentCatalog+ChannelMath.swift
/// and the dispatch entries in this file's handler table — the parity check
/// at server start enforces it.
///
/// Every handler here follows the canonical shape — `target(a)` → validate
/// indices → `performGroupedEdit`/`performPixelEdit` → `jsonResult` — and
/// every core refusal comes back as `changed: false` with a note, never as
/// an error: the channel ops answer nil both for "nothing would move" (a
/// rename to the name it already has) and for a cap that will not stretch (a
/// full channel list), and in neither case is retrying the same call the
/// right move for a model.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let channelsHandlers: [String: ToolHandler] = [
        "list_channels": { $0.listChannels },
        "add_channel": { $0.addChannel },
        "duplicate_channel": { $0.duplicateChannel },
        "delete_channel": { $0.deleteChannel },
        "rename_channel": { $0.renameChannel },
        "set_channel_options": { $0.setChannelOptions },
        "invert_channel": { $0.invertChannel },
        "load_selection": { $0.loadSelection },
        "save_selection": { $0.saveSelection },
        "apply_image": { $0.applyImage },
        "calculations": { $0.calculations },
        "add_luminosity_masks": { $0.addLuminosityMasks },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let channelsToolFacts: [String: ToolFacts] = [
        "list_channels": [.nonRecordable, .typedTool, .floatTool, .cmykTool],
        "add_channel": [.commonTool, .floatTool, .cmykTool],
        "duplicate_channel": [.commonTool, .floatTool, .cmykTool],
        "delete_channel": [.typedTool, .floatTool, .cmykTool],
        "rename_channel": [.commonTool, .floatTool, .cmykTool],
        "set_channel_options": [.commonTool, .floatTool, .cmykTool],
        "invert_channel": [.commonTool, .cmykTool],
        // Parity with the float menu/tool whitelist in
        // `EditorViewController+Precision.swift`: a selection is canvas
        // coverage, not document pixels, so everything the UI lets a user do
        // with one the agent answers too (including Save/Load Selection,
        // whose `channel_set` family is admitted at depth 32).
        "load_selection": [.commonTool, .floatTool, .cmykTool],
        "save_selection": [.selectionSensitive, .typedTool, .floatTool, .cmykTool],
        // Apply Image writes layer pixels from u16 planes clamped to 0…1, so
        // it stays refused at float (the core's operator table has the row).
        // #1: on a CMYK document onto its layer (plate for plate), one plate or a channel.
        "apply_image": [.commonTool, .cmykTool],
        // Calculations and Add Luminosity Masks read planes and write a
        // channel or a selection, which a float document holds as u16 too;
        // #1: a CMYK document's sources are its composite, plates, luma,
        // alpha and channels.
        "calculations": [.commonTool, .floatTool, .cmykTool],
        "add_luminosity_masks": [.commonTool, .floatTool, .cmykTool],
    ]

    // MARK: - The channel list

    /// The document's alpha channels, in list order.
    func listChannels(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        return try jsonResult(["ok": true, "channels": Self.channelFields(doc)])
    }

    // MARK: - Channel CRUD

    /// New Channel / Save Selection as Channel: an alpha channel built from
    /// nothing, the selection, a layer's transparency or mask, or one plane
    /// of the composite. The new channel is always APPENDED, so its index is
    /// the last one — never `channelIndex(named:)`, which would answer an
    /// older channel of the same name (names are not unique).
    func addChannel(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let from = stringArg(a, "from") ?? "empty"
        let plane: [UInt8]
        switch from {
        case "empty":
            // 0 = nothing selected, the same all-zero plane the panel's New
            // Channel button makes.
            plane = [UInt8](repeating: 0, count: doc.width * doc.height)
        case "selection":
            guard let bytes = editor(document)?.agentSelection?.maskBytes() else {
                throw ToolError(
                    message: "Nothing is selected — make a selection first (select_rect, "
                        + "select_magic_wand, select_subject, …), or use from: \"empty\".")
            }
            plane = bytes
        case "layer_alpha", "layer_mask", "plane":
            // The three sources load_selection already speaks, parsed and
            // range-checked by the one helper so the two tools cannot drift.
            let source = try selectionSource(a, document)
            guard
                let bytes = doc.selectionPlane(for: source)
            else {
                throw ToolError(
                    message: "That source has no plane to copy — check the layer index, or "
                        + "that the layer still has a mask.")
            }
            plane = bytes
        case let other:
            throw ToolError(
                message: "from must be \"empty\", \"selection\", \"layer_alpha\", "
                    + "\"layer_mask\" or \"plane\" (got \"\(other)\")")
        }
        let name = Self.channelName(stringArg(a, "name"), doc)
        let overlay = try Self.overlayBytes(
            parseColor(a, "overlay_color", fallback: Self.defaultOverlayColor))
        let opacity = Self.overlayOpacity(doubleArg(a, "overlay_opacity"))
        let width = doc.width
        let height = doc.height
        // Decided up front at both depths, as duplicate_channel does: the
        // typed core REFUSES a full channel list where the byte core answers
        // nil.
        if doc.channelBudgetRefusal(width: width, height: height, adding: 1) != nil {
            return try channelListFullResult(["name": name, "from": from])
        }
        let added = try editStructure(document, "New Channel") {
            try $0.addingChannel(
                name: name, plane: plane, width: width, height: height,
                red: overlay.red, green: overlay.green, blue: overlay.blue, opacity: opacity)
        }
        guard added else { return try channelListFullResult(["name": name, "from": from]) }
        return try jsonResult([
            "ok": true, "channel": (document.doc?.channelCount ?? 1) - 1, "name": name,
            "from": from, "channels": document.doc?.channelCount ?? 0,
        ])
    }

    /// The row menu's Delete Channel. The editor re-points its own paint
    /// target when the document changes, so nothing here reaches into it.
    func deleteChannel(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let index = try channelArgument(a, doc)
        let name = doc.channelInfo(index)?.name ?? ""
        let removed = try editStructure(document, "Delete Channel") { try $0.removingChannel(index) }
        guard removed else {
            return try noOpResult(
                ["channel": index, "name": name],
                why: "channel \(index) could not be removed.")
        }
        return try jsonResult([
            "ok": true, "channel": index, "name": name,
            "channels": document.doc?.channelCount ?? 0,
        ])
    }

    /// The row's double-click rename. An empty name is refused exactly as
    /// the panel's field refuses it (it reverts rather than committing): a
    /// nameless channel can only ever be addressed by index again.
    func renameChannel(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let index = try channelArgument(a, doc)
        let name = try requiredString(a, "name")
        guard !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            throw ToolError(message: "name must not be empty.")
        }
        let renamed = try editStructure(document, "Rename Channel", renderNeutral: true) {
            try $0.renamingChannel(index, name)
        }
        guard renamed else {
            return try noOpResult(
                ["channel": index, "name": name],
                why: "channel \(index) is already named \"\(name)\".")
        }
        return try jsonResult(["ok": true, "channel": index, "name": name])
    }

    /// The row menu's Duplicate Channel: a copy named "<name> copy",
    /// inserted right after the original (the core's rule, and the Layers
    /// panel's for a duplicated layer). One undo step.
    func duplicateChannel(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let index = try channelArgument(a, doc)
        let name = doc.channelInfo(index)?.name ?? ""
        // Decided up front at both depths: the typed core REFUSES a full
        // channel list where the byte core answers nil.
        if doc.channelBudgetRefusal(width: doc.width, height: doc.height, adding: 1) != nil {
            return try channelListFullResult(["channel": index, "name": name])
        }
        let duplicated = try editStructure(document, "Duplicate Channel") {
            try $0.duplicatingChannel(index)
        }
        guard duplicated else { return try channelListFullResult(["channel": index, "name": name]) }
        // Inserted at index + 1, so the copy's index is knowable without a
        // name lookup (names are not unique, and "<name> copy" may collide).
        let copy = index + 1
        return try jsonResult([
            "ok": true, "channel": copy, "from": index,
            "name": document.doc?.channelInfo(copy)?.name ?? "\(name) copy",
            "channels": document.doc?.channelCount ?? 0,
        ])
    }

    /// Channel Options…: the name and the rubylith, in ONE undo step. Every
    /// property is optional and defaults to what the channel already has, so
    /// a caller can change the opacity without restating the colour.
    func setChannelOptions(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let index = try channelArgument(a, doc)
        guard let info = doc.channelInfo(index) else {
            throw ToolError(message: "Channel \(index) has no options to read")
        }
        // An EMPTY name keeps the one the channel has, exactly as the sheet
        // and the panel's inline field do — a nameless channel could only
        // ever be addressed by index again. rename_channel, whose whole
        // point is the name, errors on it instead.
        let requested = stringArg(a, "name") ?? info.name
        let name =
            requested.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            ? info.name : requested
        let overlay: (red: UInt8, green: UInt8, blue: UInt8)
        if a["overlay_color"] != nil {
            overlay = try Self.overlayBytes(
                parseColor(a, "overlay_color", fallback: Self.defaultOverlayColor))
        } else {
            overlay = (info.red, info.green, info.blue)
        }
        let opacity = doubleArg(a, "overlay_opacity").map { min(max($0, 0), 1) } ?? info.opacity
        let indicatesSelected: Bool
        switch stringArg(a, "color_indicates") {
        case nil: indicatesSelected = info.colorIndicatesSelected
        case "masked": indicatesSelected = false
        case "selected": indicatesSelected = true
        case let other:
            throw ToolError(
                message: "color_indicates must be \"masked\" or \"selected\" (got "
                    + "\"\(other ?? "")\")")
        }
        // Both core calls chain inside ONE transform — either half may
        // legitimately refuse (nothing changed), so the second falls back to
        // the first's result rather than to nil. The sheet does exactly
        // this (ChannelOptionsSheetController.applyClicked).
        let changed = try editStructure(document, "Channel Options", renderNeutral: true) { current in
            let renamed = try current.renamingChannel(index, name) ?? current
            let recoloured = try renamed.settingChannelOverlay(
                index, red: overlay.red, green: overlay.green, blue: overlay.blue,
                opacity: opacity, indicatesSelected: indicatesSelected)
            let updated = recoloured ?? renamed
            return updated === current ? nil : updated
        }
        guard changed else {
            return try noOpResult(
                ["channel": index, "name": name],
                why: "channel \(index) already has exactly those options.")
        }
        return try jsonResult([
            "ok": true, "channel": index, "name": name,
            "overlay_color": Self.hex(overlay),
            "overlay_opacity": doc.isHighDepth
                ? document.doc.channelInfo(index)?.opacity ?? opacity
                : (opacity * 100).rounded() / 100,
            "color_indicates": indicatesSelected ? "selected" : "masked",
        ])
    }

    /// The row menu's Invert Channel: 255 − v per pixel.
    func invertChannel(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let index = try channelArgument(a, doc)
        let name = doc.channelInfo(index)?.name ?? ""
        let inverted = try editStructure(document, "Invert Channel") {
            try $0.invertingChannel(index)
        }
        guard inverted else {
            return try noOpResult(
                ["channel": index, "name": name],
                why: "channel \(index) could not be inverted.")
        }
        return try jsonResult(["ok": true, "channel": index, "name": name])
    }

    // MARK: - Selections

    /// Loads a channel, a layer's transparency or mask, or a colour plane
    /// as the selection. Selections are view state: no undo step, no dirty
    /// flag. An all-zero source deselects and reports `selection_empty`
    /// rather than erroring. Also the twin of Select ▸ Portrait Matte:
    /// `from:"channel"` with the matte's channel name.
    func loadSelection(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let source = try selectionSource(a, document)
        let mode = try selectionMode(a)
        guard
            var plane = doc.selectionPlane(for: source)
        else {
            throw ToolError(
                message: "That source has no plane to load — check the layer index, or that "
                    + "the layer still has a mask.")
        }
        if boolArg(a, "invert") == true {
            plane = PlaneAlgebra.inverted(plane)
        }
        return try applyPlaneSelection(document, plane, mode: mode)
    }

    /// Writes the current selection into a NEW channel (`name`) or combines
    /// it into an existing one (`channel` + `mode`), as one undo step.
    func saveSelection(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        guard let selection = editor(document)?.agentSelection?.maskBytes() else {
            throw ToolError(
                message: "Nothing is selected — make a selection first (select_rect, "
                    + "select_magic_wand, select_subject, …).")
        }
        let modeName = stringArg(a, "mode") ?? "replace"
        if a["channel"] != nil {
            let index = try channelArgument(a, doc)
            guard let existing = doc.channelPlane(index) else {
                throw ToolError(message: "Channel \(index) has no plane to combine with")
            }
            let combined = PlaneAlgebra.combine(existing, selection, mode: try selectionMode(a))
            let name = doc.channelInfo(index)?.name ?? ""
            let written = try editStructure(document, "Save Selection") {
                $0.settingChannelData(index, combined)
            }
            guard written else {
                return try noOpResult(
                    ["channel": index, "name": name, "mode": modeName],
                    why: "the channel already holds exactly that coverage.")
            }
            return try jsonResult([
                "ok": true, "channel": index, "name": name, "mode": modeName,
            ])
        }
        let name = Self.channelName(stringArg(a, "name"), doc)
        let width = doc.width
        let height = doc.height
        // Decided up front, as duplicate_channel does (see addChannel).
        if doc.channelBudgetRefusal(width: width, height: height, adding: 1) != nil {
            return try channelListFullResult(["name": name])
        }
        let added = try editStructure(document, "Save Selection") {
            try $0.addingChannel(name: name, plane: selection, width: width, height: height)
        }
        guard added else { return try channelListFullResult(["name": name]) }
        // Appended, so the new channel is the last one — see addChannel.
        return try jsonResult([
            "ok": true, "channel": (document.doc?.channelCount ?? 1) - 1, "name": name,
            "mode": "replace",
        ])
    }

    // MARK: - Channel arithmetic

    /// Image > Apply Image…: one source plane blended onto the target — a
    /// layer's pixels plane for plane, one colour plane of them, or an alpha
    /// channel. Parses arguments and hands them to `ChannelMath.applyImage`,
    /// which the sheet calls too: the blend, the source selection and the
    /// per-plane fallthrough an RGB target needs exist exactly once.
    func applyImage(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        // Default "layer", as the catalog states — not the editor's current
        // target, so the same call means the same thing whatever the app's
        // panel happens to be showing.
        let paint = try paintTarget(a, document, allowMask: false)
        // Structural, then the group guard only where the layer's own pixels
        // are written: a CHANNEL target is document state, so a group index
        // sitting in `layer` (right after ⌘G, say) must not refuse the call.
        let layer = try structuralLayerIndex(a, document)
        if paint == .layer || paint.isPlane || paint.isInk { try rejectGroupPixelEdit(document, layer) }
        var parameters = ApplyImageParameters()
        parameters.source = try sourceLayer(a, doc, key: "source")
        parameters.sourcePlane = try planeChoice(a, doc, key: "source_plane")
        parameters.invert = boolArg(a, "invert") ?? false
        parameters.blend = try blendModeArg(a) ?? DR_BLEND_NORMAL
        // The four HSL modes are defined over an RGB triple: onto ONE plane
        // they say nothing (a gray triple has zero saturation), and the core
        // refuses them there, so this refuses with the reason rather than
        // reporting an unexplained no-op.
        if paint != .layer, DrBlendMode.degeneratesOnGray(parameters.blend) {
            throw ToolError(
                message: "Blend mode "
                    + "\(DrBlendMode.displayName(for: parameters.blend)) needs an RGB "
                    + "triple, so it means nothing on the single plane "
                    + "\"\(paint.agentName(in: doc))\". Use target \"layer\" (which "
                    + "blends the three planes as one colour), or a separable mode such as "
                    + "Multiply, Screen or Overlay.")
        }
        // 1 = the blend at full strength; the value is a fraction, matching
        // every other opacity the agent speaks.
        parameters.opacity = min(max(doubleArg(a, "opacity") ?? 1, 0), 1)
        // The sheet's Scale and Offset fields: Add and Subtract only, refused
        // with the core's rule rather than clamped.
        parameters.scale = doubleArg(a, "scale") ?? 1
        parameters.offset = doubleArg(a, "offset") ?? 0
        if let refusal = parameters.scaleOffsetRefusal { throw ToolError(message: refusal) }
        parameters.target = paint
        parameters.targetLayer = layer
        if doc.isHighDepth { return try nativeApplyImage(document, parameters: parameters) }
        var applied: [String: Any] = [
            "source": parameters.source.map(String.init) ?? "merged",
            "source_plane": Self.planeChoiceName(parameters.sourcePlane, doc),
            "invert": parameters.invert,
            "blend_mode": DrBlendMode.displayName(for: parameters.blend),
            "opacity": (parameters.opacity * 100).rounded() / 100,
            "target": paint.agentName(in: doc),
        ]
        if parameters.isScaled {
            applied["scale"] = parameters.scale
            applied["offset"] = parameters.offset
        }
        if case .channel = paint {
            // A channel is document state: no layer pixels move, so no
            // described layer is rasterized.
            let merged = try mergedComposite(parameters.readsMerged, document, doc)
            let changed = try editStructure(document, "Apply Image") {
                ChannelMath.applyImage($0, parameters, merged: merged)
            }
            guard changed else { return try applyImageNoOpResult(applied) }
            return try jsonResult(["ok": true, "action": "Apply Image", "applied": applied])
        }
        // A layer or one of its colour planes: the write rewrites that
        // layer's pixels, so an adjustment layer refuses and a described
        // layer is rasterized inside the same edit.
        try rejectAdjustmentPixelEdit(document, layer)
        let merged = try mergedComposite(parameters.readsMerged, document, doc)
        var refused = false
        let rasterized: DroppedDescription?
        do {
            rasterized = try performPixelEdit(document, "Apply Image", pixelLayer: layer) {
                current in
                let out = ChannelMath.applyImage(current, parameters, merged: merged)
                if out == nil { refused = true }
                return out
            }
        } catch let error as ToolError {
            guard refused else { throw error }
            return try applyImageNoOpResult(applied)
        }
        return try pixelEditResult(
            ["ok": true, "action": "Apply Image", "layer": layer, "applied": applied],
            layer: layer, rasterized: rasterized)
    }

    /// Image > Calculations…: two source planes blended into a new plane,
    /// which becomes a new alpha channel (one undo step) or the selection
    /// (no edit at all). Source 1 is the blend layer and source 2 the base —
    /// the convention `ChannelMath` documents and implements.
    func calculations(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        var parameters = CalculationsParameters()
        parameters.source1 = CalculationsParameters.Source(
            layer: try sourceLayer(a, doc, key: "source1"),
            plane: try planeChoice(a, doc, key: "source1_plane"),
            invert: boolArg(a, "invert1") ?? false)
        parameters.source2 = CalculationsParameters.Source(
            layer: try sourceLayer(a, doc, key: "source2"),
            plane: try planeChoice(a, doc, key: "source2_plane"),
            invert: boolArg(a, "invert2") ?? false)
        parameters.blend = try blendModeArg(a) ?? DR_BLEND_NORMAL
        // Calculations always produces ONE plane, so the four HSL modes have
        // no triple to work on — the same refusal apply_image makes for a
        // single-plane target.
        if DrBlendMode.degeneratesOnGray(parameters.blend) {
            throw ToolError(
                message: "Blend mode "
                    + "\(DrBlendMode.displayName(for: parameters.blend)) needs an RGB "
                    + "triple, and Calculations produces a single plane. Pick a separable "
                    + "mode such as Multiply, Screen, Difference or Overlay.")
        }
        parameters.opacity = min(max(doubleArg(a, "opacity") ?? 1, 0), 1)
        parameters.name = Self.channelName(stringArg(a, "name"), doc)
        let result: CalculationsResult
        switch stringArg(a, "result") ?? "new_channel" {
        case "new_channel": result = .newChannel
        case "selection": result = .selection
        case let other:
            throw ToolError(
                message: "result must be \"new_channel\" or \"selection\" (got \"\(other)\")")
        }
        if doc.isHighDepth { return try nativeCalculations(document, parameters: parameters, result: result) }
        let merged = try mergedComposite(parameters.readsMerged, document, doc)
        // Computed once, against the handle the edit will be built on: a tool
        // call runs to completion on the main thread, so unlike the sheet —
        // which re-runs the math inside its transform because the document
        // can move under an open dialog — there is nothing to slip in
        // between. A nil here is an unreadable source, not a refusal: no
        // plane came out at all, so there is nothing to report as unchanged.
        guard let plane = ChannelMath.calculated(doc, parameters, merged: merged) else {
            throw ToolError(
                message: "Calculations could not read one of its sources — check the layer "
                    + "indices and plane names (list_channels names the channels).")
        }
        let applied: [String: Any] = [
            "source1": parameters.source1.layer.map(String.init) ?? "merged",
            "source1_plane": Self.planeChoiceName(parameters.source1.plane, doc),
            "invert1": parameters.source1.invert,
            "source2": parameters.source2.layer.map(String.init) ?? "merged",
            "source2_plane": Self.planeChoiceName(parameters.source2.plane, doc),
            "invert2": parameters.source2.invert,
            "blend_mode": DrBlendMode.displayName(for: parameters.blend),
            "opacity": (parameters.opacity * 100).rounded() / 100,
        ]
        switch result {
        case .selection:
            // Not an edit: the plane becomes the selection outright, and an
            // all-zero result deselects instead of erroring.
            return try applyPlaneSelection(
                document, plane, mode: .replace,
                extra: ["action": "Calculations", "result": "selection", "applied": applied])
        case .newChannel:
            let name = parameters.name
            let width = doc.width
            let height = doc.height
            // Decided up front, as duplicate_channel does (see addChannel).
            if doc.channelBudgetRefusal(width: width, height: height, adding: 1) != nil {
                return try channelListFullResult(["name": name, "applied": applied])
            }
            let added = try editStructure(document, "Calculations") {
                try $0.addingChannel(name: name, plane: plane, width: width, height: height)
            }
            guard added else {
                return try channelListFullResult(["name": name, "applied": applied])
            }
            return try jsonResult([
                "ok": true, "action": "Calculations", "result": "new_channel",
                "channel": (document.doc?.channelCount ?? 1) - 1, "name": name,
                "applied": applied,
            ])
        }
    }

    /// Select > Add Luminosity Masks: the nine "Lights 1".."Midtones 3"
    /// channels from the composite's Rec. 709 luma, as one undo step. All
    /// nine land or none do — the core refuses the whole set past either cap,
    /// so a half-built tone ladder can never exist.
    func addLuminosityMasks(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        if doc.isHighDepth {
            let capture = try PrecisionEditorCapture(document)
            guard !capture.quickMask else { throw ToolError(message: OperationScope.quickMaskRefusal) }
            let result = try PrecisionCapturedWork.run(scratchEstimate: NativeChannelMath.scratch(doc), title: "Add Luminosity Masks") {
                try NativeChannelMath.luminosity(doc, control: $0)
            }
            let changed = try capture.publish(result, to: document, name: "Add Luminosity Masks")
            return try jsonResult(["ok": true, "changed": changed, "added": Array((doc.channelCount..<document.doc.channelCount).compactMap { document.doc.channelInfo($0)?.name }), "storage": "u16"])
        }
        let before = doc.channelCount
        let added = try editStructure(document, "Add Luminosity Masks") {
            $0.addingLuminosityMasks()
        }
        guard added, let updated = document.doc else {
            return try noOpResult(
                [:],
                why: "the nine masks would not fit — the document already holds \(before) "
                    + "channels, or nine more canvas-sized planes would push it past the "
                    + "format's total pixel budget. Delete some channels first.")
        }
        let names = (before..<updated.channelCount).compactMap { updated.channelInfo($0)?.name }
        return try jsonResult([
            "ok": true, "added": names, "channels": updated.channelCount,
        ])
    }

    // MARK: - Shared pieces

    /// The in-band "nothing changed" answer. Never an error: no undo step
    /// opened and no byte moved, and saying so plainly is what lets a model
    /// carry on instead of retrying the identical call.
    private func noOpResult(_ fields: [String: Any], why: String) throws -> String {
        // The tool's own keys never overwrite the shared ones, so a refusal
        // cannot be disguised as a success.
        try jsonResult(
            fields.merging([
                "ok": true, "changed": false,
                "note": "Nothing changed: \(why) No undo step was added.",
            ]) { _, shared in shared })
    }

    /// The channel-budget question `ImageSizeSheetController` and
    /// `CanvasSizeSheetController` ask before their edit, asked for the
    /// agent's mirrors of those two commands (image_size, canvas_size).
    ///
    /// The core refuses a resize outright once the channels would break the
    /// .rz total-channel-pixel budget — sixteen saved selections stop a
    /// 6000 × 4000 photo upscaling past ~56 MP — and `performGroupedEdit` can
    /// only turn that nil into "check the parameters", which is the one thing
    /// that is NOT wrong: the fix is to delete channels. The UI names the
    /// budget in an alert; asking the same question here gives a model the
    /// same explanation, and something it can act on.
    ///
    /// A size that could not be a canvas at all answers nil
    /// (`channelBudgetRefusal`'s own guard) so the core's size refusal is
    /// what the model sees: telling it to delete channels because it asked
    /// for 200000 × 200000 px would send it deleting the user's saved
    /// selections one undo step at a time, and the retry would fail anyway.
    func requireChannelBudget(_ document: ImageDocument, width: Int, height: Int) throws {
        guard let doc = document.doc,
              let reason = doc.channelBudgetRefusal(width: width, height: height)
        else { return }
        throw ToolError(message: reason)
    }

    /// The refusal every channel-CREATING tool shares: both caps live in the
    /// core (256 channels, and a total channel-pixel budget the .rz reader
    /// enforces so a document it builds can always be reopened), and neither
    /// is knowable from here without restating them.
    private func channelListFullResult(_ fields: [String: Any]) throws -> String {
        try noOpResult(
            fields,
            why: "the channel could not be added — the document already holds the maximum "
                + "256 channels, or another canvas-sized plane would push it past the "
                + "format's total pixel budget. Delete some channels first.")
    }

    /// `doc`'s Merged composite when `reads`, else nil: the retained
    /// projection when current, else a render whose error propagates as the
    /// core's sentence — never a false "changed nothing".
    private func mergedComposite(
        _ reads: Bool, _ document: ImageDocument, _ doc: RasterDocument
    ) throws -> MergedComposite? {
        guard reads else { return nil }
        return MergedComposite(doc: doc, image: try document.byteComposite(of: doc))
    }

    private func applyImageNoOpResult(_ applied: [String: Any]) throws -> String {
        try noOpResult(
            ["action": "Apply Image", "applied": applied],
            why: "the blend left every byte of the target exactly as it was (an identity "
                + "blend, like Multiply by white or Screen by black).")
    }

    /// The name a NEW channel takes: the caller's, or the next "Alpha N"
    /// when there is none — or when the one given is blank.
    ///
    /// A nameless channel is what `rename_channel` refuses to produce and
    /// `set_channel_options` quietly declines to set, for the reason both
    /// state: it could only ever be addressed by index again, and it draws
    /// as an empty row the panel's field cannot commit. The three tools that
    /// CREATE a channel — add_channel, save_selection and calculations —
    /// answer a blank name the same way.
    private static func channelName(_ requested: String?, _ doc: RasterDocument) -> String {
        guard let requested = requested,
              !requested.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        else { return doc.nextChannelName }
        return requested
    }

    /// The rubylith a new channel gets when `overlay_color` is absent: red,
    /// the core's own default and the colour Quick Mask washes in.
    static var defaultOverlayColor: NSColor {
        NSColor(srgbRed: 1, green: 0, blue: 0, alpha: 1)
    }

    /// The rubylith opacity, defaulting to 0.5 — the core's default, half
    /// transparent so the wash and the picture under it both read.
    static func overlayOpacity(_ value: Double?) -> Double {
        min(max(value ?? 0.5, 0), 1)
    }

    /// A parsed `overlay_color` as the core's three sRGB bytes. `parseColor`
    /// builds an sRGB colour already; the conversion is belt and braces for
    /// one that somehow arrives in another space, where reading a component
    /// directly would trap.
    static func overlayBytes(_ color: NSColor) -> (red: UInt8, green: UInt8, blue: UInt8) {
        let sRGB = color.usingColorSpace(.sRGB) ?? defaultOverlayColor
        let byte: (CGFloat) -> UInt8 = { UInt8((min(max($0, 0), 1) * 255).rounded()) }
        return (byte(sRGB.redComponent), byte(sRGB.greenComponent), byte(sRGB.blueComponent))
    }

    private static func hex(_ overlay: (red: UInt8, green: UInt8, blue: UInt8)) -> String {
        RasterImage.hexString((r: overlay.red, g: overlay.green, b: overlay.blue, a: 255))
    }

    /// How a source plane is reported back, in the vocabulary the caller
    /// used: "rgb", a colour plane, or a channel's name.
    private static func planeChoiceName(_ choice: PlaneChoice, _ doc: RasterDocument) -> String {
        // #1 Lab phase 6b-i: a Lab document's names (`LabDocument.planeChoiceName`).
        if doc.isLab { return LabDocument.planeChoiceName(choice, in: doc) }
        switch choice {
        case .rgb: return doc.isCmyk ? "cmyk" : doc.isGray ? "gray" : "rgb"
        case .plane(let plane): return plane.agentName
        case .channel(let index): return doc.channelInfo(index)?.name ?? "channel \(index)"
        case .ink(let plate): return plate.key
        }
    }
}
