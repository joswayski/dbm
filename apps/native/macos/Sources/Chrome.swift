import AppKit

/// A custom-drawn clickable row with hover wash; subclasses draw content.
class RowControl: NSView {
    var onClick: (() -> Void)?
    var onDoubleClick: (() -> Void)?
    var onMiddleClick: (() -> Void)?
    var hovering = false
    var selected = false { didSet { needsDisplay = true } }

    override init(frame: NSRect) {
        super.init(frame: frame)
        translatesAutoresizingMaskIntoConstraints = false
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeInActiveApp, .inVisibleRect], owner: self, userInfo: nil))
        setAccessibilityElement(true)
        setAccessibilityRole(.button)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override var isFlipped: Bool { true }
    override func mouseEntered(with event: NSEvent) { hovering = true; needsDisplay = true }
    override func mouseExited(with event: NSEvent) { hovering = false; needsDisplay = true }
    override func mouseDown(with event: NSEvent) {}
    override func mouseUp(with event: NSEvent) {
        guard bounds.contains(convert(event.locationInWindow, from: nil)) else { return }
        if event.clickCount == 2, let onDoubleClick { onDoubleClick() } else { onClick?() }
    }
    override func otherMouseUp(with event: NSEvent) { if event.buttonNumber == 2 { onMiddleClick?() } }
    override func accessibilityPerformPress() -> Bool { onClick?(); return true }
    override func accessibilityValue() -> Any? { selected ? "selected" : nil }
}

final class ConnectionRow: RowControl {
    let profile: Profile
    var connected = false
    var connecting = false

    init(profile: Profile) {
        self.profile = profile
        super.init(frame: .zero)
        heightAnchor.constraint(equalToConstant: 44).isActive = true
        setAccessibilityLabel("\(profile.name), \(profile.subtitle)")
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    var trailingInset: CGFloat = 34

    override func draw(_ dirtyRect: NSRect) {
        if selected {
            Graphite.control.setFill()
            NSBezierPath(roundedRect: bounds, xRadius: 7, yRadius: 7).fill()
        } else if hovering {
            Graphite.hoverWash.setFill()
            NSBezierPath(roundedRect: bounds, xRadius: 7, yRadius: 7).fill()
        }
        let dot = NSRect(x: 8, y: 11, width: 8, height: 8)
        if connected {
            profile.color.setFill()
            NSBezierPath(ovalIn: dot).fill()
        } else {
            profile.color.setStroke()
            let ring = NSBezierPath(ovalIn: dot.insetBy(dx: 0.75, dy: 0.75))
            ring.lineWidth = 1.5
            ring.stroke()
        }
        let width = bounds.width - 26 - trailingInset
        let paragraph = NSMutableParagraphStyle()
        paragraph.lineBreakMode = .byTruncatingTail
        NSAttributedString(string: profile.name, attributes: [
            .font: Graphite.ui(13, .semibold), .foregroundColor: Graphite.textStrong, .paragraphStyle: paragraph,
        ]).draw(with: NSRect(x: 26, y: 6, width: width, height: 17), options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])
        NSAttributedString(string: profile.subtitle, attributes: [
            .font: Graphite.ui(11), .foregroundColor: Graphite.faint, .paragraphStyle: paragraph,
        ]).draw(with: NSRect(x: 26, y: 24, width: width, height: 15), options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])
    }
}

final class TreeRow: RowControl {
    let node: SchemaNode
    let depth: Int
    var open = false { didSet { needsDisplay = true } }
    var color = Graphite.accent

