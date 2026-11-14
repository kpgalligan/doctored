import Foundation

/// The dispatch registry: every file that defines MCP tool handlers declares
/// a `static let <feature>Handlers` table beside them (AgentServer.swift's
/// private ones in `coreHandlers`), and this file merges them. A new tool's
/// entry goes in the table of the file that declares its handler; a new
/// handler file adds its table to `handlerTables` once. A tool's facts
/// (recording, selection, depth admission) go in its file's
/// `<prefix>ToolFacts` table, listed once in `factTables` at the same
/// position as the file's handler table in `handlerTables`.
extension AgentServer {
    /// A handler as an unbound method reference, so a table — which lives as
    /// long as the type — holds no reference back to the singleton.
    typealias ToolHandler = (AgentServer) -> ([String: Any]) throws -> String

    /// Every handler table, named so the parity check can say where a
    /// duplicate key was declared. `coreHandlers` first, then file-name order.
    static let handlerTables: [(name: String, table: [String: ToolHandler])] = [
        ("coreHandlers", coreHandlers),
        ("actionAssetsHandlers", actionAssetsHandlers),
        ("actionsHandlers", actionsHandlers),
        ("adjustmentPresetsHandlers", adjustmentPresetsHandlers),
        ("adjustmentsHandlers", adjustmentsHandlers),
        ("alignDistributeHandlers", alignDistributeHandlers),
        ("assistedAnnotationHandlers", assistedAnnotationHandlers),
        ("batchHandlers", batchHandlers),
        ("brushPresetsHandlers", brushPresetsHandlers),
        ("channelsHandlers", channelsHandlers),
        ("clipboardHandlers", clipboardHandlers),
        ("colorHandlers", colorHandlers),
        ("colorSamplersHandlers", colorSamplersHandlers),
        ("contentAwareFillHandlers", contentAwareFillHandlers),
        ("contentAwareToolsHandlers", contentAwareToolsHandlers),
        ("depthBlurHandlers", depthBlurHandlers),
        ("neuralFiltersHandlers", neuralFiltersHandlers),
        ("distortHandlers", distortHandlers),
        ("documentGeometryHandlers", documentGeometryHandlers),
        ("duplicateToNewDocumentHandlers", duplicateToNewDocumentHandlers),
        ("evaluationHandlers", evaluationHandlers),
        ("equalizeHandlers", equalizeHandlers),
        ("exportHandlers", exportHandlers),
        ("fadeHandlers", fadeHandlers),
        ("fileInfoHandlers", fileInfoHandlers),
        ("fillLayersHandlers", fillLayersHandlers),
        ("fillStrokeHandlers", fillStrokeHandlers),
        ("floatDocumentHandlers", floatDocumentHandlers),
        ("formatAdditionsHandlers", formatAdditionsHandlers),
        ("generativeFillHandlers", generativeFillHandlers),
        ("geometrySetsHandlers", geometrySetsHandlers),
        ("groupsHandlers", groupsHandlers),
        ("guidesHandlers", guidesHandlers),
        ("healHandlers", healHandlers),
        ("historyHandlers", historyHandlers),
        ("horizonHandlers", horizonHandlers),
        ("independentMasksHandlers", independentMasksHandlers),
        ("infoHandlers", infoHandlers),
        ("jobsHandlers", jobsHandlers),
        ("layerCompsHandlers", layerCompsHandlers),
        ("layerLabelsHandlers", layerLabelsHandlers),
        ("layerPropertiesHandlers", layerPropertiesHandlers),
        ("layerSetsHandlers", layerSetsHandlers),
        ("layerStyleHandlers", layerStyleHandlers),
        ("lensProfilesHandlers", lensProfilesHandlers),
        ("liquifyHandlers", liquifyHandlers),
        ("livePhotoHandlers", livePhotoHandlers),
        ("modelSelectionHandlers", modelSelectionHandlers),
        ("moveLayersHandlers", moveLayersHandlers),
        ("moveSelectedHandlers", moveSelectedHandlers),
        ("newDocumentHandlers", newDocumentHandlers),
        ("objectSelectionHandlers", objectSelectionHandlers),
        ("patchHandlers", patchHandlers),
        ("pathCombineHandlers", pathCombineHandlers),
        ("pathsHandlers", pathsHandlers),
        ("patternsHandlers", patternsHandlers),
        ("persistedWarpHandlers", persistedWarpHandlers),
        ("perspectiveWarpHandlers", perspectiveWarpHandlers),
        ("photoshopPresetsHandlers", photoshopPresetsHandlers),
        ("pixelArtHandlers", pixelArtHandlers),
        ("precisionHandlers", precisionHandlers),
        ("productionOutputsHandlers", productionOutputsHandlers),
        ("proofPresetsHandlers", proofPresetsHandlers),
        ("psdHandlers", psdHandlers),
        ("psdExportHandlers", psdExportHandlers),
        ("puppetWarpHandlers", puppetWarpHandlers),
        ("rawDevelopmentHandlers", rawDevelopmentHandlers),
        ("rawLocalsHandlers", rawLocalsHandlers),
        ("rawPresetsHandlers", rawPresetsHandlers),
        ("rawSourceHandlers", rawSourceHandlers),
        ("recoveryHandlers", recoveryHandlers),
        ("redEyeHandlers", redEyeHandlers),
        ("retainedImageHandlers", retainedImageHandlers),
        ("retouchBrushHandlers", retouchBrushHandlers),
        ("retouchHandlers", retouchHandlers),
        ("rotateCanvasHandlers", rotateCanvasHandlers),
        ("selectionEssentialsHandlers", selectionEssentialsHandlers),
        ("selectionHistoryHandlers", selectionHistoryHandlers),
        ("selectionRefinementHandlers", selectionRefinementHandlers),
        ("selectSimilarHandlers", selectSimilarHandlers),
        ("shapesHandlers", shapesHandlers),
        ("skyReplacementHandlers", skyReplacementHandlers),
        ("snapshotsHandlers", snapshotsHandlers),
        ("softProofHandlers", softProofHandlers),
        ("subjectSelectionHandlers", subjectSelectionHandlers),
        ("subjectTargetsHandlers", subjectTargetsHandlers),
        ("textHandlers", textHandlers),
        ("transformAgainHandlers", transformAgainHandlers),
        ("transformSelectedHandlers", transformSelectedHandlers),
        ("viewsHandlers", viewsHandlers),
        ("workspaceHandlers", workspaceHandlers),
        ("artworkRasterizationHandlers", artworkRasterizationHandlers),
        ("precisionModeConversionHandlers", precisionModeConversionHandlers),
        ("scalarMorphologyHandlers", scalarMorphologyHandlers),
        ("canvasFramingHandlers", canvasFramingHandlers),
        ("layerDragHandlers", layerDragHandlers),
        ("stacksHandlers", stacksHandlers),
        ("mergeHDRHandlers", mergeHDRHandlers),
        ("autoBlendHandlers", autoBlendHandlers),
        ("facePartsHandlers", facePartsHandlers),
        ("cmykModeHandlers", cmykModeHandlers),
        ("cmykColourHandlers", cmykColourHandlers),
        ("stampMergeGroupHandlers", stampMergeGroupHandlers),
        ("stylePresetsHandlers", stylePresetsHandlers),
        ("mergePanoramaHandlers", mergePanoramaHandlers),
        ("hdrToningHandlers", hdrToningHandlers),
        ("indexedColorTableHandlers", indexedColorTableHandlers),
        ("duotoneHandlers", duotoneHandlers),
        ("spotChannelsHandlers", spotChannelsHandlers),
        ("missingFontsHandlers", missingFontsHandlers),
    ]

