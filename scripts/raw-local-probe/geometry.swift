import AppKit

/// Real saved synthetic-DNG source pixels and masks are transformed together;
/// the historical marker changes decoder availability only, never fixture data.
func runRawGeometryProbe(fixtures: URL, depth: Int, directory: URL) throws -> [String: Any] {
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    let opticalReedit = try runOpticalReeditProbe(fixtures: fixtures, depth: depth,
        directory: directory)
    let mapSheet = try runMigrationMapSheetProbe(depth: depth)
    for request in [
        #"{"version":1,"kind":"exif","orientation":1,"stray":1}"#,
        #"{"profile_id":"strict","center":[0.5,0.5],"radial":[0,0,0],"red_scale":1,"blue_scale":1,"filter":"bilinear","stray":1}"#,
        #"{"version":1,"orientation":1,"extent":[0,0,512,384],"stray":1}"#,
        #"{"version":1,"referenceSize":[512,384],"referenceFromSource":[1,0,0,1,0,0],"stray":1}"#
    ].enumerated() {
        try localRefuses("Strict nested RAW geometry object \(request.offset)") {
            switch request.offset {
            case 0: _ = try JSONDecoder().decode(RawSourceMap.self, from: Data(request.element.utf8))
            case 1: _ = try JSONDecoder().decode(RawLensGeometry.self, from: Data(request.element.utf8))
            case 2: _ = try JSONDecoder().decode(RawDecoderFrame.self, from: Data(request.element.utf8))
            default: _ = try JSONDecoder().decode(RawVignetteReference.self, from: Data(request.element.utf8))
            }
        }
    }
    let names = ["registered-vignette-crop", "registered-vignette-pad",
        "registered-vignette-exif", "registered-vignette-affine-chain"]
    var rows: [[String: Any]] = []
    var uiReport: [String: Any]?
    var opticalUI: [String: Any]?
    for name in names {
        let cache = RawLocalVariantCache()
        defer { cache.clear() }
        let imported = try localOpen(fixtures.appendingPathComponent("source.dng"), depth: depth)
        var settings = RawDevelopSettings()
        settings.vignette = RawVignetteSettings(edgeExposure: -1.25,
            centerX: 0.31, centerY: 0.67, falloff: 1.7)
        let create: [String: Any] = ["op": "create", "id": "geometry-local", "name": "Geometry Local",
            "exposure_offset": -0.5, "mask": ["op": "radial", "center": [193.25, 244.75],
                "radius": [150.5, 110.25], "hardness": 0.3]]
        guard let full = try localEvaluate(imported, settings: settings,
            operations: [create], cache: cache) else {
            throw LocalProbeFailure(description: "Could not create native RAW local fixture")
        }
        let fullState = try RawLocalWork.state(full, layer: 0)
        let fullW = fullState.width, fullH = fullState.height
        try localCheck(fullW * fullH == 512 * 384
            && fullState.nodes.count == 1 && fullState.masks.count == 1,
            "Geometry fixture has actual 512×384 camera pixels and native local coverage; got \(fullW)×\(fullH), nodes \(fullState.nodes.count)")
        let snapshot = try RawSourceSnapshot.restore(full, layer: 0, descriptor: fullState.descriptor)
        let camera = RawLocalEvaluation.cameraDescriptor(fullState.descriptor)
        let base = try RetainedRawDecoder.develop(url: snapshot.url, descriptor: camera,
            cameraOnly: true).0
        var alternate = camera
        guard let effective = fullState.nodes[0].effective else {
            throw LocalProbeFailure(description: "Saved local camera variant has no effective controls")
        }
        alternate.recipe.values = effective.values
        alternate.recipe.chromaticityX = effective.chromaticityX
        alternate.recipe.chromaticityY = effective.chromaticityY
        let variant = try RetainedRawDecoder.develop(url: snapshot.url, descriptor: alternate,
            cameraOnly: true).0
        let oldMap: RawSourceMap
        switch name {
        case "registered-vignette-crop":
            oldMap = RawSourceMap(width: UInt32(fullW - 100), height: UInt32(fullH - 84),
                inverse: [1, 0, 0, 1, 20, 10])
        case "registered-vignette-pad":
            oldMap = RawSourceMap(width: UInt32(fullW + 88), height: UInt32(fullH + 66),
                inverse: [1, 0, 0, 1, -20, -20])
        case "registered-vignette-exif":
            oldMap = RawSourceMap(orientation: 6)
        default:
            oldMap = RawSourceMap(width: UInt32(fullW - 100), height: UInt32(fullH - 84),
                inverse: [1, 0.025, 0.04, 1, 20.25, 9.75], filter: .bilinear)
        }
        let oldSize = try oldMap.destination(from: (fullW, fullH))
        let oldBase = try base.applyingRawSourceGeometry(oldMap.json())
        let oldVariant = try variant.applyingRawSourceGeometry(oldMap.json())
        let oldMask = try fullState.masks[0].remapped(oldMap.json())
        let controls: [[String: Any]] = [["enabled": true, "density": 1.0, "feather": 0.0]]
        let oldMixed = try oldBase.developLocals(variants: [oldVariant], masks: [oldMask],
            settings: RawLocalJSON.string(controls),
            vignette: settings.vignette?.coreJSON(width: oldSize.0, height: oldSize.1))
        var oldAsset = oldMixed
        var recipe = fullState.descriptor.recipe
        if name == "registered-vignette-exif" {
            var object = try JSONSerialization.jsonObject(with: JSONEncoder().encode(recipe)) as? [String: Any] ?? [:]
            let currentOrientation = recipe.orientation
            let cameraW = currentOrientation >= 5 ? fullH : fullW
            let cameraH = currentOrientation >= 5 ? fullW : fullH
            let target = RawMigrationGeometry.compose(
                try RawMigrationGeometry.exif(currentOrientation,
                    width: Double(cameraW), height: Double(cameraH)),
                try oldMap.affine(from: (fullW, fullH)))
            guard let oldOrientation = (1...8).first(where: {
                (try? RawMigrationGeometry.exif(UInt32($0), width: Double(cameraW),
                    height: Double(cameraH))) == target
            }) else { throw LocalProbeFailure(description: "Could not identify the physical EXIF fixture orientation") }
            object["orientation"] = oldOrientation
            recipe = try JSONDecoder().decode(RawDevelopmentRecipe.self,
                from: JSONSerialization.data(withJSONObject: object))
        }
        if name == "registered-vignette-affine-chain" {
            let lens = RawLensGeometry(profileID: "fixture-registered-optics",
                center: [0.41, 0.58], radial: [0.012, -0.001, 0.0001],
                redScale: 0.98, blueScale: 1.02, filter: .bilinear,
                referenceSize: [UInt32(oldSize.0), UInt32(oldSize.1)],
                referenceFromSource: RawMigrationGeometry.identity)
            var object = try JSONSerialization.jsonObject(with: JSONEncoder().encode(recipe)) as? [String: Any] ?? [:]
            object["version"] = 4
            object["sourceGeometry"] = lens.jsonObject
            object["decoderFrame"] = ["version": 1, "orientation": recipe.orientation,
                "extent": [0, 0, oldSize.0, oldSize.1]]
            recipe = try JSONDecoder().decode(RawDevelopmentRecipe.self,
                from: JSONSerialization.data(withJSONObject: object))
            oldAsset = try oldMixed.applyingRawSourceGeometry(lens.map().json())
        }
        let historical = RawSourceDescriptor(schema_version: fullState.descriptor.schema_version,
            original_filename: fullState.descriptor.original_filename,
            content_sha256: fullState.descriptor.content_sha256,
            decoder: .init(identifier: fullState.descriptor.decoder.identifier,
                version: fullState.descriptor.decoder.version,
                implementation: "unavailable-registered-geometry-probe"), recipe: recipe)
        let oldControl = try RasterJobControl()
        let oldSaved = try RawLocalEvaluation.replacing(source: full, layer: 0,
            asset: oldAsset, descriptor: historical, masks: [oldMask], control: oldControl,
            sourceCoordinateMap: try oldMap.affine(from: (fullW, fullH)))
        let historicalArtifact = directory.appendingPathComponent(name + "-\(depth)-historical.rz")
        try oldSaved.saveNative(to: historicalArtifact)
        let owner = localDocument(oldSaved)
        defer { owner.close() }
        let inverse = try RawMigrationGeometry.inverse(oldMap.affine(from: (fullW, fullH)))
        let reviewedMap = RawSourceMap(width: UInt32(fullW), height: UInt32(fullH), inverse: inverse,
            filter: name == "registered-vignette-affine-chain" ? .bilinear : .bicubic)
        let candidate = try RawDecoderMigration.prepare(in: owner, layer: 0, geometry: reviewedMap)
        if name == "registered-vignette-crop" {
            var viewport = RawDevelopViewport()
            viewport.nativeDetail = true
            viewport.backingSize = CGSize(width: 96, height: 80)
            let before = try oldSaved.sourceMaster16(0)
            guard let after = candidate.asset.precision?.layerSource(0) else {
                throw LocalProbeFailure(description: "Migration detail fixture lost proposed source")
            }
            let shared = try viewport.comparisonRegion(before: before, after: after)
            try localCheck(shared.width == 96 && shared.height == 80
                && shared.x + shared.width <= before.width
                && shared.x + shared.width <= after.width,
                "100% migration detail uses one region within both dimensions")
            viewport.backingSize = CGSize(width: CGFloat(before.width + 1), height: 80)
            try localRefuses("100% migration detail wider than saved source") {
                _ = try viewport.comparisonRegion(before: before, after: after)
            }
        }
        let reviewed = try localPixels(candidate.asset, retained: false)
        try candidate.apply(in: owner)
        guard let committed = owner.doc else { throw LocalProbeFailure(description: "Migration lost its owner") }
        try localCheck(try localPixels(committed) == reviewed,
            "Geometry migration committed its exact reviewed native source")
        let committedState = try RawLocalWork.state(committed, layer: 0)
        try localCheck(committedState.width == fullW && committedState.height == fullH,
            "Migration rewrote canonical source/mask dimensions together")
        let reference = committedState.descriptor.recipe.vignetteReference
        try localCheck(reference?.referenceSize == [UInt32(oldSize.0), UInt32(oldSize.1)]
            && reference?.referenceFromSource == inverse,
            "Migration retains the old vignette reference and exact new→old registration")
        try localCheck(committedState.descriptor.recipe.enhancements?.vignette == settings.vignette,
            "Migration preserves the authored off-centre vignette controls")
        if name == "registered-vignette-affine-chain" {
            try localCheck(committedState.descriptor.recipe.sourceGeometry?.referenceFromSource == inverse,
                "Optical and vignette registrations compose independently")
        }
        owner.undoManager?.undo()
        try localCheck(owner.doc === oldSaved, "Geometry migration is one Undo")
        owner.undoManager?.redo()
        try localCheck(owner.doc === committed, "Geometry migration is one Redo")
        let artifact = directory.appendingPathComponent(name + "-\(depth).rz")
        try committed.saveNative(to: artifact)
        let reopened = try RasterDocument.open(url: artifact)
        try localCheck(try localPixels(reopened) == reviewed
            && (try RawLocalWork.state(reopened, layer: 0)).descriptor == committedState.descriptor,
            "Geometry migration native reopen preserves exact pixels and recipe")
        let resaved = directory.appendingPathComponent(name + "-\(depth)-resaved.rz")
        try reopened.saveNative(to: resaved)
        try localCheck(try Data(contentsOf: artifact) == Data(contentsOf: resaved),
            "Geometry migration native reopen/resave is byte-identical")
        if name == "registered-vignette-crop" {
            uiReport = try runRegisteredVignetteUIProbe(source: reopened, depth: depth,
                directory: directory)
            opticalUI = try runOpticalUIProbe(source: reopened, depth: depth,
                directory: directory)
        }
        let oldMaskPath = directory.appendingPathComponent(name + "-\(depth)-old.drmask")
        let afterMaskPath = directory.appendingPathComponent(name + "-\(depth)-after.drmask")
        let preVignette = try base.developLocals(variants: [variant], masks: committedState.masks,
            settings: RawLocalJSON.string(controls), vignette: nil)
        let preVignettePath = directory.appendingPathComponent(name + "-\(depth)-pre-vignette.rgba16le")
        let reviewedPath = directory.appendingPathComponent(name + "-\(depth)-reviewed.rgba16le")
        let profilePath = directory.appendingPathComponent(name + "-\(depth)-profile.icc")
        try localPixels(preVignette, retained: false).write(to: preVignettePath)
        try reviewed.write(to: reviewedPath)
        guard let profile = base.iccProfile else {
            throw LocalProbeFailure(description: "Registered vignette input lost its camera ICC profile")
        }
        try profile.write(to: profilePath)
        try oldMask.encodedBytes().write(to: oldMaskPath)
        try committedState.masks[0].encodedBytes().write(to: afterMaskPath)
        rows.append(["name": name, "depth": depth, "old_dimensions": [oldSize.0, oldSize.1],
            "new_dimensions": [fullW, fullH], "inverse": inverse,
            "old_mask": oldMaskPath.path, "after_mask": afterMaskPath.path,
            "native": artifact.path, "reviewed_sha256": localHash(reviewed),
            "pre_vignette": preVignettePath.path, "reviewed_values": reviewedPath.path,
            "profile": profilePath.path,
            "optical": committedState.descriptor.recipe.sourceGeometry?.jsonObject ?? [:],
            "vignette": settings.vignette?.json ?? [:],
            "historical_native": historicalArtifact.path,
            "before_masks": candidate.beforeMasks, "after_masks": candidate.afterMasks,
            "vignette_reference": ["size": reference?.referenceSize ?? [],
                "matrix": reference?.referenceFromSource ?? []]])
        localProgress("PASS \(name)-\(depth): reviewed source, native masks, Undo/Redo, reopen")
    }
    return ["passed": true, "group_count": rows.count + (uiReport == nil ? 0 : 1)
            + (opticalUI == nil ? 0 : 1) + 2,
        "groups": rows, "ui_replay_reopen": uiReport ?? [:],
        "optical_reedit": opticalReedit, "optical_ui": opticalUI ?? [:],
        "migration_map_sheet": mapSheet]
}

