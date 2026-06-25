use crate::client::Client;
use crate::errors::XylemError;
use crate::haf;
use crate::operations::Follow;
use crate::transaction::Transaction;
use crate::types::{Authority, RCInfo};
use serde_json::{Map, Value};
use std::sync::Arc;

pub const VOTING_MANA_REGENERATION_SECONDS: f64 = 5.0 * 24.0 * 60.0 * 60.0;
pub const RC_MANA_REGENERATION_SECONDS: f64 = 5.0 * 24.0 * 60.0 * 60.0;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct AccountKeys {
    pub owner: Vec<(String, u16)>,
    pub active: Vec<(String, u16)>,
    pub posting: Vec<(String, u16)>,
    pub memo: Option<String>,
}

pub struct Account {
    pub name: String,
    pub api: Option<Arc<Client>>,
    pub data: Map<String, Value>,
    pub rc_info: Option<RCInfo>,
    pub haf_client: Option<haf::Client>,
    pub reputation: Option<f64>,
}

impl Account {
    pub fn new(name: &str, api: Option<Arc<Client>>) -> Self {
        Account {
            name: name.to_string(),
            api,
            data: Map::new(),
            rc_info: None,
            haf_client: None,
            reputation: None,
        }
    }

    pub fn set_haf_client(&mut self, client: haf::Client) {
        self.haf_client = Some(client);
    }

