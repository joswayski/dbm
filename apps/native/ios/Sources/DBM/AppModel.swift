import SwiftUI

@MainActor
final class AppModel: ObservableObject {
    enum Route { case connections, workspace }
    @Published var profiles: [Profile] = []
    @Published var route = Route.connections
    @Published var active: Profile?
    @Published var databases: [String] = []
    @Published var database = ""
    @Published var schema: [SchemaItem] = []
    @Published var sql = ""
    @Published private(set) var lastExecuted: String?
    @Published var queryGrid = GridData()
    @Published var tableGrid = GridData()
    @Published var showingTable = false
    @Published var selectedTable: (schema: String, table: String)?
    @Published var loading = false
    @Published var error: String?
    @Published var notice: String?
    @Published var privacyEpoch = 0
    private var started = false
    let bridge = Bridge()
    var isDemo: Bool { bridge.demo }
    var grid: GridData { showingTable ? tableGrid : queryGrid }

    func start() {
        guard !started else { return }; started = true; loading = true
        let epoch = privacyEpoch
        bridge.start { [weak self] result in
            guard let self, self.privacyEpoch == epoch else { return }
            if case .failure = result { self.started = false }
            self.received(result) { self.loadProfiles() }
        }
    }
    func loadProfiles() { send(["command": "listProfiles"]) { [weak self] value in
        self?.profiles = (value as? [[String: Any]] ?? []).map { Profile($0["profile"] as? [String: Any] ?? [:]) }.sorted { $0.name < $1.name }
    } }
    func save(_ draft: ProfileDraft, completion: @escaping (Bool) -> Void) {
        send(["command": "saveProfile", "input": draft.input]) { [weak self] _ in self?.loadProfiles(); completion(true) } failure: { _ in completion(false) }
    }
    func saveAndConnect(_ draft: ProfileDraft, completion: @escaping (Bool) -> Void) {
        send(["command": "saveProfile", "input": draft.input]) { [weak self] value in
            guard let self, let object = value as? [String: Any] else { completion(false); return }
            let profile = Profile(object)
            profiles.removeAll { $0.id == profile.id }
            profiles.append(profile); profiles.sort { $0.name < $1.name }
            connect(profile); completion(true)
        } failure: { _ in completion(false) }
    }
    func test(_ draft: ProfileDraft, completion: @escaping (Bool) -> Void) {
        send(["command": "testProfile", "input": draft.input]) { _ in completion(true) } failure: { _ in completion(false) }
    }
    func delete(_ profile: Profile) { send(["command": "deleteProfile", "profile_id": profile.id]) { [weak self] _ in self?.loadProfiles() } }
    func connect(_ profile: Profile) {
        send(["command": "connect", "profile_id": profile.id]) { [weak self] value in
            guard let self, let object = value as? [String: Any] else { return }
            active = profile
            databases = (object["databases"] as? [[String: Any]] ?? []).filter { $0["isConnectable"] as? Bool ?? true }.compactMap { $0["name"] as? String }
            database = profile.database; route = .workspace; sql = profile.engine == .redis ? "PING" : "SELECT 1"
            lastExecuted = nil; queryGrid = GridData(); tableGrid = GridData(); showingTable = false; selectedTable = nil; schema = []; loadSchema()
        }
    }
    func switchDatabase(_ name: String) { guard let active else { return }; send(["command": "connectDatabase", "profile_id": active.id, "database": name]) { [weak self] _ in self?.database = name; self?.lastExecuted = nil; self?.queryGrid = GridData(); self?.tableGrid = GridData(); self?.showingTable = false; self?.selectedTable = nil; self?.schema = []; self?.loadSchema() } }
    func loadSchema() { guard let active else { return }; send(["command": "loadSchemaTree", "profile_id": active.id]) { [weak self] value in self?.schema = (value as? [[String: Any]] ?? []).map(SchemaItem.init) } }
    func run() { run(sql) }
    private func run(_ statement: String) {
        guard let active, !statement.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        send(["command": "query", "request": ["profileId": active.id, "sql": statement, "maxRows": 1000]]) { [weak self] value in
            self?.lastExecuted = statement; self?.queryGrid = GridData(query: value as? [String: Any] ?? [:])
        }
    }
    func browse(schema: String, table: String, offset: Int = 0) {
        guard let active else { return }
        let request: [String: Any] = ["profileId": active.id, "schema": schema, "table": table, "offset": max(0, offset), "limit": GridData.pageSize, "filters": [], "orderBy": NSNull(), "includeTotal": false]
        send(["command": "loadTablePage", "request": request]) { [weak self] value in guard let self else { return }; tableGrid = GridData(page: value as? [String: Any] ?? [:]); selectedTable = (schema, table); showingTable = true }
    }
    func showQuery() { showingTable = false }
    func showTable() { guard selectedTable != nil else { return }; showingTable = true }
    func previous() { if let target = selectedTable { browse(schema: target.schema, table: target.table, offset: grid.offset - grid.limit) } }
    func next() { if let target = selectedTable { browse(schema: target.schema, table: target.table, offset: grid.offset + grid.limit) } }
    func refresh() { if showingTable, let target = selectedTable { browse(schema: target.schema, table: target.table, offset: grid.offset) } else if let lastExecuted { run(lastExecuted) } }
    func background() {
        privacyEpoch += 1; started = false; loading = false
        active = nil; sql = ""; lastExecuted = nil; queryGrid = GridData(); tableGrid = GridData(); showingTable = false; schema = []; databases = []; database = ""
        selectedTable = nil; error = nil; notice = nil; route = .connections
        bridge.dispose()
    }
    func disconnect() { background(); start() }

    private func send(_ request: [String: Any], success: @escaping (Any) -> Void, failure: ((Error) -> Void)? = nil) {
        guard !loading else { return }
        let epoch = privacyEpoch
        loading = true; error = nil
        bridge.call(request) { [weak self] result in
            guard let self, self.privacyEpoch == epoch else { return }
            self.loading = false
            switch result { case .success(let value): success(value); case .failure(let error): self.error = error.localizedDescription; failure?(error) }
        }
    }
    private func received(_ result: Result<Void, Error>, success: () -> Void) { loading = false; if case .failure(let error) = result { self.error = error.localizedDescription } else { success() } }
}
