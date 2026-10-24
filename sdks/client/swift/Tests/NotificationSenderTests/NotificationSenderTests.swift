import XCTest
@testable import NotificationSender

final class NotificationSenderTests: XCTestCase {
    func testKeepsConfiguration() throws {
        let url = try XCTUnwrap(URL(string: "https://api.example.com"))
        let client = NotificationSender(publishableKey: "pk_test", baseURL: url)
        XCTAssertEqual(client.publishableKey, "pk_test")
        XCTAssertEqual(client.baseURL, url)
    }
}
