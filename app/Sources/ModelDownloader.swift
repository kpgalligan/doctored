import CoreML
import CryptoKit
import Foundation

// The model downloader: a documented `URLSession` delegate seam, the one place outside
// `BackgroundWork` that owns a `DispatchQueue` (app/CLAUDE.md's Hygiene list). The session's
// delegate queue is a serial `OperationQueue` over `queue`; every job's state lives on it, the
// one synchronous `MLModel.compileModel(at:)` runs on it, and every event reaches main through
// `DispatchQueue.main.async`. No async/await: the delegate callbacks are the whole protocol.
//
// One job per catalogue entry, one file at a time, in catalogue order:
// - a file already verified (its final name exists at the catalogued size) is skipped;
// - a `.partial` at or past the catalogued size is verified on disk with no request (renamed on
//   a match, removed on a mismatch);
// - otherwise a data task sends `Range: bytes=<partial size>-`: 206 appends (the hash is seeded
//   with the bytes already on disk), 200 starts over, 416 verifies what is on disk, and any other
//   status fails and keeps the partial;
// - at completion the size and the streamed SHA-256 must match the catalogue, else the partial is
//   removed and the job fails "<path>: checksum mismatch"; a match is renamed atomically.
// When every file is verified each package (if any) is compiled into `compiled/` and
// `verified.json` is written last, which is what makes the store call the model ready.

enum ModelDownloadEvent {
    case progress(id: String, fraction: Double)
    case finished(id: String)
    case failed(id: String, message: String)
}

final class ModelDownloader: NSObject, URLSessionDataDelegate {
    private let queue = DispatchQueue(label: "com.kgalligan.doctored.model-download")
    private let operations = OperationQueue()
    private let onEvent: (ModelDownloadEvent) -> Void
    private var session: URLSession?
    /// Delegate queue only.
    private var jobs: [String: Job] = [:]

    /// One entry's download, owned by the delegate queue.
    private final class Job {
        let entry: ModelCatalogEntry
        let directory: URL
        var index = 0
        var task: URLSessionDataTask?
        var handle: FileHandle?
        var hasher = SHA256()
        var written: Int64 = 0
        /// The fraction last sent to main, so a chunk moving it less than 1% sends nothing.
        var reported: Double = -1
        /// The task was ended on purpose (a 416, a refused status, a cancel): its completion
        /// reports nothing.
        var abandoned = false

        init(entry: ModelCatalogEntry, directory: URL) {
            self.entry = entry
            self.directory = directory
        }

        var file: ModelFile { entry.files[index] }
        var finalURL: URL { directory.appendingPathComponent(file.path) }
        var partialURL: URL { directory.appendingPathComponent(file.path + ".partial") }
        /// The bytes of the files before this one.
        var completedBytes: Int64 { entry.files.prefix(index).reduce(0) { $0 + $1.size } }
    }

    init(configuration: URLSessionConfiguration, onEvent: @escaping (ModelDownloadEvent) -> Void) {
        self.onEvent = onEvent
        super.init()
        operations.maxConcurrentOperationCount = 1
        operations.underlyingQueue = queue
        session = URLSession(configuration: configuration, delegate: self, delegateQueue: operations)
    }

    /// Starts or resumes `entry` into `directory` (its version folder).
    func start(_ entry: ModelCatalogEntry, into directory: URL) {
        queue.async { [self] in
            guard jobs[entry.id] == nil else { return }
            let job = Job(entry: entry, directory: directory)
            jobs[entry.id] = job
            next(job)
        }
    }

    /// Stops `id`'s job; its partial file stays.
    func cancel(_ id: String) {
        queue.async { [self] in
            guard let job = jobs.removeValue(forKey: id) else { return }
            job.abandoned = true
            job.task?.cancel()
            close(job)
        }
    }

    // MARK: - The job

    private func emit(_ event: ModelDownloadEvent) {
        let onEvent = onEvent
        DispatchQueue.main.async { onEvent(event) }
    }

    private func fail(_ job: Job, _ message: String) {
        close(job)
        jobs[job.entry.id] = nil
        emit(.failed(id: job.entry.id, message: message))
    }

    private func close(_ job: Job) {
        try? job.handle?.close()
        job.handle = nil
    }

    private func size(_ url: URL) -> Int64? {
        (try? FileManager.default.attributesOfItem(atPath: url.path)[.size] as? NSNumber)?.int64Value
    }

    /// Sends the job's fraction when it has moved by 1% or more since the last one sent (or with
    /// `force`): each event refreshes the Models pane, which measures the folder on disk, so one
    /// event per data chunk would be thousands of refreshes for one file.
    private func reportProgress(_ job: Job, force: Bool = false) {
        let total = max(1, job.entry.totalBytes)
        let fraction = min(1, Double(job.completedBytes + job.written) / Double(total))
        guard force || abs(fraction - job.reported) >= 0.01 else { return }
        job.reported = fraction
        emit(.progress(id: job.entry.id, fraction: fraction))
    }

    /// The next file, or the finish once every file is verified.
    private func next(_ job: Job) {
        guard jobs[job.entry.id] === job else { return }
        while job.index < job.entry.files.count, size(job.finalURL) == job.file.size {
            job.index += 1
        }
        guard job.index < job.entry.files.count else {
            finish(job)
            return
        }
        do {
            try FileManager.default.createDirectory(
                at: job.partialURL.deletingLastPathComponent(), withIntermediateDirectories: true)
        } catch {
            fail(job, "\(job.file.path): \(error.localizedDescription)")
            return
        }
        let have = size(job.partialURL) ?? 0
        job.written = have
        reportProgress(job, force: true)
        if have >= job.file.size {
            verifyOnDisk(job)
            return
        }
        var request = URLRequest(url: job.file.url)
        if have > 0 { request.setValue("bytes=\(have)-", forHTTPHeaderField: "Range") }
        job.abandoned = false
        guard let session else {
            fail(job, "\(job.file.path): the download session is gone")
            return
        }
        let task = session.dataTask(with: request)
        job.task = task
        task.resume()
    }

