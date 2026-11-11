import UIKit

/// The workbench: a persistent source list at 700pt and wider, otherwise an
/// Explorer sheet; query and table tabs over one result grid.
@MainActor
final class WorkspaceViewController: UIViewController, UITextViewDelegate {
    let model: AppModel
    private var renderer: ModelRenderer?
    private lazy var sidebar = SourceListView(model: model, close: nil)
    private let titleLabel = label(size: 13, bold: true), subtitle = label(size: 11, color: Graphite.faint)
    private let colorDot = dot(Graphite.accent)
    private var explorerButton: UIButton!
    private let tabs = hstack([], spacing: 1)
    private let queryBar = UIView(), tableBar = UIView()
    private let editorTitle = label(color: Graphite.secondary)
    private var refreshQuery: UIButton!, run: UIButton!
    private let tableIcon = UIImageView(), tableTitle = label()
    private let editor = UITextView()
    private let editorBox = UIView()
    private let grid = GridView()
    private let rowsLabel = label(size: 11, color: Graphite.muted)
    private let truncated = label("Results truncated at 1,000 rows", size: 11, color: Graphite.muted)
    private let pageLabel = label(size: 11)
    private var previousPage: UIButton!, nextPage: UIButton!
    private weak var explorer: UIViewController?
    private var shownActive: String?, shownShowingTable = false, wide = false

    init(model: AppModel) { self.model = model; super.init(nibName: nil, bundle: nil) }
    required init?(coder: NSCoder) { fatalError() }

