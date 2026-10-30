import Foundation

/// The agent server's Unix socket (`designs/agent-socket.md`): where it
/// is bound, the bind, its release at quit, and the line Settings ▸ MCP copies.
/// The core's listener is the app's only one; it serves the catalog through
/// `agentToolTrampoline`, so every call reaches main through `AgentServeGate`.
///
/// **Bound at every launch, and never an alert.** No person asked for it, so a
/// launch that cannot bind logs one line, `Doctored agent socket not bound:
/// <reason>`, and carries on; a bound one logs `Doctored agent socket
/// listening at <path>`.
///
/// **The path** (`socketPath`) is `DR_AGENT_SOCKET` when it is set and not
/// empty, else Application Support's `Doctored/Agent/agent.sock`. No test
/// binds the person's socket, so every test launch sets `DR_AGENT_SOCKET`.
///
/// **Who binds** is the core's claim policy (`agent::prepare_socket_path`,
/// tested in `core/tests/agent_tests.rs`): the folder is made `0700` and must
/// be a real folder of this user's with no group or world bits; a socket that
/// answers is another instance's and is left alone, one that refuses is a
/// crash's leftover and is replaced, and a file that is not a socket is left
/// alone. The refusal arrives as the bind's error and becomes the log line.
///
/// **The quit** (`unbindSocket`) is a release, not a stop: the core unlinks the
/// file only while it is still the one this listener bound and abandons the
/// listener without a join, so a call in flight that waits on main cannot hold
/// the quit. `AgentServer.shared` is the context and lives for the process,
/// which the release's contract needs.
extension AgentServer {
    static let socketEnvironmentKey = "DR_AGENT_SOCKET"

    /// `~/Library/Application Support/Doctored/Agent/agent.sock` — the path
    /// only; creates nothing.
    static var defaultSocketPath: String? {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first?
            .appendingPathComponent("Doctored/Agent/agent.sock").path
    }

    /// Where this launch binds, or nil when it has nowhere to. Pure.
    static func socketPath(environment: [String: String], defaultPath: String?) -> String? {
        if let path = environment[socketEnvironmentKey], !path.isEmpty { return path }
        return defaultPath
    }

    /// Binds the socket, or logs why not. Main only, from
    /// `applicationDidFinishLaunching`; never throws, never alerts. A catalog
    /// that disagrees with the dispatch tables exits the app first
    /// (`enforceCatalogParity`).
    func bindSocket() {
        assert(Thread.isMainThread)
        guard boundSocketPath == nil else { return }
        guard let path = Self.socketPath(
            environment: ProcessInfo.processInfo.environment, defaultPath: Self.defaultSocketPath)
        else {
            Self.logSocketNotBound("no Application Support folder")
            return
        }
        do {
            let catalog = try catalogJSON()
            Self.enforceCatalogParity(catalog)
            let version =
                Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
                ?? "0.0"
            try RasterAgentServer.bindSocket(
                path: path, name: "doctored", version: version, catalog: catalog,
                handler: agentToolTrampoline, context: Unmanaged.passUnretained(self).toOpaque())
        } catch {
            Self.logSocketNotBound(error.localizedDescription)
            return
        }
        boundSocketPath = path
        NSLog("Doctored agent socket listening at %@", path)
    }

    /// For `applicationWillTerminate`: unlinks the socket only if it is still
    /// this instance's, with no join. Safe when none is bound.
    func unbindSocket() {
        RasterAgentServer.releaseSocket()
        boundSocketPath = nil
    }

    /// The bundled stdio helper, inside this app's own bundle wherever it was
    /// launched from.
    static var mcpHelperURL: URL {
        Bundle.main.bundleURL.appendingPathComponent("Contents/Helpers/doctored-mcp")
    }

    /// The line Settings ▸ MCP copies: Claude Code's `mcp add` for the helper
    /// at `helperPath`, single-quoted for a POSIX shell.
    static func mcpCommand(helperPath: String) -> String {
        let quoted = helperPath.replacingOccurrences(of: "'", with: "'\\''")
        return "claude mcp add --scope user doctored -- '\(quoted)'"
    }

    private static func logSocketNotBound(_ reason: String) {
        NSLog("Doctored agent socket not bound: %@", reason)
    }
}
