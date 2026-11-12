import AppKit

extension EditorViewController {
    func beginBytePaintStroke() -> Bool {
        guard let document else { return false }
        // Settle a finished job first (so the stroke starts on its result),
        // then refuse while one still runs: a healing stroke needs the one
        // worker at mouse-up, and any stroke on the job's own document would
        // make that job stale.
        let workerBound = [.heal, .spotHeal].contains(currentTool)
        guard !DocumentJobs.refusesGesture(on: document, workerBound: workerBound) else {
            return false
        }
        guard let doc = document.doc, doc.precision == nil else { return false }
        // The Pencil strokes as the Brush from here on (`EditorTool.strokeFamily`).
        self.strokeTool = self.currentTool.strokeFamily
        let idx = document.activeLayerIndex
        let isAdjustment = doc.layerIsAdjustment(idx)
        // Only brush and eraser ever paint COVERAGE; clone and dodge run
        // their own pixel ops and cannot reach a coverage target at all.
        let paintsMaskTool = self.strokeTool == .brush || self.strokeTool == .eraser
        // True when the target names a channel this document still has —
        // the one case where the active layer has nothing to do with the
        // edit. (`paintsCoverageTarget` is what validates the index.)
        let onChannel = self.paintTarget.isChannel && self.paintsCoverageTarget
        // Unsupported coverage targets stay visibly selected. Refuse a
        // pixel-only tool instead of rewriting the photograph beneath it.
        guard !self.paintTarget.isCoverage || paintsMaskTool else { return false }
        // A hidden layer: refuse instead of painting invisibly. A CHANNEL
        // is canvas-sized document state that the layer's eye says nothing
        // about, so a channel stroke is exempt — a `.mask` or `.plane`
        // target still belongs to the hidden layer and keeps refusing.
        // (The agent's brush_stroke {target: "channel:…"} has always been
        // exempt; this is the UI agreeing with it.)
        guard doc.layerInfo(idx)?.visible == true || onChannel else { return false }
        // An adjustment layer's pixels are ignored by the compositor, so
        // strokes ALWAYS land on its mask; with the mask deleted there
        // is nothing left to paint — refuse (the canvas beeps). Clone
        // and dodge rewrite pixels, which an adjustment layer hasn't
        // got, so they refuse outright.
        if self.strokeTool == .clone || self.strokeTool.strokeIsCoverage
            || self.strokeTool == .heal || self.strokeTool == .spotHeal, isAdjustment {
            return false
        }
        if isAdjustment, !doc.layerHasMask(idx), !onChannel { return false }
        // Decided once per stroke so a target change mid-drag can never
        // split it across two targets. A CHANNEL is document state: an
        // adjustment layer neither forces it nor blocks it (the layer is
        // not being painted at all), and `paintsCoverageTarget` is what
        // validates its index.
        let target: PaintTarget = paintsMaskTool && self.paintsCoverageTarget
            ? self.paintTarget
            : (isAdjustment ? .mask : .layer)
        self.strokeTarget = target
        self.canvas.paintTarget = target
        // Locks, once per stroke, against the target the stroke really
        // hits. A MASK edit answers to the Mask kind — a pixel-locked
        // layer's mask can be painted, and only Lock All freezes it —
        // while the layer's own pixels and its colour
        // PLANES answer to Pixels. A CHANNEL is canvas-sized document
        // state that belongs to no layer, so it is exempt, exactly as
        // it is exempt from the hidden-layer refusal above. Transparency
        // lock refuses nothing here: the core restores the layer's alpha
        // after the stroke, so the paint lands and stays inside the
        // existing shape.
        if !onChannel {
            let editKind: DrEditKind = target == .mask ? DR_EDIT_MASK : DR_EDIT_PIXELS
            if self.refuseLockedEdit(layer: idx, kind: editKind) { return false }
            // A GROUP has no pixels of its own, so every tick's
            // `paintingLayer` would answer nil and the whole stroke
            // would be a silent no-op (+Groups.swift). A COLOUR PLANE
            // reads the same pixels — `paintingLayerPlane` goes through
            // the core's `raster_layer` too — and the agent's mirror
            // already covers both (AgentServer+Channels).
            if target == .layer || target.isPlane, self.refuseGroupPixelEdit() {
                return false
            }
        }
        guard !target.isCoverage else {
            // Coverage strokes ghost on the canvas and commit in one
            // step from onCommitMaskOverlay: no live-edit session.
            guard self.captureBytePaintStroke(doc, layer: idx, target: target) else { return false }
            self.strokeBase = nil
            return true
        }
        // The sampling plate, before the session opens: a retouch tool
        // that cannot build one must refuse here, where refusing is
        // still free, rather than fall back to the canvas's display
        // image the way this latch used to (`RetouchPlate`).
        guard self.latchRetouchPlate(document, doc, layer: idx) else { return false }
        // The History Brush's source state, while refusing is still free:
        // no such layer in it, another canvas size or another depth beeps.
        var historySource: HistoryBrushSource?
        if self.strokeTool == .historyBrush {
            guard let source = self.latchHistoryBrushSource(document, doc, layer: idx) else {
                return false
            }
            historySource = source
        }
        // The Art History Brush paints from the same source.
        if self.strokeTool == .artHistoryBrush {
            guard let source = self.latchArtHistorySource(document, doc, layer: idx) else {
                return false
            }
            historySource = source
        }
        guard self.captureBytePaintStroke(doc, layer: idx, target: target,
                                          historySource: historySource) else { return false }
        if self.strokeTool == .artHistoryBrush {
            self.byteStrokeCapture?.artSeed = Self.artHistorySeed()
        }
        self.strokeBase = doc
        // Blend applies to strokes that PAINT layer pixels: brush and
        // clone. The eraser is its own composite op and dodge is a
        // retouch op, so both stay Normal whatever their store says.
        self.strokeBlendMode = self.paintStrokeBlendMode()
        document.beginLiveEdit()
        return true
    }