    override func viewDidLoad() {
        view.backgroundColor = Graphite.bg
        explorerButton = button(nil, image: "sidebar.left", size: 13) { [weak self] in self?.showExplorer() }
        explorerButton.accessibilityLabel = "Explorer"; explorerButton.accessibilityIdentifier = "explorer"; explorerButton.size(44, 44)
        let disconnect = button(nil, image: "rectangle.portrait.and.arrow.right", size: 13) { [weak self] in self?.model.disconnect() }
        disconnect.accessibilityLabel = "Disconnect"; disconnect.size(44, 44)
        let header = hstack([colorDot, vstack([titleLabel, subtitle], spacing: 1, alignment: .leading), spacer(), explorerButton, disconnect], spacing: 9)
            .padded(.init(top: 0, left: 12, bottom: 0, right: 12))
        header.backgroundColor = Graphite.chrome; header.size(nil, 48)

        let tabScroll = UIScrollView(); tabScroll.showsHorizontalScrollIndicator = false
        tabScroll.addSubview(tabs); tabs.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            tabs.leadingAnchor.constraint(equalTo: tabScroll.contentLayoutGuide.leadingAnchor, constant: 8),
            tabs.trailingAnchor.constraint(equalTo: tabScroll.contentLayoutGuide.trailingAnchor, constant: -8),
            tabs.topAnchor.constraint(equalTo: tabScroll.contentLayoutGuide.topAnchor),
            tabs.bottomAnchor.constraint(equalTo: tabScroll.contentLayoutGuide.bottomAnchor),
            tabs.heightAnchor.constraint(equalTo: tabScroll.frameLayoutGuide.heightAnchor),
        ])
        tabScroll.backgroundColor = Graphite.chrome; tabScroll.size(nil, 38); tabScroll.bottomBorder()

        refreshQuery = button("Refresh", image: "arrow.clockwise", style: .secondary, height: 36) { [weak self] in self?.model.refresh() }
        refreshQuery.accessibilityIdentifier = "refresh-query"
        run = button("Run", image: "play.fill", style: .primary, height: 36) { [weak self] in self?.model.run() }
        run.accessibilityIdentifier = "run-query"
        editorTitle.font = Graphite.font(12)
        editorTitle.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        bar(queryBar, [editorTitle, spacer(), refreshQuery, run], spacing: 6)
        let tableRefresh = button("Refresh", image: "arrow.clockwise", style: .secondary, height: 36) { [weak self] in self?.model.refresh() }
        tableIcon.contentMode = .center; tableIcon.setContentHuggingPriority(.required, for: .horizontal)
        bar(tableBar, [tableIcon, tableTitle, spacer(), tableRefresh], spacing: 8)

        editor.font = Graphite.font(12); editor.textColor = Graphite.text; editor.backgroundColor = Graphite.bg; editor.tintColor = Graphite.accent
        editor.autocorrectionType = .no; editor.autocapitalizationType = .none; editor.spellCheckingType = .no
        editor.smartQuotesType = .no; editor.smartDashesType = .no; editor.smartInsertDeleteType = .no
        editor.accessibilityIdentifier = "query-editor"; editor.delegate = self
        editorBox.backgroundColor = Graphite.bg
        editorBox.addSubview(editor); editor.pin(to: editorBox, insets: .init(top: 8, left: 8, bottom: 8, right: 8))
        let preferred = editor.heightAnchor.constraint(equalToConstant: 170); preferred.priority = .defaultHigh
        NSLayoutConstraint.activate([preferred, editor.heightAnchor.constraint(greaterThanOrEqualToConstant: 112)])

        truncated.accessibilityIdentifier = "query-truncated"
        previousPage = button(nil, image: "chevron.left") { [weak self] in self?.model.previous() }
        previousPage.accessibilityLabel = "Previous"; previousPage.size(36, 44)
        nextPage = button(nil, image: "chevron.right") { [weak self] in self?.model.next() }
        nextPage.accessibilityLabel = "Next"; nextPage.size(36, 44)
        let footer = hstack([rowsLabel, spacer(), truncated, previousPage, pageLabel, nextPage], spacing: 12).padded(.init(top: 0, left: 12, bottom: 0, right: 12))
        footer.backgroundColor = Graphite.chrome; footer.size(nil, 44); footer.topBorder()
        let gridMinimum = grid.heightAnchor.constraint(greaterThanOrEqualToConstant: 80); gridMinimum.priority = .init(800)
        gridMinimum.isActive = true

        let workbench = vstack([header, tabScroll, queryBar, editorBox, tableBar, grid, footer])
        let row = hstack([sidebar, workbench], spacing: 0, alignment: .fill)
        sidebar.size(250, nil); sidebar.isHidden = true
        let footerBackground = UIView(); footerBackground.backgroundColor = Graphite.chrome
        let topBackground = UIView(); topBackground.backgroundColor = Graphite.chrome
        for background in [footerBackground, topBackground] { view.addSubview(background); background.translatesAutoresizingMaskIntoConstraints = false }
        view.addSubview(row); row.translatesAutoresizingMaskIntoConstraints = false
        let guide = view.safeAreaLayoutGuide
        NSLayoutConstraint.activate([
            row.topAnchor.constraint(equalTo: guide.topAnchor), row.bottomAnchor.constraint(equalTo: guide.bottomAnchor),
            row.leadingAnchor.constraint(equalTo: guide.leadingAnchor), row.trailingAnchor.constraint(equalTo: guide.trailingAnchor),
            footerBackground.topAnchor.constraint(equalTo: footer.topAnchor), footerBackground.bottomAnchor.constraint(equalTo: view.bottomAnchor),
            footerBackground.leadingAnchor.constraint(equalTo: view.leadingAnchor), footerBackground.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            topBackground.topAnchor.constraint(equalTo: view.topAnchor), topBackground.bottomAnchor.constraint(equalTo: header.bottomAnchor),
            topBackground.leadingAnchor.constraint(equalTo: view.leadingAnchor), topBackground.trailingAnchor.constraint(equalTo: view.trailingAnchor),
        ])
        renderer = ModelRenderer(model) { [weak self] in self?.render() }
        render()
    }

    // Before layout, so the first pass already has the right columns.
    override func viewWillLayoutSubviews() {
        super.viewWillLayoutSubviews()
        let isWide = view.bounds.width >= 700
        guard isWide != wide else { return }
        wide = isWide; sidebar.isHidden = !isWide; explorerButton.isHidden = isWide
        if isWide { explorer?.dismiss(animated: false) }
    }

    func textViewDidChange(_ textView: UITextView) { model.sql = textView.text }

    private func bar(_ container: UIView, _ views: [UIView], spacing: CGFloat) {
        let stack = hstack(views, spacing: spacing)
        container.addSubview(stack); stack.pin(to: container, insets: .init(top: 0, left: 12, bottom: 0, right: 12))
        container.backgroundColor = Graphite.chrome; container.size(nil, 42); container.bottomBorder()
    }

    private func render() {
        let color = UIColor(hex: model.active?.color ?? "#4c9aff")
        let redis = model.active?.engine == .redis
        colorDot.backgroundColor = color
        titleLabel.text = model.active?.name ?? "Anybase"
        subtitle.text = "\(model.active?.engine.title ?? "") · \(model.database)"
        if model.active?.id != shownActive || (model.showingTable && !shownShowingTable) { explorer?.dismiss(animated: true) }
        shownActive = model.active?.id; shownShowingTable = model.showingTable

        tabs.arrangedSubviews.forEach { $0.removeFromSuperview() }
        tabs.addArrangedSubview(tab(redis ? "Redis" : "SQL", icon: "chevron.left.forwardslash.chevron.right", active: !model.showingTable, color: color) { [weak self] in self?.model.showQuery() })
        if let table = model.selectedTable {
            tabs.addArrangedSubview(tab(table.table, icon: redis ? "key" : "tablecells", active: model.showingTable, color: color) { [weak self] in self?.model.showTable() })
        }

        queryBar.isHidden = model.showingTable; editorBox.isHidden = model.showingTable
        tableBar.isHidden = !model.showingTable || model.selectedTable == nil
        editorTitle.text = redis ? "Command" : "SQL editor"
        refreshQuery.isEnabled = model.lastExecuted != nil
        run.isEnabled = !model.sql.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        if editor.text != model.sql { editor.text = model.sql }
        editor.accessibilityLabel = redis ? "Redis command editor" : "SQL editor"
        if let table = model.selectedTable {
            tableIcon.image = UIImage(systemName: redis ? "key" : "tablecells"); tableIcon.tintColor = color
            tableTitle.text = "\(table.schema).\(table.table)"
        }

        let data = model.grid
        grid.show(data)
        rowsLabel.text = data.rows.isEmpty ? "0 rows" : "Rows \(data.offset + 1)–\(data.offset + data.rows.count)"
        truncated.isHidden = !data.truncated
        for view in [previousPage!, pageLabel, nextPage!] { view.isHidden = !model.showingTable }
        previousPage.isEnabled = data.offset > 0; nextPage.isEnabled = data.hasMore
        pageLabel.text = "Page \(data.offset / data.limit + 1)"
    }

    private func tab(_ title: String, icon: String, active: Bool, color: UIColor, action: @escaping () -> Void) -> UIView {
        let tab = button(title, image: icon, height: 38, action: action)
        var config = tab.configuration; config?.contentInsets = .init(top: 0, leading: 12, bottom: 0, trailing: 12); tab.configuration = config
        tab.backgroundColor = active ? Graphite.bg : Graphite.chrome
        if active { tab.accessibilityTraits.insert(.selected) }
        let indicator = UIView(); indicator.backgroundColor = active ? color : .clear
        tab.addSubview(indicator); indicator.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([indicator.topAnchor.constraint(equalTo: tab.topAnchor), indicator.heightAnchor.constraint(equalToConstant: 2),
                                     indicator.leadingAnchor.constraint(equalTo: tab.leadingAnchor), indicator.trailingAnchor.constraint(equalTo: tab.trailingAnchor)])
        return tab
    }

    private func showExplorer() {
        let sheet = UIViewController()
        let list = SourceListView(model: model) { [weak sheet] in sheet?.dismiss(animated: true) }
        sheet.view.backgroundColor = Graphite.sidebar
        sheet.view.addSubview(list); list.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            list.topAnchor.constraint(equalTo: sheet.view.topAnchor), list.bottomAnchor.constraint(equalTo: sheet.view.safeAreaLayoutGuide.bottomAnchor),
            list.leadingAnchor.constraint(equalTo: sheet.view.safeAreaLayoutGuide.leadingAnchor), list.trailingAnchor.constraint(equalTo: sheet.view.safeAreaLayoutGuide.trailingAnchor),
        ])
        sheet.sheetPresentationController?.detents = [.large()]
        explorer = sheet
        topPresenter.present(sheet, animated: true)
    }
}