    /// Tool name → handler: the union of `handlerTables` (the first table
    /// wins a repeated key; the parity check refuses one). The single source
    /// of truth for what the server dispatches: catalogJSON()
    /// (AgentCatalog.swift) must describe exactly these names, and start()
    /// enforces it through `enforceCatalogParity`. Internal, not private:
    /// `Action.isKnownStepTool` reads these keys to refuse a recorded step
    /// that names no tool.
    static let handlers: [String: ToolHandler] = {
        var merged: [String: ToolHandler] = [:]
        for (_, table) in handlerTables {
            merged.merge(table) { first, _ in first }
        }
        return merged
    }()

    /// What the recorder, Repeat Last, the Actions window and the depth
    /// gates know about a tool. Each fact is membership of one set; the sets
    /// are built from the merged `toolFacts` through `tools(with:)`.
    struct ToolFacts: OptionSet {
        let rawValue: UInt16

        /// In `ActionCatalogFacts.repeatableTools`.
        static let repeatable = ToolFacts(rawValue: 1 << 0)
        /// In `ActionCatalogFacts.nonRecordable`.
        static let nonRecordable = ToolFacts(rawValue: 1 << 1)
        /// In `ActionCatalogFacts.selectionSensitive`.
        static let selectionSensitive = ToolFacts(rawValue: 1 << 2)
        /// In `DocumentPrecision.documentFreeTools`.
        static let documentFree = ToolFacts(rawValue: 1 << 3)
        /// In `DocumentPrecision.commonTools`.
        static let commonTool = ToolFacts(rawValue: 1 << 4)
        /// In `DocumentPrecision.typedTools`.
        static let typedTool = ToolFacts(rawValue: 1 << 5)
        /// In `DocumentPrecision.rasterTools`.
        static let rasterTool = ToolFacts(rawValue: 1 << 6)
        /// In `FloatDocument.tools`.
        static let floatTool = ToolFacts(rawValue: 1 << 7)
        /// In `CmykDocument.tools` (#1): answered in a CMYK document.
        static let cmykTool = ToolFacts(rawValue: 1 << 8)
    }

