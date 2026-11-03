import Darwin
import Foundation

/// An HTTP/1.1 server on 127.0.0.1 for the process test: each `POST` gets the next of
/// `replies` (the last one repeats) as a `text/event-stream` body and each `GET` a 404,
/// every connection closed after its answer. POSIX sockets only; it records every
/// request it read.
final class LoopbackServer: @unchecked Sendable {
    struct Request: Sendable {
        let line: String
        let body: Data
    }

    let port: UInt16
    private let listener: Int32
    private var replies: [String]
    private let lock = NSLock()
    private var recorded: [Request] = []
    private var stopped = false

    init(_ replies: [String]) throws {
        precondition(!replies.isEmpty)
        self.replies = replies
        let fd = socket(AF_INET, SOCK_STREAM, 0)
        guard fd >= 0 else { throw POSIXError(.init(rawValue: errno) ?? .EIO) }
        var yes: Int32 = 1
        setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &yes, socklen_t(MemoryLayout<Int32>.size))
        var addr = sockaddr_in()
        addr.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        addr.sin_family = sa_family_t(AF_INET)
        addr.sin_port = 0
        addr.sin_addr.s_addr = inet_addr("127.0.0.1")
        let bound = withUnsafePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { bind(fd, $0, socklen_t(MemoryLayout<sockaddr_in>.size)) }
        }
        guard bound == 0, listen(fd, 16) == 0 else {
            close(fd)
            throw POSIXError(.init(rawValue: errno) ?? .EIO)
        }
        var length = socklen_t(MemoryLayout<sockaddr_in>.size)
        _ = withUnsafeMutablePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { getsockname(fd, $0, &length) }
        }
        listener = fd
        port = UInt16(bigEndian: addr.sin_port)
        Thread { [self] in acceptLoop() }.start()
    }

    var baseURL: String { "http://127.0.0.1:\(port)" }
    var requests: [Request] { lock.withLock { recorded } }

    func stop() { lock.withLock { stopped = true } }

    private var isStopped: Bool { lock.withLock { stopped } }

    /// Waits up to 50 ms at a time for `fd` to be readable; false once the server is stopped.
    private func readable(_ fd: Int32) -> Bool {
        var p = pollfd(fd: fd, events: Int16(POLLIN), revents: 0)
        while !isStopped {
            let n = poll(&p, 1, 50)
            if n > 0 || (n < 0 && errno != EINTR) { return true }
        }
        return false
    }

    private func acceptLoop() {
        defer { close(listener) }
        while readable(listener) {
            let connection = accept(listener, nil, nil)
            if connection < 0 { continue }
            var yes: Int32 = 1
            setsockopt(connection, SOL_SOCKET, SO_NOSIGPIPE, &yes, socklen_t(MemoryLayout<Int32>.size))
            Thread { [self] in serve(connection) }.start()
        }
    }

    private func serve(_ fd: Int32) {
        defer { close(fd) }
        guard let request = readRequest(fd) else { return }
        lock.withLock { recorded.append(request) }
        if request.line.hasPrefix("GET") {
            let body = #"{"type":"error","error":{"type":"not_found_error","message":"no such model"}}"#
            write(fd, "HTTP/1.1 404 Not Found\r\ncontent-type: application/json\r\ncontent-length: \(body.utf8.count)\r\nconnection: close\r\n\r\n\(body)")
        } else {
            let sse = lock.withLock { replies.count > 1 ? replies.removeFirst() : replies[0] }
            write(fd, "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n\(sse)")
        }
    }

    /// Reads one request: its head and a `content-length` body.
    private func readRequest(_ fd: Int32) -> Request? {
        var buffer = [UInt8]()
        var chunk = [UInt8](repeating: 0, count: 65_536)
        let separator: [UInt8] = [13, 10, 13, 10]
        var headEnd: Int?
        var bodyLength = 0
        while true {
            if headEnd == nil, let end = buffer.firstRange(of: separator)?.upperBound {
                headEnd = end
                for line in String(decoding: buffer[..<end], as: UTF8.self).components(separatedBy: "\r\n") {
                    let parts = line.split(separator: ":", maxSplits: 1)
                    if parts.count == 2, parts[0].lowercased() == "content-length" {
                        bodyLength = Int(parts[1].trimmingCharacters(in: .whitespaces)) ?? 0
                    }
                }
            }
            if let headEnd, buffer.count >= headEnd + bodyLength {
                let head = String(decoding: buffer[..<headEnd], as: UTF8.self)
                return Request(line: head.components(separatedBy: "\r\n").first ?? "",
                               body: Data(buffer[headEnd..<(headEnd + bodyLength)]))
            }
            guard readable(fd) else { return nil }
            let n = recv(fd, &chunk, chunk.count, 0)
            if n <= 0 { return nil }
            buffer.append(contentsOf: chunk[..<n])
        }
    }

    private func write(_ fd: Int32, _ text: String) {
        let bytes = Array(text.utf8)
        var sent = 0
        while sent < bytes.count {
            let n = bytes[sent...].withUnsafeBytes { send(fd, $0.baseAddress, $0.count, 0) }
            if n <= 0 { return }
            sent += n
        }
    }
}
