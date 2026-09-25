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
    private let demo: Bool

    init(demo: Bool) {
        self.demo = demo
    }

    private func initialize() throws -> OpaquePointer {
        if stopped {
            throw BridgeFailure(message: "Database bridge is closed. Reopen DBM Native.")
        }
        if let session { return session }
        if demo {
            guard let created = dbm_bridge_demo_session_create() else {
                throw BridgeFailure(message: "The demo fixture could not start.")
            }
            session = created
            return created
        }
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
            if let session {
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
                    throw BridgeFailure(message: "Request exceeds the native bridge's 1 MiB limit.")
                }
                let activeSession = try initialize()
                let pointer = data.withUnsafeBytes { bytes in
                    dbm_bridge_session_call(activeSession, bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count)
                }
                result = try Bridge.decode(pointer)
            } catch {
                result = .failure(error)
            }
            DispatchQueue.main.async { completion(result) }
        }
    }

    /// Editor and grid helpers that need no session; safe on the main thread.
    static func helper(_ request: [String: Any]) -> Result<Any, Error> {
        do {
            let data = try JSONSerialization.data(withJSONObject: request)
            let pointer = data.withUnsafeBytes { bytes in
                dbm_bridge_helper_call(bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count)
            }
            return try decode(pointer)
        } catch {
            return .failure(error)
        }
    }

    private static func decode(_ pointer: UnsafeMutablePointer<CChar>?) throws -> Result<Any, Error> {
        guard let pointer else {
            throw BridgeFailure(message: "The database bridge returned no response. A submitted write may have completed; check before retrying.")
        }
        defer { dbm_bridge_response_free(pointer) }
        let data = Data(bytes: pointer, count: strlen(pointer))
        guard let reply = try JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed]) as? [String: Any],
              let ok = reply["ok"] as? Bool else {
            throw BridgeFailure(message: "Invalid database bridge response.")
        }
        if ok { return .success(reply["value"] ?? NSNull()) }
        return .failure(BridgeFailure(message: reply["error"] as? String ?? "Database operation failed."))
    }
}
