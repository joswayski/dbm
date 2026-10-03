import SwiftUI

struct ConnectionsView: View {
    @EnvironmentObject var model: AppModel
    @State private var editor: ProfileDraft?
    @State private var deleting: Profile?
    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 9) { Image(systemName: "cylinder").foregroundStyle(Graphite.accent); Text("DBM").font(.custom("Geist-Regular", size: 17).weight(.semibold)); Spacer(); if model.isDemo { Text("DEMO").font(.custom("Geist-Regular", size: 10).weight(.semibold)).foregroundStyle(Graphite.accent) } }.padding(.horizontal, 16).frame(height: 50).graphitePanel(Graphite.sidebar)
            HStack { Text("Connections").font(.custom("Geist-Regular", size: 11).weight(.semibold)).foregroundStyle(Graphite.muted); Spacer() }.padding(.horizontal, 16).frame(height: 34)
            ScrollView {
                LazyVStack(spacing: 3) {
                    if model.profiles.isEmpty { VStack(spacing: 8) { Image(systemName: "externaldrive"); Text("No connections"); Text("Add a read-only TLS connection to begin.").foregroundStyle(Graphite.faint).font(.custom("Geist-Regular", size: 11)) }.padding(.top, 70) }
                    ForEach(model.profiles) { profile in
                        HStack(spacing: 10) {
                            Circle().stroke(Color(hex: profile.color), lineWidth: 1.5).frame(width: 9, height: 9)
                            Button { editor = ProfileDraft(profile) } label: { VStack(alignment: .leading, spacing: 2) { Text(profile.name).font(.custom("Geist-Regular", size: 13).weight(.medium)); Text("\(profile.engine.title) · \(profile.host):\(profile.port)").font(.custom("Geist-Regular", size: 11)).foregroundStyle(Graphite.faint) }.frame(maxWidth: .infinity, alignment: .leading) }.buttonStyle(.plain).accessibilityIdentifier("edit-\(profile.id)")
                            Button("Connect") { if model.isDemo { model.connect(profile) } else { editor = ProfileDraft(profile) } }.buttonStyle(SecondaryButton()).accessibilityIdentifier("connect-\(profile.id)")
                            Menu { Button("Edit connection") { editor = ProfileDraft(profile) }; Button("Delete connection", role: .destructive) { deleting = profile } } label: { Image(systemName: "ellipsis").frame(width: 34, height: 44) }.accessibilityLabel("Actions for \(profile.name)")
                        }.padding(.horizontal, 12).frame(height: 52).background(Graphite.sidebar).clipShape(RoundedRectangle(cornerRadius: 6))
                    }
                }.padding(.horizontal, 10)
            }
            Button { editor = ProfileDraft() } label: { Label("New connection", systemImage: "plus").frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, 16).frame(height: 48) }.buttonStyle(.plain).accessibilityIdentifier("add-connection").graphitePanel(Graphite.sidebar)
        }.background(Graphite.bg).sheet(item: Binding(get: { editor.map(EditorItem.init) }, set: { if $0 == nil { editor = nil } })) { item in ProfileForm(draft: item.draft) }
            .confirmationDialog("Delete \(deleting?.name ?? "connection")?", isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } })) { Button("Delete connection", role: .destructive) { if let deleting { model.delete(deleting) }; deleting = nil } }
    }
    struct EditorItem: Identifiable { let draft: ProfileDraft; var id: String { draft.id ?? "new" } }
}