    init(node: SchemaNode, depth: Int) {
        self.node = node
        self.depth = depth
        super.init(frame: .zero)
        heightAnchor.constraint(equalToConstant: 26).isActive = true
        setAccessibilityLabel(node.name)
        if !node.isLeaf { setAccessibilityRole(.disclosureTriangle) }
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func draw(_ dirtyRect: NSRect) {
        if selected {
            color.withAlphaComponent(0.3).setFill()
            NSBezierPath(roundedRect: bounds, xRadius: 6, yRadius: 6).fill()
        } else if hovering {
            Graphite.hoverWash.setFill()
            NSBezierPath(roundedRect: bounds, xRadius: 6, yRadius: 6).fill()
        }
        var x = 8 + CGFloat(depth) * 14
        if !node.isLeaf {
            (open ? Icon.chevronDown : Icon.chevronRight).draw(in: NSRect(x: x, y: 7.5, width: 11, height: 11), color: Graphite.faint)
        }
        x += 14
        let icon: Icon = !node.isLeaf ? .folder : node.kind == "key" ? .key : node.kind == "view" ? .view : .table
        icon.draw(in: NSRect(x: x, y: 6.5, width: 13, height: 13), color: selected ? color : Graphite.muted)
        x += 20
        let paragraph = NSMutableParagraphStyle()
        paragraph.lineBreakMode = .byTruncatingTail
        NSAttributedString(string: node.name, attributes: [
            .font: Graphite.ui(13, selected ? .medium : .regular),
            .foregroundColor: selected ? Graphite.textStrong : Graphite.secondary, .paragraphStyle: paragraph,
        ]).draw(with: NSRect(x: x, y: 5, width: bounds.width - x - 6, height: 17), options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])
    }
}

/// Connections, the active connection's database picker and schema or
/// keyspace tree, and "New connection". Mirrors the sidebar in `App.tsx`.
final class SidebarView: PanelView {
    unowned let app: AppController
    private let list = vstack([], spacing: 2)
    private var openNodes = Set<String>()
    private var closedNodes = Set<String>()
    private var filterField: GTextField?
    private var filterProfile: String?
    private var widthConstraint: NSLayoutConstraint!
    private let collapsedBar = FlippedView()
    private let expandedContent = FlippedView()
    static let defaultWidth: CGFloat = 260

