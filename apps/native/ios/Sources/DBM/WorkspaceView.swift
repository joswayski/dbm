import SwiftUI

struct WorkspaceView: View {
    @EnvironmentObject var model: AppModel
    @State private var explorer = false

    var body: some View {
        GeometryReader { proxy in
            HStack(spacing: 0) {
                if proxy.size.width >= 700 { SourceList(close: nil).frame(width: 250) }
                workbench(showExplorer: proxy.size.width < 700)
            }
        }
        .background(Graphite.bg).sheet(isPresented: $explorer) { SourceList(close: { explorer = false }).presentationDetents([.large]) }
        .onChange(of: model.showingTable) { _, table in if table { explorer = false } }
        .onChange(of: model.active?.id) { _, _ in explorer = false }
    }

    private func workbench(showExplorer: Bool) -> some View {
        VStack(spacing: 0) {
            HStack(spacing: 9) {
                Circle().fill(Color(hex: model.active?.color ?? "#4c9aff")).frame(width: 8, height: 8)
                VStack(alignment: .leading, spacing: 1) {
                    Text(model.active?.name ?? "Anybase").font(.custom("Geist-Regular", size: 13).weight(.semibold))
                    Text("\(model.active?.engine.title ?? "") · \(model.database)").font(.custom("Geist-Regular", size: 11)).foregroundStyle(Graphite.faint)
                }
                Spacer()
                if showExplorer { Button { explorer = true } label: { Image(systemName: "sidebar.left").frame(width: 44, height: 44) }.accessibilityLabel("Explorer").accessibilityIdentifier("explorer") }
                Button { model.disconnect() } label: { Image(systemName: "rectangle.portrait.and.arrow.right").frame(width: 44, height: 44) }.accessibilityLabel("Disconnect")
            }.padding(.horizontal, 12).frame(height: 48).background(Graphite.chrome)

            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 1) {
                    TabButton(title: model.active?.engine == .redis ? "Redis" : "SQL", icon: "chevron.left.forwardslash.chevron.right", active: !model.showingTable) { model.showQuery() }
                    if let table = model.selectedTable { TabButton(title: table.table, icon: model.active?.engine == .redis ? "key" : "tablecells", active: model.showingTable) { model.showTable() } }
                }.padding(.horizontal, 8)
            }.frame(height: 38).background(Graphite.chrome).overlay(alignment: .bottom) { Rectangle().fill(Graphite.border).frame(height: 0.5) }

            if !model.showingTable {
                HStack(spacing: 6) {
                    Text(model.active?.engine == .redis ? "Command" : "SQL editor").foregroundStyle(Graphite.secondary).lineLimit(1)
                    Spacer(minLength: 4)
                    Button("Refresh", systemImage: "arrow.clockwise") { model.refresh() }.buttonStyle(SecondaryButton()).disabled(model.lastExecuted == nil).accessibilityIdentifier("refresh-query")
                    Button("Run", systemImage: "play.fill") { model.run() }.buttonStyle(PrimaryButton()).disabled(model.sql.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty).accessibilityIdentifier("run-query")
                }.font(.custom("Geist-Regular", size: 12)).padding(.horizontal, 12).frame(height: 42).graphitePanel()
                TextEditor(text: $model.sql).scrollContentBackground(.hidden).autocorrectionDisabled().textInputAutocapitalization(.never).font(.custom("GeistMono-Regular", size: 12)).frame(minHeight: 112, maxHeight: 170).padding(8).background(Graphite.bg)
                    .accessibilityLabel(model.active?.engine == .redis ? "Redis command editor" : "SQL editor").accessibilityIdentifier("query-editor")
            } else if let table = model.selectedTable {
                HStack { Image(systemName: model.active?.engine == .redis ? "key" : "tablecells").foregroundStyle(Color(hex: model.active?.color ?? "#4c9aff")); Text("\(table.schema).\(table.table)").font(.custom("Geist-Regular", size: 13).weight(.medium)); Spacer(); Button("Refresh", systemImage: "arrow.clockwise") { model.refresh() }.buttonStyle(SecondaryButton()) }
                    .padding(.horizontal, 12).frame(height: 42).graphitePanel()
            }
            GridView(data: model.grid).frame(maxWidth: .infinity, maxHeight: .infinity).clipped()
            HStack(spacing: 12) {
                Text(model.grid.rows.isEmpty ? "0 rows" : "Rows \(model.grid.offset + 1)–\(model.grid.offset + model.grid.rows.count)").foregroundStyle(Graphite.muted)
                Spacer()
                if model.grid.truncated {
                    Text("Results truncated at 1,000 rows").foregroundStyle(Graphite.muted).accessibilityIdentifier("query-truncated")
                }
                if model.showingTable {
                    Button { model.previous() } label: { Image(systemName: "chevron.left").frame(width: 36, height: 44) }.disabled(model.grid.offset == 0).opacity(model.grid.offset == 0 ? 0.4 : 1).accessibilityLabel("Previous")
                    Text("Page \(model.grid.offset / model.grid.limit + 1)")
                    Button { model.next() } label: { Image(systemName: "chevron.right").frame(width: 36, height: 44) }.disabled(!model.grid.hasMore).opacity(model.grid.hasMore ? 1 : 0.4).accessibilityLabel("Next")
                }
            }.font(.custom("Geist-Regular", size: 11)).padding(.horizontal, 12).frame(height: 44).graphitePanel()
        }.buttonStyle(.plain)
    }
}