    /// Returns the owner authority if present in account data.
    pub fn owner(&self) -> Option<Authority> {
        self.data
            .get("owner")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// Returns the active authority if present in account data.
    pub fn active(&self) -> Option<Authority> {
        self.data
            .get("active")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// Returns the posting authority if present in account data.
    pub fn posting(&self) -> Option<Authority> {
        self.data
            .get("posting")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// Returns the memo key if present in account data.
    pub fn memo_key(&self) -> Option<String> {
        self.data
            .get("memo_key")
            .and_then(|v| v.as_str().map(|s| s.to_string()))
    }

    /// Get the owner keys and weights directly
    pub fn owner_keys(&self) -> Vec<(String, u16)> {
        self.owner()
            .map(|a| a.key_auths.into_iter().collect())
            .unwrap_or_default()
    }

    /// Get the active keys and weights directly
    pub fn active_keys(&self) -> Vec<(String, u16)> {
        self.active()
            .map(|a| a.key_auths.into_iter().collect())
            .unwrap_or_default()
    }

    /// Get the posting keys and weights directly
    pub fn posting_keys(&self) -> Vec<(String, u16)> {
        self.posting()
            .map(|a| a.key_auths.into_iter().collect())
            .unwrap_or_default()
    }

    /// Get all keys associated with the account
    pub fn get_keys(&self) -> AccountKeys {
        AccountKeys {
            owner: self.owner_keys(),
            active: self.active_keys(),
            posting: self.posting_keys(),
            memo: self.memo_key(),
        }
    }

    pub async fn refresh(&mut self) -> Result<(), XylemError> {
        let api = self
            .api
            .as_ref()
            .ok_or_else(|| XylemError::RpcError("API not configured".to_string()))?;

        let resp = api
            .call(
                "condenser_api",
                "get_accounts",
                serde_json::json!([[self.name]]),
            )
            .await?;

        let accounts = resp
            .as_array()
            .ok_or_else(|| XylemError::RpcError("invalid response format".to_string()))?;

        if accounts.is_empty() {
            return Err(XylemError::RpcError(format!(
                "account '{}' not found",
                self.name
            )));
        }

        let data = accounts[0]
            .as_object()
            .ok_or_else(|| XylemError::RpcError("invalid account data format".to_string()))?;

        self.data = data.clone();
        self.reputation = None;
        Ok(())
    }

    pub async fn get_reputation(&mut self, refresh: bool) -> Result<f64, XylemError> {
        if let Some(rep) = self.reputation {
            if !refresh {
                return Ok(rep);
            }
        }

        // 1. Try HAF reputation using the injected Client first
        let mut client = self.haf_client.clone();

        // 2. Try HAF client with configured API node if it matches a known HAF API
        if client.is_none() {
            if let Some(ref api) = self.api {
                let nodes = api.nodes();
                if !nodes.is_empty() {
                    let is_known_haf = haf::DEFAULT_APIS.iter().any(|&haf_api| {
                        nodes[0]
                            .trim_end_matches('/')
                            .eq_ignore_ascii_case(haf_api.trim_end_matches('/'))
                    });
                    if is_known_haf {
                        if let Ok(c) = haf::Client::new(&nodes[0], 10) {
                            client = Some(c);
                        }
                    }
                }
            }
        }

        // 3. Try default client
        if client.is_none() {
            if let Ok(c) = haf::Client::default_client() {
                client = Some(c);
            }
        }

        // Query HAF if client is available
        if let Some(ref c) = client {
            if let Ok(result) = c.reputation(&self.name).await {
                let rep = result.reputation as f64;
                self.reputation = Some(rep);
                return Ok(rep);
            }
        }

        // 4. Fallback to condenser API cached reputation
        if self.data.is_empty() && self.api.is_some() {
            let _ = self.refresh().await;
        }

        if !self.data.is_empty() {
            let mut raw_rep = 0.0;
            if let Some(rep_str) = self.data.get("reputation").and_then(|v| v.as_str()) {
                if let Ok(r) = rep_str.parse::<f64>() {
                    raw_rep = r;
                }
            } else if let Some(rep_float) = self.data.get("reputation").and_then(|v| v.as_f64()) {
                raw_rep = rep_float;
            }

            if raw_rep != 0.0 {
                let rep = calculate_reputation(raw_rep);
                self.reputation = Some(rep);
                return Ok(rep);
            }
        }

        let default_rep = 25.0;
        self.reputation = Some(default_rep);
        Ok(default_rep)
    }

    pub async fn reputation(&mut self) -> Result<f64, XylemError> {
        self.get_reputation(false).await
    }

    pub async fn rep(&mut self) -> Result<f64, XylemError> {
        self.get_reputation(false).await
    }

    async fn build_follow_tx(
        &self,
        following: &str,
        what: Vec<String>,
    ) -> Result<Transaction, XylemError> {
        let api = self
            .api
            .as_ref()
            .ok_or_else(|| XylemError::RpcError("API not configured".to_string()))?;

        let props = api.get_dynamic_global_properties().await?;
        let ref_block_num = (props.head_block_number & 0xFFFF) as u16;

        let prefix_bytes = hex::decode(&props.head_block_id[8..16])
            .map_err(|e| XylemError::HexError(format!("invalid head block ID: {}", e)))?;
        let ref_block_prefix = u32::from_le_bytes(prefix_bytes.try_into().unwrap());

        let expiration = crate::types::HiveTime(props.time.0 + chrono::Duration::minutes(1));
        let mut tx = Transaction::new(ref_block_num, ref_block_prefix, expiration);

        let follow_op = Follow {
            follower: self.name.clone(),
            following: following.to_string(),
            what,
        };
        tx.append_op(Box::new(follow_op));
        Ok(tx)
    }

    pub async fn follow(&self, account_to_follow: &str) -> Result<Transaction, XylemError> {
        self.build_follow_tx(account_to_follow, vec!["blog".to_string()])
            .await
    }

    pub async fn unfollow(&self, account_to_unfollow: &str) -> Result<Transaction, XylemError> {
        self.build_follow_tx(account_to_unfollow, vec![]).await
    }

    pub async fn ignore(&self, account_to_ignore: &str) -> Result<Transaction, XylemError> {
        self.build_follow_tx(account_to_ignore, vec!["ignore".to_string()])
            .await
    }

    pub async fn unignore(&self, account_to_unignore: &str) -> Result<Transaction, XylemError> {
        self.unfollow(account_to_unignore).await
    }

    pub async fn get_voting_power(&mut self, refresh: bool) -> Result<f64, XylemError> {
        if refresh || self.data.is_empty() {
            self.refresh().await?;
        }

        let manabar = self
            .data
            .get("voting_manabar")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();

        let mut current_mana = self
            .data
            .get("voting_power")
            .and_then(|v| v.as_f64())
            .or_else(|| {
                manabar.get("current_mana").and_then(|v| {
                    v.as_f64()
                        .or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok()))
                })
            })
            .unwrap_or(0.0);

        let mut last_update_time = 0i64;
        let mut use_manabar_time = false;

        if let Some(v) = manabar.get("last_update_time").and_then(|v| v.as_i64()) {
            if v > 0 {
                last_update_time = v;
                use_manabar_time = true;
            }
        }

        if !use_manabar_time {
            if let Some(last_vote_time_str) =
                self.data.get("last_vote_time").and_then(|v| v.as_str())
            {
                if let Ok(t) =
                    chrono::NaiveDateTime::parse_from_str(last_vote_time_str, "%Y-%m-%dT%H:%M:%S")
                {
                    last_update_time = t.and_utc().timestamp();
                }
            }
        }

        let max_mana = manabar
            .get("max_mana")
            .and_then(|v| v.as_f64())
            .unwrap_or(10000.0);

        if last_update_time > 0 && use_manabar_time {
            let current_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let diff = (current_time - last_update_time).max(0) as f64;
            let regenerated = diff * max_mana / VOTING_MANA_REGENERATION_SECONDS;
            current_mana = (current_mana + regenerated).min(max_mana);
        }

        if max_mana <= 0.0 {
            return Ok(0.0);
        }

        Ok((current_mana / max_mana) * 100.0)
    }

    pub async fn voting_power(&mut self) -> Result<f64, XylemError> {
        self.get_voting_power(false).await
    }

    pub async fn vp(&mut self) -> Result<f64, XylemError> {
        self.get_voting_power(false).await
    }

    pub async fn get_rc_info(&mut self, refresh: bool) -> Result<RCInfo, XylemError> {
        if let Some(ref info) = self.rc_info {
            if !refresh {
                return Ok(info.clone());
            }
        }

        let api = self
            .api
            .as_ref()
            .ok_or_else(|| XylemError::RpcError("API not configured".to_string()))?;

        let info = api.get_rc_mana(&self.name).await?;
        self.rc_info = Some(info.clone());
        Ok(info)
    }

    pub async fn rc_info(&mut self) -> Result<RCInfo, XylemError> {
        self.get_rc_info(false).await
    }

    pub async fn rc(&mut self) -> Result<f64, XylemError> {
        let info = self.get_rc_info(false).await?;
        Ok(info.current_percent)
    }
}

fn calculate_reputation(raw_rep: f64) -> f64 {
    if raw_rep == 0.0 {
        return 25.0;
    }
    let mut sign = 1.0;
    let mut rep_val = raw_rep;
    if raw_rep < 0.0 {
        sign = -1.0;
        rep_val = -raw_rep;
    }
    let log_val = rep_val.log10();
    let rep = (log_val - 9.0) * 9.0 + 25.0;
    sign * rep
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

    #[tokio::test]
    async fn test_refresh() {
        let response_body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": [
                {
                    "balance": "10.000 HIVE"
                }
            ]
        })
        .to_string();

        let url = spawn_single_response_server(response_body).await;
        let api = Arc::new(Client::new(vec![url], 30));
        let mut acc = Account::new("alice", Some(api));

        acc.refresh().await.unwrap();
        assert_eq!(
            acc.data.get("balance").unwrap().as_str().unwrap(),
            "10.000 HIVE"
        );
    }

    #[tokio::test]
    async fn test_refresh_without_api() {
        let mut acc = Account::new("alice", None);
        assert!(acc.refresh().await.is_err());
    }

    #[tokio::test]
    async fn test_get_reputation_caching() {
        assert_eq!(calculate_reputation(0.0), 25.0);
        assert_eq!(calculate_reputation(1_000_000_000.0), 25.0);
        assert_eq!(calculate_reputation(10_000_000_000.0), 34.0);
    }

    #[test]
    fn test_account_keys() {
        let raw_data = json!({
            "owner": {
                "weight_threshold": 1,
                "account_auths": [],
                "key_auths": [["STM5owner", 1]]
            },
            "active": {
                "weight_threshold": 2,
                "account_auths": [["activeauth", 1]],
                "key_auths": [["STM6active", 2]]
            },
            "posting": {
                "weight_threshold": 1,
                "account_auths": [],
                "key_auths": [["STM7posting", 1]]
            },
            "memo_key": "STM8memo"
        });

        let mut acc = Account::new("alice", None);
        acc.data = raw_data.as_object().unwrap().clone();

        assert_eq!(acc.memo_key().unwrap(), "STM8memo");

        let posting = acc.posting().unwrap();
        assert_eq!(posting.weight_threshold, 1);
        assert_eq!(posting.key_auths.get("STM7posting").unwrap(), &1);

        let active = acc.active().unwrap();
        assert_eq!(active.weight_threshold, 2);
        assert_eq!(active.account_auths.get("activeauth").unwrap(), &1);
        assert_eq!(active.key_auths.get("STM6active").unwrap(), &2);

        let keys = acc.get_keys();
        assert_eq!(keys.memo.unwrap(), "STM8memo");
        assert_eq!(keys.posting[0], ("STM7posting".to_string(), 1));
        assert_eq!(keys.active[0], ("STM6active".to_string(), 2));
        assert_eq!(keys.owner[0], ("STM5owner".to_string(), 1));
    }
}
