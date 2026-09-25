import AppKit

final class Workbench: NSObject, NSApplicationDelegate, NSWindowDelegate,
    NSTableViewDataSource, NSTableViewDelegate, NSOutlineViewDataSource, NSOutlineViewDelegate, NSTextFieldDelegate {
    private var window: NSWindow!, bridge: Bridge!
    private let profileList = NSTableView(), schemaTree = NSOutlineView(), grid = NSTableView()
    private let database = NSPopUpButton(), tabs = NSPopUpButton(), editor = QueryTextView()
    private let editorScroll = NSScrollView(), gridScroll = NSScrollView()
    private let status = NSTextField(labelWithString: "Loading connections…")
    private let filterColumn = NSPopUpButton(), filterOperator = NSPopUpButton()
    private let filterValue = NSTextField(), pageLabel = NSTextField(labelWithString: "")
    private let runButton = NSButton(title: "Run", target: nil, action: nil)
    private let refreshButton = NSButton(title: "Refresh", target: nil, action: nil)
    private let previousButton = NSButton(title: "‹", target: nil, action: nil)
    private let nextButton = NSButton(title: "›", target: nil, action: nil)
    private let discardButton = NSButton(title: "Discard", target: nil, action: nil)
    private let saveButton = NSButton(title: "Save changes", target: nil, action: nil)
    private let pending = NSTextField(labelWithString: "")
    private var profiles: [Profile] = [], active: Profile?, schema: [SchemaItem] = []
    private var workTabs: [WorkTab] = [], activeTab: WorkTab?
    private var connected = Set<String>(), globalBusy = false
    private var workspaces: [String: Profile] = [:]
    private var databases: [String: [[String: Any]]] = [:]
    private var schemaCache: [String: [SchemaItem]] = [:]
    private let demo = CommandLine.arguments.contains("--demo")
    private var idle: Bool { !globalBusy && !workTabs.contains(where: { $0.busy }) }

    func applicationDidFinishLaunching(_ notification: Notification) {
        bridge = Bridge(); installMenu(); buildWindow()
        if demo { loadDemo() } else { loadProfiles() }
    }

    private func loadDemo() {
        window.title = "DBM Native — DEMO FIXTURE (no database access)"
        let profile = Profile(["id": "00000000-0000-0000-0000-000000000001", "name": "Acme Analytics", "engine": "postgres", "defaultDatabase": "analytics", "color": "#3dd6c6", "readOnly": false])
        profiles = [profile]; workspaces[profile.id] = profile; connected.insert(profile.id)
        databases[profile.id] = [["name": "analytics", "isConnectable": true]]
        schemaCache[profile.id] = [SchemaItem(["name": "public", "children": [["name": "customers", "schema": "public", "table": "customers"]]])]
        let tab = WorkTab(profileID: profile.id, kind: .table, title: "public.customers")
        tab.schema = "public"; tab.table = "customers"
        tab.columns = [["name": "id", "dataType": "bigint"], ["name": "email", "dataType": "text"], ["name": "active", "dataType": "boolean"]]
        tab.metadata = ["columns": tab.columns, "primaryKey": ["id"], "hasXmin": true]
        tab.rows = (1...20).map { [$0, "person\($0)@example.com", $0 % 3 != 0, "xmin-\($0)"] as [Any] }
        tab.total = 20; workTabs = [tab]; profileList.reloadData(); reloadTabs(); selectTab(tab)
        status.stringValue = "DEMO · isolated fixture · no database or credential access"
        if let index = CommandLine.arguments.firstIndex(of: "--snapshot"), CommandLine.arguments.indices.contains(index + 1) {
            let path = CommandLine.arguments[index + 1]
            DispatchQueue.main.asyncAfter(deadline: .now() + 1) { [self] in
                guard let view = window.contentView else { exit(1) }
                view.layoutSubtreeIfNeeded()
                guard let bitmap = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { exit(1) }
                view.cacheDisplay(in: view.bounds, to: bitmap)
                guard let png = bitmap.representation(using: .png, properties: [:]) else { exit(1) }
                do { try png.write(to: URL(fileURLWithPath: path)); exit(0) } catch { exit(1) }
            }
        }
    }

    private func buildWindow() {
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1280, height: 800),
                          styleMask: [.titled, .closable, .miniaturizable, .resizable],
                          backing: .buffered, defer: false)
        window.title = "DBM Native"; window.minSize = NSSize(width: 940, height: 620)
        window.delegate = self; window.appearance = NSAppearance(named: .darkAqua)
        let root = NSSplitView(); root.isVertical = true; root.dividerStyle = .thin
        root.addArrangedSubview(buildSidebar()); root.addArrangedSubview(buildWorkbench())
        root.setPosition(260, ofDividerAt: 0); window.contentView = root
        window.center(); window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
    }

    private func buildSidebar() -> NSView {
        let box = NSView(); box.wantsLayer = true; box.layer?.backgroundColor = Graphite.sidebar.cgColor
        let stack = NSStackView(); stack.orientation = .vertical; stack.spacing = 8
        stack.edgeInsets = NSEdgeInsets(top: 14, left: 12, bottom: 12, right: 12)
        stack.translatesAutoresizingMaskIntoConstraints = false; box.addSubview(stack)
        NSLayoutConstraint.activate([stack.leadingAnchor.constraint(equalTo: box.leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: box.trailingAnchor), stack.topAnchor.constraint(equalTo: box.topAnchor),
            stack.bottomAnchor.constraint(equalTo: box.bottomAnchor), box.widthAnchor.constraint(greaterThanOrEqualToConstant: 210)])
        let title = label("Connections", size: 11, weight: .semibold); stack.addArrangedSubview(title)
        profileList.headerView = nil; profileList.rowHeight = 30; profileList.delegate = self; profileList.dataSource = self
        profileList.target = self; profileList.doubleAction = #selector(connectSelected)
        profileList.addTableColumn(NSTableColumn(identifier: .init("profile")))
        let profilesScroll = scroll(profileList); profilesScroll.heightAnchor.constraint(equalToConstant: 170).isActive = true
        stack.addArrangedSubview(profilesScroll)
        let profileActions = NSStackView(views: [button("＋", #selector(newProfile)), button("Edit", #selector(editProfile)), button("−", #selector(deleteProfile))])
        profileActions.spacing = 6; stack.addArrangedSubview(profileActions)
        stack.addArrangedSubview(label("Database", size: 11, weight: .semibold))
        database.target = self; database.action = #selector(changeDatabase); database.isEnabled = false
        stack.addArrangedSubview(database)
        let schemaTitle = NSStackView(views: [label("Schema", size: 11, weight: .semibold), button("↻", #selector(loadSchema))])
        schemaTitle.distribution = .fill; stack.addArrangedSubview(schemaTitle)
        schemaTree.headerView = nil; schemaTree.rowHeight = 25; schemaTree.delegate = self; schemaTree.dataSource = self
        schemaTree.target = self; schemaTree.doubleAction = #selector(openSchemaItem)
        schemaTree.addTableColumn(NSTableColumn(identifier: .init("schema"))); schemaTree.outlineTableColumn = schemaTree.tableColumns[0]
        let treeScroll = scroll(schemaTree); stack.addArrangedSubview(treeScroll)
        for view in stack.arrangedSubviews { view.widthAnchor.constraint(equalTo: box.widthAnchor, constant: -24).isActive = true }
        return box
    }

    private func buildWorkbench() -> NSView {
        let box = NSView(); box.wantsLayer = true; box.layer?.backgroundColor = Graphite.bg.cgColor
        let stack = NSStackView(); stack.orientation = .vertical; stack.spacing = 0
        stack.translatesAutoresizingMaskIntoConstraints = false; box.addSubview(stack)
        NSLayoutConstraint.activate([stack.leadingAnchor.constraint(equalTo: box.leadingAnchor), stack.trailingAnchor.constraint(equalTo: box.trailingAnchor),
            stack.topAnchor.constraint(equalTo: box.topAnchor), stack.bottomAnchor.constraint(equalTo: box.bottomAnchor)])
        let top = bar([tabs, button("＋ Query", #selector(newQuery)), button("Close", #selector(closeTab)), button("History", #selector(showHistory))], 42)
        tabs.target = self; tabs.action = #selector(changeTab); tabs.widthAnchor.constraint(greaterThanOrEqualToConstant: 260).isActive = true
        stack.addArrangedSubview(top)
        runButton.target = self; runButton.action = #selector(runQuery); refreshButton.target = self; refreshButton.action = #selector(refresh)
        let toolbar = bar([runButton, refreshButton, filterColumn, filterOperator, filterValue, button("Apply filter", #selector(applyFilter)), button("Delete row", #selector(stageDelete))], 44)
        filterOperator.addItems(withTitles: ["contains", "equals", "notEquals", "startsWith", "endsWith", "greaterThan", "lessThan", "isNull", "isNotNull"])
        filterValue.placeholderString = "Filter value"; filterValue.widthAnchor.constraint(equalToConstant: 140).isActive = true
        stack.addArrangedSubview(toolbar)
        editor.isRichText = false; editor.allowsUndo = true; editor.font = .monospacedSystemFont(ofSize: 13, weight: .regular)
        editor.backgroundColor = Graphite.bg; editor.textColor = Graphite.text; editor.textContainerInset = NSSize(width: 12, height: 12)
        editor.isAutomaticQuoteSubstitutionEnabled = false; editor.isAutomaticDashSubstitutionEnabled = false
        editor.isAutomaticTextReplacementEnabled = false
        editor.isVerticallyResizable = true; editor.autoresizingMask = [.width]
        editor.textContainer?.widthTracksTextView = true
        editor.runAction = { [weak self] in self?.runQuery() }
        editorScroll.documentView = editor; editorScroll.hasVerticalScroller = true; editorScroll.drawsBackground = true; editorScroll.backgroundColor = Graphite.bg
        editorScroll.heightAnchor.constraint(equalToConstant: 190).isActive = true; stack.addArrangedSubview(editorScroll)
        grid.dataSource = self; grid.delegate = self; grid.rowHeight = 32; grid.usesAlternatingRowBackgroundColors = false
        grid.columnAutoresizingStyle = .noColumnAutoresizing; grid.backgroundColor = Graphite.bg; grid.gridStyleMask = [.solidHorizontalGridLineMask]
        grid.gridColor = NSColor(hex: 0x202023); grid.allowsMultipleSelection = false
        gridScroll.documentView = grid; gridScroll.hasVerticalScroller = true; gridScroll.hasHorizontalScroller = true
        stack.addArrangedSubview(gridScroll)
        pending.textColor = Graphite.modified; discardButton.target = self; discardButton.action = #selector(discardChanges)
        saveButton.target = self; saveButton.action = #selector(saveChanges)
        stack.addArrangedSubview(bar([pending, discardButton, saveButton], 48))
        previousButton.target = self; previousButton.action = #selector(previousPage); nextButton.target = self; nextButton.action = #selector(nextPage)
        status.textColor = Graphite.muted; status.lineBreakMode = .byTruncatingTail
        stack.addArrangedSubview(bar([status, pageLabel, previousButton, nextButton], 30))
        for view in stack.arrangedSubviews { view.widthAnchor.constraint(equalTo: box.widthAnchor).isActive = true }
        updateControls(); return box
    }

    private func bar(_ views: [NSView], _ height: CGFloat) -> NSView {
        let stack = NSStackView(views: views); stack.spacing = 8; stack.alignment = .centerY
        stack.edgeInsets = NSEdgeInsets(top: 6, left: 10, bottom: 6, right: 10); stack.wantsLayer = true
        stack.layer?.backgroundColor = Graphite.chrome.cgColor; stack.heightAnchor.constraint(equalToConstant: height).isActive = true
        if let first = views.first { first.setContentHuggingPriority(.defaultLow, for: .horizontal) }
        return stack
    }

    private func scroll(_ document: NSView) -> NSScrollView {
        let value = NSScrollView(); value.documentView = document; value.hasVerticalScroller = true
        value.drawsBackground = false; value.borderType = .noBorder; return value
    }
    private func label(_ text: String, size: CGFloat, weight: NSFont.Weight) -> NSTextField {
        let value = NSTextField(labelWithString: text); value.font = .systemFont(ofSize: size, weight: weight); value.textColor = Graphite.muted; return value
    }
    private func button(_ title: String, _ action: Selector) -> NSButton {
        let value = NSButton(title: title, target: self, action: action); value.bezelStyle = .rounded; value.controlSize = .small; return value
    }

    private func send(_ request: [String: Any], message: String, tab: WorkTab? = nil,
                      completion: @escaping (Result<Any, Error>) -> Void) {
        if demo { status.stringValue = "Demo fixture: database operations are disabled."; return }
        guard !globalBusy, !workTabs.contains(where: { $0.busy }) else { return }
        if let tab { guard !tab.busy else { return }; tab.busy = true }
        else { guard !globalBusy else { return }; globalBusy = true }
        status.stringValue = message; updateControls()
        bridge.send(request) { [weak self, weak tab] result in
            guard let self else { return }
            if let tab { tab.busy = false } else { self.globalBusy = false }
            if case .failure(let error) = result { self.status.stringValue = error.localizedDescription }
            self.updateControls(); completion(result)
        }
    }

    @objc private func loadProfiles() {
        send(["command": "listProfiles"], message: "Loading saved connections…") { [weak self] result in
            guard let self, case .success(let value) = result else { return }
            profiles = array(value).compactMap { dictionary($0["profile"]) }.map(Profile.init)
            profileList.reloadData(); status.stringValue = profiles.isEmpty ? "No saved connections. Use + to create one." : "Double-click a connection to open it."
        }
    }

    @objc private func connectSelected() {
        let row = profileList.selectedRow; guard idle, profiles.indices.contains(row) else { return }
        let target = profiles[row]
        if connected.contains(target.id) { ensureQueryTab(for: target); return }
        send(["command": "connect", "profile_id": target.id], message: "Connecting to \(target.name)…") { [weak self] result in
            guard let self, case .success(let value) = result, let workspace = dictionary(value), let profileRaw = dictionary(workspace["profile"]) else { return }
            active = Profile(profileRaw); connected.insert(target.id); database.removeAllItems()
            workspaces[target.id] = active; databases[target.id] = array(workspace["databases"])
            for db in array(workspace["databases"]) where bool(db["isConnectable"]) { database.addItem(withTitle: string(db["name"])) }
            database.selectItem(withTitle: active?.database ?? ""); database.isEnabled = true
            ensureQueryTab(for: target); loadSchema(); status.stringValue = "Connected · \(target.database)"
        }
    }

    private func ensureQueryTab(for profile: Profile) {
        if let existing = workTabs.last(where: { $0.profileID == profile.id }) { selectTab(existing); return }
        let tab = WorkTab(profileID: profile.id, kind: .query, title: "Query")
        tab.sql = profile.engine == "redis" ? "PING" : "SELECT 1;"; workTabs.append(tab); reloadTabs(); selectTab(tab)
    }

    @objc private func loadSchema() {
        guard let profile = active else { return }
        let expected = profile.id
        send(["command": "loadSchemaTree", "profile_id": expected], message: "Loading schema…") { [weak self] result in
            guard let self, active?.id == expected, case .success(let value) = result else { return }
            schema = array(value).map(SchemaItem.init); schemaTree.reloadData()
            schemaCache[expected] = schema
            for item in schema { schemaTree.expandItem(item) }; status.stringValue = "Schema refreshed"
        }
    }

    @objc private func changeDatabase() {
        guard let profile = active, let selected = database.selectedItem?.title, selected != profile.database else { return }
        guard !globalBusy, !workTabs.contains(where: { $0.busy }), confirmDiscard(exceptProfile: profile.id) else { database.selectItem(withTitle: profile.database); return }
        let expected = profile.id
        send(["command": "connectDatabase", "profile_id": expected, "database": selected], message: "Switching database…") { [weak self] result in
            guard let self, active?.id == expected else { return }
            if case .success(let value) = result, let workspace = dictionary(value), let raw = dictionary(workspace["profile"]) {
                active = Profile(raw); workTabs.removeAll { $0.profileID == expected && $0.kind == .table }
                workspaces[expected] = active
                for tab in workTabs where tab.profileID == expected { tab.requestToken = UUID(); tab.rows = []; tab.columns = [] }
                ensureQueryTab(for: active!); reloadTabs(); loadSchema(); status.stringValue = "Using \(selected)"
            } else { database.selectItem(withTitle: profile.database) }
        }
    }

    @objc private func openSchemaItem() {
        guard idle, let item = schemaTree.item(atRow: schemaTree.clickedRow) as? SchemaItem, !item.table.isEmpty, let profile = active else { return }
        if let existing = workTabs.first(where: { $0.profileID == profile.id && $0.kind == .table && $0.schema == item.schema && $0.table == item.table }) { selectTab(existing); return }
        let tab = WorkTab(profileID: profile.id, kind: .table, title: item.name); tab.schema = item.schema; tab.table = item.table
        workTabs.append(tab); reloadTabs(); selectTab(tab); loadTable(tab)
    }

    @objc private func newQuery() { guard idle, let profile = active else { return }; let tab = WorkTab(profileID: profile.id, kind: .query, title: "Query \(workTabs.filter { $0.kind == .query }.count + 1)"); tab.sql = profile.engine == "redis" ? "PING" : ""; workTabs.append(tab); reloadTabs(); selectTab(tab) }
    @objc private func changeTab() { guard workTabs.indices.contains(tabs.indexOfSelectedItem) else { return }; selectTab(workTabs[tabs.indexOfSelectedItem]) }
    private func selectTab(_ tab: WorkTab) {
        guard idle else { return }
        if let previous = activeTab, previous !== tab, previous.kind == .query { previous.sql = editor.string }
        activeTab = tab; active = workspaces[tab.profileID]
        schema = schemaCache[tab.profileID] ?? []; schemaTree.reloadData()
        database.removeAllItems()
        for db in databases[tab.profileID] ?? [] where bool(db["isConnectable"]) { database.addItem(withTitle: string(db["name"])) }
        database.selectItem(withTitle: active?.database ?? "")
        tabs.selectItem(at: workTabs.firstIndex { $0 === tab } ?? 0); editor.string = tab.sql; showTab(tab)
    }
    private func reloadTabs() { tabs.removeAllItems(); workTabs.forEach { tabs.menu?.addItem(NSMenuItem(title: $0.title, action: nil, keyEquivalent: "")) }; if let activeTab, let index = workTabs.firstIndex(where: { $0 === activeTab }) { tabs.selectItem(at: index) } }
    @objc private func closeTab() { guard idle, let tab = activeTab, (!tab.dirty || confirmDiscard(tab: tab)) else { return }; workTabs.removeAll { $0 === tab }; activeTab = workTabs.last(where: { $0.profileID == tab.profileID }); reloadTabs(); if let activeTab { selectTab(activeTab) } else { clearGrid() } }

    @objc private func runQuery() {
        guard let tab = activeTab, tab.kind == .query, !globalBusy, !workTabs.contains(where: { $0.busy }) else { return }
        tab.sql = editor.string; let selection = editor.selectedRange(); let selected = selection.length > 0 ? (editor.string as NSString).substring(with: selection) : editor.string
        let sql = selected.trimmingCharacters(in: .whitespacesAndNewlines); guard !sql.isEmpty else { return }
        tab.lastExecuted = sql; executeQuery(tab, sql)
    }
    private func executeQuery(_ tab: WorkTab, _ sql: String) {
        let token = UUID(); tab.requestToken = token; tab.rows = []; tab.columns = []; showTab(tab)
        send(["command": "query", "request": ["profileId": tab.profileID, "sql": sql, "maxRows": 10000]], message: "Running query…", tab: tab) { [weak self, weak tab] result in
            guard let self, let tab, tab.requestToken == token, workTabs.contains(where: { $0 === tab }), case .success(let value) = result, let response = dictionary(value) else { return }
            tab.columns = array(response["columns"]); tab.rows = response["rows"] as? [[Any]] ?? []
            if activeTab === tab { showTab(tab) }; status.stringValue = "\(tab.rows.count) rows · \(int(response["durationMs"])) ms\(bool(response["truncated"]) ? " · truncated" : "")"
        }
    }

    private func loadTable(_ tab: WorkTab) {
        guard !globalBusy, !workTabs.contains(where: { $0.busy }), !tab.dirty else { return }; let token = UUID(); tab.requestToken = token
        var filters: [[String: Any]] = []
        if !tab.filterColumn.isEmpty { filters = [["column": tab.filterColumn, "operator": tab.filterOperator, "value": ["isNull", "isNotNull"].contains(tab.filterOperator) ? NSNull() : tab.filterValue]] }
        let order: Any = tab.orderColumn.isEmpty ? NSNull() : ["column": tab.orderColumn, "descending": tab.descending]
        let request: [String: Any] = ["profileId": tab.profileID, "schema": tab.schema, "table": tab.table, "offset": tab.offset, "limit": tab.limit, "filters": filters, "orderBy": order, "includeTotal": true]
        send(["command": "loadTablePage", "request": request], message: "Loading \(tab.schema).\(tab.table)…", tab: tab) { [weak self, weak tab] result in
            guard let self, let tab, tab.requestToken == token, workTabs.contains(where: { $0 === tab }), case .success(let value) = result, let page = dictionary(value) else { return }
            tab.metadata = dictionary(page["metadata"]) ?? [:]; tab.rows = page["rows"] as? [[Any]] ?? []; tab.columns = array(tab.metadata["columns"])
            tab.total = (page["totalRows"] as? NSNumber)?.intValue; tab.hasMore = bool(page["hasMore"]); tab.mutations.removeAll()
            if activeTab === tab { showTab(tab) }; status.stringValue = "Loaded \(tab.rows.count) rows"
        }
    }

    @objc private func refresh() { guard idle, let tab = activeTab else { return }; if tab.dirty && !confirmDiscard(tab: tab) { return }; tab.kind == .query ? executeQuery(tab, tab.lastExecuted.isEmpty ? editor.string : tab.lastExecuted) : loadTable(tab) }
    @objc private func applyFilter() { guard idle, let tab = activeTab, tab.kind == .table, !tab.dirty else { return }; tab.filterColumn = filterColumn.selectedItem?.title ?? ""; tab.filterOperator = filterOperator.selectedItem?.title ?? "contains"; tab.filterValue = filterValue.stringValue; tab.offset = 0; loadTable(tab) }
    @objc private func previousPage() { guard idle, let tab = activeTab, tab.kind == .table, tab.offset > 0, !tab.dirty else { return }; tab.offset = max(0, tab.offset - tab.limit); loadTable(tab) }
    @objc private func nextPage() { guard idle, let tab = activeTab, tab.kind == .table, tab.hasMore, !tab.dirty else { return }; tab.offset += tab.limit; loadTable(tab) }

    @objc private func stageDelete() {
        guard idle, let tab = activeTab, tab.kind == .table, grid.selectedRow >= 0, !isReadOnly(tab), tab.rows.indices.contains(grid.selectedRow) else { return }
        let row = grid.selectedRow; var mutation = mutationFor(tab, row: row); mutation["deleted"] = true; tab.mutations[row] = mutation; showTab(tab)
    }
    private func mutationFor(_ tab: WorkTab, row: Int) -> [String: Any] {
        if let existing = tab.mutations[row] { return existing }
        let count = tab.columns.count
        let original = Array(tab.rows[row].prefix(count)), names = tab.columns.map { string($0["name"]) }, pk = tab.metadata["primaryKey"] as? [String] ?? []
        let primary = pk.compactMap { key -> Any? in guard let index = names.firstIndex(of: key), original.indices.contains(index) else { return nil }; return original[index] }
        let xmin: Any = bool(tab.metadata["hasXmin"]) && tab.rows[row].count > count ? jsonDisplay(tab.rows[row][count]) : NSNull()
        return ["original": original, "changes": original, "primaryKey": primary, "xmin": xmin, "deleted": false]
    }
    @objc private func discardChanges() { guard idle, let tab = activeTab else { return }; tab.mutations.removeAll(); showTab(tab) }
    @objc private func saveChanges() {
        guard idle, let tab = activeTab, tab.kind == .table, tab.dirty else { return }
        let alert = NSAlert(); alert.messageText = "Apply \(tab.mutations.count) staged change(s)?"; alert.informativeText = "Changes are written to \(tab.schema).\(tab.table)."; alert.addButton(withTitle: "Save changes"); alert.addButton(withTitle: "Cancel")
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        let batch: [String: Any] = ["profileId": tab.profileID, "schema": tab.schema, "table": tab.table, "mutations": Array(tab.mutations.values)]
        send(["command": "applyTableMutations", "batch": batch], message: "Saving changes…", tab: tab) { [weak self, weak tab] result in
            guard let self, let tab, case .success(let value) = result, let response = dictionary(value) else { return }
            let conflicts = response["conflicts"] as? [[Any]] ?? []; status.stringValue = conflicts.isEmpty ? "Saved \(int(response["applied"])) change(s)" : "Some rows changed on the server; conflicted rows were not saved."
            tab.mutations = tab.mutations.filter { _, mutation in
                conflicts.contains { ($0 as NSArray).isEqual(to: mutation["primaryKey"] as? [Any] ?? []) }
            }
            if tab.mutations.isEmpty { loadTable(tab) } else if activeTab === tab { showTab(tab) }
        }
    }

    @objc private func showHistory() {
        guard let profile = active else { return }
        send(["command": "listQueryHistory", "profile_id": profile.id, "database": profile.database, "limit": 100], message: "Loading history…") { [weak self] result in
            guard let self, case .success(let value) = result else { return }; let entries = array(value)
            let alert = NSAlert(); alert.messageText = "Query history"; let popup = NSPopUpButton(frame: NSRect(x: 0, y: 0, width: 520, height: 28))
            for entry in entries { let sql = string(entry["sql"]); let item = NSMenuItem(title: sql.replacingOccurrences(of: "\n", with: " "), action: nil, keyEquivalent: ""); item.representedObject = sql; popup.menu?.addItem(item) }
            alert.accessoryView = popup; alert.addButton(withTitle: "Open in query"); alert.addButton(withTitle: "Cancel")
            if !entries.isEmpty, alert.runModal() == .alertFirstButtonReturn { newQuery(); editor.string = popup.selectedItem?.representedObject as? String ?? ""; activeTab?.sql = editor.string }
        }
    }

    private func showTab(_ tab: WorkTab) {
        let query = tab.kind == .query; editorScroll.isHidden = !query; runButton.isHidden = !query
        filterColumn.isHidden = query; filterOperator.isHidden = query; filterValue.isHidden = query
        rebuildGrid(tab); pending.stringValue = tab.dirty ? "● \(tab.mutations.count) pending change(s)" : ""
        discardButton.isHidden = !tab.dirty; saveButton.isHidden = !tab.dirty
        if !query { pageLabel.stringValue = "Rows \(tab.offset + 1)–\(tab.offset + tab.rows.count)\(tab.total.map { " of \($0)" } ?? "")" }
        else { pageLabel.stringValue = "\(tab.rows.count) rows" }; updateControls()
    }
    private func rebuildGrid(_ tab: WorkTab) {
        grid.tableColumns.forEach(grid.removeTableColumn); filterColumn.removeAllItems()
        for (index, raw) in tab.columns.enumerated() { let column = NSTableColumn(identifier: .init(String(index))); column.title = string(raw["name"]); column.width = 160; column.isEditable = tab.kind == .table && !isReadOnly(tab); grid.addTableColumn(column); filterColumn.addItem(withTitle: column.title) }
        if !tab.filterColumn.isEmpty { filterColumn.selectItem(withTitle: tab.filterColumn) }; grid.reloadData()
    }
    private func clearGrid() { activeTab = nil; grid.tableColumns.forEach(grid.removeTableColumn); grid.reloadData(); editor.string = ""; updateControls() }
    private func isReadOnly(_ tab: WorkTab) -> Bool { profiles.first(where: { $0.id == tab.profileID })?.readOnly != false || (tab.metadata["primaryKey"] as? [String] ?? []).isEmpty }
    private func updateControls() {
        let tab = activeTab
        runButton.isEnabled = idle && tab?.kind == .query
        refreshButton.isEnabled = idle && tab != nil
        previousButton.isEnabled = idle && tab?.kind == .table && (tab?.offset ?? 0) > 0 && tab?.dirty == false
        nextButton.isEnabled = idle && tab?.kind == .table && tab?.hasMore == true && tab?.dirty == false
        saveButton.isEnabled = idle && tab?.dirty == true
        discardButton.isEnabled = idle; database.isEnabled = idle && active != nil
        editor.isEditable = idle && tab?.kind == .query
        tabs.isEnabled = idle
    }

    func numberOfRows(in tableView: NSTableView) -> Int { tableView === profileList ? profiles.count : activeTab?.rows.count ?? 0 }
    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        if tableView === profileList { guard profiles.indices.contains(row) else { return nil }; let cell = NSTextField(labelWithString: "●  \(profiles[row].name)"); cell.textColor = profiles[row].color; cell.font = .systemFont(ofSize: 13, weight: .medium); return cell }
        guard let tab = activeTab, let id = tableColumn?.identifier, let column = Int(id.rawValue), tab.rows.indices.contains(row), tab.rows[row].indices.contains(column) else { return nil }
        let cell = grid.makeView(withIdentifier: id, owner: self) as? NSTextField ?? NSTextField()
        cell.delegate = self; cell.identifier = id
        cell.stringValue = jsonDisplay((tab.mutations[row]?["changes"] as? [Any] ?? tab.rows[row])[column])
        cell.font = .monospacedSystemFont(ofSize: 12, weight: .regular)
        cell.textColor = bool(tab.mutations[row]?["deleted"]) ? Graphite.danger : (tab.mutations[row] == nil ? Graphite.text : Graphite.modified)
        let primary = tab.metadata["primaryKey"] as? [String] ?? []
        cell.isEditable = idle && tableColumn?.isEditable == true && !primary.contains(string(tab.columns[column]["name"]))
        cell.isBordered = false; cell.backgroundColor = .clear; cell.lineBreakMode = .byTruncatingTail; cell.toolTip = cell.stringValue
        return cell
    }
    func controlTextDidEndEditing(_ notification: Notification) {
        guard let field = notification.object as? NSTextField, let id = field.identifier,
              let column = grid.tableColumns.first(where: { $0.identifier == id }) else { return }
        let row = grid.row(for: field)
        guard row >= 0 else { return }
        tableView(grid, setObjectValue: field.stringValue, for: column, row: row)
    }
    func tableView(_ tableView: NSTableView, setObjectValue object: Any?, for tableColumn: NSTableColumn?, row: Int) {
        guard idle, tableView === grid, let tab = activeTab, tab.kind == .table, !isReadOnly(tab), let column = tableColumn.flatMap({ Int($0.identifier.rawValue) }), tab.rows.indices.contains(row), tab.rows[row].indices.contains(column) else { return }
        var mutation = mutationFor(tab, row: row); var changes = mutation["changes"] as? [Any] ?? tab.rows[row]; changes[column] = jsonValue(String(describing: object ?? ""), matching: tab.rows[row][column]); mutation["changes"] = changes; mutation["deleted"] = false; tab.mutations[row] = mutation; showTab(tab)
    }
    func tableView(_ tableView: NSTableView, didClick tableColumn: NSTableColumn) {
        guard idle, tableView === grid, let tab = activeTab, tab.kind == .table, !tab.dirty else { return }
        if tab.orderColumn == tableColumn.title { tab.descending.toggle() }
        else { tab.orderColumn = tableColumn.title; tab.descending = false }
        tab.offset = 0; loadTable(tab)
    }
    func outlineView(_ outlineView: NSOutlineView, numberOfChildrenOfItem item: Any?) -> Int { (item as? SchemaItem)?.children.count ?? schema.count }
    func outlineView(_ outlineView: NSOutlineView, child index: Int, ofItem item: Any?) -> Any { (item as? SchemaItem)?.children[index] ?? schema[index] }
    func outlineView(_ outlineView: NSOutlineView, isItemExpandable item: Any) -> Bool { (item as? SchemaItem).map { !$0.children.isEmpty } ?? false }
    func outlineView(_ outlineView: NSOutlineView, viewFor tableColumn: NSTableColumn?, item: Any) -> NSView? { guard let item = item as? SchemaItem else { return nil }; let cell = NSTextField(labelWithString: item.name); cell.font = .systemFont(ofSize: 12.5); cell.textColor = item.table.isEmpty ? Graphite.muted : Graphite.text; return cell }

    @objc private func newProfile() { presentProfileEditor(nil) }
    @objc private func editProfile() { let row = profileList.selectedRow; guard profiles.indices.contains(row) else { return }; presentProfileEditor(profiles[row]) }
    private func presentProfileEditor(_ profile: Profile?) {
        guard idle, profile.map({ confirmDiscard(exceptProfile: $0.id) }) ?? true else { return }
        let alert = NSAlert(); alert.messageText = profile == nil ? "New connection" : "Edit connection"
        let fields = ["Name", "Engine", "Host", "Port", "Username", "Database", "Password", "TLS", "CA certificate", "Color"]
        let defaults: [String] = [profile?.name ?? "", profile?.engine ?? "postgres", string(profile?.raw["host"]), profile == nil ? "5432" : "\(int(profile?.raw["port"]))", string(profile?.raw["username"]), profile?.database ?? "postgres", "", string(profile?.raw["tlsMode"]).isEmpty ? "preferred" : string(profile?.raw["tlsMode"]), string(profile?.raw["caCertPath"]), profile?.raw["color"] as? String ?? "#4c9aff"]
        let grid = NSGridView(); var controls: [NSTextField] = []
        for (index, name) in fields.enumerated() { let field = index == 6 ? NSSecureTextField(string: defaults[index]) : NSTextField(string: defaults[index]); field.frame.size.width = 260; controls.append(field); grid.addRow(with: [NSTextField(labelWithString: name), field]) }
        let readOnly = NSButton(checkboxWithTitle: "Read only", target: nil, action: nil); readOnly.state = bool(profile?.raw["readOnly"]) ? .on : .off; grid.addRow(with: [NSView(), readOnly]); grid.rowSpacing = 8; alert.accessoryView = grid
        alert.addButton(withTitle: "Save"); alert.addButton(withTitle: "Cancel"); guard alert.runModal() == .alertFirstButtonReturn else { return }
        let port = Int(controls[3].stringValue) ?? 0
        var input: [String: Any] = ["name": controls[0].stringValue, "color": controls[9].stringValue, "engine": controls[1].stringValue, "host": controls[2].stringValue, "port": port, "username": controls[4].stringValue, "defaultDatabase": controls[5].stringValue, "tlsMode": controls[7].stringValue, "caCertPath": controls[8].stringValue.isEmpty ? NSNull() : controls[8].stringValue, "ssh": profile?.raw["ssh"] ?? NSNull(), "readOnly": readOnly.state == .on]
        if let profile { input["id"] = profile.id }; if profile == nil || !controls[6].stringValue.isEmpty { input["password"] = controls[6].stringValue }
        send(["command": "saveProfile", "input": input], message: "Saving connection…") { [weak self] result in
            if case .success = result { if let profile { self?.closeProfile(profile.id) }; self?.loadProfiles() }
        }
    }
    private func closeProfile(_ id: String) {
        workTabs.removeAll { $0.profileID == id }; connected.remove(id)
        workspaces.removeValue(forKey: id); databases.removeValue(forKey: id); schemaCache.removeValue(forKey: id)
        if active?.id == id { active = nil; schema = []; schemaTree.reloadData(); database.removeAllItems(); clearGrid() }
        reloadTabs()
    }
    @objc private func deleteProfile() { let row = profileList.selectedRow; guard idle, profiles.indices.contains(row) else { return }; let profile = profiles[row]; guard confirmDiscard(exceptProfile: profile.id) else { return }; let alert = NSAlert(); alert.messageText = "Delete \(profile.name)?"; alert.informativeText = "The profile, password, and query history will be removed."; alert.addButton(withTitle: "Delete"); alert.addButton(withTitle: "Cancel"); alert.alertStyle = .warning; guard alert.runModal() == .alertFirstButtonReturn else { return }; send(["command": "deleteProfile", "profile_id": profile.id], message: "Deleting…") { [weak self] result in if case .success = result { self?.closeProfile(profile.id); self?.loadProfiles() } } }

    private func confirmDiscard(exceptProfile: String?) -> Bool { for tab in workTabs where tab.dirty && (exceptProfile == nil || tab.profileID == exceptProfile) { if !confirmDiscard(tab: tab) { return false }; tab.mutations.removeAll() }; return true }
    private func confirmDiscard(tab: WorkTab) -> Bool { let alert = NSAlert(); alert.messageText = "Discard staged changes?"; alert.informativeText = "This action cannot continue while \(tab.title) has unsaved edits."; alert.addButton(withTitle: "Discard"); alert.addButton(withTitle: "Cancel"); alert.alertStyle = .warning; guard alert.runModal() == .alertFirstButtonReturn else { return false }; tab.mutations.removeAll(); return true }
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply { confirmDiscard(exceptProfile: nil) ? .terminateNow : .terminateCancel }
    func windowShouldClose(_ sender: NSWindow) -> Bool { confirmDiscard(exceptProfile: nil) }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
    func applicationWillTerminate(_ notification: Notification) { bridge.stop() }

    private func installMenu() { let menu = NSMenu(); let app = NSMenuItem(); menu.addItem(app); app.submenu = NSMenu(); app.submenu?.addItem(withTitle: "Quit DBM Native", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q"); let edit = NSMenuItem(title: "Edit", action: nil, keyEquivalent: ""); menu.addItem(edit); edit.submenu = NSMenu(title: "Edit"); for item in [("Undo", "undo:", "z"), ("Cut", "cut:", "x"), ("Copy", "copy:", "c"), ("Paste", "paste:", "v"), ("Select All", "selectAll:", "a")] { edit.submenu?.addItem(withTitle: item.0, action: Selector(item.1), keyEquivalent: item.2) }; NSApp.mainMenu = menu }
}

let application = NSApplication.shared
let delegate = Workbench()
application.setActivationPolicy(.regular)
application.delegate = delegate
application.run()
