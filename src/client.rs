use crate::errors::XylemError;
use crate::transaction::Transaction;
use crate::types::{AccountData, DynamicGlobalProperties};
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

pub struct Client {
    nodes: Vec<String>,
    current_node_index: AtomicUsize,
    http_client: reqwest::Client,
}

impl Client {
    pub fn new(nodes: Vec<String>, timeout_secs: u64) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Client {
            nodes,
            current_node_index: AtomicUsize::new(0),
            http_client,
        }
    }

    fn get_current_node(&self) -> &str {
        if self.nodes.is_empty() {
            return "";
        }
        let idx = self.current_node_index.load(Ordering::Relaxed);
        &self.nodes[idx % self.nodes.len()]
    }

    fn rotate_node(&self) -> &str {
        if self.nodes.is_empty() {
            return "";
        }
        let prev = self.current_node_index.fetch_add(1, Ordering::Relaxed);
        &self.nodes[(prev + 1) % self.nodes.len()]
    }

    /// Make a JSON-RPC call to Hive node.
    pub async fn call(
        &self,
        api: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, XylemError> {
        if self.nodes.is_empty() {
            return Err(XylemError::RpcError("no nodes configured".to_string()));
        }

        let mut last_err = None;
        let mut backoff = Duration::from_millis(100);

        for attempt in 0..3 {
            for _ in 0..self.nodes.len() {
                let node_url = self.get_current_node();

                let payload = serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": format!("{}.{}", api, method),
                    "params": params,
                    "id": 1
                });

                let res = self
                    .http_client
                    .post(node_url)
                    .json(&payload)
                    .send()
                    .await;

                match res {
                    Ok(resp) => {
                        let body_val: Value = resp.json().await?;
                        if let Some(err_val) = body_val.get("error") {
                            let err_msg = err_val
                                .get("message")
                                .and_then(|m| m.as_str())
                                .unwrap_or("RPC error");
                            return Err(XylemError::RpcError(err_msg.to_string()));
                        }
                        if let Some(res_val) = body_val.get("result") {
                            return Ok(res_val.clone());
                        }
                        last_err = Some(XylemError::RpcError(
                            "invalid response format".to_string(),
                        ));
                    }
                    Err(err) => {
                        last_err = Some(XylemError::HttpError(err.to_string()));
                    }
                }

                // rotate node on error
                self.rotate_node();
            }

            if attempt < 2 {
                tokio::time::sleep(backoff).await;
                backoff *= 2;
            }
        }

        Err(last_err.unwrap_or_else(|| XylemError::RpcError("all nodes failed".to_string())))
    }

    /// Retrieve dynamic global properties.
    pub async fn get_dynamic_global_properties(
        &self,
    ) -> Result<DynamicGlobalProperties, XylemError> {
        let resp = self.call("condenser_api", "get_dynamic_global_properties", serde_json::json!([])).await?;
        let props: DynamicGlobalProperties = serde_json::from_value(resp)?;
        Ok(props)
    }

    /// Retrieve account data.
    pub async fn get_accounts(&self, names: &[String]) -> Result<Vec<AccountData>, XylemError> {
        let resp = self.call("condenser_api", "get_accounts", serde_json::json!([names])).await?;
        let accounts: Vec<AccountData> = serde_json::from_value(resp)?;
        Ok(accounts)
    }

    /// Broadcast transaction to network.
    pub async fn broadcast_transaction(&self, tx: &Transaction) -> Result<Value, XylemError> {
        let tx_dict = tx.to_dict();
        let resp = self.call("condenser_api", "broadcast_transaction", serde_json::json!([tx_dict])).await?;
        Ok(resp)
    }

    /// Retrieve account names associated with the given public keys.
    pub async fn get_key_references(&self, keys: &[String]) -> Result<Vec<String>, XylemError> {
        let resp = self.call("condenser_api", "get_key_references", serde_json::json!([keys])).await?;
        let refs: Vec<Vec<String>> = serde_json::from_value(resp)?;
        Ok(refs.into_iter().flatten().collect())
    }
}
