import AppKit

/// A Graphite button: primary (accent fill), secondary (control fill),
/// toolbar (transparent until hovered), danger text, filled danger, accent
/// text link, or icon-only. Drawn in `draw(_:)` so snapshots capture it.
final class GButton: NSButton {
    enum Style { case primary, secondary, toolbar, danger, dangerFilled, link, icon }

    var style: Style { didSet { invalidate() } }
    var icon: Icon? { didSet { invalidate() } }
    var shortcut: String? { didSet { invalidate() } }
    /// Toggle highlight, e.g. the inspector button while the inspector is open.
    var isOn = false { didSet { needsDisplay = true } }
    var handler: (() -> Void)?
    var labelFont = Graphite.ui(12.5, .medium) { didSet { invalidate() } }
    var minHeight: CGFloat = 28 { didSet { invalidate() } }
    /// Draws the icon after the title, e.g. a menu chevron.
    var iconTrailing = false { didSet { needsDisplay = true } }
    /// Overrides the label color, e.g. `.secondary-button.danger-text`.
    var tint: NSColor? { didSet { needsDisplay = true } }
    private var hovering = false
    private var tracking: NSTrackingArea?

    init(_ title: String = "", icon: Icon? = nil, style: Style = .secondary, shortcut: String? = nil,
         tooltip: String? = nil, handler: (() -> Void)? = nil) {
        self.style = style
        self.icon = icon
        self.shortcut = shortcut
        self.handler = handler
        super.init(frame: .zero)
        self.title = title
        isBordered = false
        bezelStyle = .regularSquare
        setButtonType(.momentaryChange)
        focusRingType = .none
        translatesAutoresizingMaskIntoConstraints = false
        target = self
        action = #selector(fire)
        toolTip = tooltip
        if title.isEmpty, let tooltip { setAccessibilityLabel(tooltip) }
        if style == .primary { labelFont = Graphite.ui(12.5, .semibold) }
        setContentHuggingPriority(.defaultHigh, for: .horizontal)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override var isFlipped: Bool { true }

    // `cursor: pointer`, and `not-allowed` while disabled, as in the desktop.
    override func resetCursorRects() {
        addCursorRect(bounds, cursor: isEnabled ? .pointingHand : .operationNotAllowed)
    }

    override var isEnabled: Bool {
        didSet { if isEnabled != oldValue { window?.invalidateCursorRects(for: self) } }
    }

    @objc private func fire() { handler?() }

    private func invalidate() {
        invalidateIntrinsicContentSize()
        needsDisplay = true
    }

    private var textAttributes: [NSAttributedString.Key: Any] {
        [.font: labelFont, .foregroundColor: foreground]
    }

    private var foreground: NSColor {
        guard isEnabled else { return style == .primary ? NSColor(white: 1, alpha: 0.55) : Graphite.faint }
        if let tint { return tint }
        switch style {
        case .primary, .dangerFilled: return .white
        case .danger: return Graphite.danger
        case .link: return Graphite.accentText
        // `.icon-toggle.on` is accent text on an accent wash.
        case .toolbar, .icon: return isOn ? Graphite.accentText : hovering ? Graphite.textStrong : Graphite.secondary
        case .secondary: return Graphite.text
        }
    }

    override var intrinsicContentSize: NSSize {
        let iconWidth: CGFloat = icon == nil ? 0 : 14
        let text = title.isEmpty ? 0 : (title as NSString).size(withAttributes: textAttributes).width
        let gap: CGFloat = icon != nil && !title.isEmpty ? 6 : 0
        let hint = shortcut.map { ($0 as NSString).size(withAttributes: [.font: Graphite.ui(11, .medium)]).width + 18 } ?? 0
        let padding: CGFloat = style == .icon ? 7 : (style == .link ? 6 : 12)
        return NSSize(width: ceil(iconWidth + gap + text + hint + padding * 2), height: minHeight)
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking { removeTrackingArea(tracking) }
        let area = NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeInActiveApp, .inVisibleRect],
                                  owner: self, userInfo: nil)
        addTrackingArea(area)
        tracking = area
    }

    override func mouseEntered(with event: NSEvent) { hovering = true; needsDisplay = true }
    override func mouseExited(with event: NSEvent) { hovering = false; needsDisplay = true }

    override func draw(_ dirtyRect: NSRect) {
        let rect = bounds.insetBy(dx: 0.5, dy: 0.5)
        let shape = NSBezierPath(roundedRect: rect, xRadius: 6, yRadius: 6)
        let pressed = isHighlighted && isEnabled
        switch style {
        case .primary:
            let fill = !isEnabled ? Graphite.accentStrong.withAlphaComponent(0.5)
                : pressed ? Graphite.accent : hovering ? NSColor(hex: 0x4585d6) : Graphite.accentStrong
            fill.setFill()
            shape.fill()
        case .dangerFilled:
            NSColor(hex: 0xb43c36, alpha: isEnabled ? 1 : 0.5).setFill()
            shape.fill()
        case .secondary:
            (pressed ? Graphite.controlActive : hovering && isEnabled ? Graphite.controlHover : Graphite.control).setFill()
            shape.fill()
            Graphite.borderStrong.setStroke()
            shape.lineWidth = 1
            shape.stroke()
        case .danger:
            // `.danger-button`: danger wash with a soft danger outline.
            NSColor(hex: 0xff6b61, alpha: hovering && isEnabled ? 0.16 : 0.09).setFill()
            shape.fill()
            NSColor(hex: 0xff6b61, alpha: 0.3).setStroke()
            shape.lineWidth = 1
            shape.stroke()
        case .toolbar, .icon:
            if isOn {
                Graphite.accentSoft.setFill(); shape.fill()
            } else if (hovering || pressed) && isEnabled {
                (pressed ? Graphite.controlActive : Graphite.controlHover).setFill(); shape.fill()
            }
        case .link:
            // `.text-button:hover { background: var(--accent-soft) }`
            if hovering && isEnabled {
                Graphite.accentSoft.setFill()
                NSBezierPath(roundedRect: rect, xRadius: 5, yRadius: 5).fill()
            }
        }
        let color = foreground
        let padding: CGFloat = style == .icon ? 7 : (style == .link ? 6 : 12)
        let text = title as NSString
        let textSize = title.isEmpty ? .zero : text.size(withAttributes: textAttributes)
        let hintWidth = shortcut.map { ($0 as NSString).size(withAttributes: [.font: Graphite.ui(11, .medium)]).width + 18 } ?? 0
        let content = (icon == nil ? 0 : 14) + (icon != nil && !title.isEmpty ? 6 : 0) + textSize.width + hintWidth
        var x = style == .icon ? (bounds.width - 14) / 2 : max(padding, (bounds.width - content) / 2)
        if let icon, !iconTrailing {
            icon.draw(in: NSRect(x: x, y: (bounds.height - 14) / 2, width: 14, height: 14), color: color)
            x += 14 + (title.isEmpty ? 0 : 6)
        }
        if !title.isEmpty {
            text.draw(at: NSPoint(x: x, y: (bounds.height - textSize.height) / 2), withAttributes: textAttributes)
            x += textSize.width
        }
        if let icon, iconTrailing {
            icon.draw(in: NSRect(x: x + 6, y: (bounds.height - 12) / 2, width: 12, height: 12), color: color)
            x += 18
        }
        if let shortcut {
            let attributes: [NSAttributedString.Key: Any] = [.font: Graphite.ui(11, .medium), .foregroundColor: color.withAlphaComponent(0.9)]
            let size = (shortcut as NSString).size(withAttributes: attributes)
            let chip = NSRect(x: x + 10, y: (bounds.height - 18) / 2, width: size.width + 8, height: 18)
            NSColor(white: 1, alpha: 0.18).setFill()
            NSBezierPath(roundedRect: chip, xRadius: 4, yRadius: 4).fill()
            (shortcut as NSString).draw(at: NSPoint(x: chip.minX + 4, y: chip.midY - size.height / 2), withAttributes: attributes)
        }
    }
}

