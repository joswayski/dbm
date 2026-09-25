import AppKit

protocol QueryHost: TableHost {
    func runQuery(_ tab: WorkTab, sql: String, refresh: Bool)
    func engine(of tab: WorkTab) -> Engine
    func history(for tab: WorkTab) -> [[String: Any]]
    func isRunning(_ tab: WorkTab) -> String?
}

private let sqlKeywords = [
    "ADD", "ALL", "ALTER", "AND", "ANY", "AS", "ASC", "BEGIN", "BETWEEN", "BY", "CASE", "CAST", "CHECK", "COLUMN",
    "COMMIT", "CONSTRAINT", "CREATE", "CROSS", "DATABASE", "DEFAULT", "DELETE", "DESC", "DISTINCT", "DROP", "ELSE",
    "END", "EXISTS", "EXPLAIN", "FALSE", "FETCH", "FOREIGN", "FROM", "FULL", "FUNCTION", "GRANT", "GROUP", "HAVING",
    "ILIKE", "IN", "INDEX", "INNER", "INSERT", "INTERVAL", "INTO", "IS", "JOIN", "KEY", "LEFT", "LIKE", "LIMIT", "NOT",
    "NULL", "OFFSET", "ON", "OR", "ORDER", "OUTER", "OVER", "PARTITION", "PRIMARY", "REFERENCES", "RETURNING",
    "REVOKE", "RIGHT", "ROLLBACK", "SCHEMA", "SELECT", "SET", "SHOW", "TABLE", "THEN", "TRUE", "TRUNCATE", "UNION",
    "UNIQUE", "UPDATE", "USING", "VALUES", "VIEW", "WHEN", "WHERE", "WINDOW", "WITH",
]

private let redisCommands = [
    "APPEND", "DBSIZE", "DECR", "DECRBY", "DEL", "EXISTS", "EXPIRE", "GET", "GETDEL", "GETRANGE", "HDEL", "HEXISTS",
    "HGET", "HGETALL", "HINCRBY", "HKEYS", "HLEN", "HMGET", "HSET", "HVALS", "INCR", "INCRBY", "INFO", "KEYS", "LINDEX",
    "LLEN", "LPOP", "LPUSH", "LRANGE", "LREM", "LSET", "MGET", "MSET", "PERSIST", "PEXPIRE", "PING", "PTTL", "RENAME",
    "RPOP", "RPUSH", "SADD", "SCAN", "SCARD", "SET", "SETEX", "SISMEMBER", "SMEMBERS", "SREM", "STRLEN", "TTL", "TYPE",
    "UNLINK", "XADD", "XLEN", "XRANGE", "ZADD", "ZCARD", "ZRANGE", "ZRANGEBYSCORE", "ZREM", "ZSCORE",
]

/// SQL/Redis editor: shared-core highlighting, the statement Command+Enter
/// would run outlined, and keyword completion as you type.
final class SQLTextView: NSTextView {
    var engine = Engine.postgres
    var onRun: (() -> Void)?
    var onChange: (() -> Void)?
    private(set) var activeRange: NSRange?
    private var completing = false

    override func keyDown(with event: NSEvent) {
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        if flags.contains(.command), event.keyCode == 36 || event.keyCode == 76 {
            onRun?()
            return
        }
        if flags == .control, event.charactersIgnoringModifiers == " " {
            complete(nil)
            return
        }
        super.keyDown(with: event)
        if let characters = event.characters, characters.count == 1, characters.first!.isLetter, flags.isDisjoint(with: [.command, .control]) {
            let range = rangeForUserCompletion
            if range.length >= 2, !completing {
                completing = true
                complete(nil)
                completing = false
            }
        }
    }

    override func completions(forPartialWordRange charRange: NSRange, indexOfSelectedItem index: UnsafeMutablePointer<Int>) -> [String]? {
        let prefix = (string as NSString).substring(with: charRange)
        let upper = prefix.uppercased()
        let words = engine == .redis ? redisCommands : sqlKeywords
        let matches = words.filter { $0.hasPrefix(upper) && $0 != upper }
        index.pointee = -1
        let lower = prefix == prefix.lowercased()
        return matches.map { lower ? $0.lowercased() : $0 }
    }

