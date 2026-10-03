import XCTest
@testable import DBM

final class ModelsTests: XCTestCase {
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
}
