import AppKit
import UniformTypeIdentifiers

struct Workspace {
    var profile: Profile
    var databases: [String]
}

/// Owns UI state and talks to the Rust core through the bridge. Views call
/// back into it and it refreshes them; replies are routed to the tab or
/// profile that started the request, and ignored once that is closed.
final class AppController: NSObject, NSApplicationDelegate, NSWindowDelegate, QueryHost {
    let demo = CommandLine.arguments.contains("--demo")
    lazy var bridge = Bridge(demo: demo)
    var window: NSWindow!

    var profiles: [Profile] = []
    var workspaces: [String: Workspace] = [:]
    var schemas: [String: [SchemaNode]] = [:]
    var schemaFilters: [String: String] = [:]
    var collapsedProfiles = Set<String>()
    var connecting = Set<String>()
    var refreshingSchema = Set<String>()
    var activeProfileID: String?
    var tabs: [WorkTab] = []
    var activeTab: WorkTab?
    private var histories: [String: [[String: Any]]] = [:]
    private var running: [UUID: String] = [:]
    private var saving = Set<UUID>()
    private var exporting = Set<UUID>()
    private var tableTokens: [UUID: UUID] = [:]
    /// Bumped when a profile closes so its late replies are ignored.
    private var generations: [String: Int] = [:]

    private var sidebar: SidebarView!
    private var topBar: TopBar!
    private var tabStrip: TabStrip!
    private let content = FlippedView()
    private var welcome: WelcomeView!
    private let toast = ToastView()
    private let banner = MessageView(banner: true)
    private var bannerHeight: NSLayoutConstraint!
    private var exportRows: [UUID: Int] = [:]
    private var panes: [UUID: NSView] = [:]
    private var sheet: ProfileSheet?

    // MARK: Lifecycle

    func applicationDidFinishLaunching(_ notification: Notification) {
        Graphite.registerFonts()
        NSApp.appearance = NSAppearance(named: .darkAqua)
        installMenu()
        buildWindow()
        loadProfiles()
        if let index = CommandLine.arguments.firstIndex(of: "--snapshot-dir"), CommandLine.arguments.indices.contains(index + 1) {
            SnapshotDriver(app: self, directory: CommandLine.arguments[index + 1]).start()
        }
    }

    private func buildWindow() {
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1280, height: 800),
                          styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        window.title = demo ? "DBM Native — DEMO" : "DBM Native"
        window.minSize = NSSize(width: 900, height: 600)
        window.backgroundColor = Graphite.bg
        window.delegate = self
        window.setFrameAutosaveName(demo ? "" : "DBMNativeMain")

