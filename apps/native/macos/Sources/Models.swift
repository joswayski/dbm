import AppKit

func dictionary(_ value: Any?) -> [String: Any] { value as? [String: Any] ?? [:] }
func array(_ value: Any?) -> [[String: Any]] { value as? [[String: Any]] ?? [] }
func string(_ value: Any?) -> String { value as? String ?? "" }
func int(_ value: Any?) -> Int { (value as? NSNumber)?.intValue ?? 0 }
func optionalInt(_ value: Any?) -> Int? { (value as? NSNumber)?.intValue }
func bool(_ value: Any?) -> Bool { (value as? NSNumber)?.boolValue ?? false }

/// A stable key for comparing JSON values (numbers, booleans, strings, null).
func jsonKey(_ value: Any) -> String {
    if value is NSNull { return "null" }
    if let data = try? JSONSerialization.data(withJSONObject: value, options: [.fragmentsAllowed, .sortedKeys]),
       let text = String(data: data, encoding: .utf8) { return text }
    return String(describing: value)
}

func jsonEqual(_ left: [Any], _ right: [Any]) -> Bool {
    left.count == right.count && zip(left, right).allSatisfy { jsonKey($0) == jsonKey($1) }
}

/// Grid text: NULL, raw strings, JSON for everything else.
func displayValue(_ value: Any) -> String {
    if value is NSNull { return "NULL" }
    if let text = value as? String { return text }
    if let number = value as? NSNumber, CFGetTypeID(number) == CFBooleanGetTypeID() {
        return number.boolValue ? "true" : "false"
    }
    return jsonKey(value)
}

enum Engine: String {
    case postgres, mysql, redis
    var label: String {
        switch self {
        case .postgres: return "PostgreSQL"
        case .mysql: return "MySQL"
        case .redis: return "Redis"
        }
    }
    var presetName: String { "Local \(label)" }
    var presetUser: String {
        switch self {
        case .postgres: return "postgres"
        case .mysql: return "root"
        case .redis: return "default"
        }
    }
    var defaults: (port: Int, database: String) {
        switch self {
        case .postgres: return (5432, "postgres")
        case .mysql: return (3306, "mysql")
        case .redis: return (6379, "0")
        }
    }
    var urlPlaceholder: String {
        switch self {
        case .postgres: return "postgresql://user:password@host:5432/database"
        case .mysql: return "mysql://user:password@host:3306/database"
        case .redis: return "redis://default:password@host:6379/0"
        }
    }
}

struct Profile {
    let raw: [String: Any]
    var id: String { string(raw["id"]) }
    var name: String { string(raw["name"]) }
    var engine: Engine { Engine(rawValue: string(raw["engine"])) ?? .postgres }
    var host: String { string(raw["host"]) }
    var port: Int { int(raw["port"]) }
    var username: String { string(raw["username"]) }
    var database: String { string(raw["defaultDatabase"]) }
    var colorHex: String { raw["color"] as? String ?? Graphite.defaultConnectionColor }
    var color: NSColor { NSColor(css: colorHex) ?? Graphite.accent }
    var readOnly: Bool { bool(raw["readOnly"]) }
    var tlsMode: String { string(raw["tlsMode"]).isEmpty ? "preferred" : string(raw["tlsMode"]) }
    var caCertPath: String { string(raw["caCertPath"]) }
    var subtitle: String { "\(engine.label) · \(username.isEmpty ? host : "\(username)@\(host)")" }
    var target: String {
        let base = "\(host):\(port)/\(database)"
        return username.isEmpty ? base : "\(username)@\(base)"
    }
}

final class SchemaNode {
    let raw: [String: Any]
    let children: [SchemaNode]
    init(_ raw: [String: Any]) {
        self.raw = raw
        children = array(raw["children"]).map(SchemaNode.init)
    }
    var name: String { string(raw["name"]) }
    var kind: String { string(raw["kind"]) }
    var schema: String? { raw["schema"] as? String }
    var table: String? { raw["table"] as? String }
    var isLeaf: Bool { schema != nil && table != nil }