private func runMigrationMapSheetProbe(depth: Int) throws -> [String: Any] {
    let policy = NSApp.activationPolicy()
    _ = NSApp.setActivationPolicy(.accessory)
    defer { _ = NSApp.setActivationPolicy(policy) }
    let expected = [1.0123456789012345, -0.0025, 0.00375, 0.9876543210123456,
        12.125, -8.375]
    var driven = false
    var failure: Error?
    let deadline = Date().addingTimeInterval(30)
    let timer = Timer(timeInterval: 0.025, repeats: true) { _ in
        guard !driven, failure == nil else { return }
        guard Date() < deadline else {
            failure = LocalProbeFailure(description: "RAW migration map sheet timed out")
            NSApp.abortModal(); return
        }
        guard let content = NSApp.modalWindow?.contentView else { return }
        let views = geometryDescendants(content)
        let fields = (0..<6).compactMap { index in
            views.first { $0.accessibilityIdentifier() == "raw-migration-map-\(index)" } as? NSTextField
        }
        guard fields.count == 6,
              let button = views.compactMap({ $0 as? NSButton })
                .first(where: { $0.title == "Review Geometry" }) else { return }
        for (field, value) in zip(fields, expected) { field.stringValue = String(value) }
        driven = true
        button.performClick(nil)
    }
    RunLoop.main.add(timer, forMode: .common)
    RunLoop.main.add(timer, forMode: .modalPanel)
    defer { timer.invalidate() }
    let map = RawMigrationGeometrySheetController.run(saved: (100, 80), proposed: (120, 96))
    if let failure { throw failure }
    try localCheck(driven && map?.inverse == expected && map?.width == 120
        && map?.height == 96 && map?.filter == .bilinear,
        "Numeric migration map sheet returned every binary64 coefficient and proposed dimensions")
    return ["passed": true, "depth": depth, "inverse": expected,
        "saved_dimensions": [100, 80], "proposed_dimensions": [120, 96]]
}

