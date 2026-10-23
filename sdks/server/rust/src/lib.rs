//! Server SDK for the NotificationSender API.
//!
//! Endpoint methods are generated from `spec/openapi.json` as the API lands.

/// Entry point for the server SDK.
#[derive(Debug, Clone)]
pub struct Client {
    api_key: String,
    base_url: String,
}

/// Errors from building a [`Client`].
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    MissingApiKey,
}

impl Client {
    pub fn new(api_key: impl Into<String>, base_url: impl Into<String>) -> Result<Self, Error> {
        let api_key = api_key.into();
        if api_key.is_empty() {
            return Err(Error::MissingApiKey);
        }
        let base_url = base_url.into().trim_end_matches('/').to_owned();
        Ok(Self { api_key, base_url })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn authorization(&self) -> String {
        format!("Bearer {}", self.api_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_api_key() {
        assert_eq!(
            Client::new("", "https://api.example.com").unwrap_err(),
            Error::MissingApiKey
        );
    }

    #[test]
    fn trims_trailing_slash() {
        let client = Client::new("key", "https://api.example.com/").expect("valid client");
        assert_eq!(client.base_url(), "https://api.example.com");
    }
}
