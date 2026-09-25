import AppKit

/// Column header: key glyph, name, type in faint mono, sort indicator.
final class GridHeaderCell: NSTableHeaderCell {
    enum HoverPart { case none, sort, collapse }

    var primaryKey = false
    var dataType = ""
    var sort: Bool?
    var rightAligned = false
    var collapsed = false
    /// Table grids sort and collapse from the header; result grids do not.
    var interactive = false
    var hoverPart = HoverPart.none
    /// `.column-action-target` / `.column-action-dimmed` while a collapse or
    /// expand control is hovered.
    var actionTarget = false
    var dimmed = false

    static let collapseWidth: CGFloat = 24

    override func draw(withFrame cellFrame: NSRect, in controlView: NSView) {
        (actionTarget ? NSColor(hex: 0x232a36) : Graphite.gridHeader).setFill()
        cellFrame.fill()
        Graphite.border.setFill()
        NSRect(x: cellFrame.minX, y: cellFrame.maxY - 1, width: cellFrame.width, height: 1).fill()
        // The header view also draws its filler past the last column with a
        // copy of this cell; that area stays blank.
        guard !stringValue.isEmpty else { return }
        // `.data-grid th { border-right: 1px solid #242427 }`
        NSColor(hex: 0x242427).setFill()
        NSRect(x: cellFrame.maxX - 1, y: cellFrame.minY, width: 1, height: cellFrame.height).fill()
        let fade: CGFloat = dimmed ? 0.38 : 1
        if collapsed {
            if hoverPart != .none {
                Graphite.controlHover.setFill()
                cellFrame.insetBy(dx: 0, dy: 0).fill()
            }
            Icon.expand.draw(in: NSRect(x: cellFrame.midX - 6, y: cellFrame.minY + 5, width: 12, height: 12),
                             color: (hoverPart != .none ? Graphite.text : Graphite.faint).withAlphaComponent(fade))
            let paragraph = NSMutableParagraphStyle()
            paragraph.alignment = .center
            paragraph.lineBreakMode = .byTruncatingTail
            NSAttributedString(string: stringValue, attributes: [
                .font: Graphite.ui(9.5, .medium), .foregroundColor: Graphite.faint.withAlphaComponent(fade), .paragraphStyle: paragraph,
            ]).draw(with: NSRect(x: cellFrame.minX + 4, y: cellFrame.minY + 19, width: cellFrame.width - 8, height: 12),
                    options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])
            return
        }
        let midY = cellFrame.midY
        // Result tables right-align numeric headers (`th.numeric-cell`).
        if !interactive, rightAligned, dataType.isEmpty, !primaryKey, sort == nil {
            let title = NSAttributedString(string: stringValue, attributes: [.font: Graphite.ui(12, .medium), .foregroundColor: Graphite.text])
            let width = min(title.size().width, cellFrame.width - 20)
            title.draw(with: NSRect(x: cellFrame.maxX - 10 - width, y: midY - title.size().height / 2, width: width, height: title.size().height),
                       options: [.truncatesLastVisibleLine, .usesLineFragmentOrigin])
            return
        }
        let collapseWidth = interactive ? Self.collapseWidth : 0
        let sortArea = NSRect(x: cellFrame.minX, y: cellFrame.minY, width: cellFrame.width - collapseWidth, height: cellFrame.height - 1)
        if interactive, hoverPart == .sort {
            NSColor(white: 1, alpha: 0.03).setFill()
            sortArea.fill()
        }
        if interactive, hoverPart != .none {
            let button = NSRect(x: sortArea.maxX, y: cellFrame.minY, width: collapseWidth, height: cellFrame.height - 1)
            if hoverPart == .collapse {
                Graphite.controlHover.setFill()
                button.fill()
            }
            Icon.collapse.draw(in: NSRect(x: button.midX - 6, y: midY - 6, width: 12, height: 12),
                               color: hoverPart == .collapse ? Graphite.text : Graphite.faint)
        }
        // The sort indicator sits at the sort area's right edge.
        var indicator: Icon?
        var indicatorColor = Graphite.accentText
        if let sort {
            indicator = sort ? .arrowDown : .arrowUp
        } else if interactive, hoverPart == .sort {
            indicator = .sort
            indicatorColor = NSColor(hex: 0x5c5c62)
        }
        let indicatorWidth: CGFloat = indicator == nil ? 0 : 17
        if let indicator {
            indicator.draw(in: NSRect(x: sortArea.maxX - 4 - 11, y: midY - 5.5, width: 11, height: 11),
                           color: indicatorColor.withAlphaComponent(fade))
        }
        var x = cellFrame.minX + 10
        if primaryKey {
            Icon.key.draw(in: NSRect(x: x, y: midY - 5.5, width: 11, height: 11), color: Graphite.modified.withAlphaComponent(fade))
            x += 16
        }
        let name = NSAttributedString(string: stringValue, attributes: [
            .font: Graphite.ui(12, .medium),
            .foregroundColor: (sort == nil && !actionTarget ? Graphite.text : Graphite.textStrong).withAlphaComponent(fade),
        ])
        let type = NSAttributedString(string: dataType, attributes: [.font: Graphite.mono(11), .foregroundColor: NSColor(hex: 0x6f6f76).withAlphaComponent(fade)])
        let limit = sortArea.maxX - 4 - indicatorWidth
        let nameWidth = max(0, min(name.size().width, limit - x))
        let options: NSString.DrawingOptions = [.truncatesLastVisibleLine, .usesLineFragmentOrigin]
        name.draw(with: NSRect(x: x, y: midY - name.size().height / 2, width: nameWidth, height: name.size().height), options: options)
        x += nameWidth + 5
        if !dataType.isEmpty, limit - x > 12 {
            let width = min(type.size().width, limit - x)
            type.draw(with: NSRect(x: x, y: midY - type.size().height / 2, width: width, height: type.size().height), options: options)
        }
    }

    override func drawSortIndicator(withFrame cellFrame: NSRect, in controlView: NSView, ascending: Bool, priority: Int) {}
}

