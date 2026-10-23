package notificationsender

import "testing"

func TestNewRequiresAPIKey(t *testing.T) {
	if _, err := New("", "https://api.example.com"); err == nil {
		t.Fatal("expected an error for an empty api key")
	}
}

func TestNewTrimsTrailingSlash(t *testing.T) {
	c, err := New("key", "https://api.example.com/")
	if err != nil {
		t.Fatal(err)
	}
	if c.BaseURL != "https://api.example.com" {
		t.Fatalf("BaseURL = %q", c.BaseURL)
	}
}
