// Actual modal UI qualification for the 8D native Develop workspace. This file
// compiles with app sources; it does not substitute a mock controller or decoder.
import AppKit

private struct DevelopUIProbeError: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}
private func developUIAssert(_ condition: @autoclosure () -> Bool, _ message: String) throws {
    if !condition() { throw DevelopUIProbeError(message: message) }
}
private func developDescendants(_ view: NSView) -> [NSView] {
    [view] + view.subviews.flatMap(developDescendants)
}
private func developControl<T: NSView>(_ views: [NSView], _ id: String, _: T.Type) throws -> T {
    guard let value = views.first(where: { $0.accessibilityIdentifier() == id }) as? T else {
        throw DevelopUIProbeError(message: "Missing Develop control \(id)")
    }
    return value
}
private func developSend(_ control: NSControl) {
    _ = control.sendAction(control.action, to: control.target)
}
private func developModalTimer(_ block: @escaping () -> Void) -> Timer {
    let timer = Timer(timeInterval: 0.03, repeats: true) { _ in block() }
    RunLoop.main.add(timer, forMode: .common)
    RunLoop.main.add(timer, forMode: .modalPanel)
    return timer
}
private func developScreenshot(_ view: NSView, _ url: URL) throws {
    guard let bitmap = view.bitmapImageRepForCachingDisplay(in: view.bounds) else {
        throw DevelopUIProbeError(message: "Could not capture Develop layout")
    }
    view.cacheDisplay(in: view.bounds, to: bitmap)
    guard let png = bitmap.representation(using: .png, properties: [:]) else {
        throw DevelopUIProbeError(message: "Could not encode Develop layout")
    }
    try png.write(to: url)
}