final class GridHeaderView: NSTableHeaderView {
    /// Set for table grids: headers sort on click and collapse from a
    /// button that appears on hover.
    var onToggleColumn: ((Int) -> Void)?
    /// The column whose collapse or expand control is hovered, or nil.
    var onColumnAction: ((Int?) -> Void)?
    private var hovered = (column: -1, part: GridHeaderCell.HoverPart.none)
    private var tracking: NSTrackingArea?

    override var isFlipped: Bool { true }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking { removeTrackingArea(tracking) }
        let area = NSTrackingArea(rect: .zero, options: [.mouseMoved, .mouseEnteredAndExited, .activeInActiveApp, .inVisibleRect], owner: self, userInfo: nil)
        addTrackingArea(area)
        tracking = area
    }

    private func part(at point: NSPoint, column: Int) -> GridHeaderCell.HoverPart {
        guard column >= 0, let tableView else { return .none }
        let cell = tableView.tableColumns[column].headerCell as? GridHeaderCell
        guard onToggleColumn != nil else { return .sort }
        if cell?.collapsed == true || point.x > headerRect(ofColumn: column).maxX - GridHeaderCell.collapseWidth { return .collapse }
        return .sort
    }

    override func mouseMoved(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        let column = self.column(at: point)
        let next = (column: column, part: part(at: point, column: column))
        guard next.column != hovered.column || next.part != hovered.part else { return }
        let wasAction: Int? = hovered.part == .collapse ? hovered.column : nil
        hovered = next
        if column >= 0, let tableColumn = tableView?.tableColumns[column], onToggleColumn != nil {
            let name = tableColumn.title
            let collapsed = (tableColumn.headerCell as? GridHeaderCell)?.collapsed == true
            tableColumn.headerToolTip = next.part == .collapse ? "\(collapsed ? "Expand" : "Collapse") \(name)" : "Sort by \(name)"
        }
        let isAction: Int? = next.part == .collapse ? next.column : nil
        if wasAction != isAction { onColumnAction?(isAction) }
        needsDisplay = true
    }

    override func mouseExited(with event: NSEvent) {
        if hovered.part == .collapse { onColumnAction?(nil) }
        hovered = (column: -1, part: GridHeaderCell.HoverPart.none)
        needsDisplay = true
    }

    override func mouseDown(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        let column = self.column(at: point)
        if let onToggleColumn, column >= 0, part(at: point, column: column) == .collapse {
            onToggleColumn(column)
            return
        }
        super.mouseDown(with: event)
    }

    override func draw(_ dirtyRect: NSRect) {
        let action = hovered.part == .collapse ? hovered.column : -1
        for (index, column) in (tableView?.tableColumns ?? []).enumerated() {
            guard let cell = column.headerCell as? GridHeaderCell else { continue }
            cell.interactive = onToggleColumn != nil
            cell.hoverPart = index == hovered.column ? hovered.part : .none
            cell.actionTarget = index == action
            cell.dimmed = action >= 0 && index != action
        }
        Graphite.gridHeader.setFill()
        bounds.fill()
        super.draw(dirtyRect)
        Graphite.border.setFill()
        NSRect(x: 0, y: bounds.height - 1, width: bounds.width, height: 1).fill()
    }
}

