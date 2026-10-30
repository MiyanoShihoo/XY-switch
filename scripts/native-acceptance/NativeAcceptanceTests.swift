import XCTest

final class NativeAcceptanceTests: XCTestCase {
    private var app: XCUIApplication!

    override func setUpWithError() throws {
        continueAfterFailure = false
        app = XCUIApplication(bundleIdentifier: "com.xyswitch.desktop")
        app.launch()
    }

    override func tearDownWithError() throws {
        attach("final-accessibility", text: app.debugDescription)
        let image = XCTAttachment(screenshot: app.screenshot())
        image.name = "final-application"; image.lifetime = .keepAlways; add(image)
    }

    private func attach(_ name: String, text: String) {
        let item = XCTAttachment(string: text)
        item.name = name; item.lifetime = .keepAlways; add(item)
    }

    private func matching(_ type: XCUIElement.ElementType, _ labels: [String]) -> XCUIElement {
        return app.descendants(matching: type).matching(NSPredicate(format: "label IN %@", labels)).firstMatch
    }

    private func button(_ labels: [String]) -> XCUIElement { matching(.button, labels) }

    private func readyProviders() {
        let provider = button(["切换到 Anthropic", "Switch to Anthropic"])
        XCTAssertTrue(provider.waitForExistence(timeout: 120), "Seeded providers never became visible; a loading screenshot is not readiness")
        XCTAssertFalse(matching(.staticText, ["读取供应商配置…", "Loading provider configuration…"]).exists)
        attach("providers-ready", text: app.debugDescription)
    }

    private func settings() {
        let control = button(["设置", "Settings"])
        XCTAssertTrue(control.waitForExistence(timeout: 20)); control.tap()
        XCTAssertTrue(button(["保存偏好", "Save preferences"]).waitForExistence(timeout: 30), "Native preference IPC did not finish")
        XCTAssertFalse(matching(.staticText, ["正在读取应用偏好…", "Loading application preferences…"]).exists)
    }

    private func timeoutField() -> XCUIElement {
        let field = matching(.textField, ["请求超时（秒）", "Request timeout (seconds)"])
        XCTAssertTrue(field.waitForExistence(timeout: 20), "Missing accessible request-timeout field")
        for _ in 0..<6 {
            if field.isHittable { break }
            app.webViews.firstMatch.swipeUp()
        }
        return field
    }

    func testPackagedApplicationReadinessPreferencesAndMobileSync() throws {
        readyProviders()
        settings()
        let field = timeoutField()
        field.tap()
        let original = (field.value as? String) ?? ""
        XCTAssertFalse(original.isEmpty)
        field.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: original.count) + "175")
        XCTAssertEqual(field.value as? String, "175")
        let heading = matching(.staticText, ["应用偏好", "Application preferences"])
        if heading.exists { heading.tap() }
        let save = button(["保存偏好", "Save preferences"])
        for _ in 0..<4 {
            if save.isHittable { break }
            app.webViews.firstMatch.swipeUp()
        }
        XCTAssertTrue(save.isEnabled); save.tap()
        let saved = app.staticTexts.matching(NSPredicate(format: "label CONTAINS %@ OR label CONTAINS %@", "偏好已保存。", "Preferences saved.")).firstMatch
        XCTAssertTrue(saved.waitForExistence(timeout: 30), "Native preference write did not report success")
        attach("preferences-saved", text: app.debugDescription)

        let sync = button(["加密同步", "Encrypted Sync"])
        XCTAssertTrue(sync.waitForExistence(timeout: 20)); sync.tap()
        let readOnly = app.staticTexts.matching(NSPredicate(format: "label CONTAINS %@ OR label CONTAINS %@", "此设备为只读视图", "read-only")).firstMatch
        XCTAssertTrue(readOnly.waitForExistence(timeout: 30), "Mobile sync must remain read-only")
        XCTAssertFalse(button(["预览加密导出", "Preview encrypted export"]).exists)
        attach("mobile-sync-read-only", text: app.debugDescription)
        let syncImage = XCTAttachment(screenshot: app.screenshot())
        syncImage.name = "mobile-sync-read-only"; syncImage.lifetime = .keepAlways; add(syncImage)

        app.terminate(); app.launch()
        readyProviders(); settings()
        XCTAssertEqual(timeoutField().value as? String, "175", "Preference did not survive a real process restart")
        attach("preferences-reloaded", text: app.debugDescription)
    }
}

