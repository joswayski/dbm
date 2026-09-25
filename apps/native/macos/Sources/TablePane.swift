import AppKit

let pendingRefreshError = "Save or discard pending row changes before refreshing."
let pendingExportError = "Save or discard pending row changes before exporting."

protocol TableHost: AnyObject {
    func revealExport(_ url: URL, open: Bool)
    func loadTable(_ tab: WorkTab)
    func saveChanges(_ tab: WorkTab)
    func exportTable(_ tab: WorkTab)
    func copyText(_ text: String, notice: String)
    func showError(_ message: String)
    func isReadOnly(_ tab: WorkTab) -> Bool
    func isSaving(_ tab: WorkTab) -> Bool
    func isExporting(_ tab: WorkTab) -> Bool
}

/// A table tab, or the editable viewer under a `SELECT *` query result.
/// Mirrors `TableView.tsx`.
final class TablePane: NSView, NSTableViewDataSource, NSTableViewDelegate, NSMenuDelegate {
    private weak var host: TableHost?
    let tab: WorkTab
    private let embedded: Bool
    private var state: TableState { tab.tableState! }

    private let titleSchema = label("", font: Graphite.ui(15), color: Graphite.muted)
    private let titleTable = label("", font: Graphite.ui(15, .semibold), color: Graphite.textStrong)
    private let meta = label("", font: Graphite.ui(12), color: Graphite.faint)
    private let spinner = NSProgressIndicator()
    private lazy var refreshButton = GButton("Refresh", icon: .refresh, style: .toolbar,
                                             tooltip: "Reload the current page with the same filters and sort") { [weak self] in self?.refresh() }
    private lazy var copyButton = GButton("Copy visible", icon: .copy, style: .toolbar,
                                          tooltip: "Copies only the current preview page") { [weak self] in self?.copyVisible() }
    private lazy var exportButton = GButton("Export all", icon: .download, style: .toolbar,
                                            tooltip: "Prompts for a location and exports every filtered row") { [weak self] in self?.export() }
    private lazy var selectionButton: GButton = {
        let button = GButton("", icon: .chevronDown, style: .toolbar, tooltip: "Actions for the selected rows") { [weak self] in self?.showSelectionMenu() }
        button.iconTrailing = true
        return button
    }()
    private lazy var inspectorButton = GButton("", icon: .inspector, style: .icon,
                                               tooltip: "Row inspector") { [weak self] in self?.toggleInspector() }

    private let filterRows = vstack([], spacing: 6)
    private let limitField = GTextField("200", mono: false)
    private let sortColumn = GPopUp(width: 180)
    private let sortDirection = GPopUp(items: ["Ascending", "Descending"], width: 120)
    private let directionLabel = label("Direction", font: Graphite.ui(12), color: Graphite.muted)
    private lazy var clearButton = GButton("Clear", style: .secondary) { [weak self] in self?.clearFilters() }
    private lazy var applyButton = GButton("Apply filters", style: .primary) { [weak self] in self?.applyFilters() }
    private var filterSignature = ""

    let grid = GridTableView()
    private lazy var gridScrollView = gridScroll(grid)
    private let emptyLabel = label("No rows match this view.", font: Graphite.ui(13), color: Graphite.muted)
    private lazy var retryButton = GButton("Couldn't load rows · Retry", style: .secondary) { [weak self] in self?.refresh() }
    private let inspector = InspectorView()
    private var inspectorWidth: NSLayoutConstraint!
    private var pendingHeight: NSLayoutConstraint!
    private var columnSignature = ""
    private var editor: GTextField?
    private var editingCell: (row: Int, column: Int)?

    private let pendingBar = PanelView(fill: Graphite.chrome, edges: [.top])
    private let pendingTitle = label("", font: Graphite.ui(13, .semibold), color: Graphite.textStrong)
    private let editedChip = Chip("", color: Graphite.modified)
    private let deletedChip = Chip("", color: Graphite.danger)
    private let readOnlyChip = Chip("Read-only connection", color: Graphite.muted)
    private lazy var discardButton = GButton("Discard changes", style: .secondary) { [weak self] in self?.discard() }
    private lazy var saveButton = GButton("Save changes", style: .primary) { [weak self] in self?.save() }

    private let statusRows = label("", font: Graphite.ui(12), color: Graphite.muted)
    private let perPage = label("", font: Graphite.ui(12), color: Graphite.muted)
    private let pageLabel = label("", font: Graphite.ui(12), color: Graphite.muted)
    private lazy var previousButton = GButton("", icon: .chevronLeft, style: .icon, tooltip: "Previous page") { [weak self] in self?.page(-1) }
    private lazy var nextButton = GButton("", icon: .chevronRight, style: .icon, tooltip: "Next page") { [weak self] in self?.page(1) }