/// Small rounded label, e.g. "Read-only" or "truncated".
final class Chip: NSView {
    var text: String { didSet { invalidateIntrinsicContentSize(); needsDisplay = true } }
    var color: NSColor { didSet { needsDisplay = true } }

    init(_ text: String, color: NSColor) {
        self.text = text
        self.color = color
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        setContentHuggingPriority(.required, for: .horizontal)
        setContentCompressionResistancePriority(.required, for: .horizontal)
        setAccessibilityElement(true)
        setAccessibilityRole(.staticText)
        setAccessibilityLabel(text)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override var isFlipped: Bool { true }

    private var attributes: [NSAttributedString.Key: Any] { [.font: Graphite.ui(11, .medium), .foregroundColor: color] }

    override var intrinsicContentSize: NSSize {
        let size = (text as NSString).size(withAttributes: attributes)
        return NSSize(width: ceil(size.width + 12), height: 20)
    }

    override func draw(_ dirtyRect: NSRect) {
        color.withAlphaComponent(0.14).setFill()
        NSBezierPath(roundedRect: bounds, xRadius: 5, yRadius: 5).fill()
        let size = (text as NSString).size(withAttributes: attributes)
        (text as NSString).draw(at: NSPoint(x: 6, y: (bounds.height - size.height) / 2), withAttributes: attributes)
    }
}

func label(_ text: String, font: NSFont = Graphite.ui(13), color: NSColor = Graphite.text) -> NSTextField {
    let field = NSTextField(labelWithString: text)
    field.font = font
    field.textColor = color
    field.lineBreakMode = .byTruncatingTail
    field.translatesAutoresizingMaskIntoConstraints = false
    field.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
    return field
}

func sectionLabel(_ text: String) -> NSTextField {
    label(text, font: Graphite.ui(11, .semibold), color: Graphite.muted)
}

func eyebrow(_ text: String) -> NSTextField {
    let field = label(text, font: Graphite.ui(10.5, .semibold), color: Graphite.faint)
    field.attributedStringValue = NSAttributedString(string: text, attributes: [
        .font: Graphite.ui(10.5, .semibold), .foregroundColor: Graphite.faint, .kern: 0.6,
    ])
    return field
}

/// Text field cell with Graphite bezel, padding, and vertically centered text.
final class GTextFieldCell: NSTextFieldCell {
    var focused = false
    /// Leading room, widened for a search icon.
    var leftInset: CGFloat = 8
    /// Bezel colors for states such as the inspector's changed field.
    var fillOverride: NSColor?
    var strokeOverride: NSColor?
    /// No bezel at all, like the inspector's read-only fields.
    var plain = false
    /// The grid's in-cell editor: square, edit-surface fill, inner accent line.
    var gridEditor = false

