import Foundation

struct BridgeFailure: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

/// All pipe I/O and JSON decoding happens on one worker queue, never AppKit's
/// event loop. Requests are serial and never automatically replayed after failure.
final class Bridge {
    private let queue = DispatchQueue(label: "dbm.native.bridge", qos: .userInitiated)
    private let process = Process()
    private let input = Pipe()
    private let output = Pipe()
    private var pending = Data()
    private var failure: Error?

    init(executable: URL) {
        process.executableURL = executable
        process.standardInput = input
        process.standardOutput = output
        // Avoid logging database errors or profile data to system logs.
        process.standardError = FileHandle.nullDevice
        do { try process.run() } catch { failure = error }
    }

    func stop() {
        if process.isRunning { process.terminate() }
    }

    func send(_ request: [String: Any], completion: @escaping (Result<Any, Error>) -> Void) {
        queue.async { [self] in
            let result: Result<Any, Error>
            var transportStarted = false
            do {
                if let failure = failure { throw failure }
                guard process.isRunning else {
                    throw BridgeFailure(message: "Database worker stopped. Reopen DBM Native. Do not retry writes without checking their outcome.")
                }
                var data = try JSONSerialization.data(withJSONObject: request)
                data.append(0x0a)
                guard data.count <= 1_048_576 else {
                    throw BridgeFailure(message: "Query exceeds the native preview's 1 MiB request limit.")
                }
                transportStarted = true
                try input.fileHandleForWriting.write(contentsOf: data)
                while !pending.contains(0x0a) {
                    let chunk = output.fileHandleForReading.availableData
                    guard !chunk.isEmpty else {
                        throw BridgeFailure(message: "Database worker disconnected. A submitted write may have completed; check before retrying.")
                    }
                    pending.append(chunk)
                }
                let end = pending.firstIndex(of: 0x0a)!
                let line = pending.prefix(upTo: end)
                pending.removeSubrange(...end)
                guard let reply = try JSONSerialization.jsonObject(with: line) as? [String: Any],
                      let ok = reply["ok"] as? Bool else {
                    throw BridgeFailure(message: "Invalid database worker response.")
                }
                if ok {
                    result = .success(reply["value"] ?? NSNull())
                } else {
                    result = .failure(BridgeFailure(message: reply["error"] as? String ?? "Database operation failed."))
                }
            } catch {
                // A transport failure must never let a later reply match a new request.
                if transportStarted || !process.isRunning {
                    failure = error
                    stop()
                }
                result = .failure(error)
            }
            DispatchQueue.main.async { completion(result) }
        }
    }
}