    init(tab: WorkTab, host: TableHost, embedded: Bool) {
        self.tab = tab
        self.host = host
        self.embedded = embedded
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        build()
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override var isFlipped: Bool { true }

    // MARK: Layout

    private func build() {
        let toolbar = PanelView(fill: embedded ? Graphite.bg : Graphite.chrome, edges: [.bottom])
        spinner.style = .spinning
        spinner.controlSize = .small
        spinner.isDisplayedWhenStopped = false
        let title = hstack([IconView(.table, size: 15, color: Graphite.muted), titleSchema, titleTable, meta, spinner], spacing: 6)
        titleSchema.setContentCompressionResistancePriority(.defaultHigh, for: .horizontal)
        (title.views[2] as? NSTextField)?.setContentCompressionResistancePriority(.defaultHigh, for: .horizontal)
        title.setCustomSpacing(0, after: titleSchema)
        title.setCustomSpacing(10, after: titleTable)
        let actions = hstack([refreshButton, copyButton, exportButton, selectionButton, inspectorButton], spacing: 4)
        let toolbarRow = hstack([title, spacer(), actions], spacing: 8)
        toolbar.pin(toolbarRow, insets: NSEdgeInsets(top: 0, left: 14, bottom: 1, right: 12))
        toolbar.heightAnchor.constraint(equalToConstant: Graphite.toolbarHeight).isActive = true

        let filters = PanelView(fill: Graphite.chrome, edges: [.bottom])
        limitField.widthAnchor.constraint(equalToConstant: 56).isActive = true
        limitField.onCommit = { [weak self] text in self?.applyLimit(text) }
        sortColumn.onSelect = { [weak self] _ in self?.sortChanged() }
        sortDirection.onSelect = { [weak self] _ in self?.sortChanged() }
        let addFilter = GButton("Add filter", icon: .plus, style: .link) { [weak self] in self?.addFilter() }
        let header = hstack([
            IconView(.search, size: 13, color: Graphite.muted),
            label("Filters", font: Graphite.ui(13, .semibold), color: Graphite.textStrong),
            label("All filters must match", font: Graphite.ui(12), color: Graphite.faint),
            addFilter, spacer(),
            label("Preview limit", font: Graphite.ui(12), color: Graphite.muted), limitField,
            label("Sort by", font: Graphite.ui(12), color: Graphite.muted), sortColumn,
            directionLabel, sortDirection, clearButton, applyButton,
        ], spacing: 8)
        let filterStack = vstack([header, filterRows], spacing: 8)
        header.widthAnchor.constraint(equalTo: filterStack.widthAnchor).isActive = true
        filterRows.widthAnchor.constraint(equalTo: filterStack.widthAnchor).isActive = true
        filters.pin(filterStack, insets: NSEdgeInsets(top: 8, left: 14, bottom: 9, right: 14))

        grid.dataSource = self
        grid.delegate = self
        grid.target = self
        grid.doubleAction = #selector(doubleClicked)
        grid.onDelete = { [weak self] in self?.deleteSelection() }
        grid.onEscape = { [weak self] in self?.grid.deselectAll(nil) }
        grid.onCopy = { [weak self] in self?.copySelected() }
        (grid.headerView as? GridHeaderView)?.onToggleColumn = { [weak self] column in self?.toggleColumn(column) }
        let menu = NSMenu()
        menu.delegate = self
        grid.menu = menu
        let body = FlippedView()
        body.translatesAutoresizingMaskIntoConstraints = false
        body.addSubview(gridScrollView)
        body.addSubview(inspector)
        body.addSubview(emptyLabel)
        body.addSubview(retryButton)
        inspector.onClose = { [weak self] in self?.toggleInspector() }
        inspectorWidth = inspector.widthAnchor.constraint(equalToConstant: Graphite.inspectorWidth)
        NSLayoutConstraint.activate([
            gridScrollView.leadingAnchor.constraint(equalTo: body.leadingAnchor),
            gridScrollView.topAnchor.constraint(equalTo: body.topAnchor),
            gridScrollView.bottomAnchor.constraint(equalTo: body.bottomAnchor),
            gridScrollView.trailingAnchor.constraint(equalTo: inspector.leadingAnchor),
            inspector.topAnchor.constraint(equalTo: body.topAnchor),
            inspector.bottomAnchor.constraint(equalTo: body.bottomAnchor),
            inspector.trailingAnchor.constraint(equalTo: body.trailingAnchor),
            inspectorWidth,
            emptyLabel.centerXAnchor.constraint(equalTo: gridScrollView.centerXAnchor),
            emptyLabel.centerYAnchor.constraint(equalTo: gridScrollView.centerYAnchor),
            retryButton.centerXAnchor.constraint(equalTo: gridScrollView.centerXAnchor),
            retryButton.centerYAnchor.constraint(equalTo: gridScrollView.centerYAnchor),
        ])

        let dot = IconView(.check, size: 10)
        dot.isHidden = true
        let pendingDot = PanelView(fill: Graphite.modified)
        pendingDot.radius = 4
        pendingDot.widthAnchor.constraint(equalToConstant: 8).isActive = true
        pendingDot.heightAnchor.constraint(equalToConstant: 8).isActive = true
        let pendingRow = hstack([pendingDot, pendingTitle, editedChip, deletedChip, readOnlyChip, spacer(), discardButton, saveButton], spacing: 10)
        pendingBar.pin(pendingRow, insets: NSEdgeInsets(top: 1, left: 14, bottom: 0, right: 14))
        pendingBar.heightAnchor.constraint(equalToConstant: Graphite.pendingBarHeight).isActive = true

        let status = PanelView(fill: Graphite.chrome, edges: [.top])
        let statusRow = hstack([statusRows, spacer(), perPage, previousButton, pageLabel, nextButton], spacing: 8)
        statusRow.setCustomSpacing(14, after: perPage)
        status.pin(statusRow, insets: NSEdgeInsets(top: 1, left: 14, bottom: 0, right: 10))
        status.heightAnchor.constraint(equalToConstant: Graphite.statusBarHeight).isActive = true

        // Explicit constraints: the grid body takes all space the bars leave.
        for view in [toolbar, filters, body, pendingBar, status] as [NSView] {
            view.translatesAutoresizingMaskIntoConstraints = false
            addSubview(view)
            NSLayoutConstraint.activate([
                view.leadingAnchor.constraint(equalTo: leadingAnchor),
                view.trailingAnchor.constraint(equalTo: trailingAnchor),
            ])
        }
        pendingHeight = pendingBar.heightAnchor.constraint(equalToConstant: 0)
        pendingBar.constraints.filter { $0.firstAttribute == .height && $0.secondItem == nil }.forEach { $0.isActive = false }
        NSLayoutConstraint.activate([
            toolbar.topAnchor.constraint(equalTo: topAnchor),
            filters.topAnchor.constraint(equalTo: toolbar.bottomAnchor),
            body.topAnchor.constraint(equalTo: filters.bottomAnchor),
            pendingBar.topAnchor.constraint(equalTo: body.bottomAnchor),
            status.topAnchor.constraint(equalTo: pendingBar.bottomAnchor),
            status.bottomAnchor.constraint(equalTo: bottomAnchor),
            pendingHeight,
        ])
        filters.setContentHuggingPriority(.required, for: .vertical)
        filters.setContentCompressionResistancePriority(.required, for: .vertical)
        reload()
    }

    // MARK: State → views

    func reload() {
        let state = self.state
        let page = state.page
        titleSchema.stringValue = tab.target?.schema ?? ""
        titleTable.stringValue = ".\(tab.target?.table ?? "")"
        if let page {
            meta.stringValue = "\(page.total.map(String.init) ?? "—") rows · \(page.columns.count) columns"
        } else {
            meta.stringValue = ""
        }
        state.loading ? spinner.startAnimation(nil) : spinner.stopAnimation(nil)
        refreshButton.title = state.loading ? "Refreshing…" : "Refresh"
        refreshButton.isEnabled = !state.loading
        copyButton.title = "Copy visible (\(state.copyableRows.count))"
        copyButton.isEnabled = page != nil && !state.loading
        let exporting = host?.isExporting(tab) ?? false
        exportButton.title = exporting ? "Exporting…" : "Export all\(page?.total.map { " (\($0))" } ?? "")"
        exportButton.isEnabled = page != nil && !state.loading && !exporting
        selectionButton.title = "\(state.selected.count) selected"
        selectionButton.isHidden = state.selected.isEmpty
        inspectorButton.isOn = state.inspectorOpen
        inspectorButton.toolTip = state.inspectorOpen ? "Hide row inspector" : "Show row inspector"
        inspectorWidth.constant = state.inspectorOpen ? Graphite.inspectorWidth : 0
        inspector.isHidden = !state.inspectorOpen

        reloadFilters()
        reloadGridColumns()
        if editor == nil { grid.reloadData() }
        if grid.selectedRowIndexes != state.selected {
            grid.selectRowIndexes(state.selected, byExtendingSelection: false)
        }
        emptyLabel.isHidden = page == nil || !(page?.rows.isEmpty ?? true)
        retryButton.isHidden = page != nil || state.loading
        gridScrollView.isHidden = page == nil
        let interactive = !state.loading && !(host?.isSaving(tab) ?? false)
        grid.isEnabled = interactive
        inspector.show(tab: tab, host: host, editable: editable, interactive: interactive) { [weak self] in self?.stagedFromInspector() }

        let pendingCount = state.pending.count
        let deleted = state.pending.values.filter(\.deleted).count
        let edited = pendingCount - deleted
        pendingBar.isHidden = pendingCount == 0
        pendingHeight.constant = pendingCount == 0 ? 0 : Graphite.pendingBarHeight
        pendingTitle.stringValue = "\(pendingCount) pending \(pendingCount == 1 ? "change" : "changes")"
        editedChip.text = "\(edited) \(edited == 1 ? "edited row" : "edited rows")"
        editedChip.isHidden = edited == 0
        deletedChip.text = "\(deleted) \(deleted == 1 ? "deletion" : "deletions")"
        deletedChip.isHidden = deleted == 0
        let readOnly = host?.isReadOnly(tab) ?? true
        readOnlyChip.isHidden = !readOnly
        let saving = host?.isSaving(tab) ?? false
        saveButton.title = saving ? "Saving…" : "Save changes (\(pendingCount))"
        saveButton.isEnabled = !readOnly && !saving
        discardButton.isEnabled = !saving

        if let page {
            let total = page.total.map { " of \($0)" } ?? ""
            statusRows.stringValue = page.rows.isEmpty ? "No rows\(total)" : "Rows \(page.offset + 1)–\(page.offset + page.rows.count)\(total)"
            perPage.stringValue = "\(page.limit) per page"
            let paged = page.offset > 0 || page.hasMore
            let pages = page.total.map { max(1, Int(ceil(Double($0) / Double(page.limit)))) }
            pageLabel.stringValue = "Page \(page.offset / page.limit + 1)\(pages.map { " of \($0)" } ?? "")"
            [previousButton, pageLabel, nextButton].forEach { $0.isHidden = !paged }
            previousButton.isEnabled = page.offset > 0 && state.pending.isEmpty && !state.loading
            nextButton.isEnabled = page.hasMore && state.pending.isEmpty && !state.loading
        } else {
            statusRows.stringValue = state.loading ? "Loading rows…" : ""
            perPage.stringValue = ""
            [previousButton, pageLabel, nextButton].forEach { $0.isHidden = true }
        }
        limitField.stringValue = String(state.limit)
    }

    private var editable: Bool {
        guard let page = state.page else { return false }
        return !(host?.isReadOnly(tab) ?? true) && !page.primaryKey.isEmpty
    }

    private func reloadFilters() {
        guard let page = state.page else { return }
        let names = page.columns.map(\.name)
        let effective = state.effectiveOrder
        var sortItems = names.map { page.primaryKey.contains($0) ? "\($0) (primary key)" : $0 }
        if page.primaryKey.isEmpty { sortItems.insert("Choose a sort column", at: 0) }
        if sortColumn.itemTitles != sortItems { sortColumn.setItems(sortItems) }
        if let effective, let index = names.firstIndex(of: effective.column) {
            sortColumn.selectItem(at: index + (page.primaryKey.isEmpty ? 1 : 0))
        } else {
            sortColumn.selectItem(at: 0)
        }
        sortDirection.selectItem(at: effective?.descending == true ? 1 : 0)
        sortDirection.isHidden = effective == nil
        directionLabel.isHidden = effective == nil
        clearButton.isHidden = !(!state.applied.isEmpty || state.filters.count > 1
            || state.filters.contains { !filterNeedsValue($0.operatorName) || !$0.value.trimmingCharacters(in: .whitespaces).isEmpty })

        let signature = "\(names)|\(state.filters.count)|\(state.filters.map(\.operatorName))"
        guard signature != filterSignature else { return }
        filterSignature = signature
        filterRows.arrangedSubviews.forEach { $0.removeFromSuperview() }
        for (index, filter) in state.filters.enumerated() {
            let column = GPopUp(items: names, width: 170)
            column.selectItem(withTitle: filter.column)
            column.onSelect = { [weak self] selected in self?.state.filters[index].column = names[selected] }
            let operatorPopup = GPopUp(items: filterOperators.map(\.1), width: 170)
            operatorPopup.selectItem(at: filterOperators.firstIndex { $0.0 == filter.operatorName } ?? 2)
            operatorPopup.onSelect = { [weak self] selected in
                self?.state.filters[index].operatorName = filterOperators[selected].0
                self?.reload()
            }
            let value: NSView
            if filterNeedsValue(filter.operatorName) {
                let field = GTextField(filter.value, placeholder: ["in", "notIn"].contains(filter.operatorName) ? "value 1, value 2, …" : "Value…")
                field.onChange = { [weak self] text in self?.state.filters[index].value = text }
                field.onCommit = { [weak self] text in
                    self?.state.filters[index].value = text
                    if NSApp.currentEvent?.type == .keyDown, NSApp.currentEvent?.keyCode == 36 { self?.applyFilters() }
                }
                value = field
            } else {
                value = label("No value needed", font: Graphite.ui(12.5), color: Graphite.faint)
            }
            let remove = GButton("", icon: .close, style: .icon, tooltip: "Remove filter") { [weak self] in
                self?.state.filters.remove(at: index)
                self?.reload()
            }
            let row = hstack([column, operatorPopup, value, remove], spacing: 8)
            value.setContentHuggingPriority(.init(1), for: .horizontal)
            filterRows.addArrangedSubview(row)
            row.widthAnchor.constraint(equalTo: filterRows.widthAnchor).isActive = true
        }
    }

    private func reloadGridColumns() {
        guard let page = state.page else {
            grid.tableColumns.forEach(grid.removeTableColumn)
            columnSignature = ""
            return
        }
        let order = state.effectiveOrder
        let signature = page.columns.map { "\($0.name):\($0.dataType)" }.joined(separator: ",")
        if signature != columnSignature {
            columnSignature = signature
            grid.tableColumns.forEach(grid.removeTableColumn)
            for (index, column) in page.columns.enumerated() {
                grid.addGridColumn(id: String(index), title: column.name, dataType: column.dataType,
                                   primaryKey: page.primaryKey.contains(column.name),
                                   width: defaultColumnWidth(column), rightAligned: column.numeric)
            }
        }
        for (index, column) in grid.tableColumns.enumerated() {
            let name = page.columns.indices.contains(index) ? page.columns[index].name : ""
            let header = column.headerCell as? GridHeaderCell
            header?.sort = order?.column == name ? order?.descending : nil
            let collapsed = state.collapsedColumns.contains(index)
            if header?.collapsed != collapsed {
                header?.collapsed = collapsed
                column.width = collapsed ? 76 : page.columns.indices.contains(index) ? defaultColumnWidth(page.columns[index]) : 200
                column.resizingMask = collapsed ? [] : .userResizingMask
            }
            column.headerToolTip = collapsed ? "Expand \(name)" : "Sort by \(name)"
        }
        grid.headerView?.needsDisplay = true
    }

    // MARK: Grid data

    func numberOfRows(in tableView: NSTableView) -> Int { state.page?.rows.count ?? 0 }

    func tableView(_ tableView: NSTableView, rowViewForRow row: Int) -> NSTableRowView? {
        let view = GridRowView()
        if let pending = state.pending[row], let page = state.page {
            view.mark = pending.deleted ? .deleted : .modified
            view.toolTip = changePreview(pending, columns: page.columns)
        }
        return view
    }

    /// The staged change on a row, before → after, as the desktop app's hover preview.
    private func changePreview(_ pending: PendingRow, columns: [Column]) -> String {
        if pending.deleted { return "Pending delete\nThis row will be deleted when changes are saved." }
        let lines = columns.indices.compactMap { index -> String? in
            guard jsonKey(pending.changes[index]) != jsonKey(pending.original[index]) else { return nil }
            return "\(columns[index].name): \(displayValue(pending.original[index])) → \(displayValue(pending.changes[index]))"
        }
        return (["Pending edit"] + lines).joined(separator: "\n")
    }

    private func toggleColumn(_ column: Int) {
        if state.collapsedColumns.contains(column) { state.collapsedColumns.remove(column) } else { state.collapsedColumns.insert(column) }
        reloadGridColumns()
    }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard let tableColumn, let index = Int(tableColumn.identifier.rawValue), let page = state.page else { return nil }
        let cell = (tableView.makeView(withIdentifier: .init("grid-cell"), owner: nil) as? GridCellView) ?? {
            let view = GridCellView()
            view.identifier = .init("grid-cell")
            return view
        }()
        let pending = state.pending[row]
        let values = state.values(row)
        cell.rightAligned = page.columns[index].numeric
        cell.deleted = pending?.deleted ?? false
        cell.modified = pending.map { jsonKey($0.changes[index]) != jsonKey($0.original[index]) } ?? false
        cell.value = values.indices.contains(index) ? values[index] : NSNull()
        return cell
    }