/// A previously saved optical master must never become the camera-only input
/// for the first local edit or the next optical profile after native reopen.
private func runOpticalReeditProbe(fixtures: URL, depth: Int,
                                   directory: URL) throws -> [String: Any] {
    let original = try localOpen(fixtures.appendingPathComponent("source.dng"), depth: depth)
    let cache = RawLocalVariantCache()
    defer { cache.clear() }
    let lensA = RawLensGeometry(profileID: "reedit-A", center: [0.43, 0.59],
        radial: [0.012, -0.001, 0.0001], redScale: 0.985, blueScale: 1.015,
        filter: .bilinear, referenceSize: nil, referenceFromSource: nil)
    var settings = RawDevelopSettings()
    settings.sourceGeometry = lensA
    guard let first = try localEvaluate(original, settings: settings, cache: cache) else {
        throw LocalProbeFailure(description: "Could not establish saved optical source A")
    }
    let saved = directory.appendingPathComponent("optical-reedit-\(depth).rz")
    try first.saveNative(to: saved)
    let reopened = try RasterDocument.open(url: saved)
    var lensB = lensA
    lensB.profileID = "reedit-B"
    lensB.radial[0] = -0.009
    settings.sourceGeometry = lensB
    guard let reedited = try localEvaluate(reopened, settings: settings, cache: cache),
          let direct = try localEvaluate(original, settings: settings, cache: cache) else {
        throw LocalProbeFailure(description: "Optical A→B redevelopment did not produce candidates")
    }
    try localCheck(try localPixels(reedited) == localPixels(direct),
        "Optical A→B after native reopen equals fresh original→B")
    let create: [String: Any] = ["op": "create", "id": "first-optical-local",
        "name": "First optical local", "exposure_offset": -0.5,
        "mask": ["op": "radial", "center": [191.5, 155.25],
            "radius": [125.75, 90.5], "hardness": 0.25]]
    var settingsA = RawDevelopSettings()
    settingsA.sourceGeometry = lensA
    guard let edited = try localEvaluate(reopened, operations: [create], cache: cache),
          let fresh = try localEvaluate(original, settings: settingsA,
              operations: [create], cache: cache) else {
        throw LocalProbeFailure(description: "First local after optics did not produce candidates")
    }
    try localCheck(try localPixels(edited) == localPixels(fresh),
        "First native local after optical reopen evaluates one optical pass from original")
    let state = try RawLocalWork.state(edited, layer: 0)
    try localCheck(state.nodes.count == 1 && state.descriptor.recipe.sourceGeometry == lensA,
        "First optical local retains canonical coverage and exact saved profile")
    return ["passed": true, "depth": depth, "profile_A": lensA.jsonObject,
        "profile_B": lensB.jsonObject, "first_local_mask": try state.masks[0].sha256(),
        "A_to_B_sha256": localHash(try localPixels(reedited)),
        "first_local_sha256": localHash(try localPixels(edited))]
}