    /// Every tool facts table, index-paired with `handlerTables`: entry `i`
    /// is the facts table of the file whose handler table is entry `i`
    /// there. The pairing is checked at start (`toolFactsFailure`).
    static let factTables: [(name: String, table: [String: ToolFacts])] = [
        ("coreToolFacts", coreToolFacts),
        ("actionAssetsToolFacts", actionAssetsToolFacts),
        ("actionsToolFacts", actionsToolFacts),
        ("adjustmentPresetsToolFacts", adjustmentPresetsToolFacts),
        ("adjustmentsToolFacts", adjustmentsToolFacts),
        ("alignDistributeToolFacts", alignDistributeToolFacts),
        ("assistedAnnotationToolFacts", assistedAnnotationToolFacts),
        ("batchToolFacts", batchToolFacts),
        ("brushPresetsToolFacts", brushPresetsToolFacts),
        ("channelsToolFacts", channelsToolFacts),
        ("clipboardToolFacts", clipboardToolFacts),
        ("colorToolFacts", colorToolFacts),
        ("colorSamplersToolFacts", colorSamplersToolFacts),
        ("contentAwareFillToolFacts", contentAwareFillToolFacts),
        ("contentAwareToolsToolFacts", contentAwareToolsToolFacts),
        ("depthBlurToolFacts", depthBlurToolFacts),
        ("neuralFiltersToolFacts", neuralFiltersToolFacts),
        ("distortToolFacts", distortToolFacts),
        ("documentGeometryToolFacts", documentGeometryToolFacts),
        ("duplicateToNewDocumentToolFacts", duplicateToNewDocumentToolFacts),
        ("evaluationToolFacts", evaluationToolFacts),
        ("equalizeToolFacts", equalizeToolFacts),
        ("exportToolFacts", exportToolFacts),
        ("fadeToolFacts", fadeToolFacts),
        ("fileInfoToolFacts", fileInfoToolFacts),
        ("fillLayersToolFacts", fillLayersToolFacts),
        ("fillStrokeToolFacts", fillStrokeToolFacts),
        ("floatDocumentToolFacts", floatDocumentToolFacts),
        ("formatAdditionsToolFacts", formatAdditionsToolFacts),
        ("generativeFillToolFacts", generativeFillToolFacts),
        ("geometrySetsToolFacts", geometrySetsToolFacts),
        ("groupsToolFacts", groupsToolFacts),
        ("guidesToolFacts", guidesToolFacts),
        ("healToolFacts", healToolFacts),
        ("historyToolFacts", historyToolFacts),
        ("horizonToolFacts", horizonToolFacts),
        ("independentMasksToolFacts", independentMasksToolFacts),
        ("infoToolFacts", infoToolFacts),
        ("jobsToolFacts", jobsToolFacts),
        ("layerCompsToolFacts", layerCompsToolFacts),
        ("layerLabelsToolFacts", layerLabelsToolFacts),
        ("layerPropertiesToolFacts", layerPropertiesToolFacts),
        ("layerSetsToolFacts", layerSetsToolFacts),
        ("layerStyleToolFacts", layerStyleToolFacts),
        ("lensProfilesToolFacts", lensProfilesToolFacts),
        ("liquifyToolFacts", liquifyToolFacts),
        ("livePhotoToolFacts", livePhotoToolFacts),
        ("modelSelectionToolFacts", modelSelectionToolFacts),
        ("moveLayersToolFacts", moveLayersToolFacts),
        ("moveSelectedToolFacts", moveSelectedToolFacts),
        ("newDocumentToolFacts", newDocumentToolFacts),
        ("objectSelectionToolFacts", objectSelectionToolFacts),
        ("patchToolFacts", patchToolFacts),
        ("pathCombineToolFacts", pathCombineToolFacts),
        ("pathsToolFacts", pathsToolFacts),
        ("patternsToolFacts", patternsToolFacts),
        ("persistedWarpToolFacts", persistedWarpToolFacts),
        ("perspectiveWarpToolFacts", perspectiveWarpToolFacts),
        ("photoshopPresetsToolFacts", photoshopPresetsToolFacts),
        ("pixelArtToolFacts", pixelArtToolFacts),
        ("precisionToolFacts", precisionToolFacts),
        ("productionOutputsToolFacts", productionOutputsToolFacts),
        ("proofPresetsToolFacts", proofPresetsToolFacts),
        ("psdToolFacts", psdToolFacts),
        ("psdExportToolFacts", psdExportToolFacts),
        ("puppetWarpToolFacts", puppetWarpToolFacts),
        ("rawDevelopmentToolFacts", rawDevelopmentToolFacts),
        ("rawLocalsToolFacts", rawLocalsToolFacts),
        ("rawPresetsToolFacts", rawPresetsToolFacts),
        ("rawSourceToolFacts", rawSourceToolFacts),
        ("recoveryToolFacts", recoveryToolFacts),
        ("redEyeToolFacts", redEyeToolFacts),
        ("retainedImageToolFacts", retainedImageToolFacts),
        ("retouchBrushToolFacts", retouchBrushToolFacts),
        ("retouchToolFacts", retouchToolFacts),
        ("rotateCanvasToolFacts", rotateCanvasToolFacts),
        ("selectionEssentialsToolFacts", selectionEssentialsToolFacts),
        ("selectionHistoryToolFacts", selectionHistoryToolFacts),
        ("selectionRefinementToolFacts", selectionRefinementToolFacts),
        ("selectSimilarToolFacts", selectSimilarToolFacts),
        ("shapesToolFacts", shapesToolFacts),
        ("skyReplacementToolFacts", skyReplacementToolFacts),
        ("snapshotsToolFacts", snapshotsToolFacts),
        ("softProofToolFacts", softProofToolFacts),
        ("subjectSelectionToolFacts", subjectSelectionToolFacts),
        ("subjectTargetsToolFacts", subjectTargetsToolFacts),
        ("textToolFacts", textToolFacts),
        ("transformAgainToolFacts", transformAgainToolFacts),
        ("transformSelectedToolFacts", transformSelectedToolFacts),
        ("viewsToolFacts", viewsToolFacts),
        ("workspaceToolFacts", workspaceToolFacts),
        ("artworkRasterizationToolFacts", artworkRasterizationToolFacts),
        ("precisionModeConversionToolFacts", precisionModeConversionToolFacts),
        ("scalarMorphologyToolFacts", scalarMorphologyToolFacts),
        ("canvasFramingToolFacts", canvasFramingToolFacts),
        ("layerDragToolFacts", layerDragToolFacts),
        ("stacksToolFacts", stacksToolFacts),
        ("mergeHDRToolFacts", mergeHDRToolFacts),
        ("autoBlendToolFacts", autoBlendToolFacts),
        ("facePartsToolFacts", facePartsToolFacts),
        ("cmykModeToolFacts", cmykModeToolFacts),
        ("cmykColourToolFacts", cmykColourToolFacts),
        ("stampMergeGroupToolFacts", stampMergeGroupToolFacts),
        ("stylePresetsToolFacts", stylePresetsToolFacts),
        ("mergePanoramaToolFacts", mergePanoramaToolFacts),
        ("hdrToningToolFacts", hdrToningToolFacts),
        ("indexedColorTableToolFacts", indexedColorTableToolFacts),
        ("duotoneToolFacts", duotoneToolFacts),
        ("spotChannelsToolFacts", spotChannelsToolFacts),
        ("missingFontsToolFacts", missingFontsToolFacts),
    ]