    override func titleRect(forBounds rect: NSRect) -> NSRect {
        let inset = NSRect(x: rect.minX + leftInset, y: rect.minY, width: max(0, rect.width - leftInset - 8), height: rect.height)
        let height = min(inset.height, ceil(cellSize(forBounds: inset).height))
        return NSRect(x: inset.minX, y: rect.minY + (rect.height - height) / 2, width: inset.width, height: height)
    }

    override func drawingRect(forBounds rect: NSRect) -> NSRect { titleRect(forBounds: rect) }

    override func draw(withFrame cellFrame: NSRect, in controlView: NSView) {
        if gridEditor {
            Graphite.editSurface.setFill()
            cellFrame.fill()
            let line = NSBezierPath(rect: cellFrame.insetBy(dx: 0.75, dy: 0.75))
            line.lineWidth = 1.5
            Graphite.accent.setStroke()
            line.stroke()
        } else if !plain {
            drawFieldBezel(cellFrame, focused: focused, enabled: isEnabled,
                           fill: focused && fillOverride != nil ? Graphite.editSurface : fillOverride, stroke: strokeOverride)
        }
        drawInterior(withFrame: cellFrame, in: controlView)
    }

    override func drawInterior(withFrame cellFrame: NSRect, in controlView: NSView) {
        super.drawInterior(withFrame: titleRect(forBounds: cellFrame), in: controlView)
    }

    override func edit(withFrame rect: NSRect, in controlView: NSView, editor textObj: NSText, delegate: Any?, event: NSEvent?) {
        super.edit(withFrame: titleRect(forBounds: rect), in: controlView, editor: textObj, delegate: delegate, event: event)
    }

    override func select(withFrame rect: NSRect, in controlView: NSView, editor textObj: NSText, delegate: Any?, start selStart: Int, length selLength: Int) {
        super.select(withFrame: titleRect(forBounds: rect), in: controlView, editor: textObj, delegate: delegate, start: selStart, length: selLength)
    }
}

final class GSecureTextFieldCell: NSSecureTextFieldCell {
    var focused = false