    func rehighlight() {
        guard let storage = textStorage else { return }
        let full = NSRange(location: 0, length: storage.length)
        storage.beginEditing()
        storage.setAttributes([.font: Graphite.mono(13), .foregroundColor: Graphite.text], range: full)
        for (range, kind) in Helpers.highlight(engine: engine, text: string) where NSMaxRange(range) <= storage.length {
            let color: NSColor
            switch kind {
            case "keyword": color = Graphite.keyword
            case "string": color = Graphite.string
            case "number": color = Graphite.number
            case "comment": color = Graphite.faint
            default: color = Graphite.accentText
            }
            storage.addAttribute(.foregroundColor, value: color, range: range)
        }
        storage.endEditing()
        typingAttributes = [.font: Graphite.mono(13), .foregroundColor: Graphite.text]
    }

    /// Recomputes the outlined statement; returns the current run target.
    @discardableResult
    func updateActiveStatement() -> Helpers.Target? {
        let target = Helpers.executionTarget(engine: engine, text: string, selection: selectedRange())
        let active = target.flatMap { $0.selection ? nil : $0.range }
        if active != activeRange {
            activeRange = active
            needsDisplay = true
        }
        return target
    }

    override func drawBackground(in rect: NSRect) {
        super.drawBackground(in: rect)
        guard let activeRange, let layoutManager, let textContainer else { return }
        let glyphs = layoutManager.glyphRange(forCharacterRange: activeRange, actualCharacterRange: nil)
        var union = NSRect.null
        layoutManager.enumerateLineFragments(forGlyphRange: glyphs) { lineRect, _, _, _, _ in
            union = union.union(lineRect)
        }
        guard !union.isNull else { return }
        _ = textContainer
        let origin = textContainerOrigin
        let band = NSRect(x: 0, y: union.minY + origin.y, width: bounds.width, height: union.height)
        Graphite.accent.withAlphaComponent(0.06).setFill()
        band.fill()
        Graphite.accent.withAlphaComponent(0.7).setFill()
        NSRect(x: 0, y: band.minY, width: 2, height: band.height).fill()
    }
}

/// Line numbers beside the editor.
final class LineNumberRuler: NSRulerView {
    private weak var editor: NSTextView?

    init(editor: NSTextView) {
        self.editor = editor
        super.init(scrollView: editor.enclosingScrollView, orientation: .verticalRuler)
        clientView = editor
        ruleThickness = 38
    }

    required init(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func drawHashMarksAndLabels(in rect: NSRect) {
        Graphite.bg.setFill()
        bounds.fill()
        Graphite.hairline.setFill()
        NSRect(x: bounds.maxX - 1, y: 0, width: 1, height: bounds.height).fill()
        guard let editor, let layoutManager = editor.layoutManager, let container = editor.textContainer else { return }
        let text = editor.string as NSString
        let visible = editor.visibleRect
        let glyphs = layoutManager.glyphRange(forBoundingRect: visible, in: container)
        let characters = layoutManager.characterRange(forGlyphRange: glyphs, actualGlyphRange: nil)
        var line = text.substring(to: characters.location).components(separatedBy: "\n").count
        let attributes: [NSAttributedString.Key: Any] = [.font: Graphite.mono(11.5), .foregroundColor: Graphite.faint]
        let yOffset = convert(NSPoint.zero, from: editor).y
        var index = characters.location
        func draw(_ number: Int, _ lineRect: NSRect) {
            let label = String(number) as NSString
            let size = label.size(withAttributes: attributes)
            let y = yOffset + lineRect.minY + editor.textContainerOrigin.y + (lineRect.height - size.height) / 2
            label.draw(at: NSPoint(x: ruleThickness - size.width - 8, y: y), withAttributes: attributes)
        }
        while index < NSMaxRange(characters) {
            let lineRange = text.lineRange(for: NSRange(location: index, length: 0))
            let glyph = layoutManager.glyphIndexForCharacter(at: lineRange.location)
            draw(line, layoutManager.lineFragmentRect(forGlyphAt: glyph, effectiveRange: nil))
            line += 1
            index = NSMaxRange(lineRange)
        }
        if text.length == 0 || text.hasSuffix("\n") {
            draw(line, layoutManager.extraLineFragmentRect.isEmpty
                 ? NSRect(x: 0, y: 0, width: 0, height: 17) : layoutManager.extraLineFragmentRect)
        }
    }
}

final class HistoryRow: NSView {
    private let entry: [String: Any]
    private let onPick: (String) -> Void
    private var hovering = false