    /// Hashes the partial on disk and settles it (module comment).
    private func verifyOnDisk(_ job: Job) {
        close(job)
        guard size(job.partialURL) == job.file.size, let digest = hash(job.partialURL),
              digest == job.file.sha256 else {
            try? FileManager.default.removeItem(at: job.partialURL)
            fail(job, "\(job.file.path): checksum mismatch")
            return
        }
        settle(job)
    }

    private func hash(_ url: URL) -> String? {
        guard let handle = try? FileHandle(forReadingFrom: url) else { return nil }
        defer { try? handle.close() }
        var hasher = SHA256()
        while let chunk = try? handle.read(upToCount: 1 << 20), !chunk.isEmpty {
            hasher.update(data: chunk)
        }
        return Self.hex(hasher.finalize())
    }

    private static func hex(_ digest: SHA256.Digest) -> String {
        digest.map { String(format: "%02x", $0) }.joined()
    }

    /// Renames a verified partial to its final name and moves on.
    private func settle(_ job: Job) {
        do {
            if FileManager.default.fileExists(atPath: job.finalURL.path) {
                try FileManager.default.removeItem(at: job.finalURL)
            }
            try FileManager.default.moveItem(at: job.partialURL, to: job.finalURL)
        } catch {
            fail(job, "\(job.file.path): \(error.localizedDescription)")
            return
        }
        job.index += 1
        job.written = 0
        next(job)
    }

    /// Compiles each package and writes the ready marker.
    private func finish(_ job: Job) {
        let directory = job.directory
        do {
            for package in job.entry.packages {
                let stem = (package as NSString).deletingPathExtension
                let compiledFolder = directory.appendingPathComponent("compiled", isDirectory: true)
                let destination = compiledFolder.appendingPathComponent(stem + ".mlmodelc", isDirectory: true)
                let compiled = try MLModel.compileModel(at: directory.appendingPathComponent(package))
                try FileManager.default.createDirectory(at: compiledFolder, withIntermediateDirectories: true)
                if FileManager.default.fileExists(atPath: destination.path) {
                    try FileManager.default.removeItem(at: destination)
                }
                try FileManager.default.moveItem(at: compiled, to: destination)
            }
            let marker: [String: Any] = [
                "id": job.entry.id, "version": job.entry.version,
                "files": job.entry.files.map(\.sha256),
                "verified_at": ISO8601DateFormatter().string(from: Date()),
            ]
            let data = try JSONSerialization.data(withJSONObject: marker, options: [.prettyPrinted, .sortedKeys])
            try data.write(to: directory.appendingPathComponent("verified.json"), options: .atomic)
        } catch {
            fail(job, "\(job.entry.name): \(error.localizedDescription)")
            return
        }
        jobs[job.entry.id] = nil
        emit(.finished(id: job.entry.id))
    }

    private func job(for task: URLSessionTask) -> Job? {
        jobs.values.first { $0.task === task }
    }

    // MARK: - URLSessionDataDelegate (delegate queue)

    func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive response: URLResponse,
                    completionHandler: @escaping (URLSession.ResponseDisposition) -> Void) {
        guard let job = job(for: dataTask) else {
            completionHandler(.cancel)
            return
        }
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        do {
            switch status {
            case 206:
                let handle = try FileHandle(forUpdating: job.partialURL)
                // The hash covers the whole file, so the bytes already on disk seed it.
                var hasher = SHA256()
                while let chunk = try handle.read(upToCount: 1 << 20), !chunk.isEmpty {
                    hasher.update(data: chunk)
                }
                try handle.seekToEnd()
                job.hasher = hasher
                job.handle = handle
                completionHandler(.allow)
            case 200:
                FileManager.default.createFile(atPath: job.partialURL.path, contents: nil)
                job.handle = try FileHandle(forWritingTo: job.partialURL)
                job.hasher = SHA256()
                job.written = 0
                completionHandler(.allow)
            case 416:
                job.abandoned = true
                completionHandler(.cancel)
                verifyOnDisk(job)
            default:
                job.abandoned = true
                completionHandler(.cancel)
                fail(job, "\(job.file.path): the server answered HTTP \(status)")
            }
        } catch {
            job.abandoned = true
            completionHandler(.cancel)
            fail(job, "\(job.file.path): \(error.localizedDescription)")
        }
    }

    func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive data: Data) {
        guard let job = job(for: dataTask), let handle = job.handle else { return }
        do {
            try handle.write(contentsOf: data)
        } catch {
            job.abandoned = true
            dataTask.cancel()
            fail(job, "\(job.file.path): \(error.localizedDescription)")
            return
        }
        job.hasher.update(data: data)
        job.written += Int64(data.count)
        reportProgress(job)
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        guard let job = job(for: task) else { return }
        job.task = nil
        guard !job.abandoned else { return }
        close(job)
        if let error {
            fail(job, "\(job.file.path): \(error.localizedDescription)")
            return
        }
        let digest = Self.hex(job.hasher.finalize())
        guard size(job.partialURL) == job.file.size, digest == job.file.sha256 else {
            try? FileManager.default.removeItem(at: job.partialURL)
            fail(job, "\(job.file.path): checksum mismatch")
            return
        }
        settle(job)
    }
}
