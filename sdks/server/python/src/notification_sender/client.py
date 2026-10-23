class NotificationSender:
    """Entry point for the server SDK. Endpoint methods are generated from spec/openapi.json as the API lands."""

    def __init__(self, api_key: str, base_url: str) -> None:
        if not api_key:
            raise ValueError("api_key is required")
        self._api_key = api_key
        self.base_url = base_url.rstrip("/")

    @property
    def headers(self) -> dict[str, str]:
        return {"Authorization": f"Bearer {self._api_key}", "Content-Type": "application/json"}