private struct TabButton: View {
    @EnvironmentObject var model: AppModel
    let title, icon: String; let active: Bool; let action: () -> Void
    var body: some View { Button(action: action) { HStack(spacing: 6) { Image(systemName: icon); Text(title) }.font(.custom("Geist-Regular", size: 12).weight(active ? .medium : .regular)).padding(.horizontal, 12).frame(height: 38).background(active ? Graphite.bg : Graphite.chrome).overlay(alignment: .top) { Rectangle().fill(active ? Color(hex: model.active?.color ?? "#4c9aff") : .clear).frame(height: 2) } }.accessibilityAddTraits(active ? .isSelected : []) }
}

struct SourceList: View {
    @EnvironmentObject var model: AppModel
    let close: (() -> Void)?
    @State private var filter = ""
    @State private var editor: ProfileDraft?
    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack { Image(systemName: "cylinder").foregroundStyle(Graphite.accent); Text("Anybase").font(.custom("Geist-Regular", size: 14).weight(.semibold)); Spacer(); if let close { Button("Done", action: close) } }.padding(12)
            ScrollView {
            VStack(alignment: .leading, spacing: 0) {
            Text("Connections").sectionLabel()
            ForEach(model.profiles.sorted { $0.id == model.active?.id && $1.id != model.active?.id }) { profile in
                Button {
                    if model.active?.id == profile.id { close?() }
                    else if model.isDemo { model.connect(profile); close?() }
                    else { editor = ProfileDraft(profile) }
                } label: { HStack(spacing: 9) { Circle().fill(Color(hex: profile.color)).frame(width: 8, height: 8); VStack(alignment: .leading, spacing: 2) { Text(profile.name); Text("\(profile.engine.title) · \(profile.host)").foregroundStyle(Graphite.faint).font(.custom("Geist-Regular", size: 11)) }; Spacer() }.padding(.horizontal, 10).frame(height: 44).background(model.active?.id == profile.id ? Color.white.opacity(0.06) : .clear).clipShape(RoundedRectangle(cornerRadius: 6)) }.buttonStyle(.plain).accessibilityIdentifier("source-profile-\(profile.id)")
            if model.active?.id == profile.id {
                Divider().overlay(Graphite.border).padding(.vertical, 8)
                Text("Database").sectionLabel()
                Picker("Database", selection: Binding(get: { model.database }, set: model.switchDatabase)) { ForEach(model.databases, id: \.self, content: Text.init) }.pickerStyle(.menu).padding(.horizontal, 8).accessibilityIdentifier("database-picker")
                HStack { Text(model.active?.engine == .redis ? "Keys" : "Schema").sectionLabel(); Spacer(); Button { model.loadSchema() } label: { Image(systemName: "arrow.clockwise").frame(width: 44, height: 44) }.accessibilityLabel("Refresh schema") }
                TextField("Filter schema", text: $filter).graphiteField().padding(8).accessibilityIdentifier("schema-filter")
                SchemaTree(items: model.schema, filter: filter, close: close)
            }
            }
            }
            }.frame(maxHeight: .infinity).accessibilityIdentifier("source-list-scroll")
            Divider().overlay(Graphite.border)
            Button { editor = ProfileDraft() } label: { Label("New connection", systemImage: "plus").frame(maxWidth: .infinity, alignment: .leading).frame(height: 44).padding(.horizontal, 12) }.buttonStyle(.plain)
        }.background(Graphite.sidebar).foregroundStyle(Graphite.text).disabled(model.loading)
            .sheet(item: Binding(get: { editor.map(ConnectionsView.EditorItem.init) }, set: { if $0 == nil { editor = nil } })) { item in ProfileForm(draft: item.draft) }
    }
}

