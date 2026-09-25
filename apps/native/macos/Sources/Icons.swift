import AppKit

/// Stroke icons on a 16 pt grid, matching `Icon.tsx` and the egui host.
enum Icon {
    case plus, more, database, refresh, table, key, view, folder
    case chevronRight, chevronDown, chevronUp, chevronLeft
    case play, close, copy, download, inspector, check, alert, code, sortUp, sortDown, search, trash, undo, sidebar
    case pencil, expand, collapse, filter

    /// Draws into the current context, in a flipped 16×16 space scaled to `rect`.
    func draw(in rect: NSRect, color: NSColor) {
        let scale = min(rect.width, rect.height) / 16
        let origin = NSPoint(x: rect.midX - 8 * scale, y: rect.midY - 8 * scale)
        let flipped = NSGraphicsContext.current?.isFlipped ?? true
        func p(_ x: CGFloat, _ y: CGFloat) -> NSPoint {
            let py = flipped ? origin.y + y * scale : origin.y + (16 - y) * scale
            return NSPoint(x: origin.x + x * scale, y: py)
        }
        func r(_ x: CGFloat, _ y: CGFloat, _ w: CGFloat, _ h: CGFloat) -> NSRect {
            let a = p(x, y), b = p(x + w, y + h)
            return NSRect(x: min(a.x, b.x), y: min(a.y, b.y), width: abs(b.x - a.x), height: abs(b.y - a.y))
        }
        color.setStroke()
        color.setFill()
        let path = NSBezierPath()
        path.lineWidth = 1.5 * max(scale, 0.75)
        path.lineCapStyle = .round
        path.lineJoinStyle = .round
        func line(_ points: NSPoint...) {
            path.move(to: points[0])
            points.dropFirst().forEach { path.line(to: $0) }
        }
        func dot(_ x: CGFloat, _ y: CGFloat, _ radius: CGFloat) {
            let center = p(x, y)
            NSBezierPath(ovalIn: NSRect(x: center.x - radius * scale, y: center.y - radius * scale,
                                        width: 2 * radius * scale, height: 2 * radius * scale)).fill()
        }
        func fillPolygon(_ points: NSPoint...) {
            let shape = NSBezierPath()
            shape.move(to: points[0])
            points.dropFirst().forEach { shape.line(to: $0) }
            shape.close()
            shape.fill()
        }
        switch self {
        case .plus: line(p(8, 3), p(8, 13)); line(p(3, 8), p(13, 8))
        case .more: [3.5, 8, 12.5].forEach { dot($0, 8, 1.2) }
        case .database:
            path.appendOval(in: r(3, 2.2, 10, 3.6))
            line(p(3, 4), p(3, 12)); line(p(13, 4), p(13, 12))
            for y: CGFloat in [8, 12] {
                path.move(to: p(3, y))
                path.curve(to: p(13, y), controlPoint1: p(3, y + 2.4), controlPoint2: p(13, y + 2.4))
            }
        case .refresh:
            // Grid coordinates grow downward, as in the egui host.
            let points = (0...20).map { i -> (CGFloat, CGFloat) in
                let angle = (-40 + CGFloat(i) * 14) * .pi / 180
                return (8 + 4.8 * cos(angle), 8 + 4.8 * sin(angle))
            }
            path.move(to: p(points[0].0, points[0].1))
            points.dropFirst().forEach { path.line(to: p($0.0, $0.1)) }
            let end = points[points.count - 1]
            line(p(end.0, end.1), p(end.0 + 0.4, end.1 - 3))
            line(p(end.0, end.1), p(end.0 + 3, end.1 - 0.4))
        case .table:
            path.appendRoundedRect(r(2.5, 3, 11, 10), xRadius: 1.5 * scale, yRadius: 1.5 * scale)
            line(p(2.5, 6.5), p(13.5, 6.5)); line(p(6.5, 6.5), p(6.5, 13))
        case .key:
            path.appendOval(in: r(2.9, 5.4, 5.2, 5.2))
            line(p(8.1, 8), p(13.5, 8)); line(p(11.5, 8), p(11.5, 10.5)); line(p(13.5, 8), p(13.5, 10))
        case .view:
            path.appendRoundedRect(r(2.5, 3, 11, 10), xRadius: 1.5 * scale, yRadius: 1.5 * scale)
            line(p(2.5, 6.5), p(13.5, 6.5))
            path.appendOval(in: r(6.4, 8.2, 3.2, 3.2))
        case .folder:
            line(p(2.5, 4.5), p(6.5, 4.5), p(7.8, 6), p(13.5, 6), p(13.5, 12.5), p(2.5, 12.5), p(2.5, 4.5))
        case .chevronRight: line(p(6, 4), p(10, 8), p(6, 12))
        case .chevronLeft: line(p(10, 4), p(6, 8), p(10, 12))
        case .chevronDown: line(p(4, 6), p(8, 10), p(12, 6))
        case .chevronUp: line(p(4, 10), p(8, 6), p(12, 10))
        case .play: fillPolygon(p(5, 3.5), p(12.5, 8), p(5, 12.5))
        case .close: line(p(4.5, 4.5), p(11.5, 11.5)); line(p(11.5, 4.5), p(4.5, 11.5))
        case .copy:
            path.appendRoundedRect(r(5.5, 5.5, 7.5, 7.5), xRadius: 1.5 * scale, yRadius: 1.5 * scale)
            line(p(3, 10.5), p(3, 3), p(10.5, 3))
        case .download: line(p(8, 2.5), p(8, 10)); line(p(4.5, 7), p(8, 10.5), p(11.5, 7)); line(p(3, 13), p(13, 13))
        case .inspector:
            path.appendRoundedRect(r(2.5, 3, 11, 10), xRadius: 1.5 * scale, yRadius: 1.5 * scale)
            line(p(9.5, 3), p(9.5, 13))
        case .sidebar:
            path.appendRoundedRect(r(2.5, 3, 11, 10), xRadius: 1.5 * scale, yRadius: 1.5 * scale)
            line(p(6.5, 3), p(6.5, 13))
        case .check: line(p(3.5, 8.5), p(6.5, 11.5), p(12.5, 4.5))
        case .alert:
            line(p(8, 2.5), p(14, 13), p(2, 13), p(8, 2.5))
            line(p(8, 6.5), p(8, 9.2)); dot(8, 11.1, 0.8)
        case .code: line(p(6, 4.5), p(2.5, 8), p(6, 11.5)); line(p(10, 4.5), p(13.5, 8), p(10, 11.5))
        case .sortUp: fillPolygon(p(8, 5), p(11.5, 10.5), p(4.5, 10.5))
        case .sortDown: fillPolygon(p(4.5, 5.5), p(11.5, 5.5), p(8, 11))
        case .search: path.appendOval(in: r(3, 3, 7.5, 7.5)); line(p(9.3, 9.3), p(13, 13))
        case .filter: line(p(2, 3.3), p(14, 3.3), p(9.3, 8.7), p(9.3, 12.7), p(6.7, 11.3), p(6.7, 8.7), p(2, 3.3))
        case .trash:
            line(p(3, 4.5), p(13, 4.5)); line(p(6.5, 4.5), p(6.5, 3), p(9.5, 3), p(9.5, 4.5))
            line(p(4.5, 4.5), p(5.2, 13), p(10.8, 13), p(11.5, 4.5))
        case .pencil:
            line(p(3, 13), p(3.6, 10.2), p(10.8, 3), p(13, 5.2), p(5.8, 12.4), p(3, 13))
            line(p(9.3, 4.5), p(11.5, 6.7))
        case .expand:
            line(p(3, 8), p(13, 8)); line(p(10, 5), p(13, 8), p(10, 11)); line(p(3, 4), p(3, 12))
        case .collapse:
            line(p(3, 8), p(13, 8)); line(p(6, 5), p(3, 8), p(6, 11)); line(p(13, 4), p(13, 12))
        case .undo:
            line(p(5.5, 3.5), p(3, 6), p(5.5, 8.5))
            path.move(to: p(3, 6))
            path.curve(to: p(8, 13), controlPoint1: p(12, 6), controlPoint2: p(14, 13))
        }
        path.stroke()
    }

    func image(size: CGFloat = 14, color: NSColor) -> NSImage {
        let image = NSImage(size: NSSize(width: size, height: size), flipped: true) { rect in
            self.draw(in: rect, color: color)
            return true
        }
        image.isTemplate = false
        return image
    }
}

/// A non-interactive icon.
final class IconView: NSView {
    var icon: Icon { didSet { needsDisplay = true } }
    var color: NSColor { didSet { needsDisplay = true } }

    init(_ icon: Icon, size: CGFloat = 14, color: NSColor = Graphite.muted) {
        self.icon = icon
        self.color = color
        super.init(frame: NSRect(x: 0, y: 0, width: size, height: size))
        translatesAutoresizingMaskIntoConstraints = false
        widthAnchor.constraint(equalToConstant: size).isActive = true
        heightAnchor.constraint(equalToConstant: size).isActive = true
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override var isFlipped: Bool { true }

    override func draw(_ dirtyRect: NSRect) {
        icon.draw(in: bounds, color: color)
    }
}
