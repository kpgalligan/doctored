import Foundation

// The embedded MCP server's socket: `AgentServer.bindSocket`/`unbindSocket` and its
// tool trampoline reach the core only through `RasterAgentServer`, so `dr_*` calls stay in the
// `DoctoredCore*.swift` files.

/// The core's embedded MCP server on its Unix socket: bind, release, and the
/// library-owned result strings the tool handler returns.
enum RasterAgentServer {
    /// Binds the server's Unix socket at `path` under the core's claim policy
    /// (`dr_agent_socket_start`: the folder `0700` and this user's, an
    /// answering socket left alone, a refusing leftover replaced). `handler`
    /// is called on the server's thread with `context` for the life of the
    /// process. Throws the core's message when nothing is bound.
    static func bindSocket(path: String, name: String, version: String, catalog: String,
                           handler: DrAgentToolHandler, context: UnsafeMutableRawPointer) throws {
        var err: UnsafeMutablePointer<CChar>? = nil
        guard dr_agent_socket_start(path, name, version, catalog, handler, context, &err) else {
            throw DoctoredCoreError(message: takeErrorMessage(err, fallback: "could not bind the agent socket"))
        }
        if let err { dr_string_free(err) }
    }

    /// For quit: unlinks the socket only while it is still this listener's,
    /// with no join. Safe when none is bound.
    static func releaseSocket() {
        dr_agent_socket_release()
    }

    /// A library-owned copy of `string` for the tool handler's return value.
    /// Ownership passes to the core on return; never free it.
    static func coreString(_ string: String) -> UnsafeMutablePointer<CChar>? {
        string.withCString { dr_agent_string_create($0) }
    }
}
