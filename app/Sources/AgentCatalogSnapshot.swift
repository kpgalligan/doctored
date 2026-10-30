import Foundation

/// `-DRWriteCatalog <path>`: the build's "Write Agent Catalog" phase
/// runs the just-linked app with this flag, and the app writes
/// `{"version": <CFBundleShortVersionString>, "tools": [...]}` — the
/// `tools/list` catalog it would serve — to `<path>` and exits before any
/// AppKit setup. The bundled `doctored-mcp` answers `tools/list` from that
/// file (`Contents/Resources/agent-catalog.json`) while no Doctored answers
/// the socket.
///
/// The catalog goes through the same `enforceCatalogParity` as a launch, so a
/// build whose catalog disagrees with its dispatch tables fails at this phase
/// (exit 1, one `[AgentServer]` line). A write failure is one stderr line and
/// exit 1. Without the flag this does nothing.
enum AgentCatalogSnapshot {
    static let flag = "-DRWriteCatalog"

    /// main.swift's first statement.
    static func writeIfAsked() {
        let arguments = CommandLine.arguments
        guard let index = arguments.firstIndex(of: flag) else { return }
        guard index + 1 < arguments.count else {
            fail("\(flag) needs a path")
        }
        let path = arguments[index + 1]
        do {
            let catalog = try AgentServer.shared.catalogJSON()
            AgentServer.enforceCatalogParity(catalog)
            let tools = try JSONSerialization.jsonObject(with: Data(catalog.utf8))
            let version =
                Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
                ?? "0.0"
            let data = try JSONSerialization.data(
                withJSONObject: ["version": version, "tools": tools], options: [.sortedKeys])
            try data.write(to: URL(fileURLWithPath: path), options: .atomic)
        } catch {
            fail("could not write the agent catalog to \(path): \(error.localizedDescription)")
        }
        exit(EXIT_SUCCESS)
    }

    private static func fail(_ sentence: String) -> Never {
        FileHandle.standardError.write(Data("[AgentCatalogSnapshot] \(sentence)\n".utf8))
        exit(EXIT_FAILURE)
    }
}
