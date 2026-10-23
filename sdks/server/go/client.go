// Package notificationsender is the server SDK for the NotificationSender API.
//
// This directory is mirrored to github.com/Torus403/notification-sender-go on release.
package notificationsender

import (
	"errors"
	"strings"
)

// Client is the entry point for the server SDK. Endpoint methods are generated
// from spec/openapi.json as the API lands.
type Client struct {
	apiKey  string
	BaseURL string
}

// New returns a Client for the given API key and base URL.
func New(apiKey, baseURL string) (*Client, error) {
	if apiKey == "" {
		return nil, errors.New("notificationsender: api key is required")
	}
	return &Client{apiKey: apiKey, BaseURL: strings.TrimRight(baseURL, "/")}, nil
}
