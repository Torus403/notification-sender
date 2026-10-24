import Foundation

/// Entry point for the iOS client SDK. Device registration and open tracking land here
/// once the matching endpoints exist in spec/openapi.json.
public struct NotificationSender {
    public let publishableKey: String
    public let baseURL: URL

    public init(publishableKey: String, baseURL: URL) {
        precondition(!publishableKey.isEmpty, "publishableKey is required")
        self.publishableKey = publishableKey
        self.baseURL = baseURL
    }
}
