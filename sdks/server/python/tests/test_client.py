import unittest

from notification_sender import NotificationSender


class ClientTest(unittest.TestCase):
    def test_requires_api_key(self) -> None:
        with self.assertRaises(ValueError):
            NotificationSender(api_key="", base_url="https://api.example.com")

    def test_trims_trailing_slash(self) -> None:
        client = NotificationSender(api_key="key", base_url="https://api.example.com/")
        self.assertEqual(client.base_url, "https://api.example.com")


if __name__ == "__main__":
    unittest.main()
