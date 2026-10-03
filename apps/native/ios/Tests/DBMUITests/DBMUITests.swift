import XCTest

@MainActor
final class DBMUITests: XCTestCase {
    func testDemoConnectionFormAndScreenshot() {
        let app = launchDemo()
        XCTAssertTrue(app.staticTexts["demo-banner"].waitForExistence(timeout: 10))
        app.buttons["add-connection"].tap()
        XCTAssertTrue(app.navigationBars["New connection"].waitForExistence(timeout: 5))
        let name = app.textFields["name"]; name.tap(); name.typeText("Demo PostgreSQL")
        let username = app.textFields["username"]; username.tap(); username.typeText("demo_user")
        let password = app.secureTextFields["password"]; password.tap(); password.typeText("memory-only")
        XCTAssertTrue(app.buttons["test-connection"].isEnabled)
        retainScreenshot(app, named: "iphone-connection-form")
    }

    func testDemoQueryExplorerAndTablePagination() {
        let app = launchDemo()
        XCTAssertTrue(app.staticTexts["Production"].waitForExistence(timeout: 10))
        app.buttons["connect-00000000-0000-0000-0000-000000000001"].tap()
        XCTAssertTrue(app.navigationBars["Production"].waitForExistence(timeout: 10))

        let editor = app.textViews["query-editor"]
        XCTAssertTrue(editor.waitForExistence(timeout: 5))
        XCTAssertEqual(editor.value as? String, "SELECT 1")
        app.buttons["run-query"].tap()
        let queryRow = app.otherElements["result-row-1"]
        XCTAssertTrue(queryRow.waitForExistence(timeout: 10))
        XCTAssertTrue(queryRow.label.contains("full_name:"), queryRow.label)
        retainScreenshot(app, named: "iphone-query-results")

        app.buttons["explorer"].tap()
        XCTAssertTrue(app.navigationBars["Schema"].waitForExistence(timeout: 5))
        let publicSchema = app.buttons["public"]
        XCTAssertTrue(publicSchema.waitForExistence(timeout: 5))
        XCTAssertTrue(publicSchema.isEnabled)
        publicSchema.tap()
        let users = app.buttons["users"]
        XCTAssertTrue(users.waitForExistence(timeout: 5))
        retainScreenshot(app, named: "iphone-schema-expanded")
        users.tap()
        XCTAssertTrue(app.staticTexts["Rows 1–25"].waitForExistence(timeout: 10))
        let firstTableRow = app.otherElements["result-row-1"]
        XCTAssertTrue(firstTableRow.label.contains("email:"), firstTableRow.label)
        retainScreenshot(app, named: "iphone-table-page-1")

        app.buttons["Next"].tap()
        XCTAssertTrue(app.staticTexts["Rows 26–40"].waitForExistence(timeout: 10))
        retainScreenshot(app, named: "iphone-table-page-2")
    }

    func testBackgroundReturnsToConnectionsAndClearsPasswordEntry() {
        let app = launchDemo()
        let editProfile = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "edit-")).firstMatch
        XCTAssertTrue(editProfile.waitForExistence(timeout: 10))
        editProfile.tap()
        XCTAssertTrue(app.navigationBars["Edit connection"].waitForExistence(timeout: 5))
        let password = app.secureTextFields["password"]
        password.tap(); password.typeText("temporary-password")

        XCUIDevice.shared.press(.home)
        XCTAssertTrue(app.wait(for: .runningBackground, timeout: 5))
        app.activate()

        XCTAssertTrue(app.navigationBars["Connections"].waitForExistence(timeout: 10))
        app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "edit-")).firstMatch.tap()
        XCTAssertTrue(app.navigationBars["Edit connection"].waitForExistence(timeout: 5))
        XCTAssertEqual(app.secureTextFields["password"].value as? String, "Password (session only)")
    }

    private func launchDemo() -> XCUIApplication {
        let app = XCUIApplication()
        app.launchEnvironment["DBM_DEMO"] = "1"
        app.launch()
        return app
    }

    private func retainScreenshot(_ app: XCUIApplication, named name: String) {
        let screenshot = XCTAttachment(screenshot: app.screenshot())
        screenshot.name = name
        screenshot.lifetime = .keepAlways
        add(screenshot)
    }
}