    override func titleRect(forBounds rect: NSRect) -> NSRect {
        let inset = rect.insetBy(dx: 8, dy: 0)
        let height = min(inset.height, ceil(cellSize(forBounds: inset).height))
        return NSRect(x: inset.minX, y: rect.minY + (rect.height - height) / 2, width: inset.width, height: height)
    }

    override func drawingRect(forBounds rect: NSRect) -> NSRect { titleRect(forBounds: rect) }

    override func draw(withFrame cellFrame: NSRect, in controlView: NSView) {
        drawFieldBezel(cellFrame, focused: focused, enabled: isEnabled)
        drawInterior(withFrame: cellFrame, in: controlView)
    }

    override func drawInterior(withFrame cellFrame: NSRect, in controlView: NSView) {
        super.drawInterior(withFrame: titleRect(forBounds: cellFrame), in: controlView)
    }

    override func edit(withFrame rect: NSRect, in controlView: NSView, editor textObj: NSText, delegate: Any?, event: NSEvent?) {
        super.edit(withFrame: titleRect(forBounds: rect), in: controlView, editor: textObj, delegate: delegate, event: event)
    }

    override func select(withFrame rect: NSRect, in controlView: NSView, editor textObj: NSText, delegate: Any?, start selStart: Int, length selLength: Int) {
        super.select(withFrame: titleRect(forBounds: rect), in: controlView, editor: textObj, delegate: delegate, start: selStart, length: selLength)
    }
}

func drawFieldBezel(_ frame: NSRect, focused: Bool, enabled: Bool, fill: NSColor? = nil, stroke: NSColor? = nil) {
    let path = NSBezierPath(roundedRect: frame.insetBy(dx: 0.5, dy: 0.5), xRadius: 6, yRadius: 6)
    (enabled ? fill ?? Graphite.control : Graphite.chrome).setFill()
    path.fill()
    (focused ? Graphite.accent : stroke ?? Graphite.borderStrong).setStroke()
    path.lineWidth = 1
    path.stroke()
    if focused {
        let ring = NSBezierPath(roundedRect: frame.insetBy(dx: -1.5, dy: -1.5), xRadius: 7.5, yRadius: 7.5)
        Graphite.accent.withAlphaComponent(0.18).setStroke()
        ring.lineWidth = 3
        ring.stroke()
    }
}

/// Editing callbacks shared by `GTextField` and `GSecureField`: `onCommit`
/// runs when editing ends (Enter or focus loss); Escape reverts the field,
/// runs `onCancel`, and ends editing without committing.
final class FieldBehavior: NSObject, NSTextFieldDelegate {
    var onCommit: ((String) -> Void)?
    var onChange: ((String) -> Void)?
    var onCancel: (() -> Void)?
    /// Handles Return in place of ending editing.
    var onReturn: (() -> Void)?
    /// Escape runs `onCancel` but leaves the text and focus alone, like the
    /// desktop's filter inputs.
    var keepsFocusOnCancel = false
    var original = ""
    private var canceling = false

    func controlTextDidBeginEditing(_ notification: Notification) {
        guard let field = notification.object as? NSTextField else { return }
        original = field.stringValue
        setFocused(field, true)
    }

    func controlTextDidChange(_ notification: Notification) {
        guard let field = notification.object as? NSTextField else { return }
        onChange?(field.stringValue)
    }

    func controlTextDidEndEditing(_ notification: Notification) {
        guard let field = notification.object as? NSTextField else { return }
        setFocused(field, false)
        if !canceling { onCommit?(field.stringValue) }
    }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        if selector == #selector(NSResponder.insertNewline(_:)), let onReturn {
            onReturn()
            return true
        }
        guard selector == #selector(NSResponder.cancelOperation(_:)), let field = control as? NSTextField else { return false }
        if keepsFocusOnCancel {
            onCancel?()
            return true
        }
        canceling = true
        field.stringValue = original
        field.window?.makeFirstResponder(nil)
        canceling = false
        onCancel?()
        return true
    }

    private func setFocused(_ field: NSTextField, _ focused: Bool) {
        (field.cell as? GTextFieldCell)?.focused = focused
        (field.cell as? GSecureTextFieldCell)?.focused = focused
        field.needsDisplay = true
    }
}

