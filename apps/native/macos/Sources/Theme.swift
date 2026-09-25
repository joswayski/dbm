import AppKit
import CoreText

/// Graphite design tokens (`docs/design-system.md`).
enum Graphite {
    static let bg = NSColor(hex: 0x161618)
    static let chrome = NSColor(hex: 0x1c1c1e)
    static let sidebar = NSColor(hex: 0x1f1f21)
    static let gridHeader = NSColor(hex: 0x1a1a1c)
    static let control = NSColor(hex: 0x2a2a2d)
    static let controlHover = NSColor(hex: 0x323235)
    static let controlActive = NSColor(hex: 0x3a3a3d)
    static let popover = NSColor(hex: 0x262629)
    static let hairline = NSColor(hex: 0x202023)
    static let border = NSColor(hex: 0x2a2a2d)
    static let borderStrong = NSColor(hex: 0x353538)
    static let textStrong = NSColor(hex: 0xf5f5f7)
    static let text = NSColor(hex: 0xe8e8ea)
    static let secondary = NSColor(hex: 0xc7c7cc)
    static let muted = NSColor(hex: 0xa0a0a6)
    static let faint = NSColor(hex: 0x808087)
    static let accent = NSColor(hex: 0x4c9aff)
    static let accentStrong = NSColor(hex: 0x3b78c7)
    static let accentText = NSColor(hex: 0x7db6ff)
    static let accentSoft = NSColor(hex: 0x4c9aff, alpha: 0.14)
    static let editSurface = NSColor(hex: 0x10192a)
    static let modified = NSColor(hex: 0xf0b14c)
    static let modifiedSoft = NSColor(hex: 0xf0b14c, alpha: 0.12)
    static let success = NSColor(hex: 0x5ad394)
    static let danger = NSColor(hex: 0xff8a80)
    static let dangerSoft = NSColor(hex: 0xff6b61, alpha: 0.09)
    static let hoverWash = NSColor(white: 1, alpha: 0.03)

    static let connectionColors = ["#4c9aff", "#ff9f43", "#3dd6c6", "#b48cff", "#ff6b8a", "#7ed957", "#f0b14c", "#8e8e93"]
    static let defaultConnectionColor = "#4c9aff"

    static let rowHeight: CGFloat = 32
    static let headerHeight: CGFloat = 34
    static let toolbarHeight: CGFloat = 44
    static let pendingBarHeight: CGFloat = 48
    static let statusBarHeight: CGFloat = 28
    static let inspectorWidth: CGFloat = 300

    // SQL highlighting, matching the egui host.
    static let keyword = NSColor(hex: 0xc69cff)
    static let string = NSColor(hex: 0x9fd88a)
    static let number = NSColor(hex: 0xf0b14c)

    /// Registers the bundled Geist fonts once; falls back to the system font.
    static func registerFonts() {
        for name in ["geist", "geist-mono"] {
            if let url = Bundle.main.url(forResource: name, withExtension: "ttf") {
                CTFontManagerRegisterFontsForURL(url as CFURL, .process, nil)
            }
        }
    }

    static func ui(_ size: CGFloat, _ weight: NSFont.Weight = .regular) -> NSFont {
        font(family: "Geist", size: size, weight: weight) ?? .systemFont(ofSize: size, weight: weight)
    }

    static func mono(_ size: CGFloat, _ weight: NSFont.Weight = .regular) -> NSFont {
        font(family: "Geist Mono", size: size, weight: weight)
            ?? .monospacedSystemFont(ofSize: size, weight: weight)
    }

    private static func font(family: String, size: CGFloat, weight: NSFont.Weight) -> NSFont? {
        let descriptor = NSFontDescriptor(fontAttributes: [
            .family: family,
            .traits: [NSFontDescriptor.TraitKey.weight: weight.rawValue],
        ])
        guard let font = NSFont(descriptor: descriptor, size: size), font.familyName == family else { return nil }
        return font
    }
}

extension NSColor {
    convenience init(hex: Int, alpha: CGFloat = 1) {
        self.init(srgbRed: CGFloat((hex >> 16) & 255) / 255,
                  green: CGFloat((hex >> 8) & 255) / 255,
                  blue: CGFloat(hex & 255) / 255, alpha: alpha)
    }

    convenience init?(css: String?) {
        guard let css, css.hasPrefix("#"), css.count == 7,
              let value = Int(css.dropFirst(), radix: 16) else { return nil }
        self.init(hex: value)
    }

    var cssHex: String {
        let color = usingColorSpace(.sRGB) ?? self
        return String(format: "#%02x%02x%02x",
                      Int(round(color.redComponent * 255)),
                      Int(round(color.greenComponent * 255)),
                      Int(round(color.blueComponent * 255)))
    }
}

/// A view with top-left origin, a solid background, and optional hairline edges.
class FlippedView: NSView {
    override var isFlipped: Bool { true }
}

class PanelView: NSView {
    var fill: NSColor? { didSet { needsDisplay = true } }
    var edges: Edges = [] { didSet { needsDisplay = true } }
    var edgeColor = Graphite.border
    var radius: CGFloat = 0 { didSet { needsDisplay = true } }
    var outline: NSColor? { didSet { needsDisplay = true } }

    init(fill: NSColor?, edges: Edges = []) {
        self.fill = fill
        self.edges = edges
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override var isFlipped: Bool { true }

    override func draw(_ dirtyRect: NSRect) {
        if radius > 0 {
            let path = NSBezierPath(roundedRect: bounds.insetBy(dx: 0.5, dy: 0.5), xRadius: radius, yRadius: radius)
            if let fill { fill.setFill(); path.fill() }
            if let outline { outline.setStroke(); path.lineWidth = 1; path.stroke() }
        } else if let fill {
            fill.setFill()
            bounds.fill()
        }
        edgeColor.setFill()
        if edges.contains(.top) { NSRect(x: 0, y: 0, width: bounds.width, height: 1).fill() }
        if edges.contains(.bottom) { NSRect(x: 0, y: bounds.height - 1, width: bounds.width, height: 1).fill() }
        if edges.contains(.left) { NSRect(x: 0, y: 0, width: 1, height: bounds.height).fill() }
        if edges.contains(.right) { NSRect(x: bounds.width - 1, y: 0, width: 1, height: bounds.height).fill() }
    }
}

/// Hairline edges of a flipped panel.
struct Edges: OptionSet {
    let rawValue: Int
    static let left = Edges(rawValue: 1)
    static let right = Edges(rawValue: 2)
    static let top = Edges(rawValue: 4)
    static let bottom = Edges(rawValue: 8)
}