    init(app: AppController) {
        self.app = app
        super.init(fill: Graphite.sidebar, edges: [.right])
        let stored = UserDefaults.standard.double(forKey: "dbm.sidebarWidth")
        widthConstraint = widthAnchor.constraint(equalToConstant: stored >= 220 && stored <= 480 ? stored : Self.defaultWidth)
        widthConstraint.isActive = true

        let badge = PanelView(fill: Graphite.accentStrong)
        badge.radius = 6
        badge.pin(IconView(.database, size: 14, color: .white), insets: NSEdgeInsets(top: 4, left: 4, bottom: 4, right: 4))
        let collapse = GButton("", icon: .sidebar, style: .icon, tooltip: "Collapse sidebar") { [weak self] in self?.setCollapsed(true) }
        let brand = hstack([badge, label("DBM", font: Graphite.ui(14, .semibold), color: Graphite.textStrong), spacer(), collapse], spacing: 8)
        let connections = label("Connections", font: Graphite.ui(11, .semibold), color: Graphite.muted)
        let scroll = verticalScroll(list)
        let newConnection = GButton("New connection", icon: .plus, style: .secondary) { [weak app] in app?.newProfile() }
        newConnection.minHeight = 30
        let footer = PanelView(fill: nil, edges: [.top])
        footer.pin(newConnection, insets: NSEdgeInsets(top: 10, left: 10, bottom: 10, right: 10))
        let column = vstack([brand, connections, scroll, footer], spacing: 0)
        column.setCustomSpacing(14, after: brand)
        column.setCustomSpacing(6, after: connections)
        column.distribution = .fill
        brand.widthAnchor.constraint(equalTo: column.widthAnchor, constant: -20).isActive = true
        connections.widthAnchor.constraint(equalTo: column.widthAnchor, constant: -20).isActive = true
        scroll.widthAnchor.constraint(equalTo: column.widthAnchor).isActive = true
        footer.widthAnchor.constraint(equalTo: column.widthAnchor).isActive = true
        column.alignment = .centerX
        scroll.setContentHuggingPriority(.init(1), for: .vertical)
        expandedContent.translatesAutoresizingMaskIntoConstraints = false
        expandedContent.pin(column, insets: NSEdgeInsets(top: 12, left: 0, bottom: 0, right: 1))
        pin(expandedContent)
        list.edgeInsets = NSEdgeInsets(top: 0, left: 8, bottom: 12, right: 8)

        let expand = GButton("", icon: .sidebar, style: .icon, tooltip: "Expand sidebar") { [weak self] in self?.setCollapsed(false) }
        collapsedBar.translatesAutoresizingMaskIntoConstraints = false
        collapsedBar.addSubview(expand)
        NSLayoutConstraint.activate([
            expand.topAnchor.constraint(equalTo: collapsedBar.topAnchor, constant: 12),
            expand.centerXAnchor.constraint(equalTo: collapsedBar.centerXAnchor),
        ])
        pin(collapsedBar)
        collapsedBar.isHidden = true

        let handle = ResizeHandle { [weak self] delta in self?.resize(by: delta) } reset: { [weak self] in
            self?.widthConstraint.constant = Self.defaultWidth
            UserDefaults.standard.set(Self.defaultWidth, forKey: "dbm.sidebarWidth")
        }
        addSubview(handle)
        NSLayoutConstraint.activate([
            handle.topAnchor.constraint(equalTo: topAnchor),
            handle.bottomAnchor.constraint(equalTo: bottomAnchor),
            handle.trailingAnchor.constraint(equalTo: trailingAnchor, constant: 3),
            handle.widthAnchor.constraint(equalToConstant: 6),
        ])
        if UserDefaults.standard.bool(forKey: "dbm.sidebarCollapsed") { setCollapsed(true) }
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    private var expandedWidth: CGFloat = SidebarView.defaultWidth

    private func setCollapsed(_ collapsed: Bool) {
        if collapsed {
            expandedWidth = widthConstraint.constant
            widthConstraint.constant = 48
        } else {
            widthConstraint.constant = max(220, expandedWidth)
        }
        collapsedBar.isHidden = !collapsed
        expandedContent.isHidden = collapsed
        UserDefaults.standard.set(collapsed, forKey: "dbm.sidebarCollapsed")
    }

    private func resize(by delta: CGFloat) {
        guard !expandedContent.isHidden else { return }
        widthConstraint.constant = min(480, max(220, widthConstraint.constant + delta))
        UserDefaults.standard.set(Double(widthConstraint.constant), forKey: "dbm.sidebarWidth")
    }

    func reload() {
        let focusedFilter = filterField?.currentEditor() != nil ? filterProfile : nil
        list.arrangedSubviews.forEach { $0.removeFromSuperview() }
        filterField = nil
        if app.profiles.isEmpty {
            list.addArrangedSubview(label("No saved connections.", font: Graphite.ui(13), color: Graphite.muted))
        }
        for profile in app.profiles {
            let id = profile.id
            let workspace = app.workspaces[id]
            let active = app.activeProfileID == id
            let expanded = active && workspace != nil && !app.collapsedProfiles.contains(id)
            let row = ConnectionRow(profile: profile)
            row.connected = workspace != nil
            row.selected = active
            row.toolTip = workspace == nil ? "Connect" : nil
            row.trailingInset = workspace != nil && active ? 60 : 34
            row.onClick = { [weak app] in app?.selectProfile(id) }
            let more = GButton("", icon: .more, style: .icon, tooltip: "Connection actions for \(profile.name)")
            more.handler = { [weak app, weak more] in
                guard let app, let more else { return }
                let menu = NSMenu()
                menu.addItem(ClosureMenuItem("Edit connection") { app.editProfile(id) })
                if workspace != nil {
                    let item = ClosureMenuItem("Disconnect") { app.disconnect(id) }
                    item.toolTip = "Close this connection and its tabs"
                    item.attributedTitle = NSAttributedString(string: "Disconnect", attributes: [.foregroundColor: Graphite.danger, .font: NSFont.menuFont(ofSize: 0)])
                    menu.addItem(item)
                }
                menu.popUp(positioning: nil, at: NSPoint(x: 0, y: more.bounds.height + 2), in: more)
            }
            var trailing: [NSView] = []
            if app.connecting.contains(id) {
                let spinner = NSProgressIndicator()
                spinner.style = .spinning
                spinner.controlSize = .small
                spinner.startAnimation(nil)
                trailing.append(spinner)
            } else if workspace != nil && active {
                trailing.append(GButton("", icon: expanded ? .chevronUp : .chevronDown, style: .icon,
                                        tooltip: expanded ? "Collapse connection" : "Expand connection") { [weak app] in app?.toggleExpanded(id) })
            }
            if profile.readOnly { trailing.insert(Chip("Read-only", color: Graphite.muted), at: 0) }
            trailing.append(more)
            let actions = hstack(trailing, spacing: 2)
            row.addSubview(actions)
            NSLayoutConstraint.activate([
                actions.trailingAnchor.constraint(equalTo: row.trailingAnchor, constant: -4),
                actions.topAnchor.constraint(equalTo: row.topAnchor, constant: 7),
            ])
            list.addArrangedSubview(row)
            row.widthAnchor.constraint(equalTo: list.widthAnchor, constant: -16).isActive = true
            if expanded, let workspace {
                let panel = workspacePanel(profile: profile, databases: workspace.databases, database: workspace.profile.database,
                                           refocus: focusedFilter == id)
                list.addArrangedSubview(panel)
                panel.widthAnchor.constraint(equalTo: list.widthAnchor, constant: -16).isActive = true
                list.setCustomSpacing(10, after: panel)
            }
        }
    }

    private func workspacePanel(profile: Profile, databases: [String], database: String, refocus: Bool) -> NSView {
        let id = profile.id
        let redis = profile.engine == .redis
        let databasePopup = GPopUp(items: databases)
        databasePopup.selectItem(withTitle: database)
        databasePopup.onSelect = { [weak app] index in app?.switchDatabase(id, databases[index]) }
        let refreshing = app.refreshingSchema.contains(id)
        let refresh = GButton(refreshing ? "Refreshing…" : "Refresh", icon: .refresh, style: .link) { [weak app] in app?.refreshSchema(id) }
        refresh.labelFont = Graphite.ui(12, .medium)
        refresh.isEnabled = !refreshing
        let heading = hstack([label(redis ? "Keyspace" : "Schema", font: Graphite.ui(11, .semibold), color: Graphite.muted), spacer(), refresh])
        let filter = GTextField(app.schemaFilters[id] ?? "", placeholder: redis ? "Filter keys…" : "Filter tables…")
        filter.setAccessibilityLabel(redis ? "Filter keys" : "Filter tables")
        filter.onChange = { [weak self, weak app] text in
            app?.schemaFilters[id] = text
            self?.reloadTree(profile: profile)
        }
        filter.onCancel = { [weak self, weak app, weak filter] in
            app?.schemaFilters[id] = ""
            filter?.stringValue = ""
            self?.reloadTree(profile: profile)
        }
        filterField = filter
        filterProfile = id
        let search = IconView(.search, size: 13, color: Graphite.faint)
        let filterWrap = FlippedView()
        filterWrap.translatesAutoresizingMaskIntoConstraints = false
        filterWrap.pin(filter)
        filterWrap.addSubview(search)
        NSLayoutConstraint.activate([
            search.leadingAnchor.constraint(equalTo: filterWrap.leadingAnchor, constant: 8),
            search.centerYAnchor.constraint(equalTo: filterWrap.centerYAnchor),
        ])
        (filter.cell as? GTextFieldCell)?.leftInset = 26
        tree.orientation = .vertical
        tree.alignment = .leading
        tree.spacing = 1
        tree.translatesAutoresizingMaskIntoConstraints = false
        let panel = vstack([
            label(redis ? "Database index" : "Database", font: Graphite.ui(11, .semibold), color: Graphite.muted),
            databasePopup, heading, filterWrap, tree,
        ], spacing: 6)
        panel.setCustomSpacing(12, after: databasePopup)
        panel.edgeInsets = NSEdgeInsets(top: 4, left: 10, bottom: 0, right: 2)
        for view in [databasePopup, heading, filterWrap, tree] {
            view.widthAnchor.constraint(equalTo: panel.widthAnchor, constant: -12).isActive = true
        }
        reloadTree(profile: profile)
        if refocus {
            DispatchQueue.main.async { [weak filter] in
                guard let filter else { return }
                filter.window?.makeFirstResponder(filter)
                filter.currentEditor()?.selectedRange = NSRange(location: (filter.stringValue as NSString).length, length: 0)
            }
        }
        return panel
    }

    private let tree = NSStackView()

    private func reloadTree(profile: Profile) {
        tree.arrangedSubviews.forEach { $0.removeFromSuperview() }
        guard let nodes = app.schemas[profile.id] else {
            let spinner = NSProgressIndicator()
            spinner.style = .spinning
            spinner.controlSize = .small
            spinner.startAnimation(nil)
            tree.addArrangedSubview(hstack([spinner, label("Loading…", font: Graphite.ui(12.5), color: Graphite.muted)]))
            return
        }
        let query = app.schemaFilters[profile.id] ?? ""
        let visible = SchemaNode.filter(nodes, query)
        let selected = app.activeTab.flatMap { tab in tab.profileID == profile.id ? tab.target : nil }
        func add(_ node: SchemaNode, depth: Int) {
            let key = "\(profile.id)|\(depth)|\(node.kind)|\(node.name)"
            let row = TreeRow(node: node, depth: depth)
            row.color = profile.color
            if node.isLeaf {
                row.selected = selected?.schema == node.schema && selected?.table == node.table
                row.onClick = { [weak app] in app?.openTable(profile.id, schema: node.schema ?? "", table: node.table ?? "") }
            } else {
                let open = !query.trimmingCharacters(in: .whitespaces).isEmpty || openNodes.contains(key) || (depth == 0 && !closedNodes.contains(key))
                row.open = open
                row.setAccessibilityExpanded(open)
                row.onClick = { [weak self] in
                    guard let self else { return }
                    if open { self.openNodes.remove(key); self.closedNodes.insert(key) } else { self.openNodes.insert(key); self.closedNodes.remove(key) }
                    self.reloadTree(profile: profile)
                }
            }
            tree.addArrangedSubview(row)
            row.widthAnchor.constraint(equalTo: tree.widthAnchor).isActive = true
            if row.open { node.children.forEach { add($0, depth: depth + 1) } }
        }
        visible.forEach { add($0, depth: 0) }
        if !query.trimmingCharacters(in: .whitespaces).isEmpty, visible.isEmpty {
            tree.addArrangedSubview(label("No matches for “\(query.trimmingCharacters(in: .whitespaces))”.", font: Graphite.ui(12.5), color: Graphite.muted))
        }
    }
}

final class ResizeHandle: NSView {
    private let onDrag: (CGFloat) -> Void
    private let onReset: () -> Void
    private var last: CGFloat = 0

