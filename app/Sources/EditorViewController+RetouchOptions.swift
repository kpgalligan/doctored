import AppKit

extension EditorViewController {
    static func retouchGeometryDescriptors(tool: EditorTool, read: @escaping () -> PaintToolOptions,
                                           write: @escaping (PaintToolOptions) -> Void) -> [OptionDescriptor] {
        var controls = [OptionDescriptor(id: "paint.expand", overflowLabel: "Expand destination",
            kind: .checkbox(label: "Expand destination", get: { read().expandDestination == true },
                set: { value in var options = read(); options.expandDestination = value; write(options) }))]
        guard tool == .clone || tool == .heal else { return controls }
        controls += [
            OptionDescriptor(id: "paint.sourceOverlay", overflowLabel: "Source overlay",
                kind: .checkbox(label: "Source overlay", get: { read().sourceOverlay != false },
                    set: { value in var options = read(); options.sourceOverlay = value; write(options) })),
            OptionDescriptor(id: "paint.sourceScale", microLabel: "Source", overflowLabel: "Source scale",
                kind: .field(width: 52, unit: "%", decimals: 1, min: 10, max: 1000, quick: [50, 100, 200],
                    get: { read().sourceScale ?? 100 },
                    set: { value in var options = read(); options.sourceScale = value; write(options) })),
            OptionDescriptor(id: "paint.sourceRotation", overflowLabel: "Source rotation",
                kind: .field(width: 52, unit: "°", decimals: 1, min: -180, max: 180, quick: [-90, 0, 90],
                    get: { read().sourceRotation ?? 0 },
                    set: { value in var options = read(); options.sourceRotation = value; write(options) })),
            OptionDescriptor(id: "paint.sourceFlipX", overflowLabel: "Flip source horizontally",
                kind: .checkbox(label: "Flip source H", get: { read().sourceFlipX == true },
                    set: { value in var options = read(); options.sourceFlipX = value; write(options) })),
            OptionDescriptor(id: "paint.sourceFlipY", overflowLabel: "Flip source vertically",
                kind: .checkbox(label: "Flip source V", get: { read().sourceFlipY == true },
                    set: { value in var options = read(); options.sourceFlipY = value; write(options) }))
        ]
        return controls
    }
}