    func tableViewSelectionDidChange(_ notification: Notification) {
        guard state.selected != grid.selectedRowIndexes else { return }
        state.selected = grid.selectedRowIndexes
        reload()
    }

    func tableView(_ tableView: NSTableView, didClick tableColumn: NSTableColumn) {
        guard let page = state.page, let index = Int(tableColumn.identifier.rawValue) else { return }
        guard state.pending.isEmpty else { host?.showError(pendingRefreshError); return }
        let name = page.columns[index].name
        let current = state.effectiveOrder
        state.order = (name, current?.column == name ? !(current?.descending ?? false) : false)
        state.pageIndex = 0
        host?.loadTable(tab)
    }

    // MARK: Editing

    @objc private func doubleClicked() {
        let row = grid.clickedRow, column = grid.clickedColumn
        guard row >= 0, column >= 0, editable, let page = state.page,
              !page.primaryKey.contains(page.columns[column].name), state.pending[row]?.deleted != true else { return }
        beginEditing(row: row, column: column)
    }

    private func beginEditing(row: Int, column: Int) {
        endEditingOverlay()
        let field = GTextField(Helpers.editableText(state.values(row)[column]), mono: true, height: Graphite.rowHeight - 2)
        field.frame = grid.frameOfCell(atColumn: column, row: row).insetBy(dx: 1, dy: 1)
        field.translatesAutoresizingMaskIntoConstraints = true
        field.onCommit = { [weak self] text in
            guard let self else { return }
            self.state.stage(row: row, column: column, text: text)
            self.endEditingOverlay()
            self.grid.reloadData()
            self.reload()
        }
        field.onCancel = { [weak self] in self?.endEditingOverlay() }
        grid.addSubview(field)
        editor = field
        editingCell = (row, column)
        window?.makeFirstResponder(field)
        field.currentEditor()?.selectAll(nil)
    }

