/// Entry point for the Flutter client SDK. Device registration and open tracking land here
/// once the matching endpoints exist in spec/openapi.json.
class NotificationSender {
  NotificationSender({required this.publishableKey, required String baseUrl})
      : baseUrl = baseUrl.replaceAll(RegExp(r'/+$'), '') {
    if (publishableKey.isEmpty) {
      throw ArgumentError.value(publishableKey, 'publishableKey', 'is required');
    }
  }

  final String publishableKey;
  final String baseUrl;
}