/// Returns 8D UI results to the host report. Native detail uses a photographic
/// source larger than its viewport, while the RAW control test uses the caller's
/// captured real decoder/recipe and private immutable file.
func runDevelopUIProbe(probe: RawProbe, descriptor: RawSourceDescriptor,
                       snapshot: RawSourceSnapshot, beforeSource: RasterPrecisionImage,
                       directory: URL) throws -> [String: Any] {
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    var groups: [String] = []
    var captures: [String] = []
    let width = 1536, height = 1100
    func source(_ shifted: Bool) throws -> RasterPrecisionImage {
        var values = [UInt16]()
        values.reserveCapacity(width * height * 4)
        for y in 0..<height {
            for x in 0..<width {
                let r = UInt16((x * 37 + y * 11 + (shifted ? 997 : 0)) % 65536)
                let g = UInt16((x * 7 + y * 67) % 65536)
                values += [r, g, UInt16((x * 3 + y * 19) % 65536), 65535]
            }
        }
        // Sparse endpoints exercise all-native clipping detection even when fit
        // preview cell centers would never sample these original positions.
        values[4 * (44 * width + 33)] = 0
        values[4 * (77 * width + 99) + 1] = 65535
        let doc = try RasterDocument.fromRawRGBA16(values, width: width, height: height,
            profile: WorkingSpace.sRGB.profileData)
        guard let result = doc.precision?.layerSource(0) else {
            throw DevelopUIProbeError(message: "Missing test comparison source")
        }
        return result
    }
    let before = try source(false), after = try source(true)
    var failure: Error?
    var stage = 0
    var retainedCanvas: RawDevelopCanvasView?
    let deadline = Date().addingTimeInterval(180)
    let timer = developModalTimer {
        do {
            guard Date() < deadline else { throw DevelopUIProbeError(message: "Develop comparison UI timed out at stage \(stage)") }
            guard let window = NSApp.windows.first(where: { $0.title == "8D UI comparison" && $0.isVisible }),
                  let content = window.contentView else { return }
            let views = developDescendants(content)
            let canvas = try developControl(views, "raw-develop-canvas", RawDevelopCanvasView.self)
            retainedCanvas = canvas
            let zoom = try developControl(views, "raw-develop-zoom", NSSegmentedControl.self)
            let compare = try developControl(views, "raw-develop-comparison", NSSegmentedControl.self)
            let clip = try developControl(views, "raw-develop-clipping", NSButton.self)
            let x = try developControl(views, "raw-develop-center-x", NSSlider.self)
            let y = try developControl(views, "raw-develop-center-y", NSSlider.self)
            guard let frame = canvas.preview else { return }
            switch stage {
            case 0:
                try developUIAssert(frame.after.bins.total == Double(width * height), "Fit histogram dropped native pixels")
                try developUIAssert(!frame.after.bins.sampled, "Develop histogram used sampled positions")
                try developUIAssert(!frame.viewport.nativeDetail, "Initial comparison did not use Fit")
                let apply = try developControl(views, "raw-migration-apply", NSButton.self)
                let keep = try developControl(views, "raw-migration-keep", NSButton.self)
                let visible = window.screen?.visibleFrame ?? NSScreen.main!.visibleFrame
                try developUIAssert(window.frame.height <= visible.height && window.frame.width <= visible.width,
                    "Long migration hint made the modal larger than the visible screen")
                for button in [apply, keep] {
                    let rect = button.convert(button.bounds, to: content)
                    try developUIAssert(content.bounds.contains(rect), "Migration decision button is outside the card")
                }
                groups += ["native_fit_whole_image_histogram", "long_migration_hint_keeps_decision_buttons_visible"]
                zoom.selectedSegment = 1
                developSend(zoom)
                stage = 1
            case 1:
                guard frame.viewport.nativeDetail else { return }
                let r = frame.viewport.region(frame.after.image)
                try developUIAssert(r.divisor == 1 && r.width <= 1024 && r.height <= 1024,
                    "100 percent detail resampled or exceeded the bounded native region")
                try developUIAssert(frame.afterDisplay.width == r.width && frame.afterDisplay.height == r.height,
                    "100 percent display does not contain one sample per source pixel")
                let exact = try frame.after.image.developDetail(x: r.x, y: r.y, width: r.width, height: r.height)
                try after.withRGBA16 { full in
                    try exact.withRGBA16 { crop in
                        for yy in 0..<r.height {
                            for xx in 0..<r.width {
                                for c in 0..<4 {
                                    try developUIAssert(crop[(yy * r.width + xx) * 4 + c]
                                        == full[((yy + r.y) * width + xx + r.x) * 4 + c],
                                        "Native detail changed a u16 sample")
                                }
                            }
                        }
                    }
                }
                groups.append("actual_100_percent_exact_native_detail")
                x.doubleValue = 0.0; developSend(x)
                y.doubleValue = 0.0; developSend(y)
                x.doubleValue = 1.0; developSend(x)
                y.doubleValue = 1.0; developSend(y)
                stage = 2
            case 2:
                guard frame.viewport.center == CGPoint(x: 1, y: 1) else { return }
                let r = frame.viewport.region(frame.after.image)
                try developUIAssert(r.x == width - r.width && r.y == height - r.height,
                    "Pan did not reach the final native image corner")
                groups.append("pan_latest_generation_reaches_native_corner")
                compare.selectedSegment = 2; developSend(compare)
                clip.state = .on; developSend(clip)
                stage = 3
            case 3:
                guard frame.viewport.clipping == 3 else { return }
                try developUIAssert(canvas.comparison == .split, "Split comparison did not update")
                let capture = directory.appendingPathComponent("native-detail-split.png")
                try developScreenshot(content, capture)
                captures.append(capture.path)
                groups.append("actual_before_after_split_and_clipping")
                compare.selectedSegment = 1; developSend(compare)
                stage = 4
            case 4:
                try developUIAssert(canvas.comparison == .before, "Before comparison did not update")
                try developControl(views, "raw-migration-apply", NSButton.self).performClick(nil)
                stage = 5
            default: break
            }
        } catch {
            failure = error
            NSApp.stopModal(withCode: .cancel)
        }
    }
    let longHint = String(repeating: "Before decoder: saved implementation. After decoder: available implementation. This comparison uses the exact prepared image and does not update the document until migration is accepted. ", count: 8)
    let accepted = RawDevelopComparisonController.run(before: before, after: after,
        title: "8D UI comparison", hint: longHint, applyTitle: "Accept Prepared Source")
    timer.invalidate()
    if let failure { throw failure }
    try developUIAssert(accepted && stage == 5, "Migration comparison did not accept its prepared source")
    try developUIAssert(retainedCanvas?.preview == nil, "Closed comparison window retained native preview sources")
    try developUIAssert(!BackgroundWork.occupied, "Completed comparison left the worker occupied")
    groups.append("closed_modal_releases_captured_preview_sources")

    stage = 0
    failure = nil
    retainedCanvas = nil
    let rawDeadline = Date().addingTimeInterval(180)
    let rawTimer = developModalTimer {
        do {
            guard Date() < rawDeadline else { throw DevelopUIProbeError(message: "Develop RAW controls timed out at stage \(stage)") }
            guard let window = NSApp.windows.first(where: { $0.title == "Develop RAW" && $0.isVisible }),
                  let content = window.contentView else { return }
            let views = developDescendants(content)
            let canvas = try developControl(views, "raw-develop-canvas", RawDevelopCanvasView.self)
            retainedCanvas = canvas
            guard canvas.preview != nil else { return }
            if stage == 0 {
                let outputRow = try developControl(views, "raw-develop-output-space", NSView.self)
                guard let popup = developDescendants(outputRow).compactMap({ $0 as? NSPopUpButton }).first else {
                    throw DevelopUIProbeError(message: "Develop output popup is missing")
                }
                let index = descriptor.recipe.outputColorSpace == .displayP3 ? 0 : 1
                popup.selectItem(at: index); developSend(popup)
                stage = 1
            } else if stage == 1 {
                let expected = descriptor.recipe.outputColorSpace == .displayP3 ? WorkingSpace.sRGB : WorkingSpace.displayP3
                guard let image = canvas.preview?.after.image, image.profileData == expected.profileData else { return }
                let zoom = try developControl(views, "raw-develop-zoom", NSSegmentedControl.self)
                zoom.selectedSegment = 1; developSend(zoom)
                stage = 2
            } else if stage == 2 {
                guard canvas.preview?.viewport.nativeDetail == true else { return }
                let preset = try developControl(views, "raw-develop-preset", NSPopUpButton.self)
                _ = try developControl(views, "raw-develop-save-preset", NSButton.self)
                let presetFrame = preset.convert(preset.bounds, to: content)
                let canvasFrame = canvas.convert(canvas.bounds, to: content)
                try developUIAssert(presetFrame.maxY >= canvasFrame.maxY,
                    "Develop controls were bottom-aligned below unused scroll space")
                let depth = try developControl(views, "raw-develop-document-depth", NSView.self)
                guard let depthPopup = developDescendants(depth).compactMap({ $0 as? NSPopUpButton }).first,
                      let clip = depth.enclosingScrollView?.contentView else {
                    throw DevelopUIProbeError(message: "Initial RAW document-depth popup is missing")
                }
                try developUIAssert(clip.bounds.contains(depth.convert(depth.bounds, to: clip)),
                    "Initial document-depth popup is not visible in the controls viewport")
                depthPopup.selectItem(at: 1); developSend(depthPopup)
                depthPopup.selectItem(at: 0); developSend(depthPopup)
                let capture = directory.appendingPathComponent("raw-workflow-output-space.png")
                try developScreenshot(content, capture)
                captures.append(capture.path)
                groups.append("actual_develop_output_space_and_preset_controls")
                let visible = window.screen?.visibleFrame ?? NSScreen.main!.visibleFrame
                try developUIAssert(window.frame.height <= visible.height && window.frame.width <= visible.width,
                    "Develop workspace does not fit the visible screen")
                guard let cancel = views.compactMap({ $0 as? NSButton }).first(where: { $0.title == "Cancel" }) else {
                    throw DevelopUIProbeError(message: "Develop Cancel is missing")
                }
                cancel.performClick(nil)
                stage = 3
            }
        } catch { failure = error; NSApp.stopModal(withCode: .cancel) }
    }
    let cancelled = RawDevelopWindowController.runImport(probe, descriptor: descriptor, snapshot: snapshot)
    rawTimer.invalidate()
    if let failure { throw failure }
    try developUIAssert(cancelled == nil && stage == 3, "Cancel published pending RAW settings")
    try developUIAssert(retainedCanvas?.preview == nil, "Closed Develop window retained native preview sources")
    try developUIAssert(!BackgroundWork.occupied, "Closed ready Develop left its worker occupied")
    groups.append("cancel_keeps_raw_settings_unpublished_and_clears_preview")

    stage = 0
    failure = nil
    let cancelDeadline = Date().addingTimeInterval(180)
    let cancelTimer = developModalTimer {
        do {
            guard Date() < cancelDeadline else { throw DevelopUIProbeError(message: "Queued-preview cancellation timed out") }
            guard let window = NSApp.windows.first(where: { $0.title == "Develop RAW" && $0.isVisible }),
                  let content = window.contentView else { return }
            let views = developDescendants(content)
            let canvas = try developControl(views, "raw-develop-canvas", RawDevelopCanvasView.self)
            retainedCanvas = canvas
            if stage == 0, canvas.preview != nil {
                let row = try developControl(views, "raw-develop-output-space", NSView.self)
                guard let popup = developDescendants(row).compactMap({ $0 as? NSPopUpButton }).first else {
                    throw DevelopUIProbeError(message: "Missing output-space control for cancellation")
                }
                // Hold the shared queue briefly so cancellation deterministically
                // closes a window whose native request still owns its inputs.
                // This delay is a lifetime test, never a performance measurement.
                BackgroundWork.queue.async { Thread.sleep(forTimeInterval: 0.25) }
                popup.selectItem(at: descriptor.recipe.outputColorSpace == .displayP3 ? 0 : 1)
                developSend(popup)
                stage = 1
            } else if stage == 1, BackgroundWork.occupied {
                guard let cancel = views.compactMap({ $0 as? NSButton }).first(where: { $0.title == "Cancel" }) else {
                    throw DevelopUIProbeError(message: "Missing Cancel during native preview")
                }
                cancel.performClick(nil)
                stage = 2
            }
        } catch { failure = error; NSApp.stopModal(withCode: .cancel) }
    }
    let pendingCancelled = RawDevelopWindowController.run(probe, descriptor: descriptor, snapshot: snapshot,
        redevelop: true, beforeSource: beforeSource)
    cancelTimer.invalidate()
    if let failure { throw failure }
    try developUIAssert(pendingCancelled == nil && stage == 2, "Cancel failed while native work was pending")
    let drained = Date().addingTimeInterval(60)
    while BackgroundWork.occupied && Date() < drained {
        RunLoop.main.run(until: Date().addingTimeInterval(0.01))
    }
    try developUIAssert(!BackgroundWork.occupied, "Cancelled native preview never released the worker")
    try developUIAssert(retainedCanvas?.preview == nil, "Late native result repopulated a closed window")
    groups.append("pending_native_preview_survives_cancel_then_releases_its_worker")
    return ["passed": true, "groups": groups, "captures": captures,
        "comparison_dimensions": [width, height], "native_detail_maximum_side": 1024]
}