private func style(_ field: NSTextField, text: String, placeholder: String, mono: Bool, height: CGFloat) {
    field.stringValue = text
    field.isBezeled = false
    field.isBordered = false
    field.drawsBackground = false
    field.isEditable = true
    field.isSelectable = true
    field.focusRingType = .none
    field.font = mono ? Graphite.mono(12) : Graphite.ui(12.5)
    field.textColor = Graphite.text
    field.usesSingleLineMode = true
    field.cell?.isScrollable = true
    field.cell?.wraps = false
    field.cell?.lineBreakMode = .byClipping
    field.placeholderAttributedString = NSAttributedString(string: placeholder, attributes: [
        .font: field.font ?? Graphite.ui(12.5), .foregroundColor: Graphite.faint,
    ])
    field.translatesAutoresizingMaskIntoConstraints = false
    field.heightAnchor.constraint(equalToConstant: height).isActive = true
    field.setContentHuggingPriority(.defaultLow, for: .horizontal)
    field.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
}

/// Single-line Graphite input.
final class GTextField: NSTextField {
    let behavior = FieldBehavior()
    var onCommit: ((String) -> Void)? { get { behavior.onCommit } set { behavior.onCommit = newValue } }
    var onChange: ((String) -> Void)? { get { behavior.onChange } set { behavior.onChange = newValue } }
    var onCancel: (() -> Void)? { get { behavior.onCancel } set { behavior.onCancel = newValue } }

    override class var cellClass: AnyClass? {
        get { GTextFieldCell.self }
        set { super.cellClass = newValue }
    }

    init(_ text: String = "", placeholder: String = "", mono: Bool = false, height: CGFloat = 28) {
        super.init(frame: .zero)
        style(self, text: text, placeholder: placeholder, mono: mono, height: height)
        delegate = behavior
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    func setPlaceholder(_ text: String) {
        placeholderAttributedString = NSAttributedString(string: text, attributes: [
            .font: font ?? Graphite.ui(12.5), .foregroundColor: Graphite.faint,
        ])
    }
}

final class GSecureField: NSSecureTextField {
    let behavior = FieldBehavior()
    var onCommit: ((String) -> Void)? { get { behavior.onCommit } set { behavior.onCommit = newValue } }
    var onChange: ((String) -> Void)? { get { behavior.onChange } set { behavior.onChange = newValue } }

    override class var cellClass: AnyClass? {
        get { GSecureTextFieldCell.self }
        set { super.cellClass = newValue }
    }

    init(_ text: String = "", placeholder: String = "") {
        super.init(frame: .zero)
        style(self, text: text, placeholder: placeholder, mono: false, height: 28)
        delegate = behavior
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }
}

/// Graphite pop-up: control fill, chevron, Geist title.
final class GPopUpCell: NSPopUpButtonCell {
    override func drawBezel(withFrame frame: NSRect, in controlView: NSView) {
        drawFieldBezel(frame, focused: false, enabled: isEnabled)
        Icon.chevronDown.draw(in: NSRect(x: frame.maxX - 22, y: frame.midY - 6, width: 12, height: 12), color: Graphite.muted)
    }

    override func drawTitle(_ title: NSAttributedString, withFrame frame: NSRect, in controlView: NSView) -> NSRect {
        let text = NSAttributedString(string: title.string, attributes: [
            .font: Graphite.ui(12.5), .foregroundColor: isEnabled ? Graphite.text : Graphite.faint,
        ])
        let size = text.size()
        let bounds = controlView.bounds
        let rect = NSRect(x: bounds.minX + 10, y: bounds.midY - size.height / 2,
                          width: max(0, bounds.width - 38), height: size.height)
        text.draw(with: rect, options: [.truncatesLastVisibleLine, .usesLineFragmentOrigin])
        return rect
    }

    override func drawImage(_ image: NSImage, withFrame frame: NSRect, in controlView: NSView) {}
}

final class GPopUp: NSPopUpButton {
    var onSelect: ((Int) -> Void)?

