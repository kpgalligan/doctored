// doctored-agent: the assistant helper the app launches from Contents/Helpers, one
// process per Assistant panel. It speaks JSON lines over stdio (DoctoredAgentKit's
// Protocol.swift) and owns no tools: every tool call goes back to the app, which runs
// it through the dispatcher its MCP server uses. What is specific to the app (its
// system prompt, its tools, the search guide) arrives in `configure`.

import Anthropic
import DoctoredAgentKit
import Foundation

/// The one writer of stdout, which carries the protocol only. Each event is written
/// whole under the lock, so lines never interleave.
final class StdoutWriter: EventSink, @unchecked Sendable {
    private let lock = NSLock()
    private var broken = false

    func emit(_ event: HelperEvent) {
        let line = event.line
        lock.withLock {
            guard !broken else { return }
            line.withUnsafeBytes { bytes in
                guard let base = bytes.baseAddress else { return }
                var offset = 0
                while offset < bytes.count {
                    let written = write(STDOUT_FILENO, base + offset, bytes.count - offset)
                    if written > 0 {
                        offset += written
                    } else if written < 0 && errno == EINTR {
                        continue
                    } else {
                        // The app closed its end; its stdin EOF ends this process next.
                        broken = true
                        Log.warn("stdout closed: \(String(cString: strerror(errno)))")
                        return
                    }
                }
            }
        }
    }
}

/// stdin's lines, read on a thread of their own so that a turn never holds up the
/// app's writes (a tool result can be megabytes).
func stdinLines() -> AsyncStream<Data> {
    AsyncStream { continuation in
        let reader = Thread {
            var splitter = LineSplitter()
            var buffer = [UInt8](repeating: 0, count: 1 << 16)
            while true {
                let count = buffer.withUnsafeMutableBytes { read(STDIN_FILENO, $0.baseAddress, $0.count) }
                if count > 0 {
                    for line in splitter.append(Data(buffer[0..<count])) { continuation.yield(line) }
                } else if count < 0 && errno == EINTR {
                    continue
                } else {
                    break
                }
            }
            if let last = splitter.finish() { continuation.yield(last) }
            continuation.finish()
        }
        reader.start()
    }
}

// A write after the app has gone fails instead of killing the process.
signal(SIGPIPE, SIG_IGN)
// Slaude's own diagnostics go to stderr too, with the helper's prefix.
AnthropicWarnings.setHandler { message, category in Log.warn("\(category.rawValue): \(message)") }
CredentialsLog.setHandler { level, message in if level >= .warning { Log.warn("anthropic: \(message)") } }

let session = AgentSession(sink: StdoutWriter())
for await line in stdinLines() {
    await session.receive(line)
}
// The app closing our stdin means it is gone or closed the session.
await session.shutdown()
exit(0)
