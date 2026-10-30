import AppKit

/// The stack tools for the agent:
///
/// | Handler | UI path it mirrors |
/// |---|---|
/// | `loadFilesIntoStack` | File ▸ Load Files into Stack… (`AppDelegate.loadFilesIntoStack`, AppDelegate+LoadFilesIntoStack.swift) |
/// | `autoAlignLayers` | Edit ▸ Auto-Align Layers… (`EditorViewController.commitAutoAlign`, EditorViewController+AutoAlign.swift) |
/// | `stackMode` | Layer ▸ Stack Mode ▸ … (`EditorViewController.stackMode`, EditorViewController+StackMode.swift) |
/// | `reduceNoiseFromStack` | Layer ▸ Reduce Noise from Stack… (`EditorViewController.commitReduceNoise`, EditorViewController+StackMode.swift) |
///
/// `load_files_into_stack` CREATES a document, so it is `nonRecordable` and
/// `documentFree` for `open_document`'s and `new_document`'s reasons: a replay
/// would pop a second window beside the run's target, and the handler resolves
/// no document. Unlike the menu, which sorts the chosen files by name, it
/// keeps the caller's `paths` order. `auto_align_layers`, `stack_mode` and
/// `reduce_noise_from_stack` are each one recordable edit: one history step,
/// one Action step.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let stacksHandlers: [String: ToolHandler] = [
        "load_files_into_stack": { $0.loadFilesIntoStack },
        "auto_align_layers": { $0.autoAlignLayers },
        "stack_mode": { $0.stackMode },
        "reduce_noise_from_stack": { $0.reduceNoiseFromStack },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let stacksToolFacts: [String: ToolFacts] = [
        // Creates a document and resolves none.
        "load_files_into_stack": [.nonRecordable, .documentFree],
        // The warp is the core's `perspective_set` at 16 bits and at
        // float (`geometry_perspective_set`, `DocumentPrecision.geometryFamily`);
        // in CMYK it registers on the layer's view and warps all four inks
        // (`precision_geometry_cmyk`, the CMYKA16 mesh arm).
        "auto_align_layers": [.typedTool, .floatTool, .cmykTool],
        // The statistic is the core's typed `stack_mode` at 16 bits, at
        // float and per ink in CMYK (`layer_stack_mode`, `DocumentPrecision`'s
        // family tables, `cmyk_stack_mode`).
        "stack_mode": [.typedTool, .floatTool, .cmykTool],
        // Auto-Align as above, then `stack_mode`'s statistic, per ink in CMYK.
        "reduce_noise_from_stack": [.typedTool, .floatTool, .cmykTool],
    ]

    /// Mirrors File ▸ Load Files into Stack…. The whole stack is planned and
    /// admitted before any file is decoded (`StackLoad.plan`), so a refusal
    /// opens nothing.
    func loadFilesIntoStack(_ a: [String: Any]) throws -> String {
        guard let raw = a["paths"] as? [Any] else {
            throw ToolError(
                message: "paths must be an array of at least \(StackLoad.minimumFiles) file paths.")
        }
        let urls = try raw.map { entry -> URL in
            guard let path = entry as? String, !path.isEmpty else {
                throw ToolError(message: "paths must hold file paths (non-empty strings).")
            }
            return URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
        }
        let loaded: (document: ImageDocument, outcome: StackLoad.Outcome)
        do {
            loaded = try ImageDocument.openStack(urls, name: stringArg(a, "name"))
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        let outcome = loaded.outcome
        var reply: [String: Any] = [
            "ok": true,
            "document": summary(loaded.document),
            "depth": outcome.document.isFloat ? "rgba32_float"
                : outcome.document.isHighDepth ? "rgba16" : "rgba8",
            "profile": outcome.profileName,
            "canvas": ["width": outcome.document.width, "height": outcome.document.height],
            "layers": stackLayerRows(outcome),
            "working_memory_plan_bytes": outcome.plan.bytes,
        ]
        if let notice = outcome.notice { reply["note"] = notice }
        return try jsonResult(reply)
    }

    /// Mirrors Edit ▸ Auto-Align Layers…: `AutoAlign.request`'s eligibility
    /// and reference rules, `AutoAlignWork`'s one job with the union canvas
    /// (Cylindrical and Spherical: the projected layers' union), then one
    /// grouped edit (Reveal All's canvas-geometry shape when the canvas
    /// changed). `projection` is the dialog's choice; the older
    /// `registration` (`auto` | `translation`, what recorded Actions hold)
    /// is still read when `projection` is absent. `vignette` and `distortion`
    /// are the dialog's Lens Correction ▸ Vignette Removal and Geometric
    /// Distortion (AutoAlignVignette.swift, AutoAlignDistortion.swift): each
    /// reprojects Auto and Perspective onto a sphere; Collage and Reposition
    /// ignore Vignette Removal and correct Geometric Distortion within each
    /// layer's own frame (AutoAlignPlanarLens.swift; `Registration.lens`,
    /// take3). A refusal — the empty reference and the too-big union among
    /// them — returns in-band and leaves no history step.
    func autoAlignLayers(_ a: [String: Any]) throws -> String {
        try refuseNullOrBooleanNumbers(a, ["reference"], in: "auto_align_layers")
        let document = try target(a)
        let layers = try layerIndices(a, document, minimum: 2)
        guard let source = document.doc else { throw ToolError(message: "No image loaded.") }
        var reference: Int?
        if a["reference"] != nil {
            guard let index = intArg(a, "reference") else {
                throw ToolError(message: "reference must be a layer index (an integer).")
            }
            reference = index
        }
        let registration = try alignProjection(a)
        if a["vignette"] != nil, boolArg(a, "vignette") == nil {
            throw ToolError(message: "vignette must be a boolean.")
        }
        let vignette = boolArg(a, "vignette") ?? false
        if a["distortion"] != nil, boolArg(a, "distortion") == nil {
            throw ToolError(message: "distortion must be a boolean.")
        }
        let distortion = boolArg(a, "distortion") ?? false
        let sampler: DrResizeFilter
        let samplerName = stringArg(a, "sampler") ?? "bicubic"
        if registration.reprojects, samplerName != "bicubic" {
            throw ToolError(
                message: "sampler must be bicubic with projection cylindrical or spherical.")
        }
        if vignette, AutoAlign.surface(for: registration, vignette: true) != nil,
           samplerName != "bicubic" {
            throw ToolError(message: "sampler must be bicubic with vignette true.")
        }
        if distortion, samplerName != "bicubic" {
            throw ToolError(message: "sampler must be bicubic with distortion true.")
        }
        switch samplerName {
        case "nearest": sampler = DR_FILTER_NEAREST
        case "bilinear": sampler = DR_FILTER_BILINEAR
        case "bicubic": sampler = DR_FILTER_CATMULL_ROM
        case "lanczos": sampler = DR_FILTER_LANCZOS3
        default: throw ToolError(message: "sampler must be nearest, bilinear, bicubic or lanczos.")
        }
        let result: (prepared: PreparedDocumentEdit?, outcome: AutoAlign.Outcome)
        do {
            let request = try AutoAlign.request(
                in: source, layers: layers, reference: reference, registration: registration,
                sampler: sampler, growsCanvas: true, vignette: vignette, distortion: distortion)
            result = try AutoAlignWork(source: source, request: request)
                .wait(in: document, title: AutoAlign.title)
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        let outcome = result.outcome
        if let prepared = result.prepared {
            document.preparedEdit = prepared
            let carried = outcome.canvas.flatMap { rect in
                document.selectionCarried(
                    canvasWidth: Int(rect.width), height: Int(rect.height),
                    originX: Int(-rect.minX), originY: Int(-rect.minY))
            }
            try performGroupedEdit(
                document, AutoAlign.title, changesCanvasGeometry: outcome.canvas != nil,
                resultingSelection: carried
            ) { _ in prepared.document }
        }
        var reply = autoAlignReplyFields(outcome)
        reply["ok"] = true
        reply["changed"] = result.prepared != nil
        reply["projection"] = registration.projectionName
        reply["working_memory_plan_bytes"] = outcome.planBytes
        reply["document"] = summary(document)
        return try jsonResult(reply)
    }

    /// `load_files_into_stack`'s `layers` rows, one per landed file,
    /// bottom-first; Merge Panorama's reply carries the same rows
    /// (AgentServer+MergePanorama.swift).
    func stackLayerRows(_ outcome: StackLoad.Outcome) -> [[String: Any]] {
        outcome.layers.map { layer -> [String: Any] in
            var fields: [String: Any] = [
                "index": layer.index, "name": layer.name, "path": layer.path,
                "width": layer.width, "height": layer.height,
                "source_depth": layer.sourceDepth,
                "converted_depth": layer.convertedDepth,
                "converted_profile": layer.convertedProfile,
            ]
            if !layer.disclosures.isEmpty { fields["disclosures"] = layer.disclosures }
            return fields
        }
    }

    /// What `auto_align_layers` reports of an alignment: the reference, the
    /// three lists, the registration's plan, and `focal`, `surface`,
    /// `vignette`, `distortion` and the grown `canvas` when present.
    /// Merge Panorama's `align` object is this (AgentServer+MergePanorama.swift).
    func autoAlignReplyFields(_ outcome: AutoAlign.Outcome) -> [String: Any] {
        var reply: [String: Any] = [
            "reference": outcome.reference,
            "aligned": outcome.aligned.map { layer -> [String: Any] in
                var row: [String: Any] = [
                    "layer": layer.layer, "name": layer.name, "method": layer.method.rawValue,
                    "homography": layer.homography.m,
                    "quad": layer.quad.map { [Double($0.x), Double($0.y)] },
                ]
                if let rotation = layer.rotation { row["rotation"] = rotation }
                return row
            },
            "unchanged": outcome.unchanged.map { ["layer": $0.layer, "name": $0.name] as [String: Any] },
            "unregistered": outcome.unregistered.map {
                ["layer": $0.layer, "name": $0.name, "reason": $0.reason] as [String: Any]
            },
            "registration_plan_bytes": outcome.registrationBytes,
        ]
        if let focal = outcome.focal { reply["focal"] = focal }
        if let surface = outcome.surface { reply["surface"] = surface.projectionName }
        if let fit = outcome.vignette {
            reply["vignette"] = [
                "coefficient": fit.coefficient, "gain": fit.gain, "samples": fit.samples,
                "layers": fit.layers,
            ] as [String: Any]
        }
        if let fit = outcome.distortion {
            reply["distortion"] = [
                "coefficient": fit.coefficient, "matches": fit.matches,
                "residual": fit.residual, "layers": fit.layers,
            ] as [String: Any]
        }
        if let rect = outcome.canvas {
            // Where the old canvas's top-left sits in the new one, as
            // `reveal_canvas` reports it.
            reply["canvas"] = [
                "width": Int(rect.width), "height": Int(rect.height),
                "origin": ["x": Int(-rect.minX), "y": Int(-rect.minY)],
            ] as [String: Any]
        }
        return reply
    }

    /// `auto_align_layers`' projection: `projection` (the dialog's six), or
    /// the older `registration` (`auto` | `translation`) when it is absent,
    /// else Auto.
    private func alignProjection(_ a: [String: Any]) throws -> AutoAlign.Registration {
        let accepted = "auto, perspective, collage, cylindrical, spherical or reposition"
        let projection = stringArg(a, "projection")
        let registration = stringArg(a, "registration")
        if projection != nil, registration != nil {
            throw ToolError(message: "Pass projection or registration, not both.")
        }
        if let projection {
            if let chosen = AutoAlign.Registration(projectionName: projection) { return chosen }
            throw ToolError(message: "projection must be \(accepted).")
        }
        switch registration ?? "auto" {
        case "auto": return .auto
        case "translation": return .translation
        default: throw ToolError(message: "registration must be auto or translation.")
        }
    }

    /// Mirrors Layer ▸ Stack Mode: `StackModes.members`' eligibility,
    /// `StackModeWork`'s one job, then one grouped edit; the new layer
    /// becomes the active one. A refusal returns in-band and leaves no
    /// history step.
    func stackMode(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let layers = try layerIndices(a, document, minimum: 2)
        let mode = try stackModeArgument(a, among: StackMode.allCases)
        let given = stringArg(a, "name") ?? ""
        let outcome = try runStack(
            document, layers: layers, mode: mode, name: given.isEmpty ? mode.title : given,
            title: StackModes.title, aligning: false)
        return try jsonResult([
            "ok": true,
            "layer": outcome.layer, "name": outcome.name, "mode": outcome.mode.rawValue,
            "layers": outcome.members, "hidden": outcome.hidden,
            "working_memory_plan_bytes": outcome.planBytes,
            "document": summary(document),
        ])
    }

    /// Mirrors Layer ▸ Reduce Noise from Stack…: `AutoAlign.request`'s
    /// eligibility and reference rules with the menu's registration and
    /// sampler, then `StackModeWork` with the alignment ahead of the
    /// statistic. A layer that did not align is listed in `excluded` and left
    /// untouched; fewer than two usable layers is a refusal naming them.
    func reduceNoiseFromStack(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let layers = try layerIndices(a, document, minimum: 2)
        let mode = try stackModeArgument(a, among: StackMode.noiseModes)
        let outcome = try runStack(
            document, layers: layers, mode: mode, name: StackModes.noiseLayerName,
            title: StackModes.noiseTitle, aligning: true)
        let align = outcome.align
        return try jsonResult([
            "ok": true,
            "layer": outcome.layer, "name": outcome.name, "mode": outcome.mode.rawValue,
            "reference": align?.reference ?? outcome.members.first ?? 0,
            "layers": outcome.members, "hidden": outcome.hidden,
            "aligned": (align?.aligned ?? []).map { layer -> [String: Any] in
                ["layer": layer.layer, "name": layer.name, "method": layer.method.rawValue]
            },
            "unchanged": (align?.unchanged ?? []).map {
                ["layer": $0.layer, "name": $0.name] as [String: Any]
            },
            "excluded": (align?.unregistered ?? []).map {
                ["layer": $0.layer, "name": $0.name, "reason": $0.reason] as [String: Any]
            },
            "registration_plan_bytes": align?.registrationBytes ?? 0,
            "working_memory_plan_bytes": outcome.planBytes,
            "document": summary(document),
        ])
    }

    /// The required `mode` argument, one of `allowed`; a refusal names what
    /// was given and what is accepted.
    private func stackModeArgument(_ a: [String: Any], among allowed: [StackMode]) throws
        -> StackMode
    {
        let accepted = allowed.map(\.rawValue).joined(separator: ", ")
        guard let name = stringArg(a, "mode") else {
            throw ToolError(message: "mode is required: one of \(accepted).")
        }
        guard let mode = StackMode(rawValue: name), allowed.contains(mode) else {
            throw ToolError(message: "mode “\(name)” is not accepted: use one of \(accepted).")
        }
        return mode
    }

    /// Both stack tools' one path: eligibility, the job, one grouped edit,
    /// the new layer active. Every refusal is a `ToolError`.
    private func runStack(_ document: ImageDocument, layers: [Int], mode: StackMode, name: String,
                          title: String, aligning: Bool) throws -> StackModes.Outcome {
        guard let source = document.doc else { throw ToolError(message: "No image loaded.") }
        let result: (prepared: PreparedDocumentEdit, outcome: StackModes.Outcome)
        do {
            let members = try StackModes.members(in: source, layers: layers, title: title)
            let align = aligning
                ? try AutoAlign.request(in: source, layers: members, reference: nil,
                                        registration: .auto, sampler: DR_FILTER_CATMULL_ROM)
                : nil
            result = try StackModeWork(source: source, layers: members, mode: mode, name: name,
                                       align: align)
                .wait(in: document, title: title)
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        document.preparedEdit = result.prepared
        try performGroupedEdit(document, title) { _ in result.prepared.document }
        retarget(document, .single(result.outcome.layer))
        return result.outcome
    }
}
