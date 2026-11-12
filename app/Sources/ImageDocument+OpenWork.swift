import Foundation

// Open shows Cancel: the core open of a file runs on the bounded worker under a
// `RasterJobControl`, waited with Cancel and Esc, and its panel is revealed only after
// `DocumentJobs.panelDelay`, so a small file never flashes one. A cancelled open throws
// `JobCancelled` and opens nothing; every caller rethrows it ahead of any fallback.

extension ImageDocument {
    /// Runs `open` on the bounded worker under a fresh single-use control,
    /// waited titled Opening “name”. When the worker is already occupied, or
    /// the caller is off the main thread, `open` runs inline with no control,
    /// exactly as it always did: an open never starts refusing because another
    /// background operation is running.
    static func openWork<T>(_ url: URL, scratch: UInt64 = 0,
                            _ open: @escaping (RasterJobControl?) throws -> T) throws -> T {
        guard Thread.isMainThread, !BackgroundWork.occupied else { return try open(nil) }
        let control = try RasterJobControl()
        let task = try WorkerTask(scratchEstimate: scratch) { try open(control) }
        return try BackgroundWork.wait(task, title: "Opening “\(url.lastPathComponent)”",
                                       control: control, revealAfter: DocumentJobs.panelDelay)
    }
}