    /// Tool name → facts: the union of `factTables` (the first table wins a
    /// repeated key; the start check refuses one).
    static let toolFacts: [String: ToolFacts] = {
        var merged: [String: ToolFacts] = [:]
        for (_, table) in factTables {
            merged.merge(table) { first, _ in first }
        }
        return merged
    }()

    /// The tools whose facts contain `fact`: how each of the ten sets is built.
    static func tools(with fact: ToolFacts) -> Set<String> {
        Set(toolFacts.filter { $0.value.contains(fact) }.keys)
    }

    /// Nothing else forces the catalog and the dispatch tables to agree, so
    /// the server refuses to start on a mismatch, in Debug and Release: one
    /// `[AgentServer]` line on stderr and exit status 1, which a background
    /// test instance surfaces as an exit rather than a hang. A tool facts
    /// table that `toolFactsFailure` refuses fails the same way, on the same
    /// line.
    static func enforceCatalogParity(_ catalog: String) {
        let failures = [
            catalogParityFailure(catalog), toolFactsFailure(),
            AgentPathGuard.catalogFailure(catalog),
        ].compactMap { $0 }
        guard !failures.isEmpty else { return }
        FileHandle.standardError.write(
            Data("[AgentServer] catalog parity failure: \(failures.joined(separator: "; "))\n".utf8))
        exit(EXIT_FAILURE)
    }