/// Drive the real retained Develop controls after a geometry migration. MCP
/// independently checks the corresponding recorded Action replay/reopen.
private func runRegisteredVignetteUIProbe(source: RasterDocument, depth: Int,
                                          directory: URL) throws -> [String: Any] {
    let original = try RawLocalWork.state(source, layer: 0)
    guard let basis = original.descriptor.recipe.vignetteReference else {
        throw LocalProbeFailure(description: "Migrated UI source has no saved vignette reference")
    }
    let policy = NSApp.activationPolicy()
    _ = NSApp.setActivationPolicy(.accessory)
    defer { _ = NSApp.setActivationPolicy(policy) }
    let document = localDocument(source)
    document.makeWindowControllers()
    document.showWindows()
    defer { document.close() }
    guard let editor = document.windowControllers.first?.contentViewController as? EditorViewController else {
        throw LocalProbeFailure(description: "No owned editor for registered vignette UI")
    }
    _ = editor.view
    var phase = 0
    var failure: Error?
    var reviewed: RasterDocument?
    let deadline = Date().addingTimeInterval(90)
    let timer = Timer(timeInterval: 0.025, repeats: true) { _ in
        guard failure == nil, phase < 2 else { return }
        guard Date() < deadline else {
            failure = LocalProbeFailure(description: "Registered vignette UI timed out at phase \(phase)")
            NSApp.abortModal(); return
        }
        guard let content = NSApp.modalWindow?.contentView,
              let apply = geometryDescendants(content).compactMap({ $0 as? NSButton })
                .first(where: { $0.accessibilityIdentifier() == "raw-develop-apply" }),
              let controller = apply.target as? RawDevelopWindowController,
              controller.previewIsReady else { return }
        do {
            if phase == 0 {
                let note = controller.enhancementControls.map { controls in
                    geometryDescendants(controls.view).compactMap { $0 as? NSTextField }
                        .map(\.stringValue).joined(separator: " ")
                } ?? ""
                guard note.contains("\(basis.referenceSize[0])×\(basis.referenceSize[1]) saved reference") else {
                    throw LocalProbeFailure(description: "Vignette UI omitted its saved reference dimensions")
                }
                guard let row = geometryDescendants(content)
                    .first(where: { $0.accessibilityIdentifier() == "raw-develop-vignette-x" }) as? CardControlRow,
                      let slider = row.slider, let action = slider.action else {
                    throw LocalProbeFailure(description: "Missing real vignette-center control")
                }
                slider.doubleValue = 0.37
                _ = slider.sendAction(action, to: slider.target)
                phase = 1
            } else {
                guard let result = controller.localSession?.result,
                      result.settings.vignette?.centerX == 0.37 else {
                    throw LocalProbeFailure(description: "Vignette center did not enter exact prepared settings")
                }
                let state = try RawLocalWork.state(result.document, layer: 0)
                guard state.descriptor.recipe.vignetteReference == basis,
                      state.descriptor.recipe.enhancements?.vignette.centerX == 0.37,
                      apply.isEnabled else {
                    throw LocalProbeFailure(description: "Reviewed center or receiving reference changed")
                }
                reviewed = result.document
                apply.performClick(nil)
                phase = 2
            }
        } catch {
            failure = error
            NSApp.abortModal()
        }
    }
    RunLoop.main.add(timer, forMode: .common)
    RunLoop.main.add(timer, forMode: .modalPanel)
    defer { timer.invalidate() }
    editor.redevelopRaw(nil)
    if let failure { throw failure }
    guard phase == 2, let reviewed, document.doc === reviewed else {
        throw LocalProbeFailure(description: "Registered vignette UI did not publish its reviewed candidate")
    }
    let artifact = directory.appendingPathComponent("registered-vignette-ui-replay-reopen-\(depth).rz")
    try reviewed.saveNative(to: artifact)
    let reopened = try RasterDocument.open(url: artifact)
    let restored = try RawLocalWork.state(reopened, layer: 0)
    try localCheck(restored.descriptor.recipe.vignetteReference == basis
        && restored.descriptor.recipe.enhancements?.vignette.centerX == 0.37,
        "Vignette-center UI update keeps its reference on native reopen")
    return ["name": "registered-vignette-ui-replay-reopen", "depth": depth,
        "passed": true, "reference_size": basis.referenceSize,
        "center_x": 0.37, "native": artifact.path]
}

