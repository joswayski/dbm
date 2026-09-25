import AppKit
import QuartzCore

let pendingRefreshError = "Save or discard pending row changes before refreshing."
let pendingExportError = "Save or discard pending row changes before exporting."

protocol TableHost: AnyObject {
    func revealExport(_ url: URL, open: Bool)
    func loadTable(_ tab: WorkTab)
    func saveChanges(_ tab: WorkTab)
    func exportTable(_ tab: WorkTab)
    func copyText(_ text: String)
    /// Rows written so far by this tab's running export.
    func exportProgress(_ tab: WorkTab) -> Int?
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
    private let limitField = GTextField("200", mono: true)
    private let sortColumn = GPopUp(width: 180)
    private let sortDirection = GPopUp(items: ["Ascending", "Descending"], width: 120)
    private let directionLabel = label("Direction", font: Graphite.ui(12), color: Graphite.muted)
    private lazy var clearButton = GButton("Clear", style: .secondary) { [weak self] in self?.clearFilters() }
    private lazy var resetColumnsButton = GButton("Reset columns", style: .link, tooltip: "Restore every column's default width") { [weak self] in
        self?.resetColumns()
    }
    /// Set while widths change in code, so only drags count as resizing.
    private var adjustingColumns = false
    private lazy var applyButton = GButton("Apply filters", style: .primary) { [weak self] in self?.applyFilters() }
    private var addFilterButton: GButton?
    private var filterSignature = ""