    /// The enforcement behind the facts tables' contract: nil when
    /// `factTables` pairs with `handlerTables`, every facts entry sits in its
    /// handler's table, no tool has facts in two tables and no tool breaks a
    /// set rule (`documentFreeTools` is disjoint from `commonTools`,
    /// `typedTools` and `FloatDocument.tools`; `rasterTools` is asked
    /// only after `commonTools ∪ typedTools` admits), else a message.
    private static func toolFactsFailure() -> String? {
        var clauses: [String] = []

        var unpaired: [String] = []
        for i in 0..<max(handlerTables.count, factTables.count) {
            var expected = "none"
            if i < handlerTables.count {
                let name = handlerTables[i].name
                expected = name.hasSuffix("Handlers")
                    ? String(name.dropLast("Handlers".count)) + "ToolFacts" : name
            }
            let found = i < factTables.count ? factTables[i].name : "none"
            if found != expected {
                unpaired.append("position \(i): expected \(expected), found \(found)")
            }
        }
        if !unpaired.isEmpty {
            clauses.append("tool facts tables do not pair with handlerTables: "
                + unpaired.joined(separator: ", "))
        }

        var outside: [(tool: String, index: Int)] = []
        for i in 0..<min(handlerTables.count, factTables.count) {
            for key in factTables[i].table.keys where handlerTables[i].table[key] == nil {
                outside.append((key, i))
            }
        }
        if !outside.isEmpty {
            clauses.append("tool facts outside their handler's table: "
                + outside.sorted { ($0.tool, $0.index) < ($1.tool, $1.index) }
                .map { "\($0.tool) (\(factTables[$0.index].name))" }
                .joined(separator: ", "))
        }

        var owners: [String: [String]] = [:]
        for (name, table) in factTables {
            for key in table.keys { owners[key, default: []].append(name) }
        }
        let duplicates = owners.filter { $0.value.count > 1 }.sorted { $0.key < $1.key }
        if !duplicates.isEmpty {
            clauses.append("tool facts declared in more than one table: "
                + duplicates.map { "\($0.key) (\($0.value.joined(separator: ", ")))" }
                .joined(separator: ", "))
        }

        var broken: [String] = []
        for (tool, facts) in toolFacts.sorted(by: { $0.key < $1.key }) {
            if facts.contains(.documentFree)
                && !facts.isDisjoint(with: [.commonTool, .typedTool, .floatTool]) {
                broken.append("\(tool) (documentFree with commonTool, typedTool or floatTool)")
            }
            if facts.contains(.rasterTool) && facts.isDisjoint(with: [.commonTool, .typedTool]) {
                broken.append("\(tool) (rasterTool without commonTool or typedTool)")
            }
        }
        if !broken.isEmpty {
            clauses.append("tool facts break a set rule: " + broken.joined(separator: ", "))
        }

        // #1: the Grayscale gate names its tools and its permanently refused tools; each must
        // be a registered tool.
        let unregistered = GrayDocument.tools.subtracting(handlers.keys).sorted()
        if !unregistered.isEmpty {
            clauses.append("GrayDocument.tools not registered: " + unregistered.joined(separator: ", "))
        }
        let refused = GrayDocument.rawTools.union(GrayDocument.byteTools)
            .union(GrayDocument.evaluationTools).subtracting(handlers.keys).sorted()
        if !refused.isEmpty {
            clauses.append("GrayDocument's refused tools not registered: "
                + refused.joined(separator: ", "))
        }
        // #1 Lab phase 1: the Lab Color gate's tools and later phases, by the same rule.
        let labUnregistered = LabDocument.tools.subtracting(handlers.keys).sorted()
        if !labUnregistered.isEmpty {
            clauses.append("LabDocument.tools not registered: " + labUnregistered.joined(separator: ", "))
        }
        let labRefused = LabDocument.rawTools.union(LabDocument.byteTools)
            .union(LabDocument.evaluationTools).union(LabDocument.toneMapTools)
            .union(LabDocument.disabledTools).subtracting(handlers.keys).sorted()
        if !labRefused.isEmpty {
            clauses.append("LabDocument's refused tools not registered: "
                + labRefused.joined(separator: ", "))
        }
        // #1 Indexed phase 1a: the Indexed Color gate's tools and its disabled tools (and
        // since 3a its one-layer tools), by the same rule.
        let indexedNamed = IndexedDocument.tools.union(IndexedDocument.disabledTools)
            .union(IndexedDocument.oneLayer)
        let indexedUnregistered = indexedNamed.subtracting(handlers.keys).sorted()
        if !indexedUnregistered.isEmpty {
            clauses.append("IndexedDocument's tools not registered: "
                + indexedUnregistered.joined(separator: ", "))
        }
        // #1 Bitmap phase 1a: the Bitmap gate's tools and disabled tools, by the same
        // rule (phase 2a-ii ended the later-phase list; phase 3 added the one-layer and
        // alpha-channel lists).
        let bitmapNamed = BitmapDocument.tools.union(BitmapDocument.disabledTools)
            .union(BitmapDocument.oneLayer).union(BitmapDocument.channelTools)
        let bitmapUnregistered = bitmapNamed.subtracting(handlers.keys).sorted()
        if !bitmapUnregistered.isEmpty {
            clauses.append("BitmapDocument's tools not registered: "
                + bitmapUnregistered.joined(separator: ", "))
        }

        return clauses.isEmpty ? nil : clauses.joined(separator: "; ")
    }