    /// Capture the gesture's owner and settings while refusal is still free.
    func captureBytePaintStroke(_ doc: RasterDocument, layer: Int, target: PaintTarget,
                                historySource: HistoryBrushSource? = nil) -> Bool {
        guard let document, let layerID = doc.layerInfo(layer)?.id,
              let color = ColorProfile.quantized(paintColor,
                in: document.nsColorSpace) else { return false }
        let options = ToolOptionsStore.shared.paintOptions(for: strokeTool) ?? PaintToolOptions()
        byteStrokeCapture = BytePaintStrokeCapture(base: doc, layer: layer,
            layerID: layerID, target: target, selectedTarget: paintTarget,
            channelID: target.isChannel ? channelIdentity(of: target) : nil,
            selection: document.selectionHistory, tool: strokeTool,
            options: options, color: color, blend: paintStrokeBlendMode(),
            historySource: historySource)
        byteLastPublished = doc
        byteStrokeRefused = false
        return true
    }

    func byteStrokeTargetIsCurrent() -> Bool {
        guard let capture = byteStrokeCapture, let document, let current = document.doc,
              document.activeLayerIndex == capture.layer,
              current.layerInfo(capture.layer)?.id == capture.layerID,
              document.selectionHistory == capture.selection,
              paintTarget == capture.selectedTarget,
              strokeTarget == capture.target else { return false }
        if let channelID = capture.channelID,
           channelIdentity(of: capture.target) != channelID { return false }
        return current === byteLastPublished
    }