    init(items: [String] = [], width: CGFloat? = nil) {
        super.init(frame: .zero, pullsDown: false)
        cell = GPopUpCell(textCell: "", pullsDown: false)
        (cell as? NSPopUpButtonCell)?.arrowPosition = .noArrow
        isBordered = false
        focusRingType = .none
        font = Graphite.ui(12.5)
        translatesAutoresizingMaskIntoConstraints = false
        heightAnchor.constraint(equalToConstant: 28).isActive = true
        if let width { widthAnchor.constraint(equalToConstant: width).isActive = true }
        setItems(items)
        target = self
        action = #selector(selected)
    }

    override func resetCursorRects() {
        addCursorRect(bounds, cursor: isEnabled ? .pointingHand : .operationNotAllowed)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    func setItems(_ items: [String], selected: String? = nil) {
        removeAllItems()
        addItems(withTitles: items)
        if let selected, items.contains(selected) { selectItem(withTitle: selected) }
    }

    @objc private func selected() { onSelect?(indexOfSelectedItem) }

    override func draw(_ dirtyRect: NSRect) {
        drawFieldBezel(bounds, focused: false, enabled: isEnabled)
        Icon.chevronDown.draw(in: NSRect(x: bounds.maxX - 22, y: bounds.midY - 6, width: 12, height: 12), color: Graphite.muted)
        let text = NSAttributedString(string: titleOfSelectedItem ?? "", attributes: [
            .font: Graphite.ui(12.5), .foregroundColor: isEnabled ? Graphite.text : Graphite.faint,
        ])
        let height = text.size().height
        text.draw(with: NSRect(x: 10, y: (bounds.height - height) / 2, width: max(0, bounds.width - 38), height: height),
                  options: [.truncatesLastVisibleLine, .usesLineFragmentOrigin])
    }

    override var isFlipped: Bool { true }
}

/// A vertical scroll view whose document is a flipped stack filling its width.
func verticalScroll(_ content: NSView) -> NSScrollView {
    let scroll = NSScrollView()
    scroll.translatesAutoresizingMaskIntoConstraints = false
    scroll.drawsBackground = false
    scroll.hasVerticalScroller = true
    scroll.autohidesScrollers = true
    scroll.scrollerStyle = .overlay
    let document = FlippedView()
    document.translatesAutoresizingMaskIntoConstraints = false
    document.addSubview(content)
    content.translatesAutoresizingMaskIntoConstraints = false
    scroll.documentView = document
    NSLayoutConstraint.activate([
        document.leadingAnchor.constraint(equalTo: scroll.contentView.leadingAnchor),
        document.trailingAnchor.constraint(equalTo: scroll.contentView.trailingAnchor),
        document.topAnchor.constraint(equalTo: scroll.contentView.topAnchor),
        content.leadingAnchor.constraint(equalTo: document.leadingAnchor),
        content.trailingAnchor.constraint(equalTo: document.trailingAnchor),
        content.topAnchor.constraint(equalTo: document.topAnchor),
        content.bottomAnchor.constraint(equalTo: document.bottomAnchor),
    ])
    return scroll
}

func vstack(_ views: [NSView], spacing: CGFloat = 0, alignment: NSLayoutConstraint.Attribute = .leading) -> NSStackView {
    let stack = NSStackView(views: views)
    stack.orientation = .vertical
    stack.alignment = alignment
    stack.spacing = spacing
    stack.translatesAutoresizingMaskIntoConstraints = false
    return stack
}

func hstack(_ views: [NSView], spacing: CGFloat = 8) -> NSStackView {
    let stack = NSStackView(views: views)
    stack.orientation = .horizontal
    stack.alignment = .centerY
    stack.spacing = spacing
    stack.translatesAutoresizingMaskIntoConstraints = false
    return stack
}

func spacer() -> NSView {
    let view = NSView()
    view.translatesAutoresizingMaskIntoConstraints = false
    view.setContentHuggingPriority(.init(1), for: .horizontal)
    view.setContentCompressionResistancePriority(.init(1), for: .horizontal)
    return view
}

extension NSView {
    func pin(_ child: NSView, insets: NSEdgeInsets = NSEdgeInsetsZero) {
        child.translatesAutoresizingMaskIntoConstraints = false
        addSubview(child)
        NSLayoutConstraint.activate([
            child.leadingAnchor.constraint(equalTo: leadingAnchor, constant: insets.left),
            child.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -insets.right),
            child.topAnchor.constraint(equalTo: topAnchor, constant: insets.top),
            child.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -insets.bottom),
        ])
    }
}