    init(entry: [String: Any], onPick: @escaping (String) -> Void) {
        self.entry = entry
        self.onPick = onPick
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        heightAnchor.constraint(equalToConstant: 50).isActive = true
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeInActiveApp, .inVisibleRect], owner: self, userInfo: nil))
        toolTip = "\(int(entry["durationMs"])) ms\n\n\(string(entry["sql"]))"
        setAccessibilityElement(true)
        setAccessibilityRole(.button)
        setAccessibilityLabel(string(entry["sql"]))
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override var isFlipped: Bool { true }
    override func mouseEntered(with event: NSEvent) { hovering = true; needsDisplay = true }
    override func mouseExited(with event: NSEvent) { hovering = false; needsDisplay = true }
    override func mouseUp(with event: NSEvent) { onPick(string(entry["sql"])) }
    override func accessibilityPerformPress() -> Bool { onPick(string(entry["sql"])); return true }

    static let timeFormat: DateFormatter = {
        let formatter = DateFormatter()
        formatter.timeStyle = .medium
        formatter.dateStyle = .none
        return formatter
    }()

    override func draw(_ dirtyRect: NSRect) {
        if hovering { Graphite.hoverWash.setFill(); bounds.fill() }
        let success = bool(entry["success"])
        (success ? Icon.check : Icon.alert).draw(in: NSRect(x: 14, y: 11, width: 12, height: 12),
                                                 color: success ? Graphite.success : Graphite.danger)
        let sql = string(entry["sql"]).split(whereSeparator: \.isWhitespace).joined(separator: " ")
        let paragraph = NSMutableParagraphStyle()
        paragraph.lineBreakMode = .byTruncatingTail
        (sql as NSString).draw(with: NSRect(x: 34, y: 9, width: bounds.width - 44, height: 16),
                               options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine],
                               attributes: [.font: Graphite.mono(12), .foregroundColor: Graphite.secondary, .paragraphStyle: paragraph])
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        let raw = string(entry["executedAt"])
        let date = formatter.date(from: raw) ?? ISO8601DateFormatter().date(from: raw)
        let time = date.map { HistoryRow.timeFormat.string(from: $0) } ?? ""
        (time as NSString).draw(at: NSPoint(x: 34, y: 29), withAttributes: [.font: Graphite.ui(11), .foregroundColor: Graphite.faint])
    }
}

final class QueryPane: NSView, NSTextViewDelegate, NSSplitViewDelegate {
    private weak var host: QueryHost?
    let tab: WorkTab
    private let eyebrowLabel = eyebrow("SQL WORKBENCH")
    private let titleLabel = label("", font: Graphite.ui(15, .semibold), color: Graphite.textStrong)
    private lazy var refreshButton = GButton("Refresh", icon: .refresh, style: .secondary) { [weak self] in self?.refreshResult() }
    private lazy var runButton = GButton("Run statement", icon: .play, style: .primary, shortcut: "⌘↵") { [weak self] in self?.runFromEditor() }
    let editor = SQLTextView()
    private let editorScroll = NSScrollView()
    private let hint = label("", font: Graphite.ui(11), color: Graphite.faint)
    private let historyCount = label("0", font: Graphite.ui(11), color: Graphite.faint)
    private let historyList = vstack([], spacing: 0)
    private let historyEmpty = label("Run a query to start history.", font: Graphite.ui(13), color: Graphite.muted)
    private var historySignature = ""
    private let results = FlippedView()
    private let resultMeta = label("", font: Graphite.ui(12.5), color: Graphite.muted)
    private let editableChip = Chip("Editable table", color: Graphite.accentText)
    private let readOnlyChip = Chip("Read-only result", color: Graphite.muted)
    private let truncatedChip = Chip("truncated", color: Graphite.modified)
    private let resultGrid = ResultGrid()
    private let resultCard = PanelView(fill: Graphite.bg)
    private let placeholder = label("Results will appear here.", font: Graphite.ui(13), color: Graphite.muted)
    private(set) var embeddedPane: TablePane?
    private var shownResult: UUID?
    private let split = NSSplitView()

