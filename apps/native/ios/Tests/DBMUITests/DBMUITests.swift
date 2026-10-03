import XCTest

@MainActor
final class DBMUITests: XCTestCase {
    func testDemoConnectionFormAndScreenshot() {
        let app = launchDemo()
        XCTAssertTrue(app.staticTexts["DEMO"].waitForExistence(timeout: 10))
        retainScreenshot(app, named: "iphone-connections")
        app.buttons["add-connection"].tap()
        XCTAssertTrue(app.staticTexts["New connection"].waitForExistence(timeout: 5))
        let name = app.textFields["name"]; name.tap(); name.typeText("Demo PostgreSQL")
        let username = app.textFields["username"]; username.tap(); username.typeText("demo_user")
        let password = app.secureTextFields["password"]; password.tap(); password.typeText("memory-only")
        XCTAssertTrue(app.buttons["test-connection"].isEnabled)
        retainScreenshot(app, named: "iphone-connection-form")
    }

    func testLandscapeUsesPersistentSourceList() {
        let app = launchDemo()
        XCTAssertTrue(app.staticTexts["Production"].waitForExistence(timeout: 10))
        app.buttons["connect-00000000-0000-0000-0000-000000000001"].tap()
        XCTAssertTrue(app.textViews["query-editor"].waitForExistence(timeout: 10))
        XCUIDevice.shared.orientation = .landscapeLeft
        XCTAssertTrue(app.textFields["schema-filter"].waitForExistence(timeout: 5))
        XCTAssertFalse(app.buttons["explorer"].exists)
        app.scrollViews["source-list-scroll"].swipeUp()
        let publicSchema = app.buttons["schema-item-public"]
        XCTAssertTrue(publicSchema.exists)
        publicSchema.tap()
        app.scrollViews["source-list-scroll"].swipeUp()
        app.buttons["schema-item-users"].tap()
        XCTAssertTrue(app.staticTexts["Rows 1–25"].waitForExistence(timeout: 10))
        XCTAssertFalse(app.textViews["query-editor"].exists)
        retainScreenshot(app, named: "iphone-landscape-sidebar")
        XCUIDevice.shared.orientation = .portrait
    }

    func testDemoQueryExplorerAndTablePagination() {
        let app = launchDemo()
        XCTAssertTrue(app.staticTexts["Production"].waitForExistence(timeout: 10))
        app.buttons["connect-00000000-0000-0000-0000-000000000001"].tap()
        XCTAssertTrue(app.staticTexts["Production"].waitForExistence(timeout: 10))

        let editor = app.textViews["query-editor"]
        XCTAssertTrue(editor.waitForExistence(timeout: 5))
        XCTAssertEqual(editor.value as? String, "SELECT 1")
        app.buttons["run-query"].tap()
        let queryRow = app.otherElements["result-row-1"]
        XCTAssertTrue(queryRow.waitForExistence(timeout: 10))
        XCTAssertTrue(queryRow.label.contains("full_name:"), queryRow.label)
        retainScreenshot(app, named: "iphone-query-results")

        app.buttons["explorer"].tap()
        let publicSchema = app.buttons["schema-item-public"]
        XCTAssertTrue(publicSchema.waitForExistence(timeout: 5))
        XCTAssertTrue(publicSchema.isEnabled)
        publicSchema.tap()
        let users = app.buttons["schema-item-users"]
        XCTAssertTrue(users.waitForExistence(timeout: 5))
        retainScreenshot(app, named: "iphone-schema-expanded")
        users.tap()
        XCTAssertFalse(app.textViews["query-editor"].exists)
        XCTAssertTrue(app.staticTexts["Rows 1–25"].waitForExistence(timeout: 10))
        let firstTableRow = app.otherElements["result-row-1"]
        XCTAssertTrue(firstTableRow.label.contains("email:"), firstTableRow.label)
        XCTAssertFalse(app.buttons["Previous"].isEnabled)
        XCTAssertTrue(app.buttons["Next"].isEnabled)
        retainScreenshot(app, named: "iphone-table-page-1")

        app.buttons["Next"].tap()
        XCTAssertTrue(app.staticTexts["Rows 26–40"].waitForExistence(timeout: 10))
        XCTAssertTrue(app.buttons["Previous"].isEnabled)
        XCTAssertFalse(app.buttons["Next"].isEnabled)
        retainScreenshot(app, named: "iphone-table-page-2")

        app.buttons["SQL"].tap()
        XCTAssertTrue(app.textViews["query-editor"].exists)
        XCTAssertEqual(app.textViews["query-editor"].value as? String, "SELECT 1")
        XCTAssertTrue(app.otherElements["result-row-1"].label.contains("paid_total: 18420.5"))
        XCTAssertFalse(app.otherElements["result-row-1"].label.contains("email:"))
        app.buttons["users"].tap()
        XCTAssertFalse(app.textViews["query-editor"].exists)
        XCTAssertTrue(app.staticTexts["Rows 26–40"].exists)
    }

    func testInvalidConnectionShowsValidationError() {
        let app = launchDemo()
        XCTAssertTrue(app.buttons["add-connection"].waitForExistence(timeout: 10))
        app.buttons["add-connection"].tap()
        let name = app.textFields["name"]; name.tap(); name.typeText("   ")
        let user = app.textFields["username"]; user.tap(); user.typeText("reader")
        app.buttons["test-connection"].tap()
        XCTAssertTrue(app.alerts["DBM"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.alerts["DBM"].staticTexts["invalid input: name, host, and username are required"].exists)
        retainScreenshot(app, named: "iphone-connection-error")
    }

    func testBackgroundReturnsToConnectionsAndClearsPasswordEntry() {
        let app = launchDemo()
        let editProfile = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "edit-")).firstMatch
        XCTAssertTrue(editProfile.waitForExistence(timeout: 10))
        editProfile.tap()
        XCTAssertTrue(app.staticTexts["Edit connection"].waitForExistence(timeout: 5))
        let password = app.secureTextFields["password"]
        password.tap(); password.typeText("temporary-password")

        XCUIDevice.shared.press(.home)
        XCTAssertTrue(app.wait(for: .runningBackground, timeout: 5))
        app.activate()

        XCTAssertTrue(app.staticTexts["Connections"].waitForExistence(timeout: 10))
        app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "edit-")).firstMatch.tap()
        XCTAssertTrue(app.staticTexts["Edit connection"].waitForExistence(timeout: 5))
        XCTAssertEqual(app.secureTextFields["password"].value as? String, "Not saved on this device")
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
