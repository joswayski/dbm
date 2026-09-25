import AppKit

/// The desktop app's message surfaces: the app error strip under the top bar,
/// inline errors and notices in query and table views, the export result,
/// and the bottom-right toast. Errors fade after 10 seconds and notices after
/// 6; an export result stays until dismissed.
struct InlineMessage {
    enum Kind { case error, notice }

    let id = UUID()
    let text: String
    let kind: Kind
    let export: (url: URL, rows: Int)?

    static func error(_ text: String) -> InlineMessage { InlineMessage(text: text, kind: .error, export: nil) }
    static func notice(_ text: String) -> InlineMessage { InlineMessage(text: text, kind: .notice, export: nil) }
    static func exported(_ url: URL, rows: Int) -> InlineMessage {
        InlineMessage(text: "", kind: .notice, export: (url, rows))
    }

    var lifetime: TimeInterval? { export != nil ? nil : kind == .error ? 10 : 6 }
    var isPlainNotice: Bool { kind == .notice && export == nil }
}

private let errorText = NSColor(hex: 0xffb3ac)
private let noticeText = NSColor(hex: 0x9be7bf)
private let exportLink = NSColor(hex: 0xc6f3da)

/// A 24 pt close button tinted like the message it dismisses.
final class DismissButton: NSButton {
    private let tint: NSColor
    private var hovering = false
    var onClick: (() -> Void)?

    init(tint: NSColor, label: String) {
        self.tint = tint
        super.init(frame: .zero)
        title = ""
        isBordered = false
        focusRingType = .none
        translatesAutoresizingMaskIntoConstraints = false
        widthAnchor.constraint(equalToConstant: 24).isActive = true
        heightAnchor.constraint(equalToConstant: 24).isActive = true
        toolTip = label
        setAccessibilityLabel(label)
        target = self
        action = #selector(fire)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override var isFlipped: Bool { true }
    @objc private func fire() { onClick?() }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        trackingAreas.forEach(removeTrackingArea)
        addTrackingArea(NSTrackingArea(rect: bounds, options: [.mouseEnteredAndExited, .activeInKeyWindow], owner: self))
    }

    override func mouseEntered(with event: NSEvent) { hovering = true; needsDisplay = true }
    override func mouseExited(with event: NSEvent) { hovering = false; needsDisplay = true }

    override func draw(_ dirtyRect: NSRect) {
        if hovering {
            NSColor(white: 1, alpha: 0.05).setFill()
            NSBezierPath(roundedRect: bounds, xRadius: 5, yRadius: 5).fill()
        }
        Icon.close.draw(in: bounds.insetBy(dx: 5.5, dy: 5.5), color: tint)
    }
}

/// An underlined file name that opens the exported file.
final class FileLink: NSButton {
    var onClick: (() -> Void)?

    init(_ name: String) {
        super.init(frame: .zero)
        isBordered = false
        focusRingType = .none
        translatesAutoresizingMaskIntoConstraints = false
        attributedTitle = NSAttributedString(string: name, attributes: [
            .font: Graphite.ui(12.5, .semibold), .foregroundColor: exportLink,
            .underlineStyle: NSUnderlineStyle.single.rawValue,
            .underlineColor: exportLink.withAlphaComponent(0.45),
        ])
        toolTip = "Open the exported file"
        target = self
        action = #selector(fire)
        setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    @objc private func fire() { onClick?() }
    override func resetCursorRects() { addCursorRect(bounds, cursor: .pointingHand) }
}

/// An inline error or notice (rounded, inset from the content) or the
/// full-width app error strip (`banner`). Hidden while it has no message.
final class MessageView: PanelView {
    /// Called when the message is dismissed or expires; the owner clears it.
    var onDismiss: (() -> Void)?
    var onOpen: ((URL) -> Void)?
    var onReveal: ((URL) -> Void)?
    private let banner: Bool
    private var shown: UUID?
    private var timer: Timer?