private struct SchemaTree: View {
    @EnvironmentObject var model: AppModel
    let items: [SchemaItem], filter: String
    let close: (() -> Void)?
    @State private var expanded: Set<SchemaItem.ID> = []
    var body: some View { VStack(alignment: .leading, spacing: 2) { ForEach(items) { node in
        if filter.isEmpty || node.name.localizedCaseInsensitiveContains(filter) || node.children?.contains(where: { $0.name.localizedCaseInsensitiveContains(filter) }) == true {
            if let schema = node.schema, let table = node.table { Button { model.browse(schema: schema, table: table); close?() } label: { Label(node.name, systemImage: model.active?.engine == .redis ? "key" : "tablecells").frame(maxWidth: .infinity, alignment: .leading).padding(.leading, 22).frame(height: 36).background(model.showingTable && model.selectedTable?.schema == schema && model.selectedTable?.table == table ? Color(hex: model.active?.color ?? "#4c9aff").opacity(0.18) : .clear) }.buttonStyle(.plain).accessibilityIdentifier("schema-item-\(node.name)") }
            else {
                Button { if expanded.contains(node.id) { expanded.remove(node.id) } else { expanded.insert(node.id) } } label: {
                    HStack(spacing: 6) { Image(systemName: expanded.contains(node.id) ? "chevron.down" : "chevron.right").font(.system(size: 9)).foregroundStyle(Graphite.faint).frame(width: 10); Label(node.name, systemImage: "folder"); Spacer() }.frame(height: 36).padding(.horizontal, 10)
                }.buttonStyle(.plain).accessibilityLabel(node.name).accessibilityValue(expanded.contains(node.id) ? "Expanded" : "Collapsed").accessibilityIdentifier("schema-item-\(node.name)")
                if expanded.contains(node.id) || !filter.isEmpty { SchemaTree(items: node.children ?? [], filter: filter, close: close).padding(.leading, 14) }
            }
        }
    } }.font(.custom("Geist-Regular", size: 12.5)) }
}

struct GridView: View {
    let data: GridData; private let width: CGFloat = 150
    var body: some View {
        if data.columns.isEmpty { VStack(spacing: 10) { Image(systemName: "tablecells"); Text("No results").font(.custom("Geist-Regular", size: 13)); Text("Run a query or choose a table.").foregroundStyle(Graphite.faint).font(.custom("Geist-Regular", size: 11)) }.frame(maxWidth: .infinity, maxHeight: .infinity).background(Graphite.bg) }
        else { ScrollView(.horizontal) { VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 0) { ForEach(Array(data.columns.enumerated()), id: \.offset) { index, column in Text(column).font(.custom("GeistMono-Regular", size: 11).weight(.semibold)).lineLimit(1).frame(width: width, alignment: .leading).padding(.horizontal, 8).frame(height: 34).background(Graphite.gridHeader).accessibilityIdentifier("result-column-\(index)") } }.overlay(alignment: .bottom) { Rectangle().fill(Graphite.border).frame(height: 0.5) }
            ScrollView(.vertical) { LazyVStack(alignment: .leading, spacing: 0) {
                ForEach(Array(data.rows.enumerated()), id: \.offset) { rowIndex, row in HStack(spacing: 0) { ForEach(data.columns.indices, id: \.self) { index in Text(index < row.count ? display(row[index]) : "").foregroundStyle((index < row.count && row[index] is NSNull) ? Graphite.faint : Graphite.text).font(.custom("GeistMono-Regular", size: 12)).lineLimit(1).frame(width: width, alignment: .leading).padding(.horizontal, 8).frame(height: 32).overlay(alignment: .bottom) { Rectangle().fill(Graphite.hairline).frame(height: 0.5) } } }.accessibilityElement(children: .ignore).accessibilityLabel(rowLabel(rowIndex, row)).accessibilityIdentifier("result-row-\(rowIndex + 1)") }
            } }.accessibilityIdentifier("results-rows")
        }.frame(width: CGFloat(data.columns.count) * (width + 16), alignment: .topLeading).frame(maxHeight: .infinity) }.background(Graphite.bg).accessibilityIdentifier("results-grid") }
    }
    private func rowLabel(_ index: Int, _ row: [Any]) -> String { (["Row \(index + 1)"] + data.columns.enumerated().map { "\($0.element): \($0.offset < row.count ? display(row[$0.offset]) : "")" }).joined(separator: ", ") }
}

private struct SecondaryButton: ButtonStyle {
    @Environment(\.isEnabled) private var enabled
    func makeBody(configuration: Configuration) -> some View { configuration.label.font(.custom("Geist-Regular", size: 12)).padding(.horizontal, 12).frame(minHeight: 36).background(configuration.isPressed ? Graphite.controlActive : Graphite.control).foregroundStyle(Graphite.text).clipShape(RoundedRectangle(cornerRadius: 6)).overlay(RoundedRectangle(cornerRadius: 6).stroke(Graphite.borderStrong)).opacity(enabled ? 1 : 0.4) }
}

private struct PrimaryButton: ButtonStyle {
    @Environment(\.isEnabled) private var enabled
    func makeBody(configuration: Configuration) -> some View { configuration.label.font(.custom("Geist-Regular", size: 12).weight(.semibold)).padding(.horizontal, 12).frame(minHeight: 36).background(configuration.isPressed ? Graphite.accent : Graphite.accentStrong).foregroundStyle(.white).clipShape(RoundedRectangle(cornerRadius: 6)).opacity(enabled ? 1 : 0.4) }
}
private extension View { func sectionLabel() -> some View { self.font(.custom("Geist-Regular", size: 11).weight(.semibold)).foregroundStyle(Graphite.muted).padding(.horizontal, 12).padding(.vertical, 5) } }