enum RowMark { case none, modified, deleted }

final class GridRowView: NSTableRowView {
    var mark = RowMark.none { didSet { needsDisplay = true } }
    /// `tbody tr:hover`: a faint wash, stronger on deleted rows.
    var hovered = false { didSet { if hovered != oldValue { needsDisplay = true } } }

    override func drawBackground(in dirtyRect: NSRect) {
        Graphite.bg.setFill()
        bounds.fill()
        if mark == .deleted {
            (hovered ? NSColor(hex: 0xff6b61, alpha: 0.13) : Graphite.dangerSoft).setFill()
            bounds.fill()
        } else if hovered {
            NSColor(white: 1, alpha: 0.025).setFill()
            bounds.fill()
        }
        Graphite.hairline.setFill()
        NSRect(x: 0, y: bounds.height - 1, width: bounds.width, height: 1).fill()
        let edge: NSColor? = mark == .deleted ? Graphite.danger : mark == .modified ? Graphite.modified : nil
        if let edge, !isSelected {
            edge.setFill()
            NSRect(x: 0, y: 0, width: 2, height: bounds.height).fill()
        }
    }

    override func drawSelection(in dirtyRect: NSRect) {
        // A staged delete keeps its red tint when selected, as in the desktop.
        if mark == .deleted {
            (hovered ? NSColor(hex: 0xff6b61, alpha: 0.13) : Graphite.dangerSoft).setFill()
        } else {
            NSColor(hex: 0x4c9aff, alpha: hovered ? 0.16 : 0.13).setFill()
        }
        bounds.fill()
        let edge = mark == .deleted ? Graphite.danger : mark == .modified ? Graphite.modified : Graphite.accent
        edge.setFill()
        NSRect(x: 0, y: 0, width: 2, height: bounds.height).fill()
    }

    override var isEmphasized: Bool { get { false } set {} }
}

/// One grid cell, drawn directly for speed: value text, NULL styling,
/// numeric alignment, staged-edit fill and dot, staged-delete strikethrough.
final class GridCellView: NSView {
    var value: Any = NSNull() { didSet { needsDisplay = true } }
    var rightAligned = false
    var modified = false
    var deleted = false
    /// A collapsed column shows a faint "…" instead of the value.
    var collapsed = false { didSet { needsDisplay = true } }
    /// While a header's collapse/expand control is hovered, other columns
    /// dim and the target column is washed, as `.column-action-*` does.
    var columnAction = ColumnAction.none { didSet { if columnAction != oldValue { needsDisplay = true } } }

    enum ColumnAction { case none, target, dimmed }

    override var isFlipped: Bool { true }

