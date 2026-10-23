import 'package:notification_sender/notification_sender.dart';
import 'package:test/test.dart';

void main() {
  test('trims a trailing slash from the base url', () {
    final client = NotificationSender(publishableKey: 'pk_test', baseUrl: 'https://api.example.com/');
    expect(client.baseUrl, 'https://api.example.com');
  });
}
