import Foundation

/// Open tabs kept across an update's restart, as Captures keeps open
/// captures. The app writes them just before the new build replaces it; the
/// restarted app reopens each tab when its profile connects, then deletes
/// the file. Query text, table paging, filters, sort, and staged row edits
/// come back; results are fetched again. Same JSON shape as
/// `apps/native/desktop/src/session.rs`. The file stays on this machine.
enum Session {
    static var url: URL? {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first?
            .appendingPathComponent("io.github.dbm", isDirectory: true)
            .appendingPathComponent("update-session.json")
    }

    static func save(_ session: [String: Any]) {
        guard let url, let data = try? JSONSerialization.data(withJSONObject: session) else { return }
        try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try? data.write(to: url, options: [.atomic, .completeFileProtection])
        try? FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
    }

    /// The saved session, read once: the file is removed whether or not it parses.
    static func take() -> [String: Any]? {
        guard let url, let data = try? Data(contentsOf: url) else { return nil }
        try? FileManager.default.removeItem(at: url)
        return (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
    }

    /// Puts restored edits back on a freshly loaded page's rows, matched by
    /// primary key. Returns how many found no row on this page.
    static func reapply(_ edits: [[String: Any]], to state: TableState) -> Int {
        guard let page = state.page else { return edits.count }
        let keyColumns = page.primaryKey.compactMap { key in page.columns.firstIndex { $0.name == key } }
        var missing = 0
        for edit in edits {
            let key = edit["primaryKey"] as? [Any] ?? []
            let row = keyColumns.isEmpty || key.count != keyColumns.count ? nil : page.rows.firstIndex { row in
                jsonEqual(keyColumns.map { row[$0] }, key)
            }
            guard let row, state.pending[row] == nil else { missing += 1; continue }
            state.pending[row] = PendingRow(json: edit)
        }
        return missing
    }

    static func missingRowsMessage(_ missing: Int) -> String {
        missing == 1
            ? "1 staged edit couldn’t be restored: its row isn’t on this page anymore."
            : "\(missing) staged edits couldn’t be restored: their rows aren’t on this page anymore."
    }
}
