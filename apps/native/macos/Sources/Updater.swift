import AppKit

/// Self-updates from the latest release (`crates/dbm-update`). The
/// bridge downloads a build and verifies its minisign signature; this class
/// also checks Apple's code signature, then swaps the app bundle once DBM
/// quits and reopens it. Development builds have no build number and never
/// check.
final class Updater {
    enum State {
        case idle
        case checking
        case upToDate
        case available([String: Any])
        case installing
        case failed(String)
    }

    private(set) var state = State.idle { didSet { onChange?() } }
    var onChange: (() -> Void)?
    /// Opens What's new with highlights this install has not shown yet.
    var onWhatsNew: (([[String: Any]]) -> Void)?
    /// The newest build whose release highlights this install has shown.
    private static let acknowledgedKey = "DBMWhatsNewAcknowledgedBuild"
    let currentBuild: Int?
    private var timer: Timer?

    init() {
        if case .success(let value) = Bridge.helper(["command": "updateCurrent"]), let build = value as? NSNumber {
            currentBuild = build.intValue
        } else {
            currentBuild = nil
        }
    }

    var enabled: Bool { currentBuild != nil }

    /// Checks shortly after launch and every 30 minutes, quietly.
    func start() {
        guard enabled else { return }
        DispatchQueue.main.asyncAfter(deadline: .now() + 5) { [weak self] in self?.check(quiet: true) }
        timer = Timer.scheduledTimer(withTimeInterval: 30 * 60, repeats: true) { [weak self] _ in
            self?.check(quiet: true)
        }
    }

