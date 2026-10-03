import Foundation

enum Engine: String, CaseIterable, Codable, Identifiable {
    case postgres, mysql, redis
    var id: String { rawValue }
    var title: String { self == .postgres ? "PostgreSQL" : self == .mysql ? "MySQL" : "Redis" }
    var defaultPort: Int { self == .postgres ? 5432 : self == .mysql ? 3306 : 6379 }
    var defaultDatabase: String { self == .postgres ? "postgres" : self == .mysql ? "mysql" : "0" }
}

struct Profile: Identifiable, Equatable {
    let id: String
    var name, color, host, username, database: String
    var port: Int
    var engine: Engine
    init(_ json: [String: Any]) {
        id = json["id"] as? String ?? ""
        name = json["name"] as? String ?? "Unnamed"
        color = json["color"] as? String ?? "#4c9aff"
        host = json["host"] as? String ?? ""
        username = json["username"] as? String ?? ""
        database = json["defaultDatabase"] as? String ?? ""
        port = (json["port"] as? NSNumber)?.intValue ?? 0
        engine = Engine(rawValue: json["engine"] as? String ?? "") ?? .postgres
    }
}

struct ProfileDraft {
    var id: String?
    var name = "", color = "#4c9aff", host = "localhost", username = "", database = "postgres", password = ""
    var port = 5432
    var engine = Engine.postgres
    init() {}
    init(_ profile: Profile) {
        id = profile.id; name = profile.name; color = profile.color; host = profile.host
        username = profile.username; database = profile.database; port = profile.port; engine = profile.engine
    }
    mutating func select(_ next: Engine) { engine = next; port = next.defaultPort; database = next.defaultDatabase }
    var input: [String: Any] {
        var value: [String: Any] = ["name": name, "color": color, "engine": engine.rawValue, "host": host,
            "port": port, "username": username, "defaultDatabase": database, "password": password,
            "readOnly": true, "tlsMode": "required", "caCertPath": NSNull()]
        if let id { value["id"] = id }
        return value
    }
}

struct SchemaItem: Identifiable {
    let id = UUID(), name, kind: String
    let schema, table: String?
    let children: [SchemaItem]?
    init(_ json: [String: Any]) {
        name = json["name"] as? String ?? ""; kind = json["kind"] as? String ?? ""
        schema = json["schema"] as? String; table = json["table"] as? String
        let items = (json["children"] as? [[String: Any]] ?? []).map(Self.init)
        children = items.isEmpty ? nil : items
    }
}

struct GridData {
    static let pageSize = 25
    var columns: [String] = [], rows: [[Any]] = [], offset = 0, limit = GridData.pageSize, hasMore = false
    init() {}
    init(query json: [String: Any]) {
        columns = (json["columns"] as? [[String: Any]] ?? []).map { $0["name"] as? String ?? "" }
        rows = json["rows"] as? [[Any]] ?? []
    }
    init(page json: [String: Any]) {
        let metadata = json["metadata"] as? [String: Any] ?? [:]
        columns = (metadata["columns"] as? [[String: Any]] ?? []).map { $0["name"] as? String ?? "" }
        if columns.isEmpty { columns = json["columns"] as? [String] ?? [] }
        rows = json["rows"] as? [[Any]] ?? []; offset = (json["offset"] as? NSNumber)?.intValue ?? 0
        limit = max(1, (json["limit"] as? NSNumber)?.intValue ?? Self.pageSize); hasMore = json["hasMore"] as? Bool ?? false
    }
}

func display(_ value: Any) -> String {
    if value is NSNull { return "NULL" }
    if let string = value as? String { return string }
    if let data = try? JSONSerialization.data(withJSONObject: value, options: [.fragmentsAllowed, .sortedKeys]), let text = String(data: data, encoding: .utf8) { return text }
    return String(describing: value)
}