    /// Keeps nodes whose name contains `query` plus the branches leading to them.
    static func filter(_ nodes: [SchemaNode], _ query: String) -> [SchemaNode] {
        let query = query.trimmingCharacters(in: .whitespaces).lowercased()
        guard !query.isEmpty else { return nodes }
        return nodes.compactMap { node in
            if node.name.lowercased().contains(query) { return node }
            let children = filter(node.children, query)
            guard !children.isEmpty else { return nil }
            var raw = node.raw
            raw["children"] = children.map(\.raw)
            return SchemaNode(raw)
        }
    }
}

struct Column {
    let raw: [String: Any]
    var name: String { string(raw["name"]) }
    var dataType: String { string(raw["dataType"]) }
    var nullable: Bool { bool(raw["nullable"]) }

    var numeric: Bool {
        let word = dataType.lowercased().split(whereSeparator: { !($0.isLetter || $0.isNumber || $0 == "_") }).first.map(String.init) ?? ""
        let names: Set<String> = ["smallint", "integer", "bigint", "tinyint", "mediumint", "bigserial", "smallserial",
                                  "numeric", "decimal", "real", "double", "money"]
        if names.contains(word) { return true }
        for base in ["int", "serial", "float"] where word == base || (word.hasPrefix(base) && word.count == base.count + 1 && word.last!.isNumber) {
            return true
        }
        return false
    }
}

struct TablePage {
    let raw: [String: Any]
    let columns: [Column]
    let primaryKey: [String]
    let rows: [[Any]]
    var total: Int? { optionalInt(raw["totalRows"]) }
    var offset: Int { int(raw["offset"]) }
    var limit: Int { max(1, int(raw["limit"])) }
    var hasMore: Bool { bool(raw["hasMore"]) }

    init(_ raw: [String: Any]) {
        self.raw = raw
        let metadata = dictionary(raw["metadata"])
        columns = array(metadata["columns"]).map(Column.init)
        primaryKey = metadata["primaryKey"] as? [String] ?? []
        rows = raw["rows"] as? [[Any]] ?? []
    }
}

final class PendingRow {
    let original: [Any]
    var changes: [Any]
    let primaryKey: [Any]
    let xmin: Any
    var deleted = false

    init(page: TablePage, row: [Any]) {
        let count = page.columns.count
        original = Array(row.prefix(count))
        changes = original
        primaryKey = page.primaryKey.compactMap { key in
            page.columns.firstIndex(where: { $0.name == key }).map { row[$0] }
        }
        xmin = row.count > count ? (row[count] as? String).map { $0 as Any } ?? NSNull() : NSNull()
    }

    var json: [String: Any] {
        ["original": original, "changes": changes, "primaryKey": primaryKey, "xmin": xmin, "deleted": deleted]
    }
}

struct FilterDraft {
    var column: String
    var operatorName = "contains"
    var value = ""
}

let filterOperators: [(String, String)] = [
    ("equals", "Equals"), ("notEquals", "Does not equal"), ("contains", "Contains"),
    ("startsWith", "Starts with"), ("endsWith", "Ends with"), ("greaterThan", "Greater than"),
    ("greaterThanOrEqual", "Greater than or equal"), ("lessThan", "Less than"),
    ("lessThanOrEqual", "Less than or equal"), ("in", "In list"), ("notIn", "Not in list"),
    ("isNull", "Is null"), ("isNotNull", "Is not null"),
]

func filterNeedsValue(_ name: String) -> Bool { name != "isNull" && name != "isNotNull" }

let maxPreviewRows = 200

final class TableState {
    var page: TablePage?
    /// The request that produced `page`, restored when a later load fails.
    var loaded: [String: Any]?
    var requested: [String: Any]?
    var loading = false
    var pageIndex = 0
    var limit = maxPreviewRows
    var filters: [FilterDraft] = []
    var applied: [[String: Any]] = []
    var order: (column: String, descending: Bool)?
    var selected = IndexSet()
    var pending: [Int: PendingRow] = [:]
    var inspectorOpen = true
    /// Column indexes collapsed to a narrow strip.
    var collapsedColumns = Set<Int>()
    /// A column was dragged away from its default width.
    var columnsResized = false
    /// Inline error, notice, or export result above the grid.
    var message: InlineMessage?

