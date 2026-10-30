import Foundation

extension AgentServer {
    @inline(never)
    func sessionCatalog() -> [[String: Any]] {
        var catalog = snapshotCatalog
        catalog.append(contentsOf: historyCatalog)
        catalog.append(contentsOf: recoveryCatalog)
        return catalog
    }
}
