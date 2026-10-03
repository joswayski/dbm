import SwiftUI

struct WorkspaceView: View {
    @EnvironmentObject var model: AppModel
    @State private var explorer = false
    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                HStack {
                    if !model.databases.isEmpty { Picker("Database", selection: Binding(get: { model.database }, set: model.switchDatabase)) { ForEach(model.databases, id: \.self, content: Text.init) }.accessibilityIdentifier("database-picker") }
                    Spacer(); Button { model.loadSchema() } label: { Image(systemName: "arrow.clockwise") }.accessibilityLabel("Refresh schema")
                }.padding(12).background(Graphite.chrome)
                TextEditor(text: $model.sql).font(.custom("GeistMono-Regular", size: 14)).frame(minHeight: 120, maxHeight: 210).padding(8).background(Graphite.bg).accessibilityLabel(model.active?.engine == .redis ? "Redis command editor" : "SQL editor").accessibilityIdentifier("query-editor")
                HStack { Text("Results · capped at 1,000 rows").font(.caption).foregroundStyle(Graphite.muted); Spacer(); Button("Run", systemImage: "play.fill") { model.run() }.buttonStyle(.borderedProminent).disabled(model.sql.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty).accessibilityIdentifier("run-query") }.padding(12).background(Graphite.chrome)
                GridView(data: model.grid)
                HStack { Button("Refresh results") { model.refresh() }; Spacer(); Text(model.grid.rows.isEmpty ? "0 rows" : "Rows \(model.grid.offset + 1)–\(model.grid.offset + model.grid.rows.count)").font(.caption) }.padding(12)
                if model.selectedTable != nil { HStack { Button("Previous") { model.previous() }.disabled(model.grid.offset == 0); Spacer(); Button("Next") { model.next() }.disabled(!model.grid.hasMore) }.padding(12).background(Graphite.chrome) }
            }
            .navigationTitle(model.active?.name ?? "Query")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .topBarLeading) { Button { model.disconnect() } label: { Image(systemName: "chevron.left") }.accessibilityLabel("Disconnect") }; ToolbarItem(placement: .topBarTrailing) { Button { explorer = true } label: { Label("Explorer", systemImage: "sidebar.left") }.accessibilityIdentifier("explorer") } }
            .sheet(isPresented: $explorer) { ExplorerView() }
        }.tint(Color(hex: model.active?.color ?? "4c9aff"))
    }
}

struct ExplorerView: View {
    @EnvironmentObject var model: AppModel
    @Environment(\.dismiss) var dismiss
    var body: some View { NavigationStack { List { if model.schema.isEmpty { ContentUnavailableView("Nothing to browse", systemImage: "tablecells", description: Text("Refresh the schema or keyspace.")) } else { OutlineGroup(model.schema, children: \.children) { node in Button { if let schema = node.schema, let table = node.table { model.browse(schema: schema, table: table); dismiss() } } label: { Label(node.name, systemImage: node.table == nil ? "folder" : (model.active?.engine == .redis ? "key" : "tablecells")) }.disabled(node.table == nil) } } }.navigationTitle(model.active?.engine == .redis ? "Keys" : "Schema").toolbar { ToolbarItem(placement: .topBarTrailing) { Button("Refresh") { model.loadSchema() } }; ToolbarItem(placement: .cancellationAction) { Button("Done") { dismiss() } } } } }
}

struct GridView: View {
    let data: GridData
    private let width: CGFloat = 150
    var body: some View {
        if data.columns.isEmpty { ContentUnavailableView("No results", systemImage: "tablecells", description: Text("Run a query or select a table or key.")) }
        else { ScrollView([.horizontal, .vertical]) { VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 0) { ForEach(Array(data.columns.enumerated()), id: \.offset) { _, column in Text(column).font(.custom("GeistMono-Regular", size: 12).bold()).frame(width: width, alignment: .leading).padding(.horizontal, 8).frame(height: 36).background(Color(hex: "1a1a1c")) } }
            ForEach(Array(data.rows.enumerated()), id: \.offset) { rowIndex, row in HStack(spacing: 0) { ForEach(Array(data.columns.indices), id: \.self) { index in Text(index < row.count ? display(row[index]) : "").font(.custom("GeistMono-Regular", size: 12)).lineLimit(1).frame(width: width, alignment: .leading).padding(.horizontal, 8).frame(height: 34).overlay(alignment: .bottom) { Divider() } } }.accessibilityElement(children: .ignore).accessibilityLabel(rowLabel(rowIndex: rowIndex, row: row)).accessibilityIdentifier("result-row-\(rowIndex + 1)") }
        } }.accessibilityIdentifier("results-grid") }
    }

    private func rowLabel(rowIndex: Int, row: [Any]) -> String {
        let cells = data.columns.enumerated().map { index, column in
            "\(column): \(index < row.count ? display(row[index]) : "")"
        }
        return (["Row \(rowIndex + 1)"] + cells).joined(separator: ", ")
    }
}
