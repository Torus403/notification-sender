package notificationsender

/**
 * Entry point for the Android client SDK. Device registration and open tracking land here
 * once the matching endpoints exist in spec/openapi.json.
 */
class NotificationSender(val publishableKey: String, baseUrl: String) {
    val baseUrl: String = baseUrl.trimEnd('/')

    init {
        require(publishableKey.isNotEmpty()) { "publishableKey is required" }
    }
}
