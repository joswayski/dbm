import AppKit

enum Graphite {
    static let bg = NSColor(hex: 0x161618), chrome = NSColor(hex: 0x1c1c1e)
    static let sidebar = NSColor(hex: 0x1f1f21), control = NSColor(hex: 0x2a2a2d)
    static let border = NSColor(hex: 0x353538), text = NSColor(hex: 0xe8e8ea)
    static let muted = NSColor(hex: 0xa0a0a6), accent = NSColor(hex: 0x4c9aff)
    static let modified = NSColor(hex: 0xf0b14c), danger = NSColor(hex: 0xff8a80)
}

extension NSColor {
    convenience init(hex: Int) {
        self.init(srgbRed: CGFloat((hex >> 16) & 255) / 255,
                  green: CGFloat((hex >> 8) & 255) / 255,
                  blue: CGFloat(hex & 255) / 255, alpha: 1)
    }
    convenience init?(css: String?) {
        guard let css, css.hasPrefix("#"), let value = Int(css.dropFirst(), radix: 16) else { return nil }
        self.init(hex: value)
    }
}

func dictionary(_ value: Any) -> [String: Any]? { value as? [String: Any] }
func array(_ value: Any?) -> [[String: Any]] { value as? [[String: Any]] ?? [] }
func string(_ value: Any?) -> String { value as? String ?? "" }
func int(_ value: Any?) -> Int { (value as? NSNumber)?.intValue ?? 0 }
func bool(_ value: Any?) -> Bool { (value as? NSNumber)?.boolValue ?? false }

final class Profile {
    var raw: [String: Any]
    init(_ raw: [String: Any]) { self.raw = raw }
    var id: String { string(raw["id"]) }
    var name: String { string(raw["name"]) }
    var engine: String { string(raw["engine"]) }
    var database: String { string(raw["defaultDatabase"]) }
    var color: NSColor { NSColor(css: raw["color"] as? String) ?? Graphite.accent }
    var readOnly: Bool { bool(raw["readOnly"]) }
}

final class SchemaItem {
    let raw: [String: Any]
    let children: [SchemaItem]
    init(_ raw: [String: Any]) {
        self.raw = raw
        children = array(raw["children"]).map(SchemaItem.init)
    }
    var name: String { string(raw["name"]) }
    var schema: String { string(raw["schema"]) }
    var table: String { string(raw["table"]) }
    var kind: String { string(raw["kind"]) }
}

enum TabKind: Equatable { case query, table }

final class WorkTab {
    let id = UUID(), profileID: String, kind: TabKind
    var title: String, schema = "", table = "", sql = "", lastExecuted = ""
    var columns: [[String: Any]] = [], rows: [[Any]] = [], metadata: [String: Any] = [:]
    var offset = 0, limit = 100, total: Int?, hasMore = false
    var filterColumn = "", filterOperator = "contains", filterValue = ""
    var orderColumn = "", descending = false
    var mutations: [Int: [String: Any]] = [:]
    var requestToken = UUID()
    var busy = false

    init(profileID: String, kind: TabKind, title: String) {
        self.profileID = profileID; self.kind = kind; self.title = title
    }
    var dirty: Bool { !mutations.isEmpty }
}

func jsonDisplay(_ value: Any) -> String {
    if value is NSNull { return "NULL" }
    if let value = value as? String { return value }
    if let data = try? JSONSerialization.data(withJSONObject: value, options: [.fragmentsAllowed, .sortedKeys]),
       let value = String(data: data, encoding: .utf8) { return value }
    return String(describing: value)
}

func jsonValue(_ text: String, matching original: Any) -> Any {
    if original is NSNull && text == "NULL" { return NSNull() }
    if original is NSNumber {
        if text == "true" { return true }
        if text == "false" { return false }
        if let number = Decimal(string: text) { return NSDecimalNumber(decimal: number) }
    }
    return text
}

final class QueryTextView: NSTextView {
    var runAction: (() -> Void)?
    override func keyDown(with event: NSEvent) {
        if event.modifierFlags.intersection(.deviceIndependentFlagsMask) == .command,
           event.keyCode == 36 { runAction?(); return }
        super.keyDown(with: event)
    }
}