struct ProfileForm: View {
    @EnvironmentObject var model: AppModel
    @Environment(\.dismiss) var dismiss
    @Environment(\.scenePhase) private var phase
    @State var draft: ProfileDraft
    @State private var status = ""
    var valid: Bool { !draft.name.isEmpty && !draft.host.isEmpty && (1...65535).contains(draft.port) && !draft.database.isEmpty && (draft.engine == .redis || !draft.username.isEmpty) }
    var body: some View {
        VStack(spacing: 0) {
            HStack { VStack(alignment: .leading, spacing: 2) { Text(draft.engine.title.uppercased()).font(.custom("Geist-Regular", size: 10).weight(.semibold)).foregroundStyle(Graphite.muted); Text(draft.id == nil ? "New connection" : "Edit connection").font(.custom("Geist-Regular", size: 17).weight(.semibold)) }; Spacer(); Button { clearAndDismiss() } label: { Image(systemName: "xmark").frame(width: 44, height: 44) }.accessibilityLabel("Cancel") }.padding(.horizontal, 16).frame(height: 60).graphitePanel()
            ScrollView { VStack(alignment: .leading, spacing: 13) {
                FieldLabel("Database engine") { Picker("Database engine", selection: Binding(get: { draft.engine }, set: { draft.select($0) })) { ForEach(Engine.allCases) { Text($0.title).tag($0) } }.pickerStyle(.segmented).accessibilityIdentifier("engine") }
                FieldLabel("Name") { TextField("Connection name", text: $draft.name).graphiteField().accessibilityIdentifier("name") }
                FieldLabel("Connection color") {
                    ScrollView(.horizontal, showsIndicators: false) { HStack(spacing: 2) { ForEach(Graphite.palette, id: \.self) { hex in Button { draft.color = hex } label: { Circle().fill(Color(hex: hex)).frame(width: 23, height: 23).overlay(Circle().stroke(Graphite.textStrong, lineWidth: draft.color.lowercased() == hex ? 2 : 0).padding(-4)).frame(width: 40, height: 44) }.accessibilityLabel("Use connection color \(hex)") } } }
                    TextField("#RRGGBB", text: $draft.color).textInputAutocapitalization(.never).font(.custom("GeistMono-Regular", size: 11)).graphiteField().accessibilityLabel("Custom connection color")
                }
                HStack(alignment: .top, spacing: 10) { FieldLabel("Host") { TextField("localhost", text: $draft.host).textInputAutocapitalization(.never).graphiteField().accessibilityIdentifier("host") }; FieldLabel("Port") { TextField("Port", value: $draft.port, format: .number).keyboardType(.numberPad).graphiteField() }.frame(width: 105) }
                HStack(alignment: .top, spacing: 10) { FieldLabel(draft.engine == .redis ? "Username (optional)" : "Username") { TextField("Username", text: $draft.username).textInputAutocapitalization(.never).graphiteField().accessibilityIdentifier("username") }; FieldLabel(draft.engine == .redis ? "Database number" : "Database") { TextField("Database", text: $draft.database).textInputAutocapitalization(.never).graphiteField() } }
                FieldLabel("Password · session only") { SecureField("Not saved on this device", text: $draft.password).graphiteField().accessibilityIdentifier("password") }
                HStack { Image(systemName: "lock.shield"); VStack(alignment: .leading, spacing: 2) { Text("Read only · TLS required · System CA"); Text("Password is cleared on disconnect, background, and dismissal.").foregroundStyle(Graphite.faint) } }.font(.custom("Geist-Regular", size: 11)).padding(12).frame(maxWidth: .infinity, alignment: .leading).background(Graphite.control).clipShape(RoundedRectangle(cornerRadius: 6))
                if !status.isEmpty { Text(status).font(.custom("Geist-Regular", size: 12)).foregroundStyle(status == "Connection succeeded" ? Graphite.success : Graphite.danger).accessibilityIdentifier("form-status") }
            }.padding(16) }
            HStack { Button("Test connection") { status = "Testing…"; model.test(draft) { status = $0 ? "Connection succeeded" : "Connection failed" } }.buttonStyle(SecondaryButton()).disabled(!valid || model.loading).accessibilityIdentifier("test-connection"); Spacer(); Button("Save") { model.save(draft) { if $0 { clearAndDismiss() } } }.buttonStyle(SecondaryButton()).disabled(!valid).accessibilityIdentifier("save-connection"); Button("Save & connect") { model.saveAndConnect(draft) { if $0 { clearAndDismiss() } } }.buttonStyle(PrimaryFormButton()).disabled(!valid || model.loading).accessibilityIdentifier("form-connect") }.padding(12).graphitePanel()
        }.background(Graphite.bg).disabled(model.loading).interactiveDismissDisabled(model.loading).overlay { if phase != .active { PrivacyCover() } }.onDisappear { draft.password = "" }
    }
    private func clearAndDismiss() { draft.password = ""; dismiss() }
}

private struct FieldLabel<Content: View>: View { let title: String; let content: Content; init(_ title: String, @ViewBuilder content: () -> Content) { self.title = title; self.content = content() }; var body: some View { VStack(alignment: .leading, spacing: 5) { Text(title).font(.custom("Geist-Regular", size: 11).weight(.medium)).foregroundStyle(Graphite.muted); content }.frame(maxWidth: .infinity, alignment: .leading) } }
private struct SecondaryButton: ButtonStyle {
    @Environment(\.isEnabled) private var enabled
    func makeBody(configuration: Configuration) -> some View { configuration.label.font(.custom("Geist-Regular", size: 12).weight(.medium)).padding(.horizontal, 11).frame(minHeight: 40).background(Graphite.control).overlay(RoundedRectangle(cornerRadius: 6).stroke(Graphite.borderStrong)).clipShape(RoundedRectangle(cornerRadius: 6)).opacity(enabled ? 1 : 0.4) }
}
private struct PrimaryFormButton: ButtonStyle {
    @Environment(\.isEnabled) private var enabled
    func makeBody(configuration: Configuration) -> some View { configuration.label.font(.custom("Geist-Regular", size: 12).weight(.semibold)).padding(.horizontal, 12).frame(minHeight: 40).background(Graphite.accentStrong).foregroundStyle(.white).clipShape(RoundedRectangle(cornerRadius: 6)).opacity(enabled ? 1 : 0.4) }
}
