import AppKit

/// The desktop app's hover card for a staged row (`ChangePreview` in
/// `TableView.tsx`): a pending edit's before → after per changed column with
/// the changed characters marked, or a pending delete, plus a button that
/// discards the change. It floats in a child window so it can extend past the
/// grid, and closes 140 ms after the pointer leaves both the row and the card.
final class ChangePreviewController {
    private(set) var row: Int?
    private var panel: NSPanel?
    private var closeTimer: Timer?

    func show(row: Int, pending: PendingRow, columns: [Column], rowRect: NSRect, in window: NSWindow,
              onDiscard: @escaping () -> Void) {
        cancelClose()
        guard self.row != row else { return }
        close()
        self.row = row
        let bounds = window.frame
        let width = min(680, max(380, ChangePreviewCard.idealWidth(pending: pending, columns: columns)), bounds.width - 32)
        let card = ChangePreviewCard(pending: pending, columns: columns, width: width) { [weak self] in
            self?.close()
            onDiscard()
        }
        card.onHover = { [weak self] inside in inside ? self?.cancelClose() : self?.scheduleClose() }
        let measure = card.widthAnchor.constraint(equalToConstant: width)
        measure.isActive = true
        let natural = card.fittingSize.height
        measure.isActive = false
        card.translatesAutoresizingMaskIntoConstraints = true
        let spaceBelow = max(0, rowRect.minY - bounds.minY - 16)
        let spaceAbove = max(0, bounds.maxY - rowRect.maxY - 16)
        let below = spaceBelow >= 260 || spaceBelow >= spaceAbove
        let maxHeight = max(140, min(520, below ? spaceBelow : spaceAbove))
        let height = min(maxHeight, natural)
        let x = max(bounds.minX + 16, min(rowRect.minX + 16, bounds.maxX - width - 16))
        let y = below ? rowRect.minY - height : rowRect.maxY
        let panel = NSPanel(contentRect: NSRect(x: x, y: y, width: width, height: height),
                            styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: true)
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.isReleasedWhenClosed = false
        panel.contentView = card
        window.addChildWindow(panel, ordered: .above)
        panel.alphaValue = 0
        panel.orderFront(nil)
        NSAnimationContext.runAnimationGroup { context in
            context.duration = 0.1
            panel.animator().alphaValue = 1
        }
        self.panel = panel
    }

    func scheduleClose() {
        guard panel != nil else { return }
        closeTimer?.invalidate()
        closeTimer = Timer.scheduledTimer(withTimeInterval: 0.14, repeats: false) { [weak self] _ in self?.close() }
    }

    func cancelClose() {
        closeTimer?.invalidate()
        closeTimer = nil
    }

    /// True while a close is scheduled but has not happened yet.
    var closing: Bool { closeTimer != nil }

    func close() {
        cancelClose()
        row = nil
        guard let panel else { return }
        panel.parent?.removeChildWindow(panel)
        panel.orderOut(nil)
        self.panel = nil
    }
}

private final class ChangePreviewCard: PanelView {
    var onHover: ((Bool) -> Void)?

    /// Wide enough for two value boxes of the longest changed value.
    static func idealWidth(pending: PendingRow, columns: [Column]) -> CGFloat {
        let longest = columns.indices.map { index in
            max(displayValue(pending.original[index]).count, displayValue(pending.changes[index]).count)
        }.max() ?? 0
        return CGFloat(min(longest, 42)) * 7 * 2 + 90
    }

