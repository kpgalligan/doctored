import AppKit

// Initial import exposes global enhancements; local edits follow on the retained source.

extension RawDevelopWindowController {
    /// Retained development returns the actual reviewed source. The caller
    /// publishes it once after rechecking the original document capture.
    static func runRetained(_ probe: RawProbe, descriptor: RawSourceDescriptor,
                            snapshot: RawSourceSnapshot, source: RasterDocument,
                            layer: Int, owner: ImageDocument? = nil,
                            validate: (() throws -> Void)? = nil) throws -> RawLocalDevelopResult? {
        let controller = RawDevelopWindowController(probe: probe, descriptor: descriptor,
            snapshot: snapshot, redevelop: true, space: source.drawingSpace,
            beforeSource: try source.sourceMaster(layer),
            documentIsEightBit: !source.isHighDepth, documentIsFloat: source.isFloat)
        controller.localSession = try RawLocalDevelopSession(source: source, layer: layer)
        controller.localValidate = validate
        let observer = owner.map { owner in
            NotificationCenter.default.addObserver(forName: .imageDocumentDidClose, object: owner,
                queue: .main) { [weak controller] _ in
                    controller?.localSession?.cancel()
                    controller?.localGesture?.clear()
                    controller?.localControls?.clear()
                    controller?.workflowPanel?.clear()
                    controller?.modal?.end(.cancel)
                }
        }
        defer { if let observer { NotificationCenter.default.removeObserver(observer) } }
        guard controller.run() != nil else { return nil }
        return controller.localResult
    }

    func localSidebar(global: NSStackView) -> NSStackView {
        guard let session = localSession else { return global }
        let controls = RawLocalControls(state: session.state)
        controls.toneAvailable = encoding != .linearFloat
        localControls = controls
        controls.onOperation = { [weak self] operation, dabs in
            guard let self, let session = self.localSession else {
                throw DoctoredCoreError(message: "This Develop session has closed.")
            }
            try session.append(operation, dabs: dabs)
            self.requestPreview()
        }
        controls.onSelection = { [weak self] in self?.requestPreview() }
        controls.onTool = { [weak self] tool in
            guard let self else { return }
            self.localTool = tool
            self.localGesture?.tool = self.localSection ? tool : .pan
            if tool != .pan { self.workflowPanel?.showAfter() }
        }
        controls.onBrush = { [weak self] radius, hardness, opacity in
            self?.localGesture?.radius = radius
            self?.localGesture?.hardness = hardness
            self?.localGesture?.opacity = opacity
        }
        controls.onError = { [weak self] error in self?.localError(error) }
        let tabs = NSSegmentedControl(labels: ["Global (final)", "Local Masks (canonical, before lens)"], trackingMode: .selectOne,
            target: self, action: #selector(localSectionChanged(_:)))
        tabs.selectedSegment = 0
        tabs.setAccessibilityIdentifier("raw-develop-section")
        controls.view.isHidden = true
        let sidebar = NSStackView(views: [tabs, global, controls.view])
        sidebar.orientation = .vertical; sidebar.alignment = .leading; sidebar.spacing = 12
        sidebar.detachesHiddenViews = true
        globalControlsView = global; localSidebarView = sidebar
        return sidebar
    }

    func configureLocalSession() -> Bool {
        guard let session = localSession, let panel = workflowPanel, let controls = localControls else { return false }
        let gesture = RawLocalGestureView(frame: panel.canvas.bounds)
        gesture.autoresizingMask = [.width, .height]
        gesture.canvas = panel.canvas
        gesture.enabled = false
        panel.canvas.addSubview(gesture)
        localGesture = gesture
        gesture.onCommand = { [weak controls] command, dabs in controls?.accept(command, dabs: dabs) }
        gesture.onSample = { [weak controls] point in controls?.sampled(at: point) }
        gesture.onError = { [weak self] message in self?.localError(message) }
        session.onFrame = { [weak panel, weak gesture, weak controls] frame, overlay, state in
            panel?.present(frame)
            gesture?.overlay = overlay
            controls?.update(state: state, ready: true)
        }
        session.onStatus = { [weak self, weak panel, weak gesture, weak controls] status, ready in
            panel?.setStatus(status, ready: ready)
            self?.previewIsReady = ready
            self?.applyButton?.isEnabled = ready && session.canApply
            gesture?.enabled = ready && self?.localSection == true && controls?.canDraw == true
            if !ready { gesture?.overlay = nil }
            controls?.setReady(ready)
        }
        panel.onChange = { [weak self] in self?.requestPreview() }
        return true
    }

    func requestLocalPreview() -> Bool {
        guard let session = localSession, let panel = workflowPanel else { return false }
        session.request(settings: pendingSettings(), viewport: panel.currentViewport(),
            selected: localControls?.selectedID,
            overlay: localSection && localControls?.showsOverlay == true,
            canonicalEditing: localSection)
        return true
    }

    @objc private func localSectionChanged(_ sender: NSSegmentedControl) {
        localSection = sender.selectedSegment == 1
        globalControlsView?.isHidden = localSection
        localControls?.view.isHidden = !localSection
        localGesture?.tool = localSection ? localTool : .pan
        if localSection, localTool != .pan { workflowPanel?.showAfter() }
        refitControlsDocument()
        if let clip = controlsScroll?.contentView {
            clip.scroll(to: .zero)
            controlsScroll?.reflectScrolledClipView(clip)
        }
        requestPreview()
    }

    private func localError(_ message: String) {
        workflowPanel?.setStatus(message, ready: false)
        previewIsReady = false; applyButton?.isEnabled = false
        localGesture?.overlay = nil; localGesture?.enabled = false
    }
}