    init(onDrag: @escaping (CGFloat) -> Void, reset: @escaping () -> Void) {
        self.onDrag = onDrag
        self.onReset = reset
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        setAccessibilityElement(true)
        setAccessibilityRole(.splitter)
        setAccessibilityLabel("Resize sidebar")
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func resetCursorRects() { addCursorRect(bounds, cursor: .resizeLeftRight) }
    override func mouseDown(with event: NSEvent) {
        last = event.locationInWindow.x
        if event.clickCount == 2 { onReset() }
    }
    override func mouseDragged(with event: NSEvent) {
        onDrag(event.locationInWindow.x - last)
        last = event.locationInWindow.x
    }
}

/// Connection identity for the active tab or profile.
final class TopBar: PanelView {
    private let dot = PanelView(fill: Graphite.accent)
    private let name = label("", font: Graphite.ui(13, .semibold), color: Graphite.textStrong)
    private let target = label("", font: Graphite.mono(11), color: Graphite.faint)
    private let readOnly = Chip("Read-only", color: Graphite.muted)
    private let empty = label("No active connection", font: Graphite.ui(13), color: Graphite.muted)
    private let demoChip = Chip("Demo fixture · not a live connection", color: Graphite.modified)

    init(demo: Bool) {
        super.init(fill: Graphite.chrome, edges: [.bottom])
        dot.radius = 4
        dot.widthAnchor.constraint(equalToConstant: 8).isActive = true
        dot.heightAnchor.constraint(equalToConstant: 8).isActive = true
        demoChip.isHidden = !demo
        demoChip.toolTip = "Isolated deterministic data; profiles and edits are not persisted."
        pin(hstack([dot, name, target, readOnly, empty, spacer(), demoChip], spacing: 8), insets: NSEdgeInsets(top: 0, left: 16, bottom: 1, right: 14))
        heightAnchor.constraint(equalToConstant: 40).isActive = true
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    func show(_ profile: Profile?) {
        [dot, name, target].forEach { $0.isHidden = profile == nil }
        empty.isHidden = profile != nil
        readOnly.isHidden = !(profile?.readOnly ?? false)
        guard let profile else { return }
        dot.fill = profile.color
        name.stringValue = profile.name
        target.stringValue = profile.target
    }
}

final class TabButton: RowControl {
    let tab: WorkTab
    var color = Graphite.accent
    var renameField: GTextField?

