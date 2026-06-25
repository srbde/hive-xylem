use crate::errors::XylemError;
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const DEFAULT_APIS: &[&str] = &["https://api.hive.blog", "https://api.syncad.com"];
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;
pub const USER_AGENT: &str = "xylem/1.0.0";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReputationResult {
    pub account: String,
    pub reputation: i64,
}

#[derive(Debug, Clone)]
pub struct Client {
    pub base_url: String,
    pub http_client: reqwest::Client,
}

impl Client {
    pub fn new(api: &str, timeout_secs: u64) -> Result<Self, XylemError> {
        let mut trimmed = api.trim().to_string();
        if trimmed.is_empty() {
            trimmed = DEFAULT_APIS[0].to_string();
        }

        if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
            return Err(XylemError::RpcError(format!(
                "invalid HAF API URL: {}",
                trimmed
            )));
        }

        let timeout = if timeout_secs == 0 {
            DEFAULT_TIMEOUT_SECS
        } else {
            timeout_secs
        };

        let base_url = trimmed.trim_end_matches('/').to_string();
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout))
            .user_agent(USER_AGENT)
            .build()
            .map_err(|e| XylemError::HttpError(e.to_string()))?;

        Ok(Client {
            base_url,
            http_client,
        })
    }

    pub fn default_client() -> Result<Self, XylemError> {
        Self::new("", 0)
    }

    async fn request(&self, endpoint: &str) -> Result<serde_json::Value, XylemError> {
        let url = format!("{}/{}", self.base_url, endpoint.trim_start_matches('/'));
        let resp = self
            .http_client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(XylemError::RpcError(format!(
                "haf request failed ({}): {}",
                status,
                body.trim()
            )));
        }

        let val: serde_json::Value = resp.json().await?;
        Ok(val)
    }

    pub async fn reputation(&self, account: &str) -> Result<ReputationResult, XylemError> {
        let trimmed_account = account.trim();
        if trimmed_account.is_empty() {
            return Err(XylemError::SerializationError(
                "account name must be provided".to_string(),
            ));
        }

        let endpoint = format!("reputation-api/accounts/{}/reputation", trimmed_account);
        let payload = self.request(&endpoint).await?;

        if payload.is_null() {
            return Err(XylemError::RpcError(format!(
                "empty reputation response for account {}",
                trimmed_account
            )));
        }

        if let Some(obj) = payload.as_object() {
            let rep = if let Some(r) = obj.get("reputation") {
                extract_int64(r)?
            } else {
                return Err(XylemError::SerializationError(
                    "reputation field missing".to_string(),
                ));
            };
            let acct = if let Some(a) = obj.get("account").and_then(|v| v.as_str()) {
                if !a.is_empty() {
                    a.to_string()
                } else {
                    trimmed_account.to_string()
                }
            } else {
                trimmed_account.to_string()
            };
            Ok(ReputationResult {
                account: acct,
                reputation: rep,
            })
        } else if let Some(num) = payload.as_i64() {
            Ok(ReputationResult {
                account: trimmed_account.to_string(),
                reputation: num,
            })
        } else if let Some(f) = payload.as_f64() {
            Ok(ReputationResult {
                account: trimmed_account.to_string(),
                reputation: f as i64,
            })
        } else {
            Err(XylemError::RpcError(format!(
                "unexpected reputation response type for account {}",
                trimmed_account
            )))
        }
    }

    pub async fn account_balances(&self, account: &str) -> Result<serde_json::Value, XylemError> {
        let trimmed_account = account.trim();
        if trimmed_account.is_empty() {
            return Err(XylemError::SerializationError(
                "account name must be provided".to_string(),
            ));
        }

        let endpoint = format!("balance-api/accounts/{}/balances", trimmed_account);
        let payload = self.request(&endpoint).await?;

        if payload.is_null() {
            return Err(XylemError::RpcError(format!(
                "empty balances response for account {}",
                trimmed_account
            )));
        }

        Ok(payload)
    }
}

fn extract_int64(val: &serde_json::Value) -> Result<i64, XylemError> {
    if let Some(i) = val.as_i64() {
        Ok(i)
    } else if let Some(f) = val.as_f64() {
        Ok(f as i64)
    } else if let Some(s) = val.as_str() {
        s.trim().parse::<i64>().map_err(|e| {
            XylemError::SerializationError(format!("unable to parse numeric string: {}", e))
        })
    } else {
        Err(XylemError::SerializationError(format!(
            "unsupported numeric type: {:?}",
            val
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn spawn_single_response_server(response_body: String) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{}", port);

        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0; 4096];
                let _ = stream.read(&mut buf).await;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    response_body.len(),
                    response_body
                );
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });

        url
    }

    #[test]
    fn test_new_client_validation() {
        assert!(Client::new("ftp://invalid", 1).is_err());

        let client = Client::new("http://example.com", 0).unwrap();
        assert_eq!(client.base_url, "http://example.com");
    }

    #[tokio::test]
    async fn test_reputation_request() {
        let response_body = json!({
            "account": "alice",
            "reputation": 123
        })
        .to_string();
        let url = spawn_single_response_server(response_body).await;
        let client = Client::new(&url, 2).unwrap();

        let result = client.reputation("alice").await.unwrap();
        assert_eq!(
            result,
            ReputationResult {
                account: "alice".to_string(),
                reputation: 123
            }
        );
    }

    #[tokio::test]
    async fn test_account_balances() {
        let response_body = json!({
            "HIVE": "1.000 HIVE"
        })
        .to_string();
        let url = spawn_single_response_server(response_body).await;
        let client = Client::new(&url, 2).unwrap();

        let balances = client.account_balances("bob").await.unwrap();
        assert_eq!(balances["HIVE"].as_str().unwrap(), "1.000 HIVE");
    }

    #[test]
    fn test_extract_int64() {
        assert_eq!(extract_int64(&json!(12)).unwrap(), 12);
        assert_eq!(extract_int64(&json!(34.0)).unwrap(), 34);
        assert_eq!(extract_int64(&json!("56")).unwrap(), 56);
        assert!(extract_int64(&json!(null)).is_err());
        assert!(extract_int64(&json!("zz")).is_err());
    }
}