    func updateBytePaintStroke(_ data: UnsafePointer<UInt8>, mode: DrCompositeMode,
                               alpha: Double) {
        guard !byteStrokeRefused else { return }
        guard byteStrokeTargetIsCurrent(), let capture = byteStrokeCapture,
              let document, let base = strokeDestination ?? strokeBase else { return }
        let pixelCount = base.width * base.height
        let emptyOverlay = (0..<pixelCount).allSatisfy { data[$0 * 4 + 3] == 0 }
        // The coverage tools' strength lives in the op (`opacity` is the
        // Exposure or Strength), not in the stroke's alpha.
        let zeroStrength = capture.tool.strokeIsCoverage
            ? capture.options.opacity <= 0 : alpha <= 0
        if emptyOverlay || zeroStrength {
            if document.doc === byteLastPublished, let original = strokeBase,
               document.doc !== original {
                document.updateLiveEdit(original)
                if document.doc === original { byteLastPublished = original }
                else { byteStrokeRefused = true }
            }
            return
        }
        let idx = capture.layer
        let updated: RasterDocument?
        if capture.tool == .dodge {
            updated = base.dodgeBurnLayer(idx, overlay: data, w: base.width, h: base.height,
                exposure: min(max(capture.options.opacity, 0), 100) / 100,
                range: min(max(capture.options.rangeIndex, 0), 2),
                burn: capture.options.burn, protectTones: capture.options.protectTones == true)
        } else if capture.tool == .smudge {
            // Incremental while the gesture runs, recomputed once from the
            // base on the final tick (`smudgeTick`); a throw refuses the
            // stroke, which the end of the gesture rolls back.
            do {
                switch try smudgeTick(capture, base: base) {
                case .unchanged: return
                case .incremental(let dragged): updated = dragged
                case .final(let whole): updated = whole
                }
            } catch {
                byteStrokeRefused = true
                return
            }
        } else if capture.tool == .artHistoryBrush {
            // Rerun from the base every tick (`artHistoryTick`); a throw refuses
            // the stroke, which the end of the gesture rolls back.
            do { updated = try artHistoryTick(capture, base: base) }
            catch {
                byteStrokeRefused = true
                return
            }
        } else if capture.tool.strokeIsCoverage {
            // A throw (an option the kind refuses, a failed picture render)
            // refuses the stroke, which the end of the gesture rolls back.
            do { updated = try retouchBrushTick(capture, base: base, overlay: data) }
            catch {
                byteStrokeRefused = true
                return
            }
        } else if capture.tool == .clone, strokePlate?.backdrop != nil {
            updated = try? base.cloneToNewLayer(idx, overlay: data, w: base.width,
                h: base.height, alpha: alpha, name: Self.retouchLayerName)
        } else if mode == DR_COMPOSITE_OVER, capture.blend != DR_BLEND_NORMAL {
            if capture.tool == .brush {
                // The colour brush blends its exact straight colour; only the
                // overlay's alpha is read, as coverage.
                guard let ink = ColorProfile.bytes(capture.color, in: document.nsColorSpace)
                else { byteStrokeRefused = true; return }
                updated = base.paintingLayerBlendColor(idx, overlay: data, w: base.width,
                    h: base.height, rgb: Array(ink.prefix(3)), mode: capture.blend,
                    alpha: alpha)
            } else {
                updated = base.paintingLayerBlend(idx, overlay: data, w: base.width,
                    h: base.height, mode: capture.blend, alpha: alpha)
            }
        } else {
            updated = base.paintingLayer(idx, overlay: data, w: base.width,
                h: base.height, mode: mode, alpha: alpha)
        }
        guard let updated else {
            // The core also answers nil for a genuine no-op (a clipped or
            // zero-opacity preview). If a prior tick published pixels, the
            // final overlay supersedes it and must restore the captured base.
            if document.doc === byteLastPublished, let original = strokeBase,
               document.doc !== original {
                document.updateLiveEdit(original)
                if document.doc === original { byteLastPublished = original }
                else { byteStrokeRefused = true }
            }
            if canvas.preparedByteStroke != nil { byteStrokeRefused = true }
            return
        }
        document.updateLiveEdit(updated)
        guard document.doc === updated else { byteStrokeRefused = true; return }
        byteLastPublished = updated
    }

    func endBytePaintStroke(_ actionName: String) {
        guard let document, let capture = byteStrokeCapture else {
            cancelBytePaintStroke()
            return
        }
        defer {
            byteStrokeCapture = nil
            byteLastPublished = nil
            byteStrokeRefused = false
            strokePlate = nil
            strokeDestination = nil
            strokeTarget = .layer
            canvas.paintTarget = paintTarget
            strokeBase = nil
            syncCanvasPaintState()
        }
        guard !byteStrokeRefused else { cancelBytePaintStroke(); return }
        // Coverage committed through onCommitMaskOverlay; the healing
        // workers already closed their preview session and own their
        // eventual edit.
        guard !capture.target.isCoverage, ![.spotHeal, .heal].contains(capture.tool) else { return }
        guard byteStrokeTargetIsCurrent() else {
            cancelBytePaintStroke()
            return
        }
        let base = strokeBase
        let idx = capture.layer
        if strokePlate?.backdrop == nil, let base, document.doc !== base,
           document.layerDescribesSource(idx) {
            if document.confirmRasterize(layer: idx) {
                if let cleared = document.doc?.withLayerMeta(idx, nil) {
                    document.updateLiveEdit(cleared)
                    byteLastPublished = cleared
                }
            } else {
                document.updateLiveEdit(base)
                byteLastPublished = base
            }
        }
        // Edit ▸ Fade weighs a layer stroke by the pixels it changed; a stroke
        // onto a new layer has no state before it to fade toward.
        if strokePlate?.backdrop == nil { document.fadeHint = FadeHint(kind: .stroke, layer: idx) }
        document.endLiveEdit(actionName, record: strokeRecord(actionName))
    }

    /// A cancelled byte gesture may roll back only the handle it last owned.
    func cancelBytePaintStroke() {
        guard let document else { return }
        let owned = document.doc === byteLastPublished
        let base = strokeBase
        byteStrokeCapture = nil
        byteLastPublished = nil
        byteStrokeRefused = false
        strokePlate = nil
        strokeDestination = nil
        strokeTarget = .layer
        canvas.paintTarget = paintTarget
        strokeBase = nil
        if owned, let base { document.updateLiveEdit(base) }
        if let base { document.abandonLiveEdit(ownedBy: base) }
        syncCanvasPaintState()
    }
}
