import XCTest

/// Opt-in simulator benchmarks (skipped by `build.sh test`). Run them with
/// `TEST_RUNNER_DBM_BENCH=1` and `-only-testing:DBMUITests/DBMBenchmarks`;
/// see README.md. A simulator is not a phone: compare runs on one Mac only.
@MainActor
final class DBMBenchmarks: XCTestCase {
    override func setUpWithError() throws {
        try XCTSkipUnless(ProcessInfo.processInfo.environment["DBM_BENCH"] == "1", "Set TEST_RUNNER_DBM_BENCH=1 to run benchmarks.")
    }

    func testLaunch() {
        let app = demoApp()
        measure(metrics: [XCTApplicationLaunchMetric(waitUntilResponsive: true)], options: options) { app.launch() }
    }

    func testIdleWithQueryResults() {
        let app = connectedWithResults()
        sleep(2)
        measure(metrics: [XCTCPUMetric(application: app), XCTMemoryMetric(application: app)], options: options) {
            Thread.sleep(forTimeInterval: 3)
        }
    }

    func testScrollQueryResults() {
        let app = connectedWithResults()
        let rows = app.scrollViews["results-rows"], grid = app.scrollViews["results-grid"]
        measure(metrics: [XCTCPUMetric(application: app), XCTMemoryMetric(application: app)], options: options) {
            rows.swipeUp(); rows.swipeDown(); grid.swipeLeft(); grid.swipeRight()
        }
    }

    private var options: XCTMeasureOptions { let options = XCTMeasureOptions(); options.iterationCount = 10; return options }

    private func demoApp() -> XCUIApplication {
        let app = XCUIApplication()
        app.launchEnvironment["DBM_DEMO"] = "1"
        return app
    }

    private func connectedWithResults() -> XCUIApplication {
        let app = demoApp()
        app.launch()
        let connect = app.buttons["connect-00000000-0000-0000-0000-000000000001"]
        XCTAssertTrue(connect.waitForExistence(timeout: 10))
        connect.tap()
        XCTAssertTrue(app.buttons["run-query"].waitForExistence(timeout: 10))
        app.buttons["run-query"].tap()
        XCTAssertTrue(app.otherElements["result-row-1"].waitForExistence(timeout: 10))
        return app
    }
}