    init(tab: WorkTab, active: Bool, onClose: @escaping () -> Void, onRename: (() -> Void)?, onCollapse: (() -> Void)?) {
        self.tab = tab
        super.init(frame: .zero)
        selected = active
        var buttons: [NSView] = []
        if !tab.collapsed, let onRename {
            buttons.append(TabButton.small(.pencil, "Rename \(tab.title)", onRename))
        }
        if !tab.collapsed, active, let onCollapse {
            buttons.append(TabButton.small(.collapse, "Collapse \(tab.title)", onCollapse))
        }
        buttons.append(TabButton.small(.close, "Close \(tab.title)", onClose))
        let actions = hstack(buttons, spacing: 0)
        addSubview(actions)
        let width = tab.collapsed ? 64 : min(230, max(96, titleWidth + 42 + CGFloat(buttons.count) * 22))
        NSLayoutConstraint.activate([
            actions.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -3),
            actions.centerYAnchor.constraint(equalTo: centerYAnchor),
            heightAnchor.constraint(equalToConstant: 32),
            widthAnchor.constraint(equalToConstant: width),
        ])
        actionsWidth = CGFloat(buttons.count) * 22
        setAccessibilityRole(.radioButton)
        setAccessibilityLabel(tab.collapsed ? "Expand \(tab.title)" : tab.title)
        toolTip = tab.collapsed ? "Expand \(tab.title)" : nil
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    private var actionsWidth: CGFloat = 22

