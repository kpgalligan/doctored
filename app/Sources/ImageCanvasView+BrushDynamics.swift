import AppKit

/// The pen state a canvas stroke carries for brush dynamics: the latest event's tilt and
/// barrel rotation (read at every mouse-down and drag, NaN for a mouse, both turned into
/// the canvas frame), the tilt and rotation the walk reached at its last stamp, and whether
/// the stroke has tablet pressure. Tilt and rotation are interpolated per stamp along the
/// spline exactly as pressure is (`StrokeVertex`), so a dab's pen is where the pen was at
/// that dab, not the newest event's.
struct StrokePenState {
    var eventTilt = StrokeVertex.noTilt
    var lastTilt = StrokeVertex.noTilt
    var eventRotation = CGFloat.nan
    var lastRotation = CGFloat.nan
    var hasPressure = false

    /// `a` moved toward `b` by `t`, as the walk moves pressure.
    static func mix(_ a: CGVector, _ b: CGVector, _ t: CGFloat) -> CGVector {
        CGVector(dx: a.dx + (b.dx - a.dx) * t, dy: a.dy + (b.dy - a.dy) * t)
    }
}

/// Brush dynamics on the canvas (`designs/brush-dynamics.md`): a Brush, Pencil or Eraser
/// stroke whose options hold dynamics gets the core's handle on its dab session at
/// mouse-down, with a fresh seed; the walk spaces by each stamp's nominal diameter and
/// draws exactly the dabs the session laid, each at its own shape, so the preview is the
/// recorded stroke.
extension ImageCanvasView {
    /// Mouse-down and drag: the event's tilt and rotation in the canvas frame. The view
    /// rotation (`onViewRotation`, clockwise-positive on screen) adds to the pen's
    /// counter-clockwise rotation, and turns the tilt vector counter-clockwise in its y-up
    /// frame, so a direction on screen is the same direction on the rotated canvas.
    func latchPen(_ event: NSEvent) {
        let view = CGFloat(onViewRotation?() ?? 0)
        let tilt = PenSample.tilt(of: event)
        let (sine, cosine) = (sin(view * .pi / 180), cos(view * .pi / 180))
        strokePen.eventTilt = CGVector(dx: tilt.dx * cosine - tilt.dy * sine,
                                       dy: tilt.dx * sine + tilt.dy * cosine)
        strokePen.eventRotation = PenSample.rotation(of: event) + view
    }

    /// Mouse-down, after the stroke's session is made: latches the pen state and, for a
    /// tool that takes dynamics with a session, the dynamics handle; a tip shape and a
    /// texture only on a tool that takes them, the texture only beside a handle. A refused
    /// request or texture throws, which refuses the stroke.
    func attachStrokeDynamics(pressure: CGFloat?) throws {
        strokePen.hasPressure = pressure != nil
        strokePen.lastTilt = strokePen.eventTilt
        strokePen.lastRotation = strokePen.eventRotation
        guard tool.takesBrushDynamics, nativeStrokeSession != nil else { return }
        var dynamics = brushDynamics
        if !tool.takesTipShapeDynamics { dynamics?.shape = nil }
        if !tool.takesTextureDynamics { dynamics?.texture = nil }
        let handle = try BrushDynamics.handle(
            dynamics, size: brushSize, seed: BrushDynamics.strokeSeed(),
            pressureSize: brushPressureSize, pressureOpacity: brushPressureOpacity, tip: strokeTip)
        nativeStrokeSession?.dynamics = handle
        nativeStrokeSession?.dynamicsShaped = handle != nil && dynamics?.normalized?.shape != nil
        nativeStrokeSession?.texture = try handle.flatMap { _ in try strokeTexture(dynamics) }
    }

    /// Whether a byte stroke of this tool needs a dab session for its dynamics.
    var strokeWantsDynamics: Bool {
        tool.takesBrushDynamics && brushSize > 1 && brushDynamics?.normalized != nil
    }

    func strokePenSample(pressure: CGFloat, tilt: CGVector, rotation: CGFloat) -> PenSample {
        PenSample(pressure: strokePen.hasPressure ? Double(pressure) : .nan,
                  tiltX: Double(tilt.dx), tiltY: Double(tilt.dy), rotation: Double(rotation))
    }

    /// The nominal diameter a dynamics stroke's walk spaces by; nil without dynamics.
    func dynamicsDabDiameter(_ pressure: CGFloat, tilt: CGVector) -> CGFloat? {
        nativeStrokeSession?.nominalDiameter(pen: strokePenSample(pressure: pressure, tilt: tilt,
                                                                  rotation: .nan))
    }

    /// `stampSoftDab` for a dynamics stroke: one nominal stamp laid through the session,
    /// each dab it laid drawn at its recorded centre, diameter and (with a tip shape) angle
    /// and roundness, with the context alpha at its deposit factor (the dab image already
    /// carries the tip's flow); a textured stroke's each as the core's own image
    /// (`stampTexturedDab`). False, and nothing drawn, without dynamics.
    func stampDynamicsDabs(_ dab: CGImage, in context: CGContext, at point: CGPoint,
                           pressure: CGFloat, tilt: CGVector, rotation: CGFloat) -> Bool {
        // Read, not copied: a held copy of the session would copy its dabs on the stamp.
        guard nativeStrokeSession?.dynamics != nil else { return false }
        let shaped = nativeStrokeSession?.dynamicsShaped == true
        let textured = nativeStrokeSession?.texture != nil
        let pen = strokePenSample(pressure: pressure, tilt: tilt, rotation: rotation)
        let laid = nativeStrokeSession?.stamp(at: point, pen: pen) ?? []
        for resolved in laid {
            if textured {
                stampTexturedDab(resolved, in: context)
                continue
            }
            var tip = strokeTip
            if shaped {
                tip.angleDegrees = resolved.angleDegrees
                tip.roundness = resolved.roundness
            }
            context.saveGState()
            context.setAlpha(resolved.factor)
            SoftBrush.stamp(dab, in: context, at: resolved.point, diameter: resolved.diameter,
                            tip: tip)
            context.restoreGState()
        }
        return true
    }
}