    init(pending: PendingRow, columns: [Column], width: CGFloat, onDiscard: @escaping () -> Void) {
        super.init(fill: Graphite.popover)
        radius = 10
        outline = Graphite.borderStrong
        let deleted = pending.deleted
        let dot = PanelView(fill: deleted ? Graphite.danger : Graphite.modified)
        dot.radius = 3.5
        dot.widthAnchor.constraint(equalToConstant: 7).isActive = true
        dot.heightAnchor.constraint(equalToConstant: 7).isActive = true
        let title = label(deleted ? "Pending delete" : "Pending edit", font: Graphite.ui(12.5, .semibold), color: Graphite.textStrong)
        let button = GButton(deleted ? "Undo delete" : "Discard edit", style: .secondary, handler: onDiscard)
        button.minHeight = 26
        button.labelFont = Graphite.ui(12)
        let header = hstack([dot, title, spacer(), button], spacing: 7)
        header.setCustomSpacing(18, after: title)
        var rows: [NSView] = [header]
        if deleted {
            rows.append(label("This row will be deleted when changes are saved.", font: Graphite.ui(12), color: Graphite.muted))
        } else {
            for index in columns.indices where jsonKey(pending.changes[index]) != jsonKey(pending.original[index]) {
                rows.append(Self.diffRow(column: columns[index].name,
                                         before: displayValue(pending.original[index]),
                                         after: displayValue(pending.changes[index]),
                                         textWidth: (width - 24 - 16 - 16) / 2 - 16))
            }
        }
        let stack = vstack(rows, spacing: 10)
        rows.forEach { $0.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true }
        pin(stack, insets: NSEdgeInsets(top: 12, left: 12, bottom: 12, right: 12))
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        trackingAreas.forEach(removeTrackingArea)
        addTrackingArea(NSTrackingArea(rect: bounds, options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect], owner: self))
    }

    override func mouseEntered(with event: NSEvent) { onHover?(true) }
    override func mouseExited(with event: NSEvent) { onHover?(false) }

    /// `.change-diff`: a top rule, the mono column name, then Before → After.
    private static func diffRow(column: String, before: String, after: String, textWidth: CGFloat) -> NSView {
        let diff = Helpers.inlineDiff(before: before, after: after)
        let rule = PanelView(fill: Graphite.border)
        rule.heightAnchor.constraint(equalToConstant: 1).isActive = true
        let name = label(column, font: Graphite.mono(11.5, .medium), color: Graphite.secondary)
        let beforeBox = valueBox(caption: "Before", prefix: diff.prefix, changed: diff.removed, suffix: diff.suffix,
                                 fill: NSColor(hex: 0x1e1e20), color: Graphite.muted,
                                 mark: NSColor(hex: 0xff6b61, alpha: 0.22), textWidth: textWidth)
        let afterBox = valueBox(caption: "After", prefix: diff.prefix, changed: diff.added, suffix: diff.suffix,
                                fill: Graphite.modifiedSoft, color: Graphite.textStrong,
                                mark: NSColor(hex: 0xf0b14c, alpha: 0.32), textWidth: textWidth)
        let arrow = label("→", font: Graphite.ui(12), color: Graphite.faint)
        arrow.alignment = .center
        arrow.widthAnchor.constraint(equalToConstant: 16).isActive = true
        let values = hstack([beforeBox, arrow, afterBox], spacing: 8)
        values.alignment = .top
        values.distribution = .fill
        beforeBox.widthAnchor.constraint(equalTo: afterBox.widthAnchor).isActive = true
        let block = vstack([rule, name, values], spacing: 6)
        block.setCustomSpacing(10, after: rule)
        [rule, values].forEach { $0.widthAnchor.constraint(equalTo: block.widthAnchor).isActive = true }
        return block
    }

    /// `.change-diff-values pre`: wrapped mono text with the changed middle marked.
    private static func valueBox(caption: String, prefix: String, changed: String, suffix: String,
                                 fill: NSColor, color: NSColor, mark: NSColor, textWidth: CGFloat) -> NSView {
        let font = Graphite.mono(11.5)
        let limit = 600
        let text = NSMutableAttributedString()
        func append(_ part: String, highlight: Bool) {
            guard !part.isEmpty, text.length < limit else { return }
            var attributes: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: color]
            if highlight { attributes[.backgroundColor] = mark }
            text.append(NSAttributedString(string: String(part.prefix(limit - text.length)), attributes: attributes))
        }
        append(prefix, highlight: false)
        append(changed, highlight: true)
        append(suffix, highlight: false)
        if prefix.count + changed.count + suffix.count > limit {
            text.append(NSAttributedString(string: "…", attributes: [.font: font, .foregroundColor: Graphite.faint]))
        }
        let value = NSTextField(wrappingLabelWithString: "")
        value.attributedStringValue = text
        value.lineBreakMode = .byCharWrapping
        value.maximumNumberOfLines = 12
        value.preferredMaxLayoutWidth = max(40, textWidth)
        value.translatesAutoresizingMaskIntoConstraints = false
        value.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        let box = PanelView(fill: fill)
        box.radius = 6
        box.pin(value, insets: NSEdgeInsets(top: 6, left: 8, bottom: 6, right: 8))
        let captionLabel = label(caption, font: Graphite.ui(11), color: Graphite.faint)
        let stack = vstack([captionLabel, box], spacing: 4)
        box.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        stack.setContentHuggingPriority(.defaultLow, for: .horizontal)
        return stack
    }
}