        sidebar = SidebarView(app: self)
        topBar = TopBar(demo: demo)
        tabStrip = TabStrip(app: self)
        welcome = WelcomeView()
        content.translatesAutoresizingMaskIntoConstraints = false
        let main = FlippedView()
        main.translatesAutoresizingMaskIntoConstraints = false
        for view in [topBar!, banner, tabStrip!, content] as [NSView] { main.addSubview(view) }
        // The error strip collapses to zero height while hidden.
        bannerHeight = banner.heightAnchor.constraint(equalToConstant: 0)
        bannerHeight.priority = .init(999)
        banner.onDismiss = { [weak self] in self?.bannerHeight.isActive = true }
        NSLayoutConstraint.activate([
            topBar.topAnchor.constraint(equalTo: main.topAnchor),
            topBar.leadingAnchor.constraint(equalTo: main.leadingAnchor),
            topBar.trailingAnchor.constraint(equalTo: main.trailingAnchor),
            banner.topAnchor.constraint(equalTo: topBar.bottomAnchor),
            banner.leadingAnchor.constraint(equalTo: main.leadingAnchor),
            banner.trailingAnchor.constraint(equalTo: main.trailingAnchor),
            bannerHeight,
            tabStrip.topAnchor.constraint(equalTo: banner.bottomAnchor),
            tabStrip.leadingAnchor.constraint(equalTo: main.leadingAnchor),
            tabStrip.trailingAnchor.constraint(equalTo: main.trailingAnchor),
            content.topAnchor.constraint(equalTo: tabStrip.bottomAnchor),
            content.leadingAnchor.constraint(equalTo: main.leadingAnchor),
            content.trailingAnchor.constraint(equalTo: main.trailingAnchor),
            content.bottomAnchor.constraint(equalTo: main.bottomAnchor),
        ])
        let root = PanelView(fill: Graphite.bg)
        root.addSubview(sidebar)
        root.addSubview(main)
        root.addSubview(toast)
        toast.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            sidebar.topAnchor.constraint(equalTo: root.topAnchor),
            sidebar.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            sidebar.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            main.leadingAnchor.constraint(equalTo: sidebar.trailingAnchor),
            main.topAnchor.constraint(equalTo: root.topAnchor),
            main.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            main.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            toast.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -16),
            toast.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -16),
        ])
        window.contentView = root
        window.center()
        window.makeKeyAndOrderFront(nil)
        refresh()
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        if tabs.contains(where: \.dirty) {
            showError("Save or discard staged changes before closing DBM.")
            return .terminateCancel
        }
        // Reads can be abandoned; a save or export stopped halfway can't.
        if !saving.isEmpty || !exporting.isEmpty {
            showError("Wait for the save or export to finish before closing DBM.")
            return .terminateCancel
        }
        return .terminateNow
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        applicationShouldTerminate(NSApp) == .terminateNow
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
    func applicationWillTerminate(_ notification: Notification) { bridge.stop() }

    // MARK: Requests

    private func send(_ request: [String: Any], profile: String? = nil,
                      completion: @escaping (Result<Any, Error>) -> Void) {
        let generation = profile.map { generations[$0, default: 0] }
        bridge.send(request) { [weak self] result in
            guard let self else { return }
            if let profile, let generation, self.generations[profile, default: 0] != generation { return }
            completion(result)
        }
    }

    /// App-level errors go to the strip under the top bar.
    func showError(_ message: String) {
        banner.show(.error(message))
        bannerHeight.isActive = false
    }

    private func showTableMessage(_ tab: WorkTab, _ message: InlineMessage) {
        tab.tableState?.message = message
        reloadPane(tab)
    }

    // MARK: Rendering

    func profile(_ id: String) -> Profile? {
        workspaces[id]?.profile ?? profiles.first { $0.id == id }
    }

    func refresh() {
        sidebar.reload()
        tabStrip.reload()
        topBar.show((activeTab?.profileID ?? activeProfileID).flatMap(profile))
        showActivePane()
    }

    private func showActivePane() {
        let current = activeTab.flatMap { tab -> NSView in
            if let pane = panes[tab.id] { return pane }
            let pane: NSView = tab.kind == .query ? QueryPane(tab: tab, host: self) : TablePane(tab: tab, host: self, embedded: false)
            panes[tab.id] = pane
            return pane
        } ?? welcome!
        for view in content.subviews where view !== current { view.removeFromSuperview() }
        if current.superview !== content { content.pin(current) }
        welcome.show(profile: activeProfileID.flatMap(profile),
                     connected: activeProfileID.map { workspaces[$0] != nil } ?? false, hasProfiles: !profiles.isEmpty)
        reloadPane(activeTab)
    }

    private func reloadPane(_ tab: WorkTab?) {
        guard let tab, let pane = panes[tab.id] else { return }
        (pane as? QueryPane)?.reload()
        (pane as? TablePane)?.reload()
    }

    // MARK: Profiles

    func loadProfiles() {
        send(["command": "listProfiles"]) { [weak self] result in
            guard let self else { return }
            switch result {
            case .success(let value):
                self.profiles = array(value).map { Profile(raw: dictionary($0["profile"])) }
                    .sorted { $0.name.lowercased() < $1.name.lowercased() }
                self.refresh()
            case .failure(let error):
                self.showError(error.localizedDescription)
            }
        }
    }

    func newProfile() { presentProfileSheet(nil) }

    func editProfile(_ id: String) {
        guard let profile = profiles.first(where: { $0.id == id }) else { return }
        presentProfileSheet(profile)
    }

    private func presentProfileSheet(_ profile: Profile?) {
        guard sheet == nil else { return }
        let controller = ProfileSheet(
            profile: profile,
            onTest: { [weak self] input, done in
                self?.send(["command": "testProfile", "input": input]) { result in
                    if case .failure(let error) = result { done(error) } else { done(nil) }
                }
            },
            onSave: { [weak self] input, done in self?.saveProfile(input, done: done) },
            onDelete: profile.map { profile in { [weak self] in self?.confirmDelete(profile) } })
        sheet = controller
        guard let sheetWindow = controller.window else { return }
        window.beginSheet(sheetWindow) { [weak self] _ in self?.sheet = nil }
    }

    private func saveProfile(_ input: [String: Any], done: @escaping (Error?) -> Void) {
        if let id = input["id"] as? String, !profileIsClean(id) {
            done(BridgeFailure(message: "Save or discard this connection's staged edits first."))
            return
        }
        send(["command": "saveProfile", "input": input]) { [weak self] result in
            guard let self else { return }
            switch result {
            case .failure(let error):
                done(error)
            case .success(let value):
                let saved = Profile(raw: dictionary(value))
                if let previous = self.profiles.first(where: { $0.id == saved.id }),
                   previous.engine != saved.engine || previous.host != saved.host || previous.port != saved.port
                    || previous.username != saved.username || previous.database != saved.database {
                    // Table tabs belong to the server and database they were opened on.
                    self.closeTableTabs(saved.id)
                }
                self.generations[saved.id, default: 0] += 1
                self.workspaces[saved.id] = nil
                self.schemas[saved.id] = nil
                self.profiles.removeAll { $0.id == saved.id }
                self.profiles.append(saved)
                self.profiles.sort { $0.name.lowercased() < $1.name.lowercased() }
                done(nil)
                self.connect(saved.id)
            }
        }
    }

    private func confirmDelete(_ profile: Profile) {
        guard profileIsClean(profile.id) else { return }
        let alert = NSAlert()
        alert.messageText = "Delete connection “\(profile.name)”?"
        alert.informativeText = "Saved password and query history for this profile will be removed."
        alert.addButton(withTitle: "Delete")
        alert.addButton(withTitle: "Cancel")
        alert.alertStyle = .warning
        alert.buttons.first?.hasDestructiveAction = true
        guard let host = sheet?.window ?? window else { return }
        alert.beginSheetModal(for: host) { [weak self] response in
            guard response == .alertFirstButtonReturn, let self else { return }
            self.send(["command": "deleteProfile", "profile_id": profile.id]) { result in
                switch result {
                case .success:
                    self.profiles.removeAll { $0.id == profile.id }
                    self.closeProfile(profile.id)
                    self.sheet?.dismiss()
                case .failure(let error):
                    self.showError(error.localizedDescription)
                }
            }
        }
    }

    private func profileIsClean(_ id: String) -> Bool {
        let dirty = tabs.contains { $0.profileID == id && $0.dirty }
        if dirty { showError("Save or discard this connection's staged edits first.") }
        return !dirty
    }

    // MARK: Connections

    func selectProfile(_ id: String) {
        if workspaces[id] != nil { activate(id) } else { connect(id) }
    }

    func connect(_ id: String) {
        activeProfileID = id
        connecting.insert(id)
        refresh()
        send(["command": "connect", "profile_id": id], profile: id) { [weak self] result in
            guard let self else { return }
            self.connecting.remove(id)
            switch result {
            case .success(let value): self.workspaceOpened(dictionary(value))
            case .failure(let error): self.showError(error.localizedDescription); self.refresh()
            }
        }
    }

    private func workspaceOpened(_ value: [String: Any]) {
        let profile = Profile(raw: dictionary(value["profile"]))
        let databases = array(value["databases"]).filter { $0["isConnectable"] as? Bool ?? true }.map { string($0["name"]) }
        let id = profile.id
        if let old = workspaces[id], old.profile.database != profile.database {
            // Table tabs were opened on the previous database.
            closeTableTabs(id)
        }
        workspaces[id] = Workspace(profile: profile, databases: databases)
        collapsedProfiles.remove(id)
        activate(id)
        loadSchema(id, announce: false)
    }

    /// Keeps or restores the profile's workbench: its current tab, its latest
    /// tab, or a new query tab.
    func activate(_ id: String) {
        activeProfileID = id
        if activeTab?.profileID != id {
            if let tab = tabs.last(where: { $0.profileID == id }) {
                activeTab = tab
            } else {
                openQuery(id)
                return
            }
        }
        refresh()
    }

    func toggleExpanded(_ id: String) {
        if !collapsedProfiles.insert(id).inserted { collapsedProfiles.remove(id) }
        refresh()
    }

    func disconnect(_ id: String) {
        guard profileIsClean(id) else { return }
        send(["command": "disconnect", "profile_id": id]) { [weak self] _ in self?.closeProfile(id) }
    }

    func switchDatabase(_ id: String, _ database: String) {
        guard workspaces[id]?.profile.database != database else { return }
        guard profileIsClean(id) else { refresh(); return }
        connecting.insert(id)
        refresh()
        send(["command": "connectDatabase", "profile_id": id, "database": database], profile: id) { [weak self] result in
            guard let self else { return }
            self.connecting.remove(id)
            switch result {
            case .success(let value): self.workspaceOpened(dictionary(value))
            case .failure(let error): self.showError(error.localizedDescription); self.refresh()
            }
        }
    }

    func refreshSchema(_ id: String) { loadSchema(id, announce: true) }

    private func loadSchema(_ id: String, announce: Bool) {
        refreshingSchema.insert(id)
        refresh()
        let previous = schemas[id] ?? []
        send(["command": "loadSchemaTree", "profile_id": id], profile: id) { [weak self] result in
            guard let self else { return }
            self.refreshingSchema.remove(id)
            switch result {
            case .success(let value):
                let next = array(value).map(SchemaNode.init)
                self.schemas[id] = next
                if announce {
                    let kind = self.profile(id)?.engine == .redis ? "Keyspace" : "Schema"
                    let reply = Bridge.helper(["command": "describeSchemaRefresh", "previous": previous.map(\.raw),
                                               "next": next.map(\.raw), "kind": kind])
                    if case .success(let value) = reply {
                        let summary = dictionary(value)
                        self.toast.show(string(summary["message"]), success: bool(summary["changed"]))
                    }
                }
            case .failure(let error):
                self.showError(error.localizedDescription)
            }
            self.refresh()
        }
    }

    private func closeProfile(_ id: String) {
        generations[id, default: 0] += 1
        let removed = tabs.filter { $0.profileID == id }
        removed.forEach { panes[$0.id] = nil; running[$0.id] = nil; saving.remove($0.id); exporting.remove($0.id) }
        tabs.removeAll { $0.profileID == id }
        workspaces[id] = nil
        schemas[id] = nil
        connecting.remove(id)
        refreshingSchema.remove(id)
        if activeProfileID == id { activeProfileID = nil }
        if let activeTab, !tabs.contains(where: { $0 === activeTab }) { self.activeTab = nil }
        refresh()
    }

    private func closeTableTabs(_ id: String) {
        let removed = tabs.filter { $0.profileID == id && $0.kind == .table }
        removed.forEach { panes[$0.id] = nil }
        tabs.removeAll { tab in removed.contains { $0 === tab } }
        for tab in tabs where tab.profileID == id && tab.embedded != nil {
            tab.embedded = nil
            tab.tableState = nil
        }
        if let activeTab, removed.contains(where: { $0 === activeTab }) { self.activeTab = nil }
    }

    // MARK: Tabs

    func openQuery(_ id: String) {
        let titles = Set(tabs.filter { $0.profileID == id && $0.kind == .query }.map(\.title))
        var number = 1
        while titles.contains("Query \(number)") { number += 1 }
        let tab = WorkTab(profileID: id, kind: .query, title: "Query \(number)")
        tab.sql = profile(id)?.engine == .redis ? "PING" : "SELECT now();"
        tabs.append(tab)
        activeTab = tab
        activeProfileID = id
        refresh()
    }

    func openTable(_ id: String, schema: String, table: String) {
        if let existing = tabs.first(where: { $0.profileID == id && $0.kind == .table && $0.schema == schema && $0.table == table }) {
            selectTab(existing)
            return
        }
        let tab = WorkTab(profileID: id, kind: .table, title: "\(schema).\(table)")
        tab.schema = schema
        tab.table = table
        tab.tableState = TableState()
        tabs.append(tab)
        activeTab = tab
        activeProfileID = id
        loadTable(tab)
        refresh()
    }

    func selectTab(_ tab: WorkTab) {
        tab.collapsed = false
        activeTab = tab
        activeProfileID = tab.profileID
        refresh()
    }

    func closeTab(_ tab: WorkTab) {
        if tab.dirty {
            showError("Save or discard staged changes before closing this tab.")
            return
        }
        guard let index = tabs.firstIndex(where: { $0 === tab }) else { return }
        tabs.remove(at: index)
        panes[tab.id] = nil
        running[tab.id] = nil
        tableTokens[tab.id] = nil
        if activeTab === tab {
            // The tab to the right, else the one to the left.
            activeTab = index < tabs.count ? tabs[index] : tabs.last
            if let activeTab { activeProfileID = activeTab.profileID }
        }
        refresh()
    }

    /// Shrinks a tab and, if it was active, selects its neighbor, as the
    /// desktop app does.
    func collapseTab(_ tab: WorkTab) {
        guard let index = tabs.firstIndex(where: { $0 === tab }) else { return }
        tab.collapsed = true
        if activeTab === tab {
            let next = index + 1 < tabs.count ? tabs[index + 1] : index > 0 ? tabs[index - 1] : nil
            if let next { selectTab(next); return }
            activeTab = nil
        }
        refresh()
    }

    func renameTab(_ tab: WorkTab, _ title: String) {
        let trimmed = title.trimmingCharacters(in: .whitespaces)
        if !trimmed.isEmpty { tab.title = trimmed }
        refresh()
    }

    // MARK: QueryHost

    func engine(of tab: WorkTab) -> Engine { profile(tab.profileID)?.engine ?? .postgres }

    func history(for tab: WorkTab) -> [[String: Any]] {
        let database = workspaces[tab.profileID]?.profile.database ?? ""
        let key = "\(tab.profileID)|\(database)"
        if histories[key] == nil, workspaces[tab.profileID] != nil {
            histories[key] = []
            loadHistory(tab.profileID)
        }
        return histories[key] ?? []
    }

    private func loadHistory(_ id: String) {
        guard let database = workspaces[id]?.profile.database else { return }
        send(["command": "listQueryHistory", "profile_id": id, "database": database, "limit": 100], profile: id) { [weak self] result in
            guard let self, case .success(let value) = result else { return }
            self.histories["\(id)|\(database)"] = array(value)
            self.reloadPane(self.activeTab)
        }
    }

    func isRunning(_ tab: WorkTab) -> String? { running[tab.id] }

    func runQuery(_ tab: WorkTab, sql: String, refresh: Bool) {
        let sql = sql.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !sql.isEmpty, running[tab.id] == nil else { return }
        if tab.dirty {
            tab.queryError = .error("Save or discard the pending table changes before running another query.")
            reloadPane(tab)
            return
        }
        let engine = self.engine(of: tab)
        if Helpers.requiresConfirmation(engine: engine, text: sql) {
            let alert = NSAlert()
            alert.messageText = "Run this query?"
            alert.informativeText = "This query may change or remove many rows.\n\n\(sql.prefix(400))"
            alert.addButton(withTitle: "Run anyway")
            alert.addButton(withTitle: "Cancel")
            alert.alertStyle = .warning
            alert.buttons.first?.hasDestructiveAction = true
            alert.beginSheetModal(for: window) { [weak self] response in
                if response == .alertFirstButtonReturn { self?.execute(tab, sql: sql, refresh: refresh) }
            }
            return
        }
        execute(tab, sql: sql, refresh: refresh)
    }

    private func execute(_ tab: WorkTab, sql: String, refresh: Bool) {
        running[tab.id] = refresh ? "refresh" : "run"
        tab.queryError = nil
        reloadPane(tab)
        send(["command": "query", "request": ["profileId": tab.profileID, "sql": sql, "maxRows": 10_000]], profile: tab.profileID) { [weak self] result in
            guard let self, self.tabs.contains(where: { $0 === tab }) else { return }
            self.running[tab.id] = nil
            switch result {
            case .success(let value):
                tab.lastExecuted = sql
                tab.embedded = Helpers.resolveFullTableSelect(sql, tree: self.schemas[tab.profileID] ?? [])
                tab.tableState = tab.embedded == nil ? nil : TableState()
                tab.result = dictionary(value)
                if tab.embedded != nil { self.loadTable(tab) }
            case .failure(let error):
                tab.queryError = .error(error.localizedDescription)
            }
            self.loadHistory(tab.profileID)
            self.reloadPane(tab)
        }
    }

    // MARK: TableHost

    func isReadOnly(_ tab: WorkTab) -> Bool { profile(tab.profileID)?.readOnly ?? true }
    func isSaving(_ tab: WorkTab) -> Bool { saving.contains(tab.id) }
    func isExporting(_ tab: WorkTab) -> Bool { exporting.contains(tab.id) }

    func loadTable(_ tab: WorkTab) {
        guard let state = tab.tableState, let target = tab.target else { return }
        guard state.pending.isEmpty else { showTableMessage(tab, .error(pendingRefreshError)); return }
        // Paging, filtering, or sorting replaces a stale notice, as in the
        // desktop app; errors and export results stay until they expire.
        if state.message?.isPlainNotice == true { state.message = nil }
        let request = state.request(profileID: tab.profileID, schema: target.schema, table: target.table)
        state.requested = request
        state.loading = true
        let token = UUID()
        tableTokens[tab.id] = token
        reloadPane(tab)
        send(["command": "loadTablePage", "request": request], profile: tab.profileID) { [weak self] result in
            guard let self, self.tableTokens[tab.id] == token, tab.tableState === state else { return }
            switch result {
            case .success(let value):
                state.loaded(TablePage(dictionary(value)))
            case .failure(let error):
                state.restoreLoaded()
                state.message = .error(error.localizedDescription)
            }
            self.reloadPane(tab)
            self.sidebar.reload()
        }
    }

    func saveChanges(_ tab: WorkTab) {
        guard let state = tab.tableState, let target = tab.target, !state.pending.isEmpty, !saving.contains(tab.id) else { return }
        let batch: [String: Any] = [
            "profileId": tab.profileID, "schema": target.schema, "table": target.table,
            "mutations": state.pending.keys.sorted().compactMap { state.pending[$0]?.json },
        ]
        saving.insert(tab.id)
        reloadPane(tab)
        send(["command": "applyTableMutations", "batch": batch], profile: tab.profileID) { [weak self] result in
            guard let self else { return }
            self.saving.remove(tab.id)
            switch result {
            case .success(let value):
                // As in the desktop app: clear staged rows and reload, so rows
                // another writer changed show their current values.
                state.pending.removeAll()
                let reply = dictionary(value)
                let conflicts = (reply["conflicts"] as? [Any])?.count ?? 0
                let applied = int(reply["applied"])
                self.loadTable(tab)
                state.message = conflicts > 0
                    ? .error("\(conflicts) row conflict(s); the table was refreshed.")
                    : .notice("\(applied) \(applied == 1 ? "change" : "changes") saved.")
            case .failure(let error):
                state.message = .error(error.localizedDescription)
            }
            self.reloadPane(tab)
        }
    }

    func exportTable(_ tab: WorkTab) {
        guard let state = tab.tableState, let target = tab.target, let page = state.page, let loaded = state.loaded else { return }
        let proceed = { [weak self] in self?.chooseExportDestination(tab, target: target, page: page, request: loaded) }
        if let total = page.total, total > 100_000 {
            let alert = NSAlert()
            alert.messageText = "Export a large table?"
            alert.informativeText = "This export contains \(total) rows and may take a while. Continue?"
            alert.addButton(withTitle: "Export")
            alert.addButton(withTitle: "Cancel")
            alert.beginSheetModal(for: window) { response in if response == .alertFirstButtonReturn { proceed() } }
        } else {
            proceed()
        }
    }

    private func chooseExportDestination(_ tab: WorkTab, target: (schema: String, table: String), page: TablePage, request: [String: Any]) {
        let panel = NSSavePanel()
        let safe = { (value: String) in String(value.map { "\\/:*?\"<>|".contains($0) ? "_" : $0 }) }
        panel.nameFieldStringValue = "\(safe(target.schema)).\(safe(target.table)).csv"
        panel.allowedContentTypes = [.commaSeparatedText]
        panel.beginSheetModal(for: window) { [weak self] response in
            guard let self else { return }
            guard response == .OK, let url = panel.url else { self.showTableMessage(tab, .notice("Export canceled.")); return }
            self.exporting.insert(tab.id)
            self.exportRows[tab.id] = 0
            tab.tableState?.message = nil
            self.reloadPane(tab)
            // The export holds the session, so poll the session-free helper.
            let poll = Timer.scheduledTimer(withTimeInterval: 0.25, repeats: true) { [weak self] _ in
                guard let self, self.exporting.contains(tab.id) else { return }
                if case .success(let rows) = Bridge.helper(["command": "exportProgress", "path": url.path]), !(rows is NSNull) {
                    self.exportRows[tab.id] = int(rows)
                    self.reloadPane(tab)
                }
            }
            let command: [String: Any] = ["command": "exportCsv", "request": request, "columns": page.columns.map(\.name), "path": url.path]
            self.send(command, profile: tab.profileID) { result in
                poll.invalidate()
                self.exporting.remove(tab.id)
                self.exportRows[tab.id] = nil
                switch result {
                case .success(let value):
                    tab.tableState?.message = .exported(url, rows: int(dictionary(value)["rows"]))
                case .failure(let error):
                    tab.tableState?.message = .error(error.localizedDescription)
                }
                self.reloadPane(tab)
            }
        }
    }

    func revealExport(_ url: URL, open: Bool) {
        if open { NSWorkspace.shared.open(url) } else { NSWorkspace.shared.activateFileViewerSelecting([url]) }
    }

    func exportProgress(_ tab: WorkTab) -> Int? { exportRows[tab.id] }

    func copyText(_ text: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }

    // MARK: Menus

    @objc private func menuNewConnection() { newProfile() }
    @objc private func menuNewQuery() { if let id = activeProfileID, workspaces[id] != nil { openQuery(id) } }
    @objc private func menuCloseTab() { if let activeTab { closeTab(activeTab) } else { window.performClose(nil) } }

    private func installMenu() {
        let menu = NSMenu()
        let appItem = NSMenuItem()
        menu.addItem(appItem)
        let appMenu = NSMenu()
        appMenu.addItem(withTitle: "About DBM Native", action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)), keyEquivalent: "")
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "Hide DBM Native", action: #selector(NSApplication.hide(_:)), keyEquivalent: "h")
        appMenu.addItem(withTitle: "Quit DBM Native", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        appItem.submenu = appMenu

        let fileItem = NSMenuItem()
        menu.addItem(fileItem)
        let fileMenu = NSMenu(title: "File")
        fileMenu.addItem(withTitle: "New Connection…", action: #selector(menuNewConnection), keyEquivalent: "n").target = self
        fileMenu.addItem(withTitle: "New Query", action: #selector(menuNewQuery), keyEquivalent: "t").target = self
        fileMenu.addItem(.separator())
        fileMenu.addItem(withTitle: "Close Tab", action: #selector(menuCloseTab), keyEquivalent: "w").target = self
        fileItem.submenu = fileMenu

        let editItem = NSMenuItem()
        menu.addItem(editItem)
        let editMenu = NSMenu(title: "Edit")
        editMenu.addItem(withTitle: "Undo", action: Selector(("undo:")), keyEquivalent: "z")
        editMenu.addItem(withTitle: "Redo", action: Selector(("redo:")), keyEquivalent: "Z")
        editMenu.addItem(.separator())
        for (title, action, key) in [("Cut", "cut:", "x"), ("Copy", "copy:", "c"), ("Paste", "paste:", "v"), ("Select All", "selectAll:", "a")] {
            editMenu.addItem(withTitle: title, action: Selector((action)), keyEquivalent: key)
        }
        editMenu.addItem(.separator())
        editMenu.addItem(withTitle: "Complete", action: #selector(NSTextView.complete(_:)), keyEquivalent: "\u{1b}").keyEquivalentModifierMask = [.option]
        editItem.submenu = editMenu

        let windowItem = NSMenuItem()
        menu.addItem(windowItem)
        let windowMenu = NSMenu(title: "Window")
        windowMenu.addItem(withTitle: "Minimize", action: #selector(NSWindow.performMiniaturize(_:)), keyEquivalent: "m")
        windowMenu.addItem(withTitle: "Zoom", action: #selector(NSWindow.performZoom(_:)), keyEquivalent: "")
        windowItem.submenu = windowMenu
        NSApp.windowsMenu = windowMenu
        NSApp.mainMenu = menu
    }

    // MARK: Snapshot support

    var activeQueryPane: QueryPane? { activeTab.flatMap { panes[$0.id] as? QueryPane } }
    var activeTablePane: TablePane? { activeTab.flatMap { panes[$0.id] as? TablePane } }
    var profileSheetWindow: NSWindow? { sheet?.window }
}

/// `--demo --snapshot-dir DIR` walks the fixture through the main states and
/// writes a PNG of each, for CI review without a Mac.
final class SnapshotDriver {
    private let app: AppController
    private let directory: String
    private var steps: [(String, () -> Void)] = []

    init(app: AppController, directory: String) {
        self.app = app
        self.directory = directory
    }

    func start() {
        let app = self.app
        let postgres = "00000000-0000-0000-0000-000000000001"
        let redis = "00000000-0000-0000-0000-000000000002"
        steps = [
            ("01-welcome", {}),
            ("02-query", { [app] in app.selectProfile(postgres) }),
            ("03-results", { [app] in app.activeQueryPane?.editor.setSelectedRange(NSRange(location: 0, length: 0)); if let tab = app.activeTab { app.runQuery(tab, sql: "SELECT customer, revenue FROM revenue_by_month;", refresh: false) } }),
            ("04-table", { [app] in app.openTable(postgres, schema: "public", table: "customers") }),
            ("05-pending", { [app] in
                guard let tab = app.activeTab, let state = tab.tableState else { return }
                state.selected = IndexSet(integer: 1)
                state.stage(row: 1, column: 1, text: "ada@example.com")
                state.stage(row: 1, column: 3, text: "")
                state.toggleDelete([3])
                app.activeTablePane?.grid.reloadData()
                app.refresh()
            }),
            ("06-embedded", { [app] in
                app.openQuery(postgres)
                if let tab = app.activeTab {
                    app.activeQueryPane?.editor.string = "SELECT * FROM public.customers;"
                    tab.sql = "SELECT * FROM public.customers;"
                    app.runQuery(tab, sql: tab.sql, refresh: false)
                }
            }),
            ("07-profile", { [app] in app.newProfile() }),
            ("08-redis", { [app] in
                if let sheet = app.profileSheetWindow { app.window.endSheet(sheet) }
                app.selectProfile(redis)
            }),
            ("09-redis-hash", { [app] in app.openTable(redis, schema: "hash", table: "user:1") }),
            ("10-messages", { [app] in
                app.openTable(postgres, schema: "public", table: "orders")
                app.activeTab?.tableState?.message = .exported(URL(fileURLWithPath: "/tmp/public.orders.csv"), rows: 500)
                app.showError("Demo fixture: saving is disabled.")
                app.refresh()
            }),
            ("11-query-error", { [app] in
                app.openQuery(postgres)
                app.activeTab?.queryError = .error("database error: syntax error at or near \"SELEC\" (SQLSTATE 42601)")
                app.refresh()
            }),
        ]
        next(0)
    }

    private func next(_ index: Int) {
        guard index < steps.count else { exit(0) }
        let (name, action) = steps[index]
        action()
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.2) { [self] in
            capture(name)
            next(index + 1)
        }
    }

    private func capture(_ name: String) {
        guard let view = app.window.contentView else { exit(1) }
        view.layoutSubtreeIfNeeded()
        guard let bitmap = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { exit(1) }
        view.cacheDisplay(in: view.bounds, to: bitmap)
        write(bitmap, name)
        if let sheet = app.profileSheetWindow?.contentView, let sheetBitmap = sheet.bitmapImageRepForCachingDisplay(in: sheet.bounds) {
            sheet.cacheDisplay(in: sheet.bounds, to: sheetBitmap)
            write(sheetBitmap, "\(name)-sheet")
        }
    }

    private func write(_ bitmap: NSBitmapImageRep, _ name: String) {
        guard let png = bitmap.representation(using: .png, properties: [:]) else { exit(1) }
        do {
            try png.write(to: URL(fileURLWithPath: directory).appendingPathComponent("\(name).png"))
        } catch {
            FileHandle.standardError.write("snapshot failed: \(error)\n".data(using: .utf8)!)
            exit(1)
        }
    }
}

let application = NSApplication.shared
let controller = AppController()
application.delegate = controller
application.setActivationPolicy(.regular)
application.run()