private func runOpticalUIProbe(source: RasterDocument, depth: Int,
                               directory: URL) throws -> [String: Any] {
    let policy = NSApp.activationPolicy()
    _ = NSApp.setActivationPolicy(.accessory)
    defer { _ = NSApp.setActivationPolicy(policy) }
    let document = localDocument(source)
    document.makeWindowControllers()
    document.showWindows()
    defer { document.close() }
    guard let editor = document.windowControllers.first?.contentViewController as? EditorViewController else {
        throw LocalProbeFailure(description: "No owned editor for optical RAW UI")
    }
    _ = editor.view
    let originalMask = try RawLocalWork.state(source, layer: 0).masks[0].sha256()
    let coefficient = 0.012345678901234567
    var phase = 0
    var failure: Error?
    var reviewed: RasterDocument?
    var reviewedMask = ""
    var remapTimer: Timer?
    let deadline = Date().addingTimeInterval(150)
    let timer = Timer(timeInterval: 0.025, repeats: true) { _ in
        if failure != nil { NSApp.abortModal(); return }
        guard phase < 7 else { return }
        guard Date() < deadline else {
            failure = LocalProbeFailure(description: "Optical RAW UI timed out at phase \(phase)")
            localProgress("RAW optical UI \(depth): timeout at phase \(phase)")
            NSApp.abortModal(); return
        }
        guard let content = NSApp.modalWindow?.contentView else { return }
        let views = geometryDescendants(content)
        func named(_ id: String) -> NSView? {
            views.first { $0.accessibilityIdentifier() == id }
        }
        do {
            guard let apply = named("raw-develop-apply") as? NSButton,
                  let controller = apply.target as? RawDevelopWindowController,
                  controller.previewIsReady,
                  let tabs = named("raw-develop-section") as? NSSegmentedControl,
                  let tabAction = tabs.action else { return }
            switch phase {
            case 0:
                tabs.selectedSegment = 1
                _ = tabs.sendAction(tabAction, to: tabs.target)
                phase = 1
                localProgress("RAW optical UI \(depth): canonical section")
            case 1:
                guard let field = named("raw-lens-3") as? NSTextField,
                      let action = field.action else {
                    throw LocalProbeFailure(description: "Missing numeric optical k1 control")
                }
                field.stringValue = String(coefficient)
                _ = field.sendAction(action, to: field.target)
                phase = 2
                localProgress("RAW optical UI \(depth): exact lens k1")
            case 2:
                guard let result = controller.localSession?.result,
                      result.settings.sourceGeometry?.radial[0] == coefficient,
                      controller.localSession?.canApply == false, !apply.isEnabled,
                      let remap = named("raw-local-remap") as? NSButton else {
                    throw LocalProbeFailure(description: "Canonical preview did not hold exact lens coefficients and refuse unreviewed Apply")
                }
                let nestedDeadline = Date().addingTimeInterval(20)
                let nested = Timer(timeInterval: 0.025, repeats: true) { nestedTimer in
                    guard Date() < nestedDeadline else {
                        failure = LocalProbeFailure(description: "Numeric local Remap sheet timed out")
                        nestedTimer.invalidate()
                        NSApp.abortModal()
                        return
                    }
                    guard let nestedContent = NSApp.modalWindow?.contentView else { return }
                    let nestedViews = geometryDescendants(nestedContent)
                    guard let offset = nestedViews.first(where: {
                        $0.accessibilityIdentifier() == "raw-local-remap-4"
                    }) as? NSTextField,
                          let button = nestedViews.compactMap({ $0 as? NSButton })
                            .first(where: { $0.title == "Remap Mask" }) else { return }
                    offset.stringValue = "1.25"
                    phase = 4
                    localProgress("RAW optical UI \(depth): numeric local remap sheet")
                    nestedTimer.invalidate()
                    button.performClick(nil)
                }
                remapTimer = nested
                RunLoop.main.add(nested, forMode: .common)
                RunLoop.main.add(nested, forMode: .modalPanel)
                phase = 3
                localProgress("RAW optical UI \(depth): canonical review refusal")
                remap.performClick(nil)
                remapTimer?.invalidate()
            case 4:
                guard let result = controller.localSession?.result else { return }
                let state = try RawLocalWork.state(result.document, layer: 0)
                guard try state.masks[0].sha256() != originalMask,
                      !apply.isEnabled else {
                    throw LocalProbeFailure(description: "Numeric local remap did not change canonical mask and require Global review")
                }
                reviewedMask = try state.masks[0].sha256()
                tabs.selectedSegment = 0
                _ = tabs.sendAction(tabAction, to: tabs.target)
                phase = 5
                localProgress("RAW optical UI \(depth): global review")
            case 5:
                guard apply.isEnabled,
                      controller.localSession?.result?.settings.sourceGeometry?.radial[0] == coefficient,
                      let row = named("raw-develop-vignette-x") as? CardControlRow,
                      let slider = row.slider, let action = slider.action else {
                    throw LocalProbeFailure(description: "Global review omitted exact lens settings or Apply readiness")
                }
                slider.doubleValue = 0.37
                _ = slider.sendAction(action, to: slider.target)
                phase = 6
                localProgress("RAW optical UI \(depth): unrelated vignette control")
            case 6:
                guard let result = controller.localSession?.result,
                      result.settings.sourceGeometry?.radial[0] == coefficient,
                      result.settings.vignette?.centerX == 0.37,
                      apply.isEnabled else {
                    throw LocalProbeFailure(description: "Unrelated vignette edit rounded the exact optical coefficient")
                }
                let state = try RawLocalWork.state(result.document, layer: 0)
                guard try state.masks[0].sha256() == reviewedMask else {
                    throw LocalProbeFailure(description: "Global review changed authored remap coverage")
                }
                reviewed = result.document
                phase = 7
                localProgress("RAW optical UI \(depth): exact Apply")
                apply.performClick(nil)
            default: break
            }
        } catch {
            failure = error
            localProgress("RAW optical UI \(depth) failure: \(error)")
            NSApp.abortModal()
        }
    }
    RunLoop.main.add(timer, forMode: .common)
    RunLoop.main.add(timer, forMode: .modalPanel)
    defer { timer.invalidate(); remapTimer?.invalidate() }
    editor.redevelopRaw(nil)
    if let failure { throw failure }
    guard phase == 7, let reviewed, document.doc === reviewed else {
        throw LocalProbeFailure(description: "Optical UI did not publish its exact reviewed candidate")
    }
    let result = try RawLocalWork.state(reviewed, layer: 0)
    try localCheck(result.descriptor.recipe.sourceGeometry?.radial[0] == coefficient
        && (try result.masks[0].sha256()) == reviewedMask,
        "Optical UI published full-precision coefficients and native remap coverage")
    let artifact = directory.appendingPathComponent("optical-ui-\(depth).rz")
    try reviewed.saveNative(to: artifact)
    let reopened = try RasterDocument.open(url: artifact)
    try localCheck((try RawLocalWork.state(reopened, layer: 0)).descriptor
        == result.descriptor && (try localPixels(reopened)) == localPixels(reviewed),
        "Optical UI candidate persists after native reopen")
    let revision = document.revision
    var cancelled = false
    var cancelFailure: Error?
    let cancelDeadline = Date().addingTimeInterval(60)
    let cancelTimer = Timer(timeInterval: 0.025, repeats: true) { _ in
        guard !cancelled, cancelFailure == nil else { return }
        guard Date() < cancelDeadline else {
            cancelFailure = LocalProbeFailure(description: "Optical RAW Cancel timed out")
            NSApp.abortModal(); return
        }
        guard let content = NSApp.modalWindow?.contentView else { return }
        let views = geometryDescendants(content)
        guard let apply = views.first(where: {
            $0.accessibilityIdentifier() == "raw-develop-apply"
        }) as? NSButton,
              let controller = apply.target as? RawDevelopWindowController,
              controller.previewIsReady,
              let cancel = views.compactMap({ $0 as? NSButton })
                .first(where: { $0.title == "Cancel" }) else { return }
        cancelled = true
        cancel.performClick(nil)
    }
    RunLoop.main.add(cancelTimer, forMode: .common)
    RunLoop.main.add(cancelTimer, forMode: .modalPanel)
    defer { cancelTimer.invalidate() }
    editor.redevelopRaw(nil)
    if let cancelFailure { throw cancelFailure }
    try localCheck(cancelled && document.doc === reviewed && document.revision == revision,
        "Optical RAW Cancel leaves reviewed source owner and history unchanged")
    return ["passed": true, "depth": depth, "coefficient": coefficient,
        "canonical_mask": reviewedMask, "source_sha256": localHash(try localPixels(reviewed)),
        "native": artifact.path, "cancel_preserved_owner": cancelled]
}

private func geometryDescendants(_ view: NSView) -> [NSView] {
    [view] + view.subviews.flatMap(geometryDescendants)
}