/// Connections, the active connection's database, and its schema tree.
@MainActor
final class SourceListView: UIView {
    let model: AppModel
    private let close: (() -> Void)?
    private var renderer: ModelRenderer?
    private let activeRow = vstack([]), otherRows = vstack([])
    private let activeSection = vstack([])
    private let databasePicker = UIButton(configuration: .plain())
    private let filter = GraphiteTextField(placeholder: "Filter schema", identifier: "schema-filter")
    private let tree = vstack([], spacing: 2)
    private let schemaTitle = label(size: 11, bold: true, color: Graphite.muted)
    private var refreshSchema: UIButton!, addConnection: UIButton!
    private var expanded: Set<SchemaItem.ID> = []

    init(model: AppModel, close: (() -> Void)?) {
        self.model = model; self.close = close
        super.init(frame: .zero)
        backgroundColor = Graphite.sidebar
        var header: [UIView] = [symbol("cylinder", color: Graphite.accent), label("Anybase", size: 14, bold: true), spacer()]
        if let close { header.append(button("Done", size: 15, action: close)) }
        let top = hstack(header).padded(.init(top: 12, left: 12, bottom: 12, right: 12))

        var picker = UIButton.Configuration.plain()
        picker.indicator = .popup; picker.baseForegroundColor = Graphite.accent; picker.contentInsets = .init(top: 0, leading: 12, bottom: 0, trailing: 12)
        databasePicker.configuration = picker
        databasePicker.showsMenuAsPrimaryAction = true
        databasePicker.contentHorizontalAlignment = .leading; databasePicker.accessibilityIdentifier = "database-picker"
        databasePicker.size(nil, 36)
        refreshSchema = button(nil, image: "arrow.clockwise") { [weak self] in self?.model.loadSchema() }
        refreshSchema.accessibilityLabel = "Refresh schema"; refreshSchema.size(44, 44)
        filter.addAction(UIAction { [weak self] _ in self?.renderTree() }, for: .editingChanged)
        filter.returnKeyType = .done
        filter.addAction(UIAction { [weak self] _ in self?.filter.resignFirstResponder() }, for: .editingDidEndOnExit)
        let divider = UIView(); divider.backgroundColor = Graphite.border; divider.size(nil, 0.5)
        for view in [divider.padded(.init(top: 8, left: 0, bottom: 8, right: 0)), sectionLabel("Database"), databasePicker.padded(.init(top: 0, left: 8, bottom: 0, right: 8)),
                     hstack([schemaTitle, spacer(), refreshSchema]).padded(.init(top: 0, left: 12, bottom: 0, right: 0)),
                     filter.padded(.init(top: 8, left: 8, bottom: 8, right: 8)), tree] {
            activeSection.addArrangedSubview(view)
        }

        let scroll = UIScrollView(); scroll.alwaysBounceVertical = true; scroll.accessibilityIdentifier = "source-list-scroll"
        scroll.keyboardDismissMode = .onDrag
        let content = vstack([sectionLabel("Connections"), activeRow, activeSection, otherRows])
        scroll.addSubview(content); content.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            content.topAnchor.constraint(equalTo: scroll.contentLayoutGuide.topAnchor),
            content.bottomAnchor.constraint(equalTo: scroll.contentLayoutGuide.bottomAnchor),
            content.leadingAnchor.constraint(equalTo: scroll.frameLayoutGuide.leadingAnchor),
            content.trailingAnchor.constraint(equalTo: scroll.frameLayoutGuide.trailingAnchor),
        ])
        addConnection = button("New connection", image: "plus", size: 13) { [weak self] in self?.edit(ProfileDraft()) }
        addConnection.contentHorizontalAlignment = .leading; addConnection.size(nil, 44)
        let bottom = addConnection.padded(.init(top: 0, left: 12, bottom: 0, right: 12)); bottom.topBorder()
        let column = vstack([top, scroll, bottom])
        addSubview(column); column.pin(to: self)
        renderer = ModelRenderer(model) { [weak self] in self?.render() }
        render()
    }
    required init?(coder: NSCoder) { fatalError() }

    private func sectionLabel(_ text: String) -> UIView { label(text, size: 11, bold: true, color: Graphite.muted).padded(.init(top: 5, left: 12, bottom: 5, right: 12)) }

    private func render() {
        let active = model.active
        for stack in [activeRow, otherRows] { stack.arrangedSubviews.forEach { $0.removeFromSuperview() } }
        for profile in model.profiles { (profile.id == active?.id ? activeRow : otherRows).addArrangedSubview(row(profile)) }
        activeSection.isHidden = active == nil
        schemaTitle.text = active?.engine == .redis ? "Keys" : "Schema"
        var picker = databasePicker.configuration
        picker?.attributedTitle = AttributedString(model.database, attributes: AttributeContainer([.font: Graphite.font(13)]))
        databasePicker.configuration = picker
        databasePicker.menu = UIMenu(children: model.databases.map { name in
            UIAction(title: name, state: name == model.database ? .on : .off) { [weak self] _ in self?.model.switchDatabase(name) }
        })
        refreshSchema.isEnabled = !model.loading; addConnection.isEnabled = !model.loading
        databasePicker.isEnabled = !model.loading; filter.isEnabled = !model.loading
        renderTree()
    }

    private func row(_ profile: Profile) -> UIView {
        let isActive = model.active?.id == profile.id
        let details = vstack([label(profile.name), label("\(profile.engine.title) · \(profile.host)", size: 11, color: Graphite.faint)], spacing: 2, alignment: .leading)
        let content = hstack([dot(UIColor(hex: profile.color)), details, spacer()], spacing: 9)
        content.isUserInteractionEnabled = false
        let row = UIButton(primaryAction: UIAction { [weak self] _ in
            guard let self else { return }
            if isActive { close?() } else if model.isDemo { model.connect(profile); close?() } else { edit(ProfileDraft(profile)) }
        })
        row.addSubview(content); content.pin(to: row, insets: .init(top: 0, left: 10, bottom: 0, right: 10))
        row.backgroundColor = isActive ? UIColor.white.withAlphaComponent(0.06) : .clear; row.layer.cornerRadius = 6
        row.accessibilityLabel = profile.name; row.accessibilityIdentifier = "source-profile-\(profile.id)"
        row.isEnabled = !model.loading; row.size(nil, 44)
        return row
    }

    private func renderTree() {
        tree.arrangedSubviews.forEach { $0.removeFromSuperview() }
        add(model.schema, depth: 0, filter: filter.text ?? "")
    }

    private func add(_ items: [SchemaItem], depth: Int, filter: String) {
        let redis = model.active?.engine == .redis
        let color = UIColor(hex: model.active?.color ?? "#4c9aff")
        for node in items where filter.isEmpty || node.name.localizedCaseInsensitiveContains(filter)
            || node.children?.contains(where: { $0.name.localizedCaseInsensitiveContains(filter) }) == true {
            let indent = CGFloat(depth) * 14
            let item: UIButton
            if let schema = node.schema, let table = node.table {
                item = button(node.name, image: redis ? "key" : "tablecells", size: 12.5) { [weak self] in
                    self?.model.browse(schema: schema, table: table); self?.close?()
                }
                var config = item.configuration; config?.contentInsets = .init(top: 0, leading: 22 + indent, bottom: 0, trailing: 8); item.configuration = config
                let selected = model.showingTable && model.selectedTable?.schema == schema && model.selectedTable?.table == table
                item.backgroundColor = selected ? color.withAlphaComponent(0.18) : .clear
            } else {
                let open = expanded.contains(node.id)
                item = button(node.name, image: "folder", size: 12.5) { [weak self] in
                    guard let self else { return }
                    if expanded.contains(node.id) { expanded.remove(node.id) } else { expanded.insert(node.id) }
                    renderTree()
                }
                var config = item.configuration; config?.contentInsets = .init(top: 0, leading: 26 + indent, bottom: 0, trailing: 10); item.configuration = config
                let chevron = symbol(open ? "chevron.down" : "chevron.right", size: 9, color: Graphite.faint)
                item.addSubview(chevron); chevron.translatesAutoresizingMaskIntoConstraints = false
                chevron.leadingAnchor.constraint(equalTo: item.leadingAnchor, constant: 10 + indent).isActive = true
                chevron.centerYAnchor.constraint(equalTo: item.centerYAnchor).isActive = true
                item.accessibilityValue = open ? "Expanded" : "Collapsed"
            }
            item.contentHorizontalAlignment = .leading
            item.accessibilityLabel = node.name; item.accessibilityIdentifier = "schema-item-\(node.name)"
            item.isEnabled = !model.loading; item.size(nil, 36)
            tree.addArrangedSubview(item)
            if node.table == nil, expanded.contains(node.id) || !filter.isEmpty { add(node.children ?? [], depth: depth + 1, filter: filter) }
        }
    }

    private func edit(_ draft: ProfileDraft) {
        guard let presenter = window?.rootViewController?.topPresenter else { return }
        presenter.present(ProfileFormViewController(model: model, draft: draft), animated: true)
    }
}