    private func endEditingOverlay() {
        guard let editor else { return }
        self.editor = nil
        editingCell = nil
        editor.onCommit = nil
        editor.removeFromSuperview()
    }

    private func stagedFromInspector() {
        grid.reloadData()
        reload()
    }

    private func deleteSelection() {
        guard editable, !state.selected.isEmpty else { return }
        state.toggleDelete(Array(state.selected))
        grid.reloadData()
        reload()
    }

    // MARK: Menus

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        let clicked = grid.clickedRow
        if clicked >= 0, !state.selected.contains(clicked) {
            grid.selectRowIndexes(IndexSet(integer: clicked), byExtendingSelection: false)
        }
        if clicked >= 0, let pending = state.pending[clicked] {
            menu.addItem(ClosureMenuItem(pending.deleted ? "Undo delete" : "Discard edit") { [weak self] in
                self?.state.pending[clicked] = nil
                self?.grid.reloadData()
                self?.reload()
            })
            menu.addItem(.separator())
        }
        fillSelectionMenu(menu)
    }

    private func fillSelectionMenu(_ menu: NSMenu) {
        let rows = Array(state.selected)
        guard !rows.isEmpty else { return }
        menu.addItem(ClosureMenuItem("Copy selected as CSV") { [weak self] in self?.copySelected() })
        if editable {
            let allDeleted = rows.allSatisfy { state.pending[$0]?.deleted == true }
            let title = allDeleted ? "Undo staged deletion" : rows.count == 1 ? "Stage row for deletion" : "Stage \(rows.count) rows for deletion"
            let item = ClosureMenuItem(title) { [weak self] in self?.deleteSelection() }
            if !allDeleted {
                item.attributedTitle = NSAttributedString(string: title, attributes: [.foregroundColor: Graphite.danger, .font: NSFont.menuFont(ofSize: 0)])
            }
            menu.addItem(item)
        }
        menu.addItem(ClosureMenuItem("Clear selection") { [weak self] in self?.grid.deselectAll(nil) })
    }

    private func showSelectionMenu() {
        let menu = NSMenu()
        fillSelectionMenu(menu)
        menu.popUp(positioning: nil, at: NSPoint(x: 0, y: selectionButton.bounds.height + 4), in: selectionButton)
    }

    // MARK: Actions

    private func refresh() {
        guard state.pending.isEmpty else { host?.showError(pendingRefreshError); return }
        host?.loadTable(tab)
    }

    private func copyVisible() {
        let rows = state.copyableRows
        host?.copyText(state.csv(rows), notice: "Copied \(rows.count) visible \(rows.count == 1 ? "row" : "rows") as CSV.")
    }

    private func copySelected() {
        let rows = state.selected.filter { state.pending[$0]?.deleted != true }
        guard !rows.isEmpty else { return }
        host?.copyText(state.csv(Array(rows)), notice: "Copied \(rows.count) selected \(rows.count == 1 ? "row" : "rows") as CSV.")
    }

    private func export() {
        guard state.pending.isEmpty else { host?.showError(pendingExportError); return }
        host?.exportTable(tab)
    }

    private func toggleInspector() {
        state.inspectorOpen.toggle()
        reload()
    }

    private func addFilter() {
        state.filters.append(FilterDraft(column: state.page?.columns.first?.name ?? ""))
        reload()
    }

    private func applyFilters() {
        window?.makeFirstResponder(nil)
        state.applied = state.filters.compactMap { filter in
            let value = filter.value.trimmingCharacters(in: .whitespaces)
            guard !filter.column.isEmpty, !filterNeedsValue(filter.operatorName) || !value.isEmpty else { return nil }
            return ["column": filter.column, "operator": filter.operatorName,
                    "value": filterNeedsValue(filter.operatorName) ? value as Any : NSNull()]
        }
        state.pageIndex = 0
        reloadIfClean()
    }

    private func clearFilters() {
        state.filters = state.page?.columns.first.map { [FilterDraft(column: $0.name)] } ?? []
        state.applied = []
        state.pageIndex = 0
        filterSignature = ""
        reloadIfClean()
    }

    private func applyLimit(_ text: String) {
        let limit = min(maxPreviewRows, max(1, Int(text.trimmingCharacters(in: .whitespaces)) ?? state.limit))
        guard limit != state.limit else { limitField.stringValue = String(limit); return }
        state.limit = limit
        state.pageIndex = 0
        reloadIfClean()
    }

    private func sortChanged() {
        guard let page = state.page else { return }
        let offset = page.primaryKey.isEmpty ? 1 : 0
        let index = sortColumn.indexOfSelectedItem - offset
        guard page.columns.indices.contains(index) else { return }
        let name = page.columns[index].name
        let changedColumn = state.effectiveOrder?.column != name
        state.order = (name, changedColumn ? false : sortDirection.indexOfSelectedItem == 1)
        state.pageIndex = 0
        reloadIfClean()
    }

    private func reloadIfClean() {
        guard state.pending.isEmpty else { host?.showError(pendingRefreshError); reload(); return }
        host?.loadTable(tab)
    }

    private func page(_ delta: Int) {
        guard state.pending.isEmpty else { return }
        state.pageIndex = max(0, state.pageIndex + delta)
        host?.loadTable(tab)
    }

    private func discard() {
        state.pending.removeAll()
        endEditingOverlay()
        grid.reloadData()
        reload()
    }

    private func save() {
        window?.makeFirstResponder(nil)
        host?.saveChanges(tab)
    }
}

