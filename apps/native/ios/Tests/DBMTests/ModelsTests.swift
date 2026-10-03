import XCTest
import UIKit
@testable import DBM

final class ModelsTests: XCTestCase {
    func testBundledFontsUseRegisteredPostScriptNames() {
        XCTAssertNotNil(UIFont(name: "Geist-Regular", size: 15))
        XCTAssertNotNil(UIFont(name: "GeistMono-Regular", size: 14))
    }
    func testAppMetadataPreservesNativeDisplayAndPrivateNetworkUsage() {
        XCTAssertNotNil(Bundle.main.object(forInfoDictionaryKey: "UILaunchScreen") as? [String: Any])
        let networkUsage = Bundle.main.object(forInfoDictionaryKey: "NSLocalNetworkUsageDescription") as? String
        XCTAssertEqual(networkUsage, "DBM connects directly to the databases you configure on your private network.")
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
    func testEngineDefaults() { var draft = ProfileDraft(); draft.select(.redis); XCTAssertEqual(draft.port, 6379); XCTAssertEqual(draft.database, "0") }

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
}