/// The result grid: a fixed header row over vertically scrolling rows, both
/// inside one horizontal scroll view. Only visible rows have views.
final class GridView: UIView {
    static let columnWidth: CGFloat = 166, rowHeight: CGFloat = 32
    private let horizontal = UIScrollView()
    private let content = UIView()
    private let header = UIStackView()
    private let rows = RowsView()
    private let empty = vstack([symbol("tablecells"), label("No results"), label("Run a query or choose a table.", size: 11, color: Graphite.faint)], spacing: 10, alignment: .center)
    private var width: NSLayoutConstraint!
    private var columns: [String] = []

    override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = Graphite.bg
        horizontal.accessibilityIdentifier = "results-grid"; horizontal.alwaysBounceVertical = false
        addSubview(horizontal); horizontal.pin(to: self)
        horizontal.addSubview(content); content.translatesAutoresizingMaskIntoConstraints = false
        width = content.widthAnchor.constraint(equalToConstant: 0)
        NSLayoutConstraint.activate([
            content.leadingAnchor.constraint(equalTo: horizontal.contentLayoutGuide.leadingAnchor),
            content.trailingAnchor.constraint(equalTo: horizontal.contentLayoutGuide.trailingAnchor),
            content.topAnchor.constraint(equalTo: horizontal.contentLayoutGuide.topAnchor),
            content.bottomAnchor.constraint(equalTo: horizontal.contentLayoutGuide.bottomAnchor),
            content.heightAnchor.constraint(equalTo: horizontal.frameLayoutGuide.heightAnchor), width,
        ])
        header.axis = .horizontal; header.backgroundColor = Graphite.gridHeader; header.bottomBorder()
        rows.accessibilityIdentifier = "results-rows"; rows.alwaysBounceVertical = true
        for view in [header, rows] { content.addSubview(view); view.translatesAutoresizingMaskIntoConstraints = false }
        NSLayoutConstraint.activate([
            header.topAnchor.constraint(equalTo: content.topAnchor), header.heightAnchor.constraint(equalToConstant: 34),
            header.leadingAnchor.constraint(equalTo: content.leadingAnchor), header.trailingAnchor.constraint(equalTo: content.trailingAnchor),
            rows.topAnchor.constraint(equalTo: header.bottomAnchor), rows.bottomAnchor.constraint(equalTo: content.bottomAnchor),
            rows.leadingAnchor.constraint(equalTo: content.leadingAnchor), rows.trailingAnchor.constraint(equalTo: content.trailingAnchor),
        ])
        addSubview(empty); empty.translatesAutoresizingMaskIntoConstraints = false
        empty.centerXAnchor.constraint(equalTo: centerXAnchor).isActive = true
        empty.centerYAnchor.constraint(equalTo: centerYAnchor).isActive = true
    }
    required init?(coder: NSCoder) { fatalError() }

    func show(_ data: GridData) {
        empty.isHidden = !data.columns.isEmpty; horizontal.isHidden = data.columns.isEmpty
        if columns != data.columns {
            columns = data.columns
            header.arrangedSubviews.forEach { $0.removeFromSuperview() }
            for (index, column) in columns.enumerated() {
                let title = label(column, size: 11, bold: true)
                title.accessibilityIdentifier = "result-column-\(index)"
                header.addArrangedSubview(title.padded(.init(top: 0, left: 8, bottom: 0, right: 8)).size(Self.columnWidth, nil))
            }
            width.constant = CGFloat(columns.count) * Self.columnWidth
            horizontal.contentOffset.x = 0
        }
        rows.show(data)
    }
}