    private static func small(_ icon: Icon, _ tooltip: String, _ action: @escaping () -> Void) -> GButton {
        let button = GButton("", icon: icon, style: .icon, tooltip: tooltip, handler: action)
        button.minHeight = 22
        button.widthAnchor.constraint(equalToConstant: 22).isActive = true
        return button
    }

    private var titleWidth: CGFloat {
        (tab.title as NSString).size(withAttributes: [.font: Graphite.ui(12.5, .medium)]).width
    }

    override func draw(_ dirtyRect: NSRect) {
        let shape = NSBezierPath()
        let r = bounds.insetBy(dx: 0.5, dy: 0)
        shape.move(to: NSPoint(x: r.minX, y: r.maxY))
        shape.line(to: NSPoint(x: r.minX, y: r.minY + 7))
        shape.appendArc(from: NSPoint(x: r.minX, y: r.minY), to: NSPoint(x: r.minX + 7, y: r.minY), radius: 7)
        shape.line(to: NSPoint(x: r.maxX - 7, y: r.minY))
        shape.appendArc(from: NSPoint(x: r.maxX, y: r.minY), to: NSPoint(x: r.maxX, y: r.minY + 7), radius: 7)
        shape.line(to: NSPoint(x: r.maxX, y: r.maxY))
        if selected {
            Graphite.bg.setFill()
            shape.fill()
            Graphite.border.setStroke()
            shape.lineWidth = 1
            shape.stroke()
            color.setFill()
            NSBezierPath(roundedRect: NSRect(x: r.minX, y: 0, width: r.width, height: 2), xRadius: 1, yRadius: 1).fill()
        } else if hovering {
            NSColor(white: 1, alpha: 0.04).setFill()
            shape.fill()
        }
        let paragraph = NSMutableParagraphStyle()
        paragraph.lineBreakMode = .byTruncatingTail
        if tab.collapsed {
            Icon.expand.draw(in: NSRect(x: 18, y: 5, width: 12, height: 12), color: hovering ? Graphite.text : Graphite.faint)
            paragraph.alignment = .center
            NSAttributedString(string: tab.title, attributes: [
                .font: Graphite.ui(9.5, .medium), .foregroundColor: Graphite.faint, .paragraphStyle: paragraph,
            ]).draw(with: NSRect(x: 4, y: 18, width: bounds.width - 24, height: 12), options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])
            return
        }
        let tint = color.blended(withFraction: 0.25, of: Graphite.muted) ?? color
        (tab.kind == .table ? Icon.table : Icon.code).draw(in: NSRect(x: 12, y: 9.5, width: 13, height: 13), color: tint)
        guard renameField == nil else { return }
        NSAttributedString(string: tab.title, attributes: [
            .font: Graphite.ui(12.5, selected ? .medium : .regular),
            .foregroundColor: selected ? Graphite.textStrong : Graphite.muted, .paragraphStyle: paragraph,
        ]).draw(with: NSRect(x: 32, y: 8.5, width: bounds.width - 38 - actionsWidth, height: 17), options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])
    }
}

final class TabStrip: PanelView {
    unowned let app: AppController
    private let row = hstack([], spacing: 1)