final class ClosureMenuItem: NSMenuItem {
    private let handler: () -> Void

    init(_ title: String, key: String = "", handler: @escaping () -> Void) {
        self.handler = handler
        super.init(title: title, action: #selector(run), keyEquivalent: key)
        target = self
    }

    required init(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    @objc private func run() { handler() }
}

/// The row inspector: every field of the single selected row, with staged
/// edits committed on Enter or focus loss and reverted with Escape.
final class InspectorView: PanelView {
    var onClose: (() -> Void)?
    private let titleRow = NSStackView()
    private let keySummary = label("", font: Graphite.mono(11), color: Graphite.muted)
    private let badge = Chip("", color: Graphite.modified)
    private let fields = vstack([], spacing: 10)
    private lazy var scroll = verticalScroll(fieldsContainer)
    private let fieldsContainer = FlippedView()
    private let empty = label("", font: Graphite.ui(13), color: Graphite.muted)
    private let footer = PanelView(fill: nil, edges: [.top])
    private lazy var deleteButton = GButton("Delete row", icon: .trash, style: .secondary) { [weak self] in self?.toggleDelete() }
    private var editors: [GTextField] = []
    private var notes: [NSTextField] = []
    private var shownRow: Int?
    private var shownSignature = ""
    private weak var tab: WorkTab?
    private var onStaged: (() -> Void)?

    init() {
        super.init(fill: Graphite.chrome, edges: [.left])
        let close = GButton("", icon: .close, style: .icon, tooltip: "Hide row inspector") { [weak self] in self?.onClose?() }
        let title = label("Row", font: Graphite.ui(13, .semibold), color: Graphite.textStrong)
        let header = PanelView(fill: nil, edges: [.bottom])
        header.pin(hstack([title, keySummary, badge, spacer(), close], spacing: 8), insets: NSEdgeInsets(top: 0, left: 12, bottom: 1, right: 8))
        header.heightAnchor.constraint(equalToConstant: 40).isActive = true
        fieldsContainer.pin(fields, insets: NSEdgeInsets(top: 12, left: 12, bottom: 12, right: 12))
        empty.alignment = .center
        empty.maximumNumberOfLines = 3
        empty.lineBreakMode = .byWordWrapping
        footer.pin(hstack([deleteButton, spacer()]), insets: NSEdgeInsets(top: 11, left: 12, bottom: 10, right: 12))
        let column = vstack([header, scroll, footer], spacing: 0)
        column.distribution = .fill
        [header, scroll, footer].forEach { $0.widthAnchor.constraint(equalTo: column.widthAnchor).isActive = true }
        scroll.setContentHuggingPriority(.init(1), for: .vertical)
        pin(column, insets: NSEdgeInsets(top: 0, left: 1, bottom: 0, right: 0))
        addSubview(empty)
        NSLayoutConstraint.activate([
            empty.topAnchor.constraint(equalTo: topAnchor, constant: 64),
            empty.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 20),
            empty.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -20),
        ])
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    func show(tab: WorkTab, host: TableHost?, editable: Bool, interactive: Bool, onStaged: @escaping () -> Void) {
        self.tab = tab
        self.onStaged = onStaged
        guard let state = tab.tableState, let page = state.page else { return }
        let single = state.selected.count == 1 ? state.selected.first.flatMap { page.rows.indices.contains($0) ? $0 : nil } : nil
        empty.isHidden = single != nil
        scroll.isHidden = single == nil
        footer.isHidden = single == nil || !editable
        empty.stringValue = state.selected.count > 1
            ? "\(state.selected.count) rows selected. Select a single row to inspect it."
            : "Select a row to inspect and edit its fields."
        guard let row = single else {
            keySummary.stringValue = ""
            badge.isHidden = true
            shownRow = nil
            return
        }
        let pending = state.pending[row]
        let deleted = pending?.deleted ?? false
        badge.isHidden = pending == nil
        badge.text = deleted ? "Staged for deletion" : "Edited"
        badge.color = deleted ? Graphite.danger : Graphite.modified
        keySummary.stringValue = page.primaryKey.compactMap { key in
            page.columns.firstIndex { $0.name == key }.map { "\(key) = \(displayValue(page.rows[row][$0]))" }
        }.joined(separator: ", ")
        deleteButton.title = deleted ? "Restore row" : "Delete row"
        deleteButton.icon = deleted ? .undo : .trash
        deleteButton.style = deleted ? .secondary : .danger
        deleteButton.isEnabled = interactive

        let values = state.values(row)
        let signature = "\(row)|\(page.columns.map(\.name))|\(editable)|\(deleted)"
        if signature != shownSignature || shownRow != row {
            shownSignature = signature
            shownRow = row
            fields.arrangedSubviews.forEach { $0.removeFromSuperview() }
            editors = []
            notes = []
            for (index, column) in page.columns.enumerated() {
                let isKey = page.primaryKey.contains(column.name)
                let name = label(column.name, font: Graphite.ui(12, .semibold), color: Graphite.textStrong)
                let type = label(column.dataType, font: Graphite.mono(11), color: Graphite.faint)
                let note = label("", font: Graphite.ui(11), color: Graphite.modified)
                note.alignment = .right
                let header = hstack([name, type, spacer(), note], spacing: 6)
                let field = GTextField("", mono: true)
                field.isEditable = editable && !isKey && !deleted
                field.onCommit = { [weak self] text in self?.commit(row: row, column: index, text: text) }
                let group = vstack([header, field], spacing: 4)
                header.widthAnchor.constraint(equalTo: group.widthAnchor).isActive = true
                field.widthAnchor.constraint(equalTo: group.widthAnchor).isActive = true
                fields.addArrangedSubview(group)
                group.widthAnchor.constraint(equalTo: fields.widthAnchor).isActive = true
                editors.append(field)
                notes.append(note)
            }
        }
        for (index, column) in page.columns.enumerated() where editors.indices.contains(index) {
            let field = editors[index]
            let original = page.rows[row][index]
            let current = values[index]
            let changed = !deleted && jsonKey(current) != jsonKey(original)
            notes[index].stringValue = page.primaryKey.contains(column.name) ? "Primary key" : changed ? "was \(displayValue(original))" : ""
            if field.currentEditor() == nil {
                field.stringValue = Helpers.editableText(current)
                field.setPlaceholder(current is NSNull ? "NULL" : "")
            }
            field.isEnabled = interactive
        }
    }

    private func commit(row: Int, column: Int, text: String) {
        guard let state = tab?.tableState, state.selected.first == row else { return }
        state.stage(row: row, column: column, text: text)
        onStaged?()
    }

    private func toggleDelete() {
        guard let state = tab?.tableState, let row = shownRow else { return }
        state.toggleDelete([row])
        onStaged?()
    }
}