final class RowsView: UIScrollView {
    private var data = GridData()
    private var visible: [Int: RowView] = [:]
    private var pool: [RowView] = []

    func show(_ data: GridData) {
        let reset = data.offset != self.data.offset || data.rows.count != self.data.rows.count || data.columns != self.data.columns
        self.data = data
        visible.values.forEach { $0.removeFromSuperview(); pool.append($0) }
        visible = [:]
        contentSize = CGSize(width: 1, height: CGFloat(data.rows.count) * GridView.rowHeight)
        if reset { contentOffset.y = -adjustedContentInset.top }
        setNeedsLayout()
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        guard !data.rows.isEmpty else { return }
        let first = max(0, Int(contentOffset.y / GridView.rowHeight) - 2)
        let last = min(data.rows.count - 1, Int((contentOffset.y + bounds.height) / GridView.rowHeight) + 2)
        guard first <= last else { return }
        for (index, view) in visible where !(first...last).contains(index) {
            view.removeFromSuperview(); pool.append(view); visible[index] = nil
        }
        for index in first...last where visible[index] == nil {
            let view = pool.popLast() ?? RowView()
            view.configure(index: index, row: data.rows[index], columns: data.columns)
            view.frame = CGRect(x: 0, y: CGFloat(index) * GridView.rowHeight, width: bounds.width, height: GridView.rowHeight)
            addSubview(view); visible[index] = view
        }
        for (index, view) in visible { view.frame.size.width = bounds.width; view.frame.origin.y = CGFloat(index) * GridView.rowHeight }
    }
}

