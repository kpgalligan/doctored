import Anthropic
import Foundation

/// One streamed reply, read with every event's JSON kept (Slaude's `stream(body:)`).
public typealias ReplyStream = JSONMessageStream<BetaRawMessageStreamEvent>

/// The two calls the helper makes, behind a seam so tests can drive the real client
/// over Slaude's injectable transport (scripted SSE, recorded request bodies).
public protocol MessagesAPI: Sendable {
    /// Opens one streamed `POST /v1/messages` with `betas` in `anthropic-beta`. Returns
    /// once the response headers arrived, after the client's retries.
    func stream(body: RawJSON, betas: [String]) async throws -> ReplyStream
    /// The model's context window from the Models API, or nil when it cannot be read.
    func maxInputTokens(model: String) async -> Int?
    /// Releases the transport (URLSession holds its delegate until invalidated).
    func close() async
}

/// Makes the client for one `configure`.
public struct ClientFactory: Sendable {
    public static let defaultBaseURL = "https://api.anthropic.com"
    /// Slaude's default is 2; the contract's is 3 (designs/assistant-helper.md, "Requests").
    public static let maxRetries = 3

    /// The transport each client gets; nil is Slaude's URLSessionTransport.
    public var transport: (@Sendable () -> any HTTPTransport)?
    /// Replaces the client's retry sleep (tests record the delays instead of waiting).
    public var sleeper: (@Sendable (TimeInterval) async throws -> Void)?

    public init(
        transport: (@Sendable () -> any HTTPTransport)? = nil,
        sleeper: (@Sendable (TimeInterval) async throws -> Void)? = nil)
    {
        self.transport = transport
        self.sleeper = sleeper
    }

    public static let live = ClientFactory()

    /// The key and base URL come only from `configure`. An explicit `apiKey` keeps
    /// Slaude's credential chain (profile files under HOME, federation) from running,
    /// and an empty `environment` keeps it from reading ANTHROPIC_BASE_URL,
    /// ANTHROPIC_CUSTOM_HEADERS, the webhook key or OpenTelemetry settings; the app
    /// also scrubs the helper's environment to HOME, TMPDIR and PATH.
    public func make(apiKey: String, baseURL: String?) throws -> any MessagesAPI {
        let client = try Anthropic(
            apiKey: apiKey, baseURL: baseURL ?? Self.defaultBaseURL, maxRetries: Self.maxRetries,
            transport: transport?(), environment: [:])
        if let sleeper { client.sleeper = sleeper }
        return SlaudeMessages(client: client)
    }
}

struct SlaudeMessages: MessagesAPI {
    let client: Anthropic

    func stream(body: RawJSON, betas: [String]) async throws -> ReplyStream {
        try await client.beta.messages.stream(body: body, betas: betas.map { AnthropicBetaParam(rawValue: $0) })
    }

    /// `models.retrieve`, read from its raw JSON so that a field this SDK's ModelInfo
    /// requires and a response lacks does not lose the limit.
    func maxInputTokens(model: String) async -> Int? {
        do {
            let response = try await client.withRawResponse.models.retrieve(model)
            return try await response.json()["max_input_tokens"]?.intValue
        } catch {
            return nil  // the meter shows usage without a limit
        }
    }

    func close() async {
        await client.close()
    }
}
