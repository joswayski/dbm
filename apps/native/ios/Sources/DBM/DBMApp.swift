import SwiftUI

@main
struct DBMApp: App {
    @StateObject private var model = AppModel()
    @Environment(\.scenePhase) private var phase
    var body: some Scene {
        WindowGroup {
            RootView().id(model.privacyEpoch).environmentObject(model).preferredColorScheme(.dark)
                .overlay { if phase != .active { PrivacyCover() } }
                .task { if phase == .active { model.start() } }
                .onChange(of: phase) { _, next in
                    if next == .background { model.background() }
                    if next == .active { model.start() }
                }
        }
    }
}

struct PrivacyCover: View {
    var body: some View { ZStack { Graphite.bg.ignoresSafeArea(); VStack(spacing: 12) { Image(systemName: "lock.fill").font(.largeTitle); Text("DBM Locked").font(.custom("SpaceMono-Bold", size: 20)) } }.accessibilityLabel("DBM content hidden") }
}

struct RootView: View {
    @EnvironmentObject var model: AppModel
    var body: some View {
        ZStack {
            Graphite.bg.ignoresSafeArea()
            Group { if model.route == .connections { ConnectionsView() } else { WorkspaceView() } }.disabled(model.loading)
            if model.loading { ProgressView().padding(20).background(.ultraThinMaterial).clipShape(RoundedRectangle(cornerRadius: 12)).accessibilityLabel("Loading") }
        }
        .font(.custom("SpaceMono-Regular", size: 13)).foregroundStyle(Graphite.text)
        .alert("DBM", isPresented: Binding(get: { model.error != nil }, set: { if !$0 { model.error = nil } })) { Button("OK") { model.error = nil } } message: { Text(model.error ?? "") }
    }
}