/// One result row: one label per column, read by VoiceOver as a single element.
final class RowView: UIView {
    private var labels: [UILabel] = []
    override init(frame: CGRect) {
        super.init(frame: frame)
        isAccessibilityElement = true
        let line = UIView(); line.backgroundColor = Graphite.hairline; line.autoresizingMask = [.flexibleWidth, .flexibleTopMargin]
        line.frame = CGRect(x: 0, y: GridView.rowHeight - 0.5, width: 0, height: 0.5); addSubview(line)
    }
    required init?(coder: NSCoder) { fatalError() }

    func configure(index: Int, row: [Any], columns: [String]) {
        while labels.count < columns.count {
            let cell = label(size: 12); cell.frame = CGRect(x: CGFloat(labels.count) * GridView.columnWidth + 8, y: 0, width: GridView.columnWidth - 16, height: GridView.rowHeight)
            addSubview(cell); labels.append(cell)
        }
        for (column, cell) in labels.enumerated() {
            cell.isHidden = column >= columns.count
            guard column < columns.count else { continue }
            let value: Any? = column < row.count ? row[column] : nil
            cell.text = value.map(cellText) ?? ""
            cell.textColor = value is NSNull ? Graphite.faint : Graphite.text
        }
        accessibilityIdentifier = "result-row-\(index + 1)"
        accessibilityLabel = (["Row \(index + 1)"] + columns.enumerated().map { "\($0.element): \($0.offset < row.count ? cellText(row[$0.offset]) : "")" }).joined(separator: ", ")
    }
}

private func cellText(_ value: Any) -> String { display(value) }
