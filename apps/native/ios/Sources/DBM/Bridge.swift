import Foundation

enum BridgeError: LocalizedError {
    case message(String)
    var errorDescription: String? { if case .message(let text) = self { return text }; return nil }
}

/// Owns the Rust session. Creation, calls and disposal are confined to one queue.
/// The MainActor model discards completions from prior foreground sessions.
final class Bridge: @unchecked Sendable {
    private static let allowedCommands: Set<String> = [
        "listProfiles", "saveProfile", "deleteProfile", "testProfile", "connect", "disconnect",
        "connectDatabase", "listDatabases", "loadSchemaTree", "loadTablePage", "query", "listQueryHistory",
    ]
    private let queue = DispatchQueue(label: "app.dbm.bridge", qos: .userInitiated)
    private var session: OpaquePointer?
    let demo: Bool

    init(environment: [String: String] = ProcessInfo.processInfo.environment) {
        #if DEBUG
        demo = environment["DBM_DEMO"] == "1"
        #else
        demo = false
        #endif
    }

    func start(_ completion: @escaping @MainActor (Result<Void, Error>) -> Void) {
        queue.async { [self] in
            guard session == nil else { return finish(.success(()), completion) }
            var error: UnsafeMutablePointer<CChar>?
            if demo {
                session = dbm_bridge_demo_session_create()
            } else {
                do {
                    let url = try Self.storeURL()
                    let bytes = Array(url.path.utf8)
                    session = bytes.withUnsafeBufferPointer { buffer in
                        dbm_bridge_mobile_session_create(buffer.baseAddress, buffer.count, &error)
                    }
                } catch { return finish(.failure(error), completion) }
            }
            guard session != nil else {
                let message = error.map { String(cString: $0) } ?? "The local database session could not start."
                dbm_bridge_response_free(error)
                return finish(.failure(BridgeError.message(message)), completion)
            }
            finish(.success(()), completion)
        }
    }

    func call(_ request: [String: Any], completion: @escaping @MainActor (Result<Any, Error>) -> Void) {
        queue.async { [self] in
            guard let command = request["command"] as? String, Self.allowedCommands.contains(command) else {
                return finish(.failure(BridgeError.message("This operation is unavailable in the read-only mobile app.")), completion)
            }
            guard let session else { return finish(.failure(BridgeError.message("Session is closed.")), completion) }
            do {
                let data = try JSONSerialization.data(withJSONObject: request)
                let pointer = data.withUnsafeBytes { bytes in
                    dbm_bridge_session_call(session, bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count)
                }
                guard let pointer else { throw BridgeError.message("The database core returned no response.") }
                defer { dbm_bridge_response_free(pointer) }
                guard let response = String(validatingUTF8: pointer)?.data(using: .utf8),
                      let object = try JSONSerialization.jsonObject(with: response) as? [String: Any] else {
                    throw BridgeError.message("The database core returned an invalid response.")
                }
                if object["ok"] as? Bool == true { finish(.success(object["value"] ?? NSNull()), completion) }
                else { throw BridgeError.message(object["error"] as? String ?? "Database request failed.") }
            } catch { finish(.failure(error), completion) }
        }
    }

    func dispose(_ completion: (@MainActor () -> Void)? = nil) {
        queue.async { [self] in
            if let session { dbm_bridge_session_free(session) }
            session = nil
            if let completion { Task { @MainActor in completion() } }
        }
    }

    private func finish<T>(_ result: Result<T, Error>,
                           _ completion: @escaping @MainActor (Result<T, Error>) -> Void) {
        Task { @MainActor in completion(result) }
    }

    private static func storeURL() throws -> URL {
        let support = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask,
                                                  appropriateFor: nil, create: true)
        let directory = support.appending(path: "DBM", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true,
                                                attributes: [.protectionKey: FileProtectionType.complete])
        var values = URLResourceValues(); values.isExcludedFromBackup = true
        var mutable = directory; try mutable.setResourceValues(values)
        return directory.appending(path: "dbm.sqlite")
    }
}