    init(tab: WorkTab, host: QueryHost) {
        self.tab = tab
        self.host = host
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        build()
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override var isFlipped: Bool { true }

    private func build() {
        let engine = host?.engine(of: tab) ?? .postgres
        let toolbar = FlippedView()
        toolbar.translatesAutoresizingMaskIntoConstraints = false
        let title = vstack([eyebrowLabel, titleLabel], spacing: 2)
        toolbar.pin(hstack([title, spacer(), refreshButton, runButton], spacing: 8),
                    insets: NSEdgeInsets(top: 10, left: 14, bottom: 6, right: 14))
        refreshButton.minHeight = 30
        runButton.minHeight = 30

        editor.engine = engine
        editor.delegate = self
        editor.isRichText = false
        editor.allowsUndo = true
        editor.isAutomaticQuoteSubstitutionEnabled = false
        editor.isAutomaticDashSubstitutionEnabled = false
        editor.isAutomaticTextReplacementEnabled = false
        editor.isAutomaticSpellingCorrectionEnabled = false
        editor.isContinuousSpellCheckingEnabled = false
        editor.smartInsertDeleteEnabled = false
        editor.drawsBackground = true
        editor.backgroundColor = Graphite.bg
        editor.insertionPointColor = Graphite.accent
        editor.selectedTextAttributes = [.backgroundColor: Graphite.accent.withAlphaComponent(0.3)]
        editor.textContainerInset = NSSize(width: 6, height: 8)
        editor.isVerticallyResizable = true
        editor.isHorizontallyResizable = true
        editor.autoresizingMask = [.width]
        editor.textContainer?.widthTracksTextView = false
        editor.textContainer?.containerSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        editor.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        editor.font = Graphite.mono(13)
        editor.string = tab.sql
        editor.setAccessibilityLabel(engine == .redis ? "Redis command editor" : "SQL editor")
        editor.onRun = { [weak self] in self?.runFromEditor() }
        editorScroll.translatesAutoresizingMaskIntoConstraints = false
        editorScroll.documentView = editor
        editorScroll.hasVerticalScroller = true
        editorScroll.hasHorizontalScroller = true
        editorScroll.autohidesScrollers = true
        editorScroll.scrollerStyle = .overlay
        editorScroll.drawsBackground = true
        editorScroll.backgroundColor = Graphite.bg
        let ruler = LineNumberRuler(editor: editor)
        editorScroll.verticalRulerView = ruler
        editorScroll.hasVerticalRuler = true
        editorScroll.rulersVisible = true
        editor.rehighlight()
        editor.setSelectedRange(tab.selection)
        editor.updateActiveStatement()

        hint.stringValue = engine == .redis
            ? "The outlined command or selection will run · Command/Ctrl+Enter · results capped at 10,000 rows"
            : "The outlined statement or selected SQL will run · Command/Ctrl+Enter · results capped at 10,000 rows"
        eyebrowLabel.stringValue = engine == .redis ? "REDIS WORKBENCH" : "SQL WORKBENCH"
        let editorCard = PanelView(fill: Graphite.bg)
        editorCard.radius = 10
        editorCard.outline = Graphite.border
        let hintBar = PanelView(fill: nil, edges: [.top])
        hintBar.edgeColor = Graphite.hairline
        hintBar.pin(hint, insets: NSEdgeInsets(top: 6, left: 10, bottom: 6, right: 10))
        let editorColumn = vstack([editorScroll, hintBar], spacing: 0)
        editorColumn.distribution = .fill
        editorScroll.widthAnchor.constraint(equalTo: editorColumn.widthAnchor).isActive = true
        hintBar.widthAnchor.constraint(equalTo: editorColumn.widthAnchor).isActive = true
        editorScroll.setContentHuggingPriority(.init(1), for: .vertical)
        editorCard.pin(editorColumn, insets: NSEdgeInsets(top: 1, left: 1, bottom: 1, right: 1))

        let historyCard = PanelView(fill: Graphite.chrome)
        historyCard.radius = 10
        historyCard.outline = Graphite.border
        let historyHeader = PanelView(fill: nil, edges: [.bottom])
        historyHeader.pin(hstack([label("History", font: Graphite.ui(13, .semibold), color: Graphite.textStrong), spacer(), historyCount]),
                          insets: NSEdgeInsets(top: 10, left: 12, bottom: 10, right: 12))
        let historyScroll = verticalScroll(historyList)
        let historyColumn = vstack([historyHeader, historyScroll], spacing: 0)
        historyColumn.distribution = .fill
        historyHeader.widthAnchor.constraint(equalTo: historyColumn.widthAnchor).isActive = true
        historyScroll.widthAnchor.constraint(equalTo: historyColumn.widthAnchor).isActive = true
        historyScroll.setContentHuggingPriority(.init(1), for: .vertical)
        historyCard.pin(historyColumn, insets: NSEdgeInsets(top: 1, left: 1, bottom: 1, right: 1))
        historyCard.addSubview(historyEmpty)
        NSLayoutConstraint.activate([
            historyEmpty.leadingAnchor.constraint(equalTo: historyCard.leadingAnchor, constant: 12),
            historyEmpty.topAnchor.constraint(equalTo: historyCard.topAnchor, constant: 52),
            historyCard.widthAnchor.constraint(equalToConstant: 250),
        ])

        let top = FlippedView()
        top.translatesAutoresizingMaskIntoConstraints = false
        let row = hstack([editorCard, historyCard], spacing: 10)
        row.distribution = .fill
        top.pin(row, insets: NSEdgeInsets(top: 2, left: 14, bottom: 10, right: 14))
        editorCard.heightAnchor.constraint(equalTo: row.heightAnchor).isActive = true
        historyCard.heightAnchor.constraint(equalTo: row.heightAnchor).isActive = true
        editorCard.setContentHuggingPriority(.init(1), for: .horizontal)

        resultCard.radius = 10
        resultCard.outline = Graphite.border
        resultCard.pin(resultGrid.scroll, insets: NSEdgeInsets(top: 1, left: 1, bottom: 1, right: 1))
        results.translatesAutoresizingMaskIntoConstraints = false
        let meta = hstack([resultMeta, editableChip, spacer(), truncatedChip, readOnlyChip], spacing: 8)
        results.addSubview(meta)
        results.addSubview(resultCard)
        results.addSubview(placeholder)
        NSLayoutConstraint.activate([
            meta.topAnchor.constraint(equalTo: results.topAnchor, constant: 8),
            meta.leadingAnchor.constraint(equalTo: results.leadingAnchor, constant: 14),
            meta.trailingAnchor.constraint(equalTo: results.trailingAnchor, constant: -14),
            meta.heightAnchor.constraint(equalToConstant: 22),
            resultCard.topAnchor.constraint(equalTo: meta.bottomAnchor, constant: 8),
            resultCard.leadingAnchor.constraint(equalTo: results.leadingAnchor, constant: 14),
            resultCard.trailingAnchor.constraint(equalTo: results.trailingAnchor, constant: -14),
            resultCard.bottomAnchor.constraint(equalTo: results.bottomAnchor, constant: -12),
            placeholder.centerXAnchor.constraint(equalTo: results.centerXAnchor),
            placeholder.centerYAnchor.constraint(equalTo: results.centerYAnchor),
        ])

        split.isVertical = false
        split.dividerStyle = .thin
        split.delegate = self
        split.translatesAutoresizingMaskIntoConstraints = false
        split.addArrangedSubview(top)
        split.addArrangedSubview(results)
        split.setHoldingPriority(.init(260), forSubviewAt: 0)
        split.setHoldingPriority(.init(250), forSubviewAt: 1)

        addSubview(toolbar)
        addSubview(split)
        NSLayoutConstraint.activate([
            toolbar.topAnchor.constraint(equalTo: topAnchor),
            toolbar.leadingAnchor.constraint(equalTo: leadingAnchor),
            toolbar.trailingAnchor.constraint(equalTo: trailingAnchor),
            toolbar.heightAnchor.constraint(equalToConstant: 58),
            split.topAnchor.constraint(equalTo: toolbar.bottomAnchor),
            split.leadingAnchor.constraint(equalTo: leadingAnchor),
            split.trailingAnchor.constraint(equalTo: trailingAnchor),
            split.bottomAnchor.constraint(equalTo: bottomAnchor),
            top.heightAnchor.constraint(greaterThanOrEqualToConstant: 150),
            results.heightAnchor.constraint(greaterThanOrEqualToConstant: 120),
        ])
        reload()
    }

    private var positioned = false

    override func layout() {
        super.layout()
        if !positioned, split.bounds.height > 0 {
            positioned = true
            split.setPosition(min(300, split.bounds.height * 0.45), ofDividerAt: 0)
        }
    }

    func splitView(_ splitView: NSSplitView, constrainMinCoordinate proposedMinimumPosition: CGFloat, ofSubviewAt dividerIndex: Int) -> CGFloat { 150 }
    func splitView(_ splitView: NSSplitView, constrainMaxCoordinate proposedMaximumPosition: CGFloat, ofSubviewAt dividerIndex: Int) -> CGFloat {
        splitView.bounds.height - 120
    }

    // MARK: State → views

    func reload() {
        titleLabel.stringValue = tab.title
        let running = host?.isRunning(tab)
        let target = editor.updateActiveStatement()
        let engine = host?.engine(of: tab) ?? .postgres
        runButton.title = running == "run" ? "Running…"
            : target?.selection == true ? "Run selection" : engine == .redis ? "Run command" : "Run statement"
        runButton.isEnabled = running == nil && target != nil
        refreshButton.title = running == "refresh" ? "Refreshing…" : "Refresh"
        refreshButton.isEnabled = running == nil && tab.lastExecuted != nil && !(embeddedPane?.tab.dirty ?? false)
        refreshButton.toolTip = tab.lastExecuted != nil
            ? "Re-run the last executed statement for fresh results" : "Run a statement first to enable refresh"
        reloadHistory()
        reloadResult()
    }

    private func reloadHistory() {
        let entries = host?.history(for: tab) ?? []
        historyCount.stringValue = String(entries.count)
        historyEmpty.isHidden = !entries.isEmpty
        let signature = entries.prefix(100).map { "\(string($0["id"]))" }.joined(separator: ",")
        guard signature != historySignature else { return }
        historySignature = signature
        historyList.arrangedSubviews.forEach { $0.removeFromSuperview() }
        for entry in entries.prefix(100) {
            let row = HistoryRow(entry: entry) { [weak self] sql in self?.load(sql: sql) }
            historyList.addArrangedSubview(row)
            row.widthAnchor.constraint(equalTo: historyList.widthAnchor).isActive = true
        }
    }

    private func reloadResult() {
        guard let result = tab.result else {
            [resultMeta, editableChip, readOnlyChip, truncatedChip, resultCard].forEach { $0.isHidden = true }
            embeddedPane?.removeFromSuperview()
            embeddedPane = nil
            placeholder.isHidden = false
            shownResult = nil
            return
        }
        placeholder.isHidden = true
        resultMeta.isHidden = false
        let duration = int(result["durationMs"])
        if let host, tab.embedded != nil {
            resultMeta.stringValue = "Table viewer · query completed in \(duration) ms"
            editableChip.isHidden = false
            readOnlyChip.isHidden = true
            truncatedChip.isHidden = true
            resultCard.isHidden = true
            if embeddedPane == nil, tab.tableState != nil {
                let pane = TablePane(tab: tab, host: host, embedded: true)
                results.addSubview(pane)
                NSLayoutConstraint.activate([
                    pane.topAnchor.constraint(equalTo: resultMeta.bottomAnchor, constant: 8),
                    pane.leadingAnchor.constraint(equalTo: results.leadingAnchor),
                    pane.trailingAnchor.constraint(equalTo: results.trailingAnchor),
                    pane.bottomAnchor.constraint(equalTo: results.bottomAnchor),
                ])
                embeddedPane = pane
            }
            embeddedPane?.reload()
            return
        }
        embeddedPane?.removeFromSuperview()
        embeddedPane = nil
        editableChip.isHidden = true
        let columns = array(result["columns"])
        let affected = optionalInt(result["affectedRows"]).map { " · \($0) affected" } ?? ""
        resultMeta.stringValue = "\(int(result["rowCount"])) rows\(affected) · \(duration) ms"
        truncatedChip.isHidden = !bool(result["truncated"])
        readOnlyChip.isHidden = columns.isEmpty
        readOnlyChip.toolTip = "This query does not resolve to one complete table, so DBM cannot safely map edits back to rows."
        resultCard.isHidden = columns.isEmpty
        if shownResult != tab.resultID {
            shownResult = tab.resultID
            resultGrid.show(columns: columns, rows: result["rows"] as? [[Any]] ?? [])
        }
    }

    // MARK: Editing

    func textDidChange(_ notification: Notification) {
        tab.sql = editor.string
        editor.rehighlight()
        reload()
    }

    func textViewDidChangeSelection(_ notification: Notification) {
        tab.selection = editor.selectedRange()
        reload()
    }

    func textView(_ textView: NSTextView, completions words: [String], forPartialWordRange charRange: NSRange,
                  indexOfSelectedItem index: UnsafeMutablePointer<Int>?) -> [String] {
        words
    }

    private func load(sql: String) {
        editor.string = sql
        tab.sql = sql
        editor.rehighlight()
        editor.setSelectedRange(NSRange(location: (sql as NSString).length, length: 0))
        window?.makeFirstResponder(editor)
        reload()
    }

    private func runFromEditor() {
        guard let target = editor.updateActiveStatement() else { return }
        host?.runQuery(tab, sql: target.sql, refresh: false)
    }

    private func refreshResult() {
        guard let sql = tab.lastExecuted else { return }
        host?.runQuery(tab, sql: sql, refresh: true)
    }
}