    var dirty: Bool { !pending.isEmpty }

    var effectiveOrder: (column: String, descending: Bool)? {
        if let order { return order }
        guard let key = page?.primaryKey.first else { return nil }
        return (key, false)
    }

    func request(profileID: String, schema: String, table: String) -> [String: Any] {
        [
            "profileId": profileID, "schema": schema, "table": table,
            "offset": pageIndex * limit, "limit": limit, "filters": applied,
            "orderBy": order.map { ["column": $0.column, "descending": $0.descending] as Any } ?? NSNull(),
            "includeTotal": true,
        ]
    }

    func loaded(_ page: TablePage) {
        loading = false
        pageIndex = page.offset / page.limit
        if filters.isEmpty, let first = page.columns.first { filters = [FilterDraft(column: first.name)] }
        loaded = requested
        requested = nil
        selected = IndexSet()
        self.page = page
    }

    func restoreLoaded() {
        loading = false
        requested = nil
        guard let request = loaded else { pageIndex = 0; return }
        limit = max(1, int(request["limit"]))
        pageIndex = int(request["offset"]) / limit
        applied = request["filters"] as? [[String: Any]] ?? []
        if let order = request["orderBy"] as? [String: Any] {
            self.order = (string(order["column"]), bool(order["descending"]))
        } else {
            order = nil
        }
    }

    func values(_ row: Int) -> [Any] {
        pending[row]?.changes ?? page.map { Array($0.rows[row].prefix($0.columns.count)) } ?? []
    }

    func stage(row: Int, column: Int, text: String) {
        guard let page, page.rows.indices.contains(row) else { return }
        let pendingRow = pending[row] ?? PendingRow(page: page, row: page.rows[row])
        let parsed = Helpers.parseCell(text, column: page.columns[column], original: pendingRow.original[column])
        pendingRow.changes[column] = parsed
        if !pendingRow.deleted && jsonEqual(pendingRow.changes, pendingRow.original) {
            pending[row] = nil
        } else {
            pending[row] = pendingRow
        }
    }

    /// Stages deletion of `rows`, or restores them when all are already staged.
    /// Like the desktop's `discardPendingRow`: undoing a delete keeps the
    /// row's edits; otherwise the staged change is dropped.
    func discardPending(_ row: Int) {
        guard let pendingRow = pending[row] else { return }
        if pendingRow.deleted, !jsonEqual(pendingRow.changes, pendingRow.original) {
            pendingRow.deleted = false
        } else {
            pending[row] = nil
        }
    }

    func toggleDelete(_ rows: [Int]) {
        guard let page else { return }
        let restore = rows.allSatisfy { pending[$0]?.deleted == true }
        for row in rows where page.rows.indices.contains(row) {
            if restore {
                guard let pendingRow = pending[row] else { continue }
                pendingRow.deleted = false
                if jsonEqual(pendingRow.changes, pendingRow.original) { pending[row] = nil }
            } else {
                let pendingRow = pending[row] ?? PendingRow(page: page, row: page.rows[row])
                pendingRow.deleted = true
                pending[row] = pendingRow
            }
        }
    }

    var copyableRows: [Int] {
        (0..<(page?.rows.count ?? 0)).filter { pending[$0]?.deleted != true }
    }

    func csv(_ rows: [Int]) -> String {
        guard let page else { return "" }
        return Helpers.csv(columns: page.columns.map(\.name), rows: rows.map(values))
    }
}

enum TabKind { case query, table }

final class WorkTab {
    let id = UUID()
    let profileID: String
    let kind: TabKind
    var title: String
    var schema = "", table = ""
    var sql = ""
    var selection = NSRange(location: 0, length: 0)
    var lastExecuted: String?
    /// A `SELECT * FROM table` result shown in the editable table viewer.
    var embedded: (schema: String, table: String)?
    /// Inline error under the editor, like the desktop query view.
    var queryError: InlineMessage?
    var result: [String: Any]? { didSet { resultID = UUID() } }
    /// Shrunk to a narrow pill in the tab strip until selected again.
    var collapsed = false
    private(set) var resultID = UUID()
    var tableState: TableState?