    override func draw(_ dirtyRect: NSRect) {
        if columnAction == .target {
            NSColor(hex: 0x4c9aff, alpha: 0.06).setFill()
            bounds.fill()
        }
        if modified && !deleted {
            Graphite.modifiedSoft.setFill()
            bounds.fill()
            Graphite.modified.setFill()
            NSBezierPath(ovalIn: NSRect(x: bounds.maxX - 8.5, y: 3.5, width: 5, height: 5)).fill()
        }
        let fade: CGFloat = columnAction == .dimmed ? 0.38 : 1
        if collapsed {
            var attributes: [NSAttributedString.Key: Any] = [
                .font: Graphite.mono(12), .foregroundColor: (deleted ? Graphite.danger : Graphite.faint).withAlphaComponent(fade),
            ]
            if deleted { attributes[.strikethroughStyle] = NSUnderlineStyle.single.rawValue }
            let dots = NSAttributedString(string: "…", attributes: attributes)
            let size = dots.size()
            dots.draw(at: NSPoint(x: (bounds.width - size.width) / 2, y: (bounds.height - size.height) / 2))
            return
        }
        let isNull = value is NSNull
        var text = displayValue(value)
        if text.count > 200 { text = String(text.prefix(200)) }
        text = text.replacingOccurrences(of: "\n", with: " ")
        var attributes: [NSAttributedString.Key: Any] = [
            .font: isNull ? NSFontManager.shared.convert(Graphite.mono(12), toHaveTrait: .italicFontMask) : Graphite.mono(12),
            .foregroundColor: (deleted ? Graphite.danger : isNull ? Graphite.faint : modified ? Graphite.textStrong : Graphite.text)
                .withAlphaComponent(fade),
        ]
        if deleted { attributes[.strikethroughStyle] = NSUnderlineStyle.single.rawValue }
        let paragraph = NSMutableParagraphStyle()
        paragraph.lineBreakMode = .byTruncatingTail
        paragraph.alignment = rightAligned ? .right : .left
        attributes[.paragraphStyle] = paragraph
        let string = NSAttributedString(string: text, attributes: attributes)
        let height = string.size().height
        string.draw(with: NSRect(x: 10, y: (bounds.height - height) / 2, width: bounds.width - 20, height: height),
                    options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])
    }

    override func accessibilityRole() -> NSAccessibility.Role? { .cell }
    override func accessibilityValue() -> Any? { displayValue(value) }
    override func isAccessibilityElement() -> Bool { true }
}

/// NSTableView with Graphite chrome and grid keyboard handling.
final class GridTableView: NSTableView {
    var onDelete: (() -> Void)?
    var onEscape: (() -> Void)?
    var onCopy: (() -> Void)?
    /// The row under the pointer, or nil when it leaves the grid.
    var onHoverRow: ((Int?) -> Void)?
    private var hoveredRow: Int?

    override init(frame: NSRect) {
        super.init(frame: frame)
        backgroundColor = Graphite.bg
        rowHeight = Graphite.rowHeight
        intercellSpacing = .zero
        gridStyleMask = []
        usesAlternatingRowBackgroundColors = false
        allowsColumnReordering = false
        allowsColumnResizing = true
        allowsMultipleSelection = true
        allowsEmptySelection = true
        columnAutoresizingStyle = .noColumnAutoresizing
        selectionHighlightStyle = .regular
        focusRingType = .none
        style = .plain
        let header = GridHeaderView(frame: NSRect(x: 0, y: 0, width: 100, height: Graphite.headerHeight))
        headerView = header
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func keyDown(with event: NSEvent) {
        if (event.keyCode == 51 || event.keyCode == 117), let onDelete {
            onDelete()
        } else if event.keyCode == 53, let onEscape {
            onEscape()
        } else {
            super.keyDown(with: event)
        }
    }

    @objc func copy(_ sender: Any?) { onCopy?() }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        trackingAreas.filter { $0.owner === self }.forEach(removeTrackingArea)
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseMoved, .mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect],
                                       owner: self, userInfo: nil))
    }

    override func mouseMoved(with event: NSEvent) {
        super.mouseMoved(with: event)
        let row = self.row(at: convert(event.locationInWindow, from: nil))
        let hovered = row >= 0 ? row : nil
        guard hovered != hoveredRow else { return }
        setHovered(hoveredRow, false)
        hoveredRow = hovered
        setHovered(hovered, true)
        onHoverRow?(hovered)
    }

    override func mouseExited(with event: NSEvent) {
        super.mouseExited(with: event)
        setHovered(hoveredRow, false)
        hoveredRow = nil
        onHoverRow?(nil)
    }

    private func setHovered(_ row: Int?, _ hovered: Bool) {
        guard let row, row < numberOfRows else { return }
        (rowView(atRow: row, makeIfNecessary: false) as? GridRowView)?.hovered = hovered
    }

    /// Clicking below the last row keeps the selection, as in the desktop.
    override func mouseDown(with event: NSEvent) {
        guard row(at: convert(event.locationInWindow, from: nil)) >= 0 else { return }
        super.mouseDown(with: event)
    }

    override func drawBackground(inClipRect clipRect: NSRect) {
        Graphite.bg.setFill()
        clipRect.fill()
    }

    func addGridColumn(id: String, title: String, dataType: String, primaryKey: Bool, width: CGFloat, rightAligned: Bool) {
        let column = NSTableColumn(identifier: .init(id))
        let cell = GridHeaderCell(textCell: title)
        cell.dataType = dataType
        cell.primaryKey = primaryKey
        cell.rightAligned = rightAligned
        column.headerCell = cell
        column.title = title
        column.width = width
        column.minWidth = 70
        column.maxWidth = 800
        column.resizingMask = .userResizingMask
        addTableColumn(column)
    }
}