    init(app: AppController) {
        self.app = app
        super.init(fill: Graphite.chrome, edges: [.bottom])
        row.alignment = .bottom
        addSubview(row)
        NSLayoutConstraint.activate([
            row.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 6),
            row.bottomAnchor.constraint(equalTo: bottomAnchor),
            row.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor, constant: -6),
            heightAnchor.constraint(equalToConstant: 36),
        ])
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    func reload() {
        row.arrangedSubviews.forEach { $0.removeFromSuperview() }
        for tab in app.tabs {
            var button: TabButton!
            button = TabButton(
                tab: tab, active: app.activeTab === tab,
                onClose: { [weak app] in app?.closeTab(tab) },
                onRename: tab.kind == .query ? { [weak self] in if let button { self?.beginRename(button) } } : nil,
                onCollapse: { [weak app] in app?.collapseTab(tab) })
            button.color = app.profile(tab.profileID)?.color ?? Graphite.accent
            button.onClick = { [weak app] in app?.selectTab(tab) }
            button.onMiddleClick = { [weak app] in app?.closeTab(tab) }
            if tab.kind == .query { button.onDoubleClick = { [weak self, weak button] in if let button { self?.beginRename(button) } } }
            row.addArrangedSubview(button)
        }
        if let profile = app.activeProfileID, app.workspaces[profile] != nil {
            let add = GButton("", icon: .plus, style: .icon, tooltip: "New query") { [weak app] in app?.openQuery(profile) }
            row.addArrangedSubview(add)
            row.setCustomSpacing(4, after: row.arrangedSubviews[max(0, row.arrangedSubviews.count - 2)])
        }
    }

    private func beginRename(_ button: TabButton) {
        let field = GTextField(button.tab.title, height: 24)
        field.translatesAutoresizingMaskIntoConstraints = false
        button.renameField = field
        button.addSubview(field)
        NSLayoutConstraint.activate([
            field.leadingAnchor.constraint(equalTo: button.leadingAnchor, constant: 30),
            field.trailingAnchor.constraint(equalTo: button.trailingAnchor, constant: -30),
            field.centerYAnchor.constraint(equalTo: button.centerYAnchor),
        ])
        field.setAccessibilityLabel("Rename \(button.tab.title)")
        field.onCommit = { [weak app] text in app?.renameTab(button.tab, text) }
        field.onCancel = { [weak self] in self?.reload() }
        window?.makeFirstResponder(field)
        field.currentEditor()?.selectAll(nil)
        button.needsDisplay = true
    }
}

