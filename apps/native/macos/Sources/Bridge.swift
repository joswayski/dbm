import Foundation

struct BridgeFailure: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

/// Initialization, calls, and disposal are serialized on one worker queue,
/// never AppKit's event loop. Requests are never automatically replayed.
final class Bridge {
    private let queue = DispatchQueue(label: "dbm.native.bridge", qos: .userInitiated)
    private var session: OpaquePointer?
    private var stopped = false

    private func initialize() throws -> OpaquePointer {
        if stopped {
            throw BridgeFailure(message: "Database bridge is closed. Reopen DBM Native.")
        }
        if let session = session { return session }
        var errorPointer: UnsafeMutablePointer<CChar>?
        guard let created = dbm_bridge_session_create(&errorPointer) else {
            let message = errorPointer.map { String(cString: $0) }
                ?? "The database bridge could not be initialized."
            dbm_bridge_response_free(errorPointer)
            throw BridgeFailure(message: message)
        }
        session = created
        return created
    }

    func stop() {
        // Never wait for a long-running database query on AppKit's event loop.
        queue.async { [self] in
            stopped = true
            if let session = session {
                dbm_bridge_session_free(session)
                self.session = nil
            }
        }
    }

    func send(_ request: [String: Any], completion: @escaping (Result<Any, Error>) -> Void) {
        queue.async { [self] in
            let result: Result<Any, Error>
            do {
                let data = try JSONSerialization.data(withJSONObject: request)
                guard data.count <= 1_048_576 else {
                    throw BridgeFailure(message: "Query exceeds the native preview's 1 MiB request limit.")
                }
                let activeSession = try initialize()
                let responsePointer = data.withUnsafeBytes { bytes in
                    dbm_bridge_session_call(activeSession,
                                            bytes.bindMemory(to: UInt8.self).baseAddress,
                                            bytes.count)
                }
                guard let responsePointer = responsePointer else {
                    throw BridgeFailure(message: "The database bridge returned no response. A submitted write may have completed; check before retrying.")
                }
                defer { dbm_bridge_response_free(responsePointer) }
                let responseData = Data(bytes: responsePointer,
                                        count: strlen(responsePointer))
                guard let reply = try JSONSerialization.jsonObject(with: responseData) as? [String: Any],
                      let ok = reply["ok"] as? Bool else {
                    throw BridgeFailure(message: "Invalid database bridge response.")
                }
                if ok {
                    result = .success(reply["value"] ?? NSNull())
                } else {
                    result = .failure(BridgeFailure(message: reply["error"] as? String ?? "Database operation failed."))
                }
            } catch {
                result = .failure(error)
            }
            DispatchQueue.main.async { completion(result) }
        }
    }
}