func gridScroll(_ table: NSTableView) -> NSScrollView {
    let scroll = NSScrollView()
    scroll.translatesAutoresizingMaskIntoConstraints = false
    scroll.documentView = table
    scroll.hasVerticalScroller = true
    scroll.hasHorizontalScroller = true
    scroll.autohidesScrollers = true
    scroll.scrollerStyle = .overlay
    scroll.drawsBackground = true
    scroll.backgroundColor = Graphite.bg
    scroll.borderType = .noBorder
    return scroll
}

/// The desktop's `defaultColumnWidth`, rule for rule.
func defaultColumnWidth(_ column: Column) -> CGFloat {
    let type = column.dataType.lowercased()
    if type.contains("json") || type.contains("array") { return 320 }
    if type.contains("timestamp") { return 230 }
    if type.contains("text") || type.contains("character") || type.contains("uuid") { return 220 }
    if type.hasPrefix("bool") { return 110 }
    if column.numeric { return 130 }
    return 160
}

/// Read-only query results.
final class ResultGrid: NSObject, NSTableViewDataSource, NSTableViewDelegate {
    let table = GridTableView()
    lazy var scroll = gridScroll(table)
    private var rows: [[Any]] = []
    private var numeric: [Bool] = []

    override init() {
        super.init()
        table.dataSource = self
        table.delegate = self
        table.allowsMultipleSelection = true
        // Like the desktop's auto-width result table, columns share the width.
        table.columnAutoresizingStyle = .uniformColumnAutoresizingStyle
        table.onCopy = { [weak self] in self?.copySelection() }
    }

    private var columnNames: [String] = []

    /// ⌘C copies the selected result rows, or all of them, as CSV.
    private func copySelection() {
        let selected = table.selectedRowIndexes
        let picked = selected.isEmpty ? rows : selected.map { rows[$0] }
        let text = Helpers.csv(columns: columnNames, rows: picked)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }

    func show(columns: [[String: Any]], rows: [[Any]]) {
        self.rows = rows
        table.tableColumns.forEach(table.removeTableColumn)
        numeric = columns.map { Column(raw: $0).numeric }
        columnNames = columns.map { Column(raw: $0).name }
        for (index, column) in columns.enumerated() {
            let model = Column(raw: column)
            table.addGridColumn(id: String(index), title: model.name, dataType: "", primaryKey: false,
                                width: defaultColumnWidth(model), rightAligned: numeric[index])
        }
        table.reloadData()
        // After layout, spread spare width across the columns, like the
        // desktop's auto-width result table.
        DispatchQueue.main.async { [weak self] in self?.fillWidth() }
    }

    private func fillWidth() {
        guard let available = table.enclosingScrollView?.contentView.bounds.width, !table.tableColumns.isEmpty else { return }
        let total = table.tableColumns.reduce(0) { $0 + $1.width + table.intercellSpacing.width }
        guard total < available - 1 else { return }
        let extra = (available - total) / CGFloat(table.tableColumns.count)
        for column in table.tableColumns { column.width += extra }
    }

    func numberOfRows(in tableView: NSTableView) -> Int { rows.count }

    func tableView(_ tableView: NSTableView, rowViewForRow row: Int) -> NSTableRowView? { GridRowView() }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard let tableColumn, let index = Int(tableColumn.identifier.rawValue) else { return nil }
        let cell = (tableView.makeView(withIdentifier: .init("result-cell"), owner: nil) as? GridCellView) ?? {
            let view = GridCellView()
            view.identifier = .init("result-cell")
            return view
        }()
        cell.rightAligned = numeric.indices.contains(index) && numeric[index]
        cell.modified = false
        cell.deleted = false
        cell.value = rows[row].indices.contains(index) ? rows[row][index] : NSNull()
        return cell
    }
}
