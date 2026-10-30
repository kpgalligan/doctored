import Foundation

extension AgentServer {
    @inline(never)
    func advancedSelectionCatalog() -> [[String: Any]] {
        var catalog = selectionEssentialsCatalog
        catalog.append(contentsOf: independentMaskCatalog)
        catalog.append(contentsOf: subjectTargetingCatalog)
        catalog.append(contentsOf: facePartsCatalog)
        catalog.append(contentsOf: objectSelectionCatalog)
        catalog.append(contentsOf: selectionRefinementCatalog)
        catalog.append(contentsOf: selectSimilarCatalog)
        return catalog
    }
}