    /// A quiet check only surfaces a found update; a manual one also reports
    /// "Up to date" and errors.
    func check(quiet: Bool = false) {
        switch state {
        case .checking, .installing: return
        default: break
        }
        let previous = state
        if !quiet { state = .checking }
        DispatchQueue.global(qos: .utility).async {
            let reply = Bridge.helper(["command": "updateCheck"])
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                switch reply {
                case .success(let value):
                    if let update = value as? [String: Any] {
                        self.state = .available(update)
                    } else if quiet {
                        self.state = previous
                    } else {
                        self.state = .upToDate
                    }
                case .failure(let error):
                    self.state = quiet ? previous : .failed(error.localizedDescription)
                }
            }
        }
    }

    func install(_ update: [String: Any]) {
        let current = Bundle.main.bundleURL
        guard FileManager.default.isWritableFile(atPath: current.deletingLastPathComponent().path) else {
            state = .failed("Move Anybase into a folder you can write to, such as Applications, to update it.")
            return
        }
        // The review just showed these highlights; the restarted build should
        // not open them again.
        if update["highlights"] is [Any], let build = update["build"] as? NSNumber {
            acknowledge(build.intValue)
        }
        state = .installing
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("dbm-native-update-\(UUID().uuidString)")
        // Downloading, unpacking, and verifying take a while, so they run off
        // the main thread; only the result and the relaunch come back to it.
        DispatchQueue.global(qos: .userInitiated).async {
            let failure = Updater.prepare(update, in: directory, current: current)
            DispatchQueue.main.async { [weak self] in
                if let failure {
                    self?.state = .failed(failure.localizedDescription)
                } else {
                    // The user confirmed that unsaved work is discarded.
                    exit(0)
                }
            }
        }
    }

    // MARK: What's new

    /// A first launch only records the installed build; after an update, the
    /// highlights it has not shown yet open once.
    func startWhatsNew() {
        guard let currentBuild else { return }
        guard let seen = UserDefaults.standard.object(forKey: Self.acknowledgedKey) as? Int else {
            acknowledge(currentBuild)
            return
        }
        guard seen < currentBuild else { return }
        loadWhatsNew(after: seen) { [weak self] result in
            // A failed quiet fetch tries again next launch.
            guard case .success(let days) = result else { return }
            if days.isEmpty {
                self?.acknowledge(currentBuild)
            } else {
                self?.onWhatsNew?(days)
            }
        }
    }

    /// Highlighted releases built after `build` up to this one, grouped by
    /// UTC day, newest first.
    func loadWhatsNew(after build: Int, completion: @escaping (Result<[[String: Any]], Error>) -> Void) {
        DispatchQueue.global(qos: .utility).async {
            let reply = Bridge.helper(["command": "whatsNew", "after": build]).map { $0 as? [[String: Any]] ?? [] }
            DispatchQueue.main.async { completion(reply) }
        }
    }

    /// Records that highlights up to `build` (this one by default) were shown.
    func acknowledge(_ build: Int? = nil) {
        guard let build = build ?? currentBuild else { return }
        let defaults = UserDefaults.standard
        if let seen = defaults.object(forKey: Self.acknowledgedKey) as? Int, seen >= build { return }
        defaults.set(build, forKey: Self.acknowledgedKey)
    }

    /// Downloads and stages the update, returning the error that stopped it.
    private static func prepare(_ update: [String: Any], in directory: URL, current: URL) -> Error? {
        do {
            let value = try Bridge.helper(["command": "updateDownload", "update": update, "directory": directory.path]).get()
            guard let archive = value as? String else {
                throw BridgeFailure(message: "The update download returned no file.")
            }
            try scheduleSwap(archive: archive, in: directory, current: current)
            return nil
        } catch {
            return error
        }
    }

    private static func run(_ tool: String, _ arguments: [String]) throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: tool)
        process.arguments = arguments
        try process.run()
        process.waitUntilExit()
        guard process.terminationStatus == 0 else {
            throw BridgeFailure(message: "The update could not be prepared (\(URL(fileURLWithPath: tool).lastPathComponent) failed).")
        }
    }

    /// Unpacks and verifies the new bundle, then starts a script that swaps
    /// it in and reopens it once this process exits.
    private static func scheduleSwap(archive: String, in directory: URL, current: URL) throws {
        let unpacked = directory.appendingPathComponent("unpacked")
        try run("/usr/bin/ditto", ["-x", "-k", archive, unpacked.path])
        let contents = try FileManager.default.contentsOfDirectory(at: unpacked, includingPropertiesForKeys: nil)
        guard let app = contents.first(where: { $0.pathExtension == "app" }) else {
            throw BridgeFailure(message: "The update archive does not contain the app.")
        }
        // Apple's signature on top of the release key's: the new bundle must
        // be intact and Developer ID signed.
        try run("/usr/bin/codesign", ["--verify", "--deep", "--strict", app.path])
        // Swap the bundle once this process exits, then reopen it. If the
        // move fails, the old copy is put back and reopened.
        let script = """
        while kill -0 "$DBM_PID" 2>/dev/null; do sleep 0.2; done
        rm -rf "$DBM_OLD"
        if mv "$DBM_CURRENT" "$DBM_OLD"; then
          if mv "$DBM_NEW" "$DBM_CURRENT"; then rm -rf "$DBM_OLD"; else mv "$DBM_OLD" "$DBM_CURRENT"; fi
        fi
        /usr/bin/open -n "$DBM_CURRENT"
        """
        let swap = Process()
        swap.executableURL = URL(fileURLWithPath: "/bin/sh")
        swap.arguments = ["-c", script]
        swap.environment = [
            "DBM_PID": String(ProcessInfo.processInfo.processIdentifier),
            "DBM_CURRENT": current.path,
            "DBM_NEW": app.path,
            "DBM_OLD": current.path + ".previous",
            "PATH": "/usr/bin:/bin",
        ]
        try swap.run()
    }

    /// The top bar button's label and tooltip, as the desktop's
    /// `UpdateControl` words them.
    var label: String {
        switch state {
        case .idle: return ""
        case .checking: return "Checking…"
        case .upToDate: return "Up to date"
        case .available(let update): return "Update to v\(string(update["version"]))"
        case .installing: return "Installing…"
        case .failed: return "Retry update"
        }
    }

    var versionLabel: String {
        "Version \(Bundle.main.object(forInfoDictionaryKey: "DBMVersion") as? String ?? "build \(currentBuild ?? 0)")"
    }

    var detail: String {
        switch state {
        case .available(let update): return string(update["notes"]).isEmpty ? string(update["version"]) : string(update["notes"])
        case .failed(let message): return message
        default: return "Installed \(Bundle.main.object(forInfoDictionaryKey: "DBMVersion") as? String ?? "build \(currentBuild ?? 0)")"
        }
    }
}
