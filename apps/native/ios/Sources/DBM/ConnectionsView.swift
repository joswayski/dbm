import SwiftUI

struct ConnectionsView: View {
    @EnvironmentObject var model: AppModel
    @State private var editor: ProfileDraft?
    @State private var deleting: Profile?
    var body: some View {
        NavigationStack {
            Group {
                if model.profiles.isEmpty { ContentUnavailableView("No connections", systemImage: "externaldrive", description: Text("Add a read-only TLS connection to begin.")) }
                else { List(model.profiles) { profile in
                    HStack(spacing: 12) {
                        Circle().fill(Color(hex: profile.color)).frame(width: 12, height: 12)
                        Button { editor = ProfileDraft(profile) } label: {
                            VStack(alignment: .leading) { Text(profile.name).font(.custom("Geist-Regular", size: 16).weight(.semibold)); Text("\(profile.engine.title) · \(profile.host):\(profile.port)").foregroundStyle(Graphite.muted) }
                        }.buttonStyle(.plain).accessibilityIdentifier("edit-\(profile.id)")
                        Spacer()
                        Button("Connect") { if model.isDemo { model.connect(profile) } else { editor = ProfileDraft(profile) } }.buttonStyle(.borderedProminent).accessibilityIdentifier("connect-\(profile.id)")
                    }.swipeActions { Button("Delete", role: .destructive) { deleting = profile } }
                }.scrollContentBackground(.hidden) }
            }
            .navigationTitle("Connections")
            .toolbar { ToolbarItem(placement: .topBarTrailing) { Button { editor = ProfileDraft() } label: { Label("Add connection", systemImage: "plus") }.accessibilityIdentifier("add-connection") } }
            .safeAreaInset(edge: .top) { if model.isDemo { Text("DEMO — local Rust fixture").font(.caption.bold()).frame(maxWidth: .infinity).padding(6).background(Graphite.accent).accessibilityIdentifier("demo-banner") } }
            .sheet(item: Binding(get: { editor.map(EditorItem.init) }, set: { if $0 == nil { editor = nil } })) { item in ProfileForm(draft: item.draft) }
            .confirmationDialog("Delete \(deleting?.name ?? "connection")?", isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } }), titleVisibility: .visible) { Button("Delete connection", role: .destructive) { if let deleting { model.delete(deleting) }; deleting = nil }; Button("Cancel", role: .cancel) {} } message: { Text("Local metadata and history will be removed. Passwords are never stored by the mobile app.") }
        }
    }
    struct EditorItem: Identifiable { let draft: ProfileDraft; var id: String { draft.id ?? "new" } }
}

struct ProfileForm: View {
    @EnvironmentObject var model: AppModel
    @Environment(\.dismiss) var dismiss
    @Environment(\.scenePhase) private var phase
    @State var draft: ProfileDraft
    @State private var status = ""
    var valid: Bool { !draft.name.isEmpty && !draft.host.isEmpty && draft.port > 0 && draft.port <= 65535 && !draft.database.isEmpty && (draft.engine == .redis || !draft.username.isEmpty) }
    var body: some View {
        NavigationStack { Form {
            Picker("Database", selection: Binding(get: { draft.engine }, set: { draft.select($0) })) { ForEach(Engine.allCases) { Text($0.title).tag($0) } }.pickerStyle(.segmented).accessibilityIdentifier("engine")
            TextField("Name", text: $draft.name).accessibilityIdentifier("name")
            TextField("Color (#RRGGBB)", text: $draft.color).textInputAutocapitalization(.never)
            TextField("Host", text: $draft.host).textInputAutocapitalization(.never).accessibilityIdentifier("host")
            TextField("Port", value: $draft.port, format: .number).keyboardType(.numberPad)
            TextField("Username", text: $draft.username).textInputAutocapitalization(.never).accessibilityIdentifier("username")
            TextField(draft.engine == .redis ? "Database number" : "Database", text: $draft.database).textInputAutocapitalization(.never)
            SecureField("Password (session only)", text: $draft.password).accessibilityIdentifier("password")
            LabeledContent("Security", value: "Read only · TLS required · System CA")
            Text("Passwords are not saved. Enter yours again after disconnecting or leaving DBM.").font(.caption).foregroundStyle(Graphite.muted)
            if !status.isEmpty { Text(status).foregroundStyle(status == "Connection succeeded" ? .green : Graphite.danger).accessibilityIdentifier("form-status") }
            Button("Test connection") { status = "Testing…"; model.test(draft) { status = $0 ? "Connection succeeded" : "Connection failed" } }.disabled(!valid || model.loading).accessibilityIdentifier("test-connection")
            Button("Connect") { model.saveAndConnect(draft) { if $0 { draft.password = ""; dismiss() } } }.disabled(!valid || model.loading).accessibilityIdentifier("form-connect")
        }.navigationTitle(draft.id == nil ? "New connection" : "Edit connection").navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Cancel") { draft.password = ""; dismiss() } }; ToolbarItem(placement: .confirmationAction) { Button("Save") { model.save(draft) { if $0 { draft.password = ""; dismiss() } } }.disabled(!valid).accessibilityIdentifier("save-connection") } }
        }.disabled(model.loading).interactiveDismissDisabled(model.loading)
            .overlay { if phase != .active { PrivacyCover() } }
            .onDisappear { draft.password = "" }
    }
}