    let grid = GridTableView()
    private lazy var gridScrollView = gridScroll(grid)
    private let emptyLabel = label("No rows match this view.", font: Graphite.ui(13), color: Graphite.muted)
    private lazy var retryButton = GButton("Couldn't load rows · Retry", style: .secondary) { [weak self] in self?.refresh() }
    private let inspector = InspectorView()
    private var inspectorWidth: NSLayoutConstraint!
    private var pendingHeight: NSLayoutConstraint!
    private let loadingOverlay = LoadingOverlay()
    private let skeleton = GridSkeleton()
    private let changePreview = ChangePreviewController()
    private let messageView = MessageView()
    private var bodyBelowFilters: NSLayoutConstraint?
    private var bodyBelowMessage: NSLayoutConstraint?
    private var overlayTimer: Timer?
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
        limitField.onCommit = { [weak self] text in self?.applyLimit(text) }
        let limitStepper = LimitStepper { [weak self] amount in self?.stepLimit(amount) }
        // `.limit-input-wrap`: an 84 pt field with the stepper inside it.
        let limitBox = PanelView(fill: Graphite.control)
        limitBox.radius = 6
        limitBox.outline = Graphite.borderStrong
        (limitField.cell as? GTextFieldCell)?.plain = true
        limitBox.addSubview(limitField)
        limitBox.addSubview(limitStepper)
        NSLayoutConstraint.activate([
            limitBox.widthAnchor.constraint(equalToConstant: 84),
            limitBox.heightAnchor.constraint(equalToConstant: 28),
            limitField.leadingAnchor.constraint(equalTo: limitBox.leadingAnchor),
            limitField.centerYAnchor.constraint(equalTo: limitBox.centerYAnchor),
            limitField.trailingAnchor.constraint(equalTo: limitStepper.leadingAnchor),
            limitStepper.trailingAnchor.constraint(equalTo: limitBox.trailingAnchor, constant: -1),
            limitStepper.topAnchor.constraint(equalTo: limitBox.topAnchor, constant: 1),
            limitStepper.bottomAnchor.constraint(equalTo: limitBox.bottomAnchor, constant: -1),
        ])
        sortColumn.onSelect = { [weak self] _ in self?.sortChanged() }
        sortDirection.onSelect = { [weak self] _ in self?.sortChanged() }
        addFilterButton = GButton("Add filter", icon: .plus, style: .link) { [weak self] in self?.addFilter() }
        let addFilter = addFilterButton!
        let join = label("All filters must match", font: Graphite.ui(12), color: Graphite.faint)
        let header = hstack([
            IconView(.filter, size: 13, color: Graphite.faint),
            label("Filters", font: Graphite.ui(12, .semibold), color: Graphite.secondary),
            join, addFilter, spacer(),
            label("Preview limit", font: Graphite.ui(12), color: Graphite.muted), limitBox,
            label("Sort by", font: Graphite.ui(12), color: Graphite.muted), sortColumn,
            directionLabel, sortDirection, resetColumnsButton, clearButton, applyButton,
        ], spacing: 6)
        // 12 pt between control groups, as in `.table-query-controls`.
        for view in [limitBox, sortColumn, sortDirection, resetColumnsButton] as [NSView] {
            header.setCustomSpacing(12, after: view)
        }
        header.setCustomSpacing(8, after: header.views[0])
        header.setCustomSpacing(8, after: header.views[1])
        header.setCustomSpacing(8, after: join)
        // When space runs out the join hint goes first, like a wrapped row.
        header.setVisibilityPriority(.detachOnlyIfNecessary, for: join)
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
        grid.onHoverRow = { [weak self] row in self?.hoverRow(row) }
        NotificationCenter.default.addObserver(self, selector: #selector(gridScrolled), name: NSView.boundsDidChangeNotification,
                                               object: gridScrollView.contentView)
        gridScrollView.contentView.postsBoundsChangedNotifications = true
        (grid.headerView as? GridHeaderView)?.onToggleColumn = { [weak self] column in self?.toggleColumn(column) }
        (grid.headerView as? GridHeaderView)?.onColumnAction = { [weak self] column in self?.setActionColumn(column) }
        let menu = NSMenu()
        menu.delegate = self
        grid.menu = menu
        let body = FlippedView()
        body.translatesAutoresizingMaskIntoConstraints = false
        body.addSubview(gridScrollView)
        body.addSubview(inspector)
        body.addSubview(emptyLabel)
        body.addSubview(retryButton)
        body.addSubview(loadingOverlay)
        body.addSubview(skeleton)
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
            skeleton.leadingAnchor.constraint(equalTo: gridScrollView.leadingAnchor),
            skeleton.trailingAnchor.constraint(equalTo: gridScrollView.trailingAnchor),
            skeleton.topAnchor.constraint(equalTo: gridScrollView.topAnchor),
            skeleton.heightAnchor.constraint(equalToConstant: Graphite.headerHeight + 5 * Graphite.rowHeight),
            loadingOverlay.leadingAnchor.constraint(equalTo: gridScrollView.leadingAnchor),
            loadingOverlay.trailingAnchor.constraint(equalTo: gridScrollView.trailingAnchor),
            loadingOverlay.topAnchor.constraint(equalTo: gridScrollView.topAnchor),
            loadingOverlay.bottomAnchor.constraint(equalTo: gridScrollView.bottomAnchor),
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
        for view in [toolbar, filters, messageView, body, pendingBar, status] as [NSView] {
            view.translatesAutoresizingMaskIntoConstraints = false
            addSubview(view)
            // The inline message is inset 12 pt from the sides.
            let inset: CGFloat = view === messageView ? 12 : 0
            NSLayoutConstraint.activate([
                view.leadingAnchor.constraint(equalTo: leadingAnchor, constant: inset),
                view.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -inset),
            ])
        }
        pendingHeight = pendingBar.heightAnchor.constraint(equalToConstant: 0)
        pendingBar.constraints.filter { $0.firstAttribute == .height && $0.secondItem == nil }.forEach { $0.isActive = false }
        NSLayoutConstraint.activate([
            toolbar.topAnchor.constraint(equalTo: topAnchor),
            filters.topAnchor.constraint(equalTo: toolbar.bottomAnchor),
            messageView.topAnchor.constraint(equalTo: filters.bottomAnchor, constant: 8),
            pendingBar.topAnchor.constraint(equalTo: body.bottomAnchor),
            status.topAnchor.constraint(equalTo: pendingBar.bottomAnchor),
            status.bottomAnchor.constraint(equalTo: bottomAnchor),
            pendingHeight,
        ])
        // Stack views stretch at the default hugging priority, and the scroll
        // view has no intrinsic height, so pin the filter panel to its content.
        // Not the header row: hugging it vertically squeezes its controls.
        for stack in [filterStack, filterRows] {
            stack.setHuggingPriority(.required, for: .vertical)
        }
        bodyBelowFilters = body.topAnchor.constraint(equalTo: filters.bottomAnchor)
        bodyBelowMessage = body.topAnchor.constraint(equalTo: messageView.bottomAnchor, constant: 8)
        bodyBelowFilters?.isActive = true
        messageView.onDismiss = { [weak self] in
            self?.tab.tableState?.message = nil
            self?.layoutMessage()
        }
        messageView.onOpen = { [weak self] url in self?.host?.revealExport(url, open: true) }
        messageView.onReveal = { [weak self] url in self?.host?.revealExport(url, open: false) }
        let grow = body.heightAnchor.constraint(equalToConstant: 10_000)
        grow.priority = .defaultLow
        grow.isActive = true
        reload()
    }

    // MARK: State → views

    func reload() {
        let state = self.state
        let page = state.page
        titleSchema.stringValue = tab.target?.schema ?? ""
        titleTable.stringValue = ".\(tab.target?.table ?? "")"
        if let page {
            meta.stringValue = "\(page.total.map(formatCount) ?? "—") rows · \(page.columns.count) columns"
        } else {
            meta.stringValue = ""
        }
        state.loading ? spinner.startAnimation(nil) : spinner.stopAnimation(nil)
        refreshButton.title = state.loading ? "Refreshing…" : "Refresh"
        refreshButton.isEnabled = !state.loading
        copyButton.title = "Copy visible (\(state.copyableRows.count))"
        copyButton.isEnabled = page != nil && !state.loading
        let exporting = host?.isExporting(tab) ?? false
        if exporting {
            let done = host?.exportProgress(tab) ?? 0
            exportButton.title = "Exporting \(formatCount(done))\(page?.total.map { " / \(formatCount($0))" } ?? "")…"
        } else {
            exportButton.title = "Export all\(page?.total.map { " (\(formatCount($0)))" } ?? "")"
        }
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
        updateLoadingOverlay(visible: state.loading && page != nil)
        skeleton.isHidden = !(state.loading && page == nil)
        if let row = changePreview.row, state.pending[row] == nil { changePreview.close() }
        messageView.show(state.message)
        layoutMessage()
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
            let total = page.total.map { " of \(formatCount($0))" } ?? ""
            statusRows.stringValue = page.rows.isEmpty ? "No rows\(total)" : "Rows \(formatCount(page.offset + 1))–\(formatCount(page.offset + page.rows.count))\(total)"
            perPage.stringValue = "\(page.limit) per page"
            let paged = page.offset > 0 || page.hasMore
            let pages = page.total.map { max(1, Int(ceil(Double($0) / Double(page.limit)))) }
            pageLabel.stringValue = "Page \(formatCount(page.offset / page.limit + 1))\(pages.map { " of \(formatCount($0))" } ?? "")"
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
        // Disabled until a page has loaded, as in the desktop.
        let loaded = state.page != nil
        addFilterButton?.isEnabled = loaded
        applyButton.isEnabled = loaded
        clearButton.isEnabled = loaded
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
        resetColumnsButton.isHidden = !state.columnsResized && state.collapsedColumns.isEmpty
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
            adjustingColumns = true
            defer { adjustingColumns = false }
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
                adjustingColumns = true
                column.width = collapsed ? 76 : page.columns.indices.contains(index) ? defaultColumnWidth(page.columns[index]) : 200
                adjustingColumns = false
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
        if let pending = state.pending[row] {
            view.mark = pending.deleted ? .deleted : .modified
        }
        return view
    }

    // MARK: Change preview

    /// Shows the staged row's before → after card while the pointer is over
    /// it, like the desktop app. A card that is closing finishes first.
    func hoverRow(_ row: Int?) {
        guard let row, editor == nil, let pending = state.pending[row], let page = state.page, let window,
              row < grid.numberOfRows else {
            changePreview.scheduleClose()
            return
        }
        if changePreview.row == row { changePreview.cancelClose(); return }
        if changePreview.row != nil, changePreview.closing { return }
        let rect = window.convertToScreen(grid.convert(grid.rect(ofRow: row), to: nil))
        changePreview.show(row: row, pending: pending, columns: page.columns, rowRect: rect, in: window) { [weak self] in
            self?.discardPending(row)
        }
    }

    @objc private func gridScrolled() { changePreview.close() }

    /// Discards a staged row, keeping its edits when only a delete is undone.
    private func discardPending(_ row: Int) {
        state.discardPending(row)
        grid.reloadData()
        reload()
    }

    override func viewWillMove(toWindow newWindow: NSWindow?) {
        super.viewWillMove(toWindow: newWindow)
        if newWindow == nil { changePreview.close() }
    }

    private func toggleColumn(_ column: Int) {
        if state.collapsedColumns.contains(column) { state.collapsedColumns.remove(column) } else { state.collapsedColumns.insert(column) }
        reloadGridColumns()
        reloadFilters()
    }

    private func resetColumns() {
        guard let page = state.page else { return }
        state.collapsedColumns.removeAll()
        state.columnsResized = false
        reloadGridColumns()
        adjustingColumns = true
        for (index, column) in grid.tableColumns.enumerated() where page.columns.indices.contains(index) {
            column.width = defaultColumnWidth(page.columns[index])
        }
        adjustingColumns = false
        reloadFilters()
    }

    func tableViewColumnDidResize(_ notification: Notification) {
        guard !adjustingColumns, !state.columnsResized, let column = notification.userInfo?["NSTableColumn"] as? NSTableColumn,
              let index = Int(column.identifier.rawValue), !state.collapsedColumns.contains(index) else { return }
        state.columnsResized = true
        reloadFilters()
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
        cell.collapsed = state.collapsedColumns.contains(index)
        cell.columnAction = actionColumn.map { $0 == index ? GridCellView.ColumnAction.target : .dimmed } ?? GridCellView.ColumnAction.none
        return cell
    }

    /// The column whose header collapse/expand control is hovered.
    private var actionColumn: Int?

    private func setActionColumn(_ column: Int?) {
        actionColumn = column
        let rows = grid.rows(in: grid.visibleRect)
        guard rows.length > 0 else { return }
        for row in rows.location..<(rows.location + rows.length) {
            for index in 0..<grid.numberOfColumns {
                guard let cell = grid.view(atColumn: index, row: row, makeIfNecessary: false) as? GridCellView else { continue }
                cell.columnAction = column.map { $0 == index ? GridCellView.ColumnAction.target : .dimmed } ?? GridCellView.ColumnAction.none
            }
        }
    }

    func tableViewSelectionDidChange(_ notification: Notification) {
        guard state.selected != grid.selectedRowIndexes else { return }
        state.selected = grid.selectedRowIndexes
        reload()
    }

    func tableView(_ tableView: NSTableView, didClick tableColumn: NSTableColumn) {
        guard let page = state.page, let index = Int(tableColumn.identifier.rawValue) else { return }
        guard state.pending.isEmpty else { showMessage(.error(pendingRefreshError)); return }
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
              !page.primaryKey.contains(page.columns[column].name), state.pending[row]?.deleted != true,
              !state.collapsedColumns.contains(column) else { return }
        if let size = largeValueSize(state.values(row)[column]) {
            showMessage(.error("This \(size) value is too large to edit in the grid; update it with a query."))
            return
        }
        beginEditing(row: row, column: column)
    }

    private func beginEditing(row: Int, column: Int) {
        endEditingOverlay()
        let field = GTextField(Helpers.editableText(state.values(row)[column]), mono: true, height: Graphite.rowHeight)
        if let cell = field.cell as? GTextFieldCell {
            cell.gridEditor = true
            cell.leftInset = 10
        }
        field.textColor = .white
        if state.page?.columns[column].numeric == true { field.alignment = .right }
        field.constraints.filter { $0.firstAttribute == .height }.forEach { $0.isActive = false }
        field.frame = grid.frameOfCell(atColumn: column, row: row)
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
                self?.discardPending(clicked)
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
        guard state.pending.isEmpty else { showMessage(.error(pendingRefreshError)); return }
        host?.loadTable(tab)
    }

    private func copyVisible() {
        let rows = state.copyableRows
        host?.copyText(state.csv(rows))
        showMessage(.notice("Copied \(rows.count) visible \(rows.count == 1 ? "row" : "rows") as CSV."))
    }

    private func copySelected() {
        let rows = state.selected.filter { state.pending[$0]?.deleted != true }
        guard !rows.isEmpty else { return }
        host?.copyText(state.csv(Array(rows)))
        showMessage(.notice("Copied \(rows.count) selected \(rows.count == 1 ? "row" : "rows") as CSV."))
    }

    private func export() {
        guard state.pending.isEmpty else { showMessage(.error(pendingExportError)); return }
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

    /// Like the desktop app, only dim a visible page when a reload takes
    /// longer than 120 ms, so quick refreshes don't flash.
    private func updateLoadingOverlay(visible: Bool) {
        guard visible else {
            overlayTimer?.invalidate()
            overlayTimer = nil
            loadingOverlay.isHidden = true
            return
        }
        guard loadingOverlay.isHidden, overlayTimer == nil else { return }
        overlayTimer = Timer.scheduledTimer(withTimeInterval: 0.12, repeats: false) { [weak self] _ in
            guard let self else { return }
            self.overlayTimer = nil
            guard self.state.loading && self.state.page != nil else { return }
            // `loading-overlay-in`: a 180 ms fade.
            self.loadingOverlay.alphaValue = 0
            self.loadingOverlay.isHidden = false
            NSAnimationContext.runAnimationGroup { context in
                context.duration = 0.18
                context.timingFunction = CAMediaTimingFunction(name: .easeOut)
                self.loadingOverlay.animator().alphaValue = 1
            }
        }
    }

    func showMessage(_ message: InlineMessage) {
        state.message = message
        messageView.show(message)
        layoutMessage()
    }

    private func layoutMessage() {
        let visible = !messageView.isHidden
        bodyBelowFilters?.isActive = !visible
        bodyBelowMessage?.isActive = visible
    }

    private func stepLimit(_ amount: Int) {
        let current = Int(limitField.stringValue.trimmingCharacters(in: .whitespaces)) ?? state.limit
        let next = min(maxPreviewRows, max(1, current + amount))
        limitField.stringValue = String(next)
        applyLimit(String(next))
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
        guard state.pending.isEmpty else { showMessage(.error(pendingRefreshError)); reload(); return }
        host?.loadTable(tab)
    }

    private func page(_ delta: Int) {
        guard state.pending.isEmpty else { return }
        state.pageIndex = max(0, state.pageIndex + delta)
        host?.loadTable(tab)
    }

    private func discard() {
        state.pending.removeAll()
        // The "save or discard first" errors no longer apply.
        if let text = state.message?.text, text == pendingRefreshError || text == pendingExportError { state.message = nil }
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
    private let keySummary = label("", font: Graphite.mono(12), color: Graphite.muted)
    private let badge = Chip("", color: Graphite.modified)
    private let fields = vstack([], spacing: 12)
    private lazy var scroll = verticalScroll(fieldsContainer)
    private let fieldsContainer = FlippedView()
    private let empty = label("", font: Graphite.ui(12.5), color: Graphite.faint)
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
        header.pin(hstack([title, keySummary, badge, spacer(), close], spacing: 8), insets: NSEdgeInsets(top: 0, left: 14, bottom: 1, right: 8))
        header.heightAnchor.constraint(equalToConstant: 40).isActive = true
        fieldsContainer.pin(fields, insets: NSEdgeInsets(top: 14, left: 14, bottom: 14, right: 14))
        empty.alignment = .center
        empty.maximumNumberOfLines = 3
        empty.lineBreakMode = .byWordWrapping
        // `.row-inspector-actions`: one full-width secondary button.
        deleteButton.minHeight = 30
        footer.pin(deleteButton, insets: NSEdgeInsets(top: 13, left: 14, bottom: 12, right: 14))
        // Takes the height while the fields are hidden, so the empty
        // inspector never shrinks the grid body to its header.
        let filler = NSView()
        filler.translatesAutoresizingMaskIntoConstraints = false
        filler.setContentHuggingPriority(.init(2), for: .vertical)
        filler.setContentCompressionResistancePriority(.init(1), for: .vertical)
        let column = vstack([header, scroll, filler, footer], spacing: 0)
        column.distribution = .fill
        [header, scroll, filler, footer].forEach { $0.widthAnchor.constraint(equalTo: column.widthAnchor).isActive = true }
        scroll.setContentHuggingPriority(.init(1), for: .vertical)
        scroll.setContentCompressionResistancePriority(.init(1), for: .vertical)
        column.setHuggingPriority(.init(1), for: .vertical)
        pin(column, insets: NSEdgeInsets(top: 0, left: 1, bottom: 0, right: 0))
        addSubview(empty)
        NSLayoutConstraint.activate([
            empty.topAnchor.constraint(equalTo: topAnchor, constant: 40 + 28),
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
        deleteButton.tint = deleted ? nil : Graphite.danger
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
                // `.inspector-field`: mono name and type, a note, a 30 pt input.
                let name = label(column.name, font: Graphite.mono(11.5), color: Graphite.secondary)
                let type = label(column.dataType, font: Graphite.mono(11), color: NSColor(hex: 0x6f6f76))
                let note = label("", font: Graphite.ui(11), color: Graphite.faint)
                note.alignment = .right
                let header = hstack([name, type, spacer(), note], spacing: 6)
                let field = GTextField("", mono: true, height: 30)
                let large = largeValueSize(values[index])
                let readOnly = !editable || isKey || deleted || large != nil
                field.isEditable = !readOnly
                field.isSelectable = true
                if let cell = field.cell as? GTextFieldCell {
                    cell.leftInset = 10
                    cell.plain = readOnly
                }
                if readOnly { field.textColor = Graphite.muted }
                field.onCommit = { [weak self] text in self?.commit(row: row, column: index, text: text) }
                let group = vstack([header, field], spacing: 5)
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
            let large = largeValueSize(current)
            notes[index].stringValue = page.primaryKey.contains(column.name) ? "Primary key"
                : large.map { "\($0) — too large to edit here" } ?? (changed ? "was \(displayValue(original))" : "")
            notes[index].textColor = changed ? Graphite.modified : Graphite.faint
            if let cell = field.cell as? GTextFieldCell, !cell.plain {
                cell.fillOverride = changed ? Graphite.modifiedSoft : NSColor(hex: 0x232326)
                cell.strokeOverride = changed ? NSColor(hex: 0xf0b14c, alpha: 0.45) : Graphite.borderStrong
                field.needsDisplay = true
            }
            if let large, field.currentEditor() == nil {
                field.stringValue = String((current as? String ?? "").prefix(160)) + "…"
                field.toolTip = "This \(large) value is too large to edit here; update it with a query."
            } else if field.currentEditor() == nil {
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

/// Up/down arrows beside the preview limit, like the desktop stepper.
final class LimitStepper: NSView {
    private let onStep: (Int) -> Void
    private var hoverHalf: Int?

    init(onStep: @escaping (Int) -> Void) {
        self.onStep = onStep
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        // Sits inside the limit field; its height comes from the field.
        widthAnchor.constraint(equalToConstant: 22).isActive = true
        toolTip = "Step the preview limit"
        let up = IconView(.chevronUp, size: 10, color: Graphite.muted)
        let down = IconView(.chevronDown, size: 10, color: Graphite.muted)
        for (icon, offset) in [(up, CGFloat(-6)), (down, CGFloat(6))] {
            addSubview(icon)
            icon.centerXAnchor.constraint(equalTo: centerXAnchor).isActive = true
            icon.centerYAnchor.constraint(equalTo: centerYAnchor, constant: offset).isActive = true
        }
        setAccessibilityElement(true)
        setAccessibilityRole(.incrementor)
        setAccessibilityLabel("Preview limit stepper")
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override var isFlipped: Bool { true }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        trackingAreas.forEach(removeTrackingArea)
        addTrackingArea(NSTrackingArea(rect: bounds, options: [.mouseMoved, .mouseEnteredAndExited, .activeInKeyWindow], owner: self))
    }

    private func half(_ event: NSEvent) -> Int {
        convert(event.locationInWindow, from: nil).y < bounds.midY ? 1 : -1
    }

    override func mouseMoved(with event: NSEvent) { hoverHalf = half(event); needsDisplay = true }
    override func mouseExited(with event: NSEvent) { hoverHalf = nil; needsDisplay = true }
    override func mouseDown(with event: NSEvent) { onStep(half(event)) }
    override func accessibilityPerformIncrement() -> Bool { onStep(1); return true }
    override func accessibilityPerformDecrement() -> Bool { onStep(-1); return true }

    override func draw(_ dirtyRect: NSRect) {
        Graphite.borderStrong.setFill()
        NSRect(x: 0, y: 0, width: 1, height: bounds.height).fill()
        guard let hoverHalf else { return }
        let rect = NSRect(x: 1, y: hoverHalf == 1 ? 0 : bounds.midY, width: bounds.width - 1, height: bounds.height / 2)
        Graphite.controlHover.setFill()
        rect.fill()
    }
}

/// Dims the grid with a "Refreshing…" pill while a visible page reloads.
final class LoadingOverlay: NSView {
    init() {
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        isHidden = true
        let pill = PanelView(fill: Graphite.popover)
        pill.radius = 13
        pill.outline = Graphite.borderStrong
        let text = label("Refreshing…", font: Graphite.ui(12), color: Graphite.secondary)
        pill.pin(text, insets: NSEdgeInsets(top: 6, left: 12, bottom: 6, right: 12))
        addSubview(pill)
        pill.centerXAnchor.constraint(equalTo: centerXAnchor).isActive = true
        pill.centerYAnchor.constraint(equalTo: centerYAnchor).isActive = true
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func draw(_ dirtyRect: NSRect) {
        NSColor(red: 22 / 255, green: 22 / 255, blue: 24 / 255, alpha: 0.35).setFill()
        bounds.fill()
    }

    // Like `pointer-events: none`: the grid underneath keeps the cursor.
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
}

/// `.initial-grid-skeleton`: a header band and five rows that pulse while
/// the first page loads.
final class GridSkeleton: NSView {
    init() {
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        wantsLayer = true
        isHidden = true
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        guard window != nil, layer?.animation(forKey: "pulse") == nil else { return }
        let pulse = CABasicAnimation(keyPath: "opacity")
        pulse.fromValue = 0.8
        pulse.toValue = 0.45
        pulse.duration = 0.75
        pulse.autoreverses = true
        pulse.repeatCount = .infinity
        layer?.add(pulse, forKey: "pulse")
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override var isFlipped: Bool { true }

    override func draw(_ dirtyRect: NSRect) {
        Graphite.gridHeader.setFill()
        NSRect(x: 0, y: 0, width: bounds.width, height: Graphite.headerHeight).fill()
        NSColor(white: 1, alpha: 0.03).setFill()
        NSRect(x: 0, y: Graphite.headerHeight, width: bounds.width, height: bounds.height - Graphite.headerHeight).fill()
        Graphite.hairline.setFill()
        for row in 0...5 {
            let y = Graphite.headerHeight + CGFloat(row) * Graphite.rowHeight - 1
            NSRect(x: 0, y: y, width: bounds.width, height: 1).fill()
        }
    }
}
