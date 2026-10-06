import XCTest
import UIKit
import SwiftUI
@testable import DBM

final class ModelsTests: XCTestCase {
    func testBundledFontsUseRegisteredPostScriptNames() {
        for name in ["SpaceMono-Regular", "SpaceMono-Bold", "SpaceMono-Italic", "SpaceMono-BoldItalic"] {
            XCTAssertEqual(UIFont(name: name, size: 15)?.familyName, "Space Mono", name)
        }
    }
    func testAppMetadataPreservesNativeDisplayAndPrivateNetworkUsage() {
        XCTAssertNotNil(Bundle.main.object(forInfoDictionaryKey: "UILaunchScreen") as? [String: Any])
        let networkUsage = Bundle.main.object(forInfoDictionaryKey: "NSLocalNetworkUsageDescription") as? String
        XCTAssertEqual(networkUsage, "Anybase connects directly to the databases you configure on your private network.")
    }
    func testMobileProfileAlwaysUsesSafeSettings() {
        var draft = ProfileDraft(); draft.password = "secret"
        XCTAssertEqual(draft.input["readOnly"] as? Bool, true)
        XCTAssertEqual(draft.input["tlsMode"] as? String, "required")
        XCTAssertTrue(draft.input["caCertPath"] is NSNull)
    }
    func testQueryColumnsAreObjectsAndTableFallbackIsStrings() {
        XCTAssertEqual(GridData(query: ["columns": [["name": "id"]], "rows": [[1]]]).columns, ["id"])
        XCTAssertEqual(GridData(page: ["columns": ["key"], "rows": [["a"]]]).columns, ["key"])
    }
    func testSchemaIdentitySurvivesRefreshAndDistinguishesSchemas() {
        let json: [String: Any] = ["name": "public", "kind": "schema", "schema": "public", "children": [
            ["name": "users", "kind": "table", "schema": "public", "table": "users"]
        ]]
        let original = SchemaItem(json)
        let refreshed = SchemaItem(json)
        XCTAssertEqual(original.id, refreshed.id)
        XCTAssertEqual(original.children?.first?.id, refreshed.children?.first?.id)
        XCTAssertTrue(Set([original.id]).contains(refreshed.id))
        var updated = json
        updated["children"] = [["name": "orders", "kind": "table", "schema": "public", "table": "orders"]]
        XCTAssertEqual(original.id, SchemaItem(updated).id)
        let privateUsers = SchemaItem(["name": "users", "kind": "table", "schema": "private", "table": "users"])
        XCTAssertNotEqual(original.children?.first?.id, privateUsers.id)
    }
    func testEngineDefaults() { var draft = ProfileDraft(); draft.select(.redis); XCTAssertEqual(draft.port, 6379); XCTAssertEqual(draft.database, "0") }

    @MainActor func testQueryTruncationUsesMetadataAndStaysOnQueryTab() throws {
        XCTAssertTrue(GridData(query: ["rows": [[73]], "truncated": true]).truncated)
        XCTAssertFalse(GridData(query: ["rows": [[73]]]).truncated)
        let rows: [[Any]] = (1...1000).map { [$0, "User \($0)"] }
        XCTAssertFalse(GridData(query: ["rows": rows, "truncated": false]).truncated)
        let model = AppModel()
        model.route = .workspace
        model.active = Profile(["id": "00000000-0000-0000-0000-000000000001", "name": "Production", "color": "#ff9f43", "engine": "postgres", "defaultDatabase": "warehouse"])
        model.database = "warehouse"
        model.sql = "SELECT id, full_name FROM public.users"
        model.queryGrid = GridData(query: ["columns": [["name": "id"], ["name": "full_name"]], "rows": rows, "truncated": true])
        XCTAssertTrue(model.grid.truncated)
        try retainWorkbench(model, named: "iphone-truncated-query-results")
        model.tableGrid = GridData(page: ["columns": ["id"], "rows": [[26]], "offset": 25, "hasMore": false])
        model.selectedTable = ("public", "users")
        model.showTable()
        XCTAssertFalse(model.grid.truncated)
        model.showQuery()
        XCTAssertTrue(model.grid.truncated)
    }

    @MainActor func testWorkbenchTabsKeepSeparateResultsAndTablePage() {
        let model = AppModel()
        model.sql = "SELECT full_name, paid_total FROM report"
        model.queryGrid = GridData(query: ["columns": [["name": "full_name"], ["name": "paid_total"]], "rows": [["Maya", 73]]])
        model.tableGrid = GridData(page: ["columns": ["id", "email"], "rows": [[26, "page2@example.test"]], "offset": 25, "limit": 25, "hasMore": false])
        model.selectedTable = ("public", "users")
        model.showTable()
        XCTAssertEqual(model.grid.columns, ["id", "email"])
        XCTAssertEqual(model.grid.offset, 25)
        model.showQuery()
        XCTAssertEqual(model.sql, "SELECT full_name, paid_total FROM report")
        XCTAssertEqual(model.grid.columns, ["full_name", "paid_total"])
        XCTAssertEqual(model.grid.rows[0][1] as? Int, 73)
        model.showTable()
        XCTAssertEqual(model.grid.rows[0][1] as? String, "page2@example.test")
        XCTAssertEqual(model.grid.offset, 25)
    }

    @MainActor func testBackgroundClearsBothRetainedTabs() {
        let model = AppModel()
        model.sql = "SELECT private_value"
        model.queryGrid = GridData(query: ["columns": [["name": "private_value"]], "rows": [["query secret"]]])
        model.tableGrid = GridData(page: ["columns": ["secret"], "rows": [["table secret"]]])
        model.selectedTable = ("private", "records")
        model.showTable()
        model.background()
        XCTAssertEqual(model.sql, "")
        XCTAssertTrue(model.queryGrid.rows.isEmpty)
        XCTAssertTrue(model.tableGrid.rows.isEmpty)
        XCTAssertNil(model.selectedTable)
        XCTAssertFalse(model.showingTable)
        XCTAssertEqual(model.route, .connections)
    }

    @MainActor private func retainWorkbench(_ model: AppModel, named name: String) throws {
        let scene = try XCTUnwrap(UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.first)
        let previous = scene.windows.first { $0.isKeyWindow }
        let window = UIWindow(windowScene: scene)
        window.frame = scene.coordinateSpace.bounds
        window.rootViewController = UIHostingController(rootView: RootView().environmentObject(model).preferredColorScheme(.dark))
        window.makeKeyAndVisible()
        defer { window.isHidden = true; previous?.makeKey() }
        window.layoutIfNeeded()
        let image = UIGraphicsImageRenderer(bounds: window.bounds).image { _ in
            XCTAssertTrue(window.drawHierarchy(in: window.bounds, afterScreenUpdates: true))
        }
        let attachment = XCTAttachment(image: image)
        attachment.name = name; attachment.lifetime = .keepAlways
        add(attachment)
    }
}