    /// The enforcement behind the tables' contract: nil when catalogJSON()
    /// and `handlers` name exactly the same tools and no tool is declared in
    /// more than one table, else a message listing the difference.
    private static func catalogParityFailure(_ catalog: String) -> String? {
        guard
            let tools = (try? JSONSerialization.jsonObject(with: Data(catalog.utf8)))
                as? [[String: Any]]
        else { return "the tool catalog did not parse as an array of tool objects" }
        var owners: [String: [String]] = [:]
        for (name, table) in handlerTables {
            for key in table.keys { owners[key, default: []].append(name) }
        }
        let duplicates = owners.filter { $0.value.count > 1 }.sorted { $0.key < $1.key }
        let catalogNames = Set(tools.compactMap { $0["name"] as? String })
        let handlerNames = Set(handlers.keys)
        guard catalogNames != handlerNames || !duplicates.isEmpty else { return nil }
        let missing = catalogNames.subtracting(handlerNames).sorted()
        let extra = handlerNames.subtracting(catalogNames).sorted()
        var message =
            "agent tool catalog and dispatch table disagree — in catalog but not "
            + "dispatch: \(missing); in dispatch but not catalog: \(extra)"
        if !duplicates.isEmpty {
            message += "; declared in more than one table: "
                + duplicates.map { "\($0.key) (\($0.value.joined(separator: ", ")))" }
                .joined(separator: ", ")
        }
        return message
    }
}