    init(profileID: String, kind: TabKind, title: String) {
        self.profileID = profileID
        self.kind = kind
        self.title = title
    }

    var target: (schema: String, table: String)? {
        kind == .table ? (schema, table) : embedded
    }

    var dirty: Bool { tableState?.dirty ?? false }
}

/// Shared core rules reached through `dbm_bridge_helper_call`.
enum Helpers {
    struct Target {
        let range: NSRange
        let sql: String
        let selection: Bool
    }

    static func executionTarget(engine: Engine, text: String, selection: NSRange) -> Target? {
        let reply = Bridge.helper([
            "command": "executionTarget", "engine": engine.rawValue, "text": text,
            "selection_from": selection.location, "selection_to": NSMaxRange(selection),
        ])
        guard case .success(let value) = reply, let target = value as? [String: Any] else { return nil }
        let from = int(target["from"]), to = int(target["to"])
        return Target(range: NSRange(location: from, length: to - from), sql: string(target["sql"]),
                      selection: string(target["kind"]) == "selection")
    }

    static func highlight(engine: Engine, text: String) -> [(NSRange, String)] {
        guard case .success(let value) = Bridge.helper(["command": "highlight", "engine": engine.rawValue, "text": text]) else { return [] }
        return array(value).map { token in
            let from = int(token["from"]), to = int(token["to"])
            return (NSRange(location: from, length: to - from), string(token["kind"]))
        }
    }

    /// The common prefix and suffix of two values and what changed between.
    static func inlineDiff(before: String, after: String) -> (prefix: String, removed: String, added: String, suffix: String) {
        guard case .success(let value) = Bridge.helper(["command": "inlineDiff", "before": before, "after": after]) else {
            return ("", before, after, "")
        }
        let diff = dictionary(value)
        return (string(diff["prefix"]), string(diff["removed"]), string(diff["added"]), string(diff["suffix"]))
    }

    static func completions(engine: Engine, prefix: String) -> [String] {
        guard case .success(let value) = Bridge.helper(["command": "completions", "engine": engine.rawValue, "prefix": prefix]) else { return [] }
        return value as? [String] ?? []
    }

    static func requiresConfirmation(engine: Engine, text: String) -> Bool {
        guard case .success(let value) = Bridge.helper(["command": "requiresConfirmation", "engine": engine.rawValue, "text": text]) else { return false }
        return bool(value)
    }

    static func resolveFullTableSelect(_ text: String, tree: [SchemaNode]) -> (schema: String, table: String)? {
        guard case .success(let value) = Bridge.helper(["command": "resolveFullTableSelect", "text": text, "tree": tree.map(\.raw)]),
              let target = value as? [String: Any] else { return nil }
        return (string(target["schema"]), string(target["table"]))
    }

    static func parseCell(_ text: String, column: Column, original: Any) -> Any {
        guard case .success(let value) = Bridge.helper(["command": "parseCell", "text": text, "column": column.raw, "original": original]) else { return text }
        return value
    }

    static func editableText(_ value: Any) -> String {
        guard case .success(let text) = Bridge.helper(["command": "editableText", "value": value]) else { return displayValue(value) }
        return string(text)
    }

    static func csv(columns: [String], rows: [[Any]]) -> String {
        guard case .success(let text) = Bridge.helper(["command": "csv", "columns": columns, "rows": rows]) else { return "" }
        return string(text)
    }

    static func parseConnectionURL(_ url: String) -> Result<[String: Any], Error> {
        Bridge.helper(["command": "parseConnectionUrl", "url": url]).map { dictionary($0) }
    }
}

/// Text values above 64 KiB are previewed, not edited in place: laying out
/// megabytes in a text field is slow. Returns a size like "1.0 MiB".
func largeValueSize(_ value: Any?) -> String? {
    guard let text = value as? String else { return nil }
    let bytes = text.utf8.count
    guard bytes > 64 * 1024 else { return nil }
    return bytes >= 1024 * 1024 ? String(format: "%.1f MiB", Double(bytes) / 1_048_576) : "\(bytes / 1024) KiB"
}