final class ToastView: PanelView {
    private let message = label("", font: Graphite.ui(13), color: Graphite.success)
    private let actions = hstack([], spacing: 6)
    private var timer: Timer?

    init() {
        super.init(fill: Graphite.popover)
        radius = 10
        outline = Graphite.borderStrong
        message.maximumNumberOfLines = 3
        message.lineBreakMode = .byWordWrapping
        message.preferredMaxLayoutWidth = 420
        let close = GButton("", icon: .close, style: .icon, tooltip: "Dismiss") { [weak self] in self?.hide() }
        pin(hstack([message, actions, close], spacing: 8), insets: NSEdgeInsets(top: 8, left: 12, bottom: 8, right: 6))
        widthAnchor.constraint(lessThanOrEqualToConstant: 640).isActive = true
        isHidden = true
        setAccessibilityRole(.staticText)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    func show(_ text: String, error: Bool, actions buttons: [(String, () -> Void)] = []) {
        message.stringValue = text
        message.textColor = error ? Graphite.danger : Graphite.success
        outline = error ? Graphite.danger.withAlphaComponent(0.6) : Graphite.borderStrong
        actions.arrangedSubviews.forEach { $0.removeFromSuperview() }
        for (title, action) in buttons {
            actions.addArrangedSubview(GButton(title, style: .secondary, handler: action))
        }
        isHidden = false
        NSAccessibility.post(element: self, notification: .announcementRequested,
                             userInfo: [.announcement: text, .priority: NSAccessibilityPriorityLevel.high.rawValue])
        timer?.invalidate()
        timer = Timer.scheduledTimer(withTimeInterval: error ? 10 : (buttons.isEmpty ? 6 : 12), repeats: false) { [weak self] _ in self?.hide() }
    }

    func hide() {
        timer?.invalidate()
        isHidden = true
    }
}

final class WelcomeView: FlippedView {
    private let title = label("", font: Graphite.ui(20, .semibold), color: Graphite.textStrong)
    private let message = label("", font: Graphite.ui(13), color: Graphite.muted)

    override init(frame: NSRect) {
        super.init(frame: frame)
        translatesAutoresizingMaskIntoConstraints = false
        let mark = PanelView(fill: NSColor(hex: 0x27272a))
        mark.radius = 16
        mark.outline = NSColor(white: 1, alpha: 0.07)
        mark.widthAnchor.constraint(equalToConstant: 64).isActive = true
        mark.heightAnchor.constraint(equalToConstant: 64).isActive = true
        let icon = IconView(.database, size: 28, color: Graphite.accentText)
        mark.addSubview(icon)
        NSLayoutConstraint.activate([
            icon.centerXAnchor.constraint(equalTo: mark.centerXAnchor),
            icon.centerYAnchor.constraint(equalTo: mark.centerYAnchor),
        ])
        message.alignment = .center
        message.maximumNumberOfLines = 3
        message.lineBreakMode = .byWordWrapping
        message.preferredMaxLayoutWidth = 440
        title.alignment = .center
        let stack = vstack([mark, title, message], spacing: 8, alignment: .centerX)
        stack.setCustomSpacing(22, after: mark)
        addSubview(stack)
        NSLayoutConstraint.activate([
            stack.centerXAnchor.constraint(equalTo: centerXAnchor),
            stack.topAnchor.constraint(equalTo: topAnchor, constant: 130),
            message.widthAnchor.constraint(lessThanOrEqualToConstant: 440),
        ])
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    func show(profile: Profile?, connected: Bool, hasProfiles: Bool) {
        title.stringValue = profile?.name ?? "No connection selected"
        if profile != nil {
            message.stringValue = connected
                ? "Choose a table from the sidebar or open a new query with the plus button above."
                : "This connection is selected but not connected. Select it again to connect."
        } else {
            message.stringValue = hasProfiles
                ? "Select a saved connection from the sidebar to browse its data."
                : "Create a connection from the sidebar to get started."
        }
    }
}
