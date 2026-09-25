import AppKit

/// Column header: key glyph, name, type in faint mono, sort indicator.
final class GridHeaderCell: NSTableHeaderCell {
    var primaryKey = false
    var dataType = ""
    var sort: Bool?
    var rightAligned = false
    var collapsed = false
    var showCollapse = false

    override func draw(withFrame cellFrame: NSRect, in controlView: NSView) {
        Graphite.gridHeader.setFill()
        cellFrame.fill()
        Graphite.border.setFill()
        NSRect(x: cellFrame.minX, y: cellFrame.maxY - 1, width: cellFrame.width, height: 1).fill()
        // The header view also draws its filler past the last column with a
        // copy of this cell; that area stays blank.
        guard !stringValue.isEmpty else { return }
        // `.data-grid th { border-right: 1px solid #242427 }`
        NSColor(hex: 0x242427).setFill()
        NSRect(x: cellFrame.maxX - 1, y: cellFrame.minY, width: 1, height: cellFrame.height).fill()
        if collapsed {
            Icon.expand.draw(in: NSRect(x: cellFrame.midX - 6, y: cellFrame.minY + 5, width: 12, height: 12), color: Graphite.muted)
            let paragraph = NSMutableParagraphStyle()
            paragraph.alignment = .center
            paragraph.lineBreakMode = .byTruncatingTail
            NSAttributedString(string: stringValue, attributes: [
                .font: Graphite.ui(9.5, .medium), .foregroundColor: Graphite.faint, .paragraphStyle: paragraph,
            ]).draw(with: NSRect(x: cellFrame.minX + 4, y: cellFrame.minY + 19, width: cellFrame.width - 8, height: 12),
                    options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])
            return
        }
        if showCollapse {
            let button = NSRect(x: cellFrame.maxX - 26, y: cellFrame.midY - 10, width: 20, height: 20)
            Graphite.controlHover.setFill()
            NSBezierPath(roundedRect: button, xRadius: 5, yRadius: 5).fill()
            Icon.collapse.draw(in: button.insetBy(dx: 4, dy: 4), color: Graphite.secondary)
        }
        var x = cellFrame.minX + 10
        let midY = cellFrame.midY
        if primaryKey {
            Icon.key.draw(in: NSRect(x: x, y: midY - 5.5, width: 11, height: 11), color: Graphite.modified)
            x += 16
        }
        let name = NSAttributedString(string: stringValue, attributes: [
            .font: Graphite.ui(12, .medium), .foregroundColor: sort == nil ? Graphite.text : Graphite.textStrong,
        ])
        let type = NSAttributedString(string: dataType, attributes: [.font: Graphite.mono(11), .foregroundColor: NSColor(hex: 0x6f6f76)])
        let sortWidth: CGFloat = sort == nil ? 0 : 16
        let available = cellFrame.maxX - x - 8 - sortWidth
        let nameWidth = min(name.size().width, available)
        let options: NSString.DrawingOptions = [.truncatesLastVisibleLine, .usesLineFragmentOrigin]
        name.draw(with: NSRect(x: x, y: midY - name.size().height / 2, width: nameWidth, height: name.size().height), options: options)
        x += nameWidth + 5
        if !dataType.isEmpty, cellFrame.maxX - x - sortWidth > 12 {
            let width = min(type.size().width, cellFrame.maxX - x - 8 - sortWidth)
            type.draw(with: NSRect(x: x, y: midY - type.size().height / 2, width: width, height: type.size().height), options: options)
            x += width + 5
        }
        if let sort {
            (sort ? Icon.sortDown : Icon.sortUp).draw(in: NSRect(x: min(x, cellFrame.maxX - 16), y: midY - 5, width: 10, height: 10), color: Graphite.accent)
        }
    }

    override func drawSortIndicator(withFrame cellFrame: NSRect, in controlView: NSView, ascending: Bool, priority: Int) {}
}

final class GridHeaderView: NSTableHeaderView {
    /// Set for table grids: hovering a header shows a collapse button.
    var onToggleColumn: ((Int) -> Void)?
    private var hoveredColumn = -1
    private var tracking: NSTrackingArea?

    override var isFlipped: Bool { true }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking { removeTrackingArea(tracking) }
        let area = NSTrackingArea(rect: .zero, options: [.mouseMoved, .mouseEnteredAndExited, .activeInActiveApp, .inVisibleRect], owner: self, userInfo: nil)
        addTrackingArea(area)
        tracking = area
    }

    override func mouseMoved(with event: NSEvent) {
        let column = self.column(at: convert(event.locationInWindow, from: nil))
        if column != hoveredColumn { hoveredColumn = column; needsDisplay = true }
    }

    override func mouseExited(with event: NSEvent) {
        hoveredColumn = -1
        needsDisplay = true
    }

    override func mouseDown(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        let column = self.column(at: point)
        if let onToggleColumn, column >= 0, let tableView {
            let cell = tableView.tableColumns[column].headerCell as? GridHeaderCell
            if cell?.collapsed == true || point.x > headerRect(ofColumn: column).maxX - 26 {
                onToggleColumn(column)
                return
            }
        }
        super.mouseDown(with: event)
    }

    override func draw(_ dirtyRect: NSRect) {
        for (index, column) in (tableView?.tableColumns ?? []).enumerated() {
            (column.headerCell as? GridHeaderCell)?.showCollapse = onToggleColumn != nil && index == hoveredColumn
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

    override func drawBackground(in dirtyRect: NSRect) {
        Graphite.bg.setFill()
        bounds.fill()
        if mark == .deleted {
            Graphite.dangerSoft.setFill()
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
        Graphite.accentSoft.setFill()
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

    override var isFlipped: Bool { true }

    override func draw(_ dirtyRect: NSRect) {
        if modified && !deleted {
            Graphite.modifiedSoft.setFill()
            bounds.fill()
            Graphite.modified.setFill()
            NSBezierPath(ovalIn: NSRect(x: bounds.maxX - 8.5, y: 3.5, width: 5, height: 5)).fill()
        }
        let isNull = value is NSNull
        var text = displayValue(value)
        if text.count > 200 { text = String(text.prefix(200)) }
        text = text.replacingOccurrences(of: "\n", with: " ")
        var attributes: [NSAttributedString.Key: Any] = [
            .font: isNull ? NSFontManager.shared.convert(Graphite.mono(12), toHaveTrait: .italicFontMask) : Graphite.mono(12),
            .foregroundColor: deleted ? Graphite.danger : isNull ? Graphite.faint : Graphite.text,
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
        column.minWidth = 60
        column.maxWidth = 1200
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

func defaultColumnWidth(_ column: Column) -> CGFloat {
    let type = column.dataType.lowercased()
    if type.contains("bool") { return 110 }
    if column.numeric { return 130 }
    if type.contains("uuid") { return 200 }
    if type.contains("time") || type.contains("date") { return 200 }
    return 200
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
    }

    func show(columns: [[String: Any]], rows: [[Any]]) {
        self.rows = rows
        table.tableColumns.forEach(table.removeTableColumn)
        numeric = columns.map { Column(raw: $0).numeric }
        for (index, column) in columns.enumerated() {
            let model = Column(raw: column)
            table.addGridColumn(id: String(index), title: model.name, dataType: "", primaryKey: false,
                                width: defaultColumnWidth(model), rightAligned: numeric[index])
        }
        table.reloadData()
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
