import AppKit
import Darwin

final class Workbench: NSObject, NSApplicationDelegate, NSTableViewDataSource, NSTableViewDelegate {
    private var window: NSWindow!
    private var bridge: Bridge!
    private let profiles = NSPopUpButton(frame: .zero, pullsDown: false)
    private let connect = NSButton(title: "Connect", target: nil, action: nil)
    private let reload = NSButton(title: "Reload Connections", target: nil, action: nil)
    private let run = NSButton(title: "Run Selection / Query", target: nil, action: nil)
    private let disconnect = NSButton(title: "Disconnect", target: nil, action: nil)
    private let editor = NSTextView()
    private let table = NSTableView()
    private let status = NSTextField(wrappingLabelWithString: "Loading saved connections…")
    private var saved: [[String: Any]] = []
    private var active: [String: Any]?
    private var rows: [[Any]] = []
    private var busy = false

    func applicationDidFinishLaunching(_ notification: Notification) {
        installMenu()
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1080, height: 740),
                          styleMask: [.titled, .closable, .miniaturizable, .resizable],
                          backing: .buffered, defer: false)
        window.title = "DBM Native — Development Preview"
        window.minSize = NSSize(width: 720, height: 580)
        let content = NSStackView()
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 12
        content.edgeInsets = NSEdgeInsets(top: 16, left: 16, bottom: 16, right: 16)
        window.contentView = content

        connect.target = self; connect.action = #selector(connectProfile)
        reload.target = self; reload.action = #selector(loadProfiles)
        run.target = self; run.action = #selector(runQuery)
        run.keyEquivalent = "\r"; run.keyEquivalentModifierMask = [.command]
        disconnect.target = self; disconnect.action = #selector(disconnectProfile)
        profiles.setAccessibilityLabel("Saved connection")
        let toolbar = NSStackView(views: [profiles, connect, disconnect, reload])
        toolbar.spacing = 8
        content.addArrangedSubview(toolbar)
        let notice = NSTextField(wrappingLabelWithString:
            "Native preview • Uses your existing local profiles and query history. Manage connections in the shipping DBM app.")
        notice.textColor = .secondaryLabelColor
        content.addArrangedSubview(notice)

        editor.isRichText = false
        editor.allowsUndo = true
        editor.isAutomaticQuoteSubstitutionEnabled = false
        editor.isAutomaticDashSubstitutionEnabled = false
        editor.isAutomaticTextReplacementEnabled = false
        editor.font = .monospacedSystemFont(ofSize: 13, weight: .regular)
        editor.textContainerInset = NSSize(width: 10, height: 10)
        editor.isVerticallyResizable = true
        editor.autoresizingMask = [.width]
        editor.textContainer?.widthTracksTextView = true
        editor.setAccessibilityLabel("SQL or Redis command")
        let editorScroll = NSScrollView()
        editorScroll.hasVerticalScroller = true
        editorScroll.borderType = .bezelBorder
        editorScroll.documentView = editor
        content.addArrangedSubview(editorScroll)
        content.addArrangedSubview(run)

        table.dataSource = self; table.delegate = self
        table.usesAlternatingRowBackgroundColors = true
        table.rowHeight = 24
        table.columnAutoresizingStyle = .noColumnAutoresizing
        table.setAccessibilityLabel("Query results")
        let results = NSScrollView()
        results.hasVerticalScroller = true; results.hasHorizontalScroller = true
        results.borderType = .bezelBorder
        results.documentView = table
        content.addArrangedSubview(results)
        status.maximumNumberOfLines = 3
        status.isSelectable = true
        content.addArrangedSubview(status)
        for view in [toolbar, notice, editorScroll, results, status] {
            view.widthAnchor.constraint(equalTo: content.widthAnchor, constant: -32).isActive = true
        }
        editorScroll.heightAnchor.constraint(equalToConstant: 180).isActive = true
        results.heightAnchor.constraint(greaterThanOrEqualToConstant: 100).isActive = true
        profiles.widthAnchor.constraint(greaterThanOrEqualToConstant: 240).isActive = true
        window.center(); window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        bridge = Bridge(executable: Bundle.main.bundleURL
            .appendingPathComponent("Contents/MacOS/dbm-native-bridge"))
        loadProfiles()
    }

    private func installMenu() {
        let menu = NSMenu()
        let app = NSMenuItem(); menu.addItem(app)
        app.submenu = NSMenu()
        app.submenu?.addItem(withTitle: "Quit DBM Native", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        let edit = NSMenuItem(title: "Edit", action: nil, keyEquivalent: ""); menu.addItem(edit)
        edit.submenu = NSMenu(title: "Edit")
        for (title, action, key) in [("Undo", "undo:", "z"), ("Cut", "cut:", "x"),
                                     ("Copy", "copy:", "c"), ("Paste", "paste:", "v"),
                                     ("Select All", "selectAll:", "a")] {
            edit.submenu?.addItem(withTitle: title, action: Selector(action), keyEquivalent: key)
        }
        NSApp.mainMenu = menu
    }

    private func setBusy(_ value: Bool) {
        busy = value
        profiles.isEnabled = !value && active == nil
        reload.isEnabled = !value && active == nil
        connect.isEnabled = !value && active == nil && !saved.isEmpty
        disconnect.isEnabled = !value && active != nil
        run.isEnabled = !value && active != nil
        editor.isEditable = !value && active != nil
    }

    private func send(_ request: [String: Any], message: String, success: @escaping (Any) -> Void) {
        guard !busy else { return }
        setBusy(true); status.stringValue = message
        bridge.send(request) { [weak self] result in
            guard let self = self else { return }
            switch result {
            case .success(let value): success(value)
            case .failure(let error): self.status.stringValue = error.localizedDescription
            }
            self.setBusy(false)
        }
    }

    @objc private func loadProfiles() {
        send(["command": "listProfiles"], message: "Loading saved connections…") { [self] value in
            saved = (value as? [[String: Any]] ?? []).compactMap { $0["profile"] as? [String: Any] }
            profiles.removeAllItems()
            for profile in saved {
                // Add individually: duplicate profile names still represent different IDs.
                profiles.menu?.addItem(NSMenuItem(title: "\(profile["name"] ?? "Connection") · \(profile["engine"] ?? "")",
                                                 action: nil, keyEquivalent: ""))
            }
            if !saved.isEmpty { profiles.selectItem(at: 0) }
            status.stringValue = saved.isEmpty
                ? "No saved connections. Create one in DBM, then reload here."
                : "Choose a connection. Queries use its saved default database."
        }
    }

    @objc private func connectProfile() {
        guard !busy, active == nil, saved.indices.contains(profiles.indexOfSelectedItem),
              let id = saved[profiles.indexOfSelectedItem]["id"] as? String else { return }
        send(["command": "connect", "profile_id": id], message: "Connecting…") { [self] value in
            active = value as? [String: Any]
            editor.string = active?["engine"] as? String == "redis" ? "PING" : "SELECT 1;"
            editor.undoManager?.removeAllActions()
            status.stringValue = "Connected to \(active?["defaultDatabase"] ?? ""). \(active?["readOnly"] as? Bool == true ? "Read-only profile." : "Queries can modify this database.")"
            window.makeFirstResponder(editor)
        }
    }

    @objc private func disconnectProfile() {
        guard let id = active?["id"] as? String else { return }
        send(["command": "disconnect", "profile_id": id], message: "Disconnecting…") { [self] _ in
            active = nil; editor.string = ""; showResults([:])
            editor.undoManager?.removeAllActions()
            status.stringValue = "Disconnected."
        }
    }

    @objc private func runQuery() {
        guard !busy, let id = active?["id"] as? String else { return }
        let selection = editor.selectedRange()
        let sql = selection.length > 0 ? (editor.string as NSString).substring(with: selection) : editor.string
        guard !sql.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        // Clear previous results so an error never leaves rows labeled as the new query.
        showResults([:])
        send(["command": "query", "request": ["profileId": id, "sql": sql, "maxRows": 10000]],
             message: "Running… Closing the app does not guarantee rollback.") { [self] value in
            guard let result = value as? [String: Any] else { return }
            showResults(result)
            let truncated = result["truncated"] as? Bool == true ? " • Truncated at 10,000 rows" : ""
            let affected = (result["affectedRows"] as? NSNumber).map { " • \($0) affected" } ?? ""
            let notices = (result["notices"] as? [String] ?? []).joined(separator: "\n")
            status.stringValue = "\(rows.count) rows • \(result["durationMs"] ?? 0) ms\(affected)\(truncated)\(notices.isEmpty ? "" : "\n" + notices)"
        }
    }

    private func showResults(_ result: [String: Any]) {
        for column in table.tableColumns { table.removeTableColumn(column) }
        rows = result["rows"] as? [[Any]] ?? []
        for (index, column) in (result["columns"] as? [[String: Any]] ?? []).enumerated() {
            let item = NSTableColumn(identifier: NSUserInterfaceItemIdentifier(String(index)))
            item.title = column["name"] as? String ?? "Column"
            item.width = 180; table.addTableColumn(item)
        }
        table.reloadData()
    }

    func numberOfRows(in tableView: NSTableView) -> Int { rows.count }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard let identifier = tableColumn?.identifier, let index = Int(identifier.rawValue),
              rows.indices.contains(row), rows[row].indices.contains(index) else { return nil }
        let cell = tableView.makeView(withIdentifier: identifier, owner: self) as? NSTextField
            ?? NSTextField(labelWithString: "")
        cell.identifier = identifier
        let value = rows[row][index]
        if value is NSNull {
            cell.stringValue = "NULL"
        } else if let text = value as? String {
            cell.stringValue = text
        } else if let data = try? JSONSerialization.data(withJSONObject: value, options: [.fragmentsAllowed, .sortedKeys]),
                  let text = String(data: data, encoding: .utf8) {
            cell.stringValue = text
        } else { cell.stringValue = String(describing: value) }
        cell.font = .monospacedSystemFont(ofSize: 12, weight: .regular)
        cell.lineBreakMode = .byTruncatingTail
        cell.toolTip = cell.stringValue
        return cell
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
    func applicationWillTerminate(_ notification: Notification) { bridge?.stop() }
}

// A helper crash must produce a transport error, not terminate the UI on write.
signal(SIGPIPE, SIG_IGN)
let application = NSApplication.shared
let delegate = Workbench()
application.setActivationPolicy(.regular)
application.delegate = delegate
application.run()