    init(banner: Bool = false) {
        self.banner = banner
        super.init(fill: nil)
        isHidden = true
        if !banner { radius = 7 }
        setAccessibilityRole(.group)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    func show(_ message: InlineMessage?) {
        guard message?.id != shown else { return }
        shown = message?.id
        timer?.invalidate()
        subviews.forEach { $0.removeFromSuperview() }
        guard let message else { isHidden = true; return }
        let isError = message.kind == .error
        let text = isError ? errorText : noticeText
        fill = isError ? NSColor(hex: 0xff6b61, alpha: 0.09) : NSColor(hex: 0x5ad394, alpha: 0.1)
        let edge = isError ? NSColor(hex: 0xff6b61, alpha: 0.25) : NSColor(hex: 0x5ad394, alpha: 0.25)
        if banner {
            edges = [.bottom]
            edgeColor = edge
        } else {
            outline = edge
        }
        let close = DismissButton(tint: text, label: message.export != nil ? "Dismiss export result" : banner ? "Dismiss error" : "Dismiss message")
        close.onClick = { [weak self] in self?.dismiss() }

        let row: NSStackView
        if let export = message.export {
            let noun = export.rows == 1 ? "row" : "rows"
            let prefix = label("Exported \(export.rows) filtered \(noun) to ", font: Graphite.ui(12.5), color: noticeText)
            let link = FileLink(export.url.lastPathComponent)
            link.onClick = { [weak self] in self?.onOpen?(export.url) }
            let reveal = GButton("Show in folder", icon: .folder, style: .secondary) { [weak self] in self?.onReveal?(export.url) }
            row = hstack([prefix, link, label(".", font: Graphite.ui(12.5), color: noticeText), spacer(), reveal, close], spacing: 0)
            row.setCustomSpacing(8, after: row.views[3])
            row.setCustomSpacing(5, after: reveal)
        } else {
            let body = NSTextField(wrappingLabelWithString: message.text)
            body.font = Graphite.ui(12.5)
            body.textColor = text
            body.alignment = isError ? .center : .left
            body.isSelectable = true
            body.translatesAutoresizingMaskIntoConstraints = false
            body.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
            body.setContentHuggingPriority(.defaultLow, for: .horizontal)
            // Errors keep a matching gutter on the left so the text centers.
            let gutter = NSView()
            gutter.translatesAutoresizingMaskIntoConstraints = false
            gutter.widthAnchor.constraint(equalToConstant: isError ? 24 : 0).isActive = true
            row = hstack([gutter, body, close], spacing: isError ? 8 : 0)
            row.setCustomSpacing(8, after: body)
        }
        row.alignment = .centerY
        pin(row, insets: banner
            ? NSEdgeInsets(top: 6, left: 14, bottom: 7, right: 14)
            : NSEdgeInsets(top: 6, left: 12, bottom: 6, right: 12))
        isHidden = false
        alphaValue = 0
        NSAnimationContext.runAnimationGroup { context in
            context.duration = 0.15
            animator().alphaValue = 1
        }
        NSAccessibility.post(element: self, notification: .announcementRequested, userInfo: [
            .announcement: message.export.map { "Exported \($0.rows) rows to \($0.url.lastPathComponent)" } ?? message.text,
            .priority: NSAccessibilityPriorityLevel.high.rawValue,
        ])
        if let lifetime = message.lifetime {
            let id = message.id
            timer = Timer.scheduledTimer(withTimeInterval: lifetime - 0.4, repeats: false) { [weak self] _ in
                NSAnimationContext.runAnimationGroup({ context in
                    context.duration = 0.4
                    self?.animator().alphaValue = 0
                }, completionHandler: {
                    guard let self, self.shown == id else { return }
                    self.dismiss()
                })
            }
        }
    }

    private func dismiss() {
        timer?.invalidate()
        shown = nil
        isHidden = true
        onDismiss?()
    }
}

/// Bottom-right toast with a status dot, as the desktop app shows schema
/// refresh summaries.
final class ToastView: PanelView {
    private let message = NSTextField(wrappingLabelWithString: "")
    private let dot = PanelView(fill: Graphite.accent)
    private var timer: Timer?

    init() {
        super.init(fill: Graphite.popover)
        radius = 10
        outline = Graphite.borderStrong
        message.font = Graphite.ui(12.5)
        message.textColor = Graphite.text
        message.preferredMaxLayoutWidth = 440
        dot.radius = 4
        dot.widthAnchor.constraint(equalToConstant: 8).isActive = true
        dot.heightAnchor.constraint(equalToConstant: 8).isActive = true
        let close = DismissButton(tint: Graphite.muted, label: "Dismiss notification")
        close.onClick = { [weak self] in self?.hide() }
        let row = hstack([dot, message, close], spacing: 12)
        row.alignment = .centerY
        pin(row, insets: NSEdgeInsets(top: 10, left: 14, bottom: 10, right: 10))
        widthAnchor.constraint(lessThanOrEqualToConstant: 520).isActive = true
        isHidden = true
        setAccessibilityRole(.staticText)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    func show(_ text: String, success: Bool) {
        message.stringValue = text
        dot.fill = success ? Graphite.success : Graphite.accent
        isHidden = false
        NSAccessibility.post(element: self, notification: .announcementRequested,
                             userInfo: [.announcement: text, .priority: NSAccessibilityPriorityLevel.medium.rawValue])
        timer?.invalidate()
        timer = Timer.scheduledTimer(withTimeInterval: 6, repeats: false) { [weak self] _ in self?.hide() }
    }

    func hide() {
        timer?.invalidate()
        isHidden = true
    }
}
