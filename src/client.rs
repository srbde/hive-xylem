use crate::errors::XylemError;
use crate::transaction::Transaction;
use crate::types::{
    AccountData, AppliedOperation, Block, BlockHeader, ChainProperties, DynamicGlobalProperties,
    HistoryItem, Price, RCInfo, StreamingMode, VestingDelegation,
};
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
    pub async fn call(&self, api: &str, method: &str, params: Value) -> Result<Value, XylemError> {
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

                let res = self.http_client.post(node_url).json(&payload).send().await;

                match res {
                    Ok(resp) => {
                        if !resp.status().is_success() {
                            last_err = Some(XylemError::RpcError(format!(
                                "HTTP status {}",
                                resp.status()
                            )));
                            self.rotate_node();
                            continue;
                        }
                        match resp.json::<Value>().await {
                            Ok(body_val) => {
                                if let Some(err_val) = body_val.get("error") {
                                    let err_code = err_val.get("code").and_then(|c| c.as_i64());
                                    if let Some(code) = err_code {
                                        if code == -32601 || code == -32603 {
                                            last_err = Some(XylemError::RpcError(format!(
                                                "node returned JSON-RPC error code {}",
                                                code
                                            )));
                                            self.rotate_node();
                                            continue;
                                        }
                                    }
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
                                last_err = Some(XylemError::RpcError(err.to_string()));
                            }
                        }
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
        let resp = self
            .call(
                "condenser_api",
                "get_dynamic_global_properties",
                serde_json::json!([]),
            )
            .await?;
        let props: DynamicGlobalProperties = serde_json::from_value(resp)?;
        Ok(props)
    }

    /// Retrieve account data.
    pub async fn get_accounts(&self, names: &[String]) -> Result<Vec<AccountData>, XylemError> {
        let resp = self
            .call("condenser_api", "get_accounts", serde_json::json!([names]))
            .await?;
        let accounts: Vec<AccountData> = serde_json::from_value(resp)?;
        Ok(accounts)
    }

    /// Broadcast transaction to network.
    pub async fn broadcast_transaction(&self, tx: &Transaction) -> Result<Value, XylemError> {
        let tx_dict = tx.to_dict();
        let resp = self
            .call(
                "condenser_api",
                "broadcast_transaction",
                serde_json::json!([tx_dict]),
            )
            .await?;
        Ok(resp)
    }

    /// Retrieve account names associated with the given public keys.
    pub async fn get_key_references(&self, keys: &[String]) -> Result<Vec<String>, XylemError> {
        let resp = self
            .call(
                "condenser_api",
                "get_key_references",
                serde_json::json!([keys]),
            )
            .await?;
        let refs: Vec<Vec<String>> = serde_json::from_value(resp)?;
        Ok(refs.into_iter().flatten().collect())
    }

    /// Retrieve node's configuration map.
    pub async fn get_config(&self) -> Result<serde_json::Value, XylemError> {
        match self
            .call("database_api", "get_config", serde_json::json!({}))
            .await
        {
            Ok(resp) => Ok(resp),
            Err(_) => {
                // Fallback to condenser_api
                self.call("condenser_api", "get_config", serde_json::json!([]))
                    .await
            }
        }
    }

    /// Retrieve current chain properties.
    pub async fn get_chain_properties(&self) -> Result<ChainProperties, XylemError> {
        let resp = self
            .call(
                "condenser_api",
                "get_chain_properties",
                serde_json::json!([]),
            )
            .await?;
        let props: ChainProperties = serde_json::from_value(resp)?;
        Ok(props)
    }

    /// Retrieve current median history price for HIVE/HBD.
    pub async fn get_current_median_history_price(&self) -> Result<Price, XylemError> {
        let resp = self
            .call(
                "condenser_api",
                "get_current_median_history_price",
                serde_json::json!([]),
            )
            .await?;
        let price: Price = serde_json::from_value(resp)?;
        Ok(price)
    }

    /// Retrieve operation history of an account.
    pub async fn get_account_history(
        &self,
        account: &str,
        start: i64,
        limit: u32,
    ) -> Result<Vec<HistoryItem>, XylemError> {
        if limit > 1000 {
            return Err(XylemError::SerializationError(
                "limit cannot exceed 1000".to_string(),
            ));
        }
        let resp = self
            .call(
                "condenser_api",
                "get_account_history",
                serde_json::json!([account, start, limit]),
            )
            .await?;
        let history: Vec<HistoryItem> = serde_json::from_value(resp)?;
        Ok(history)
    }

    /// Retrieve active vesting delegations for delegator.
    pub async fn get_vesting_delegations(
        &self,
        delegator: &str,
        start: &str,
        limit: u32,
    ) -> Result<Vec<VestingDelegation>, XylemError> {
        if limit > 1000 {
            return Err(XylemError::SerializationError(
                "limit cannot exceed 1000".to_string(),
            ));
        }
        let resp = self
            .call(
                "condenser_api",
                "get_vesting_delegations",
                serde_json::json!([delegator, start, limit]),
            )
            .await?;
        let delegations: Vec<VestingDelegation> = serde_json::from_value(resp)?;
        Ok(delegations)
    }

    /// Retrieve block header for block_num.
    pub async fn get_block_header(&self, block_num: u32) -> Result<BlockHeader, XylemError> {
        let resp = self
            .call(
                "condenser_api",
                "get_block_header",
                serde_json::json!([block_num]),
            )
            .await?;
        let header: BlockHeader = serde_json::from_value(resp)?;
        Ok(header)
    }

    /// Retrieve full block by block_num.
    pub async fn get_block(&self, block_num: u32) -> Result<Block, XylemError> {
        let resp = self
            .call("condenser_api", "get_block", serde_json::json!([block_num]))
            .await?;
        if resp.is_null() {
            return Err(XylemError::RpcError(format!(
                "block {} not found",
                block_num
            )));
        }
        let block: Block = serde_json::from_value(resp)?;
        Ok(block)
    }

    /// Retrieve applied operations in a block.
    pub async fn get_ops_in_block(
        &self,
        block_num: u32,
        only_virtual: bool,
    ) -> Result<Vec<AppliedOperation>, XylemError> {
        let resp = self
            .call(
                "condenser_api",
                "get_ops_in_block",
                serde_json::json!([block_num, only_virtual]),
            )
            .await?;
        let ops: Vec<AppliedOperation> = serde_json::from_value(resp)?;
        Ok(ops)
    }

    /// Retrieve Resource Credit resource parameters.
    pub async fn get_rc_resource_params(&self) -> Result<serde_json::Value, XylemError> {
        self.call("rc_api", "get_rc_resource_params", serde_json::json!({}))
            .await
    }

    /// Retrieve Resource Credit resource pool.
    pub async fn get_rc_resource_pool(&self) -> Result<serde_json::Value, XylemError> {
        self.call("rc_api", "get_rc_resource_pool", serde_json::json!({}))
            .await
    }

    /// Retrieve and calculate Resource Credit details for a specific account.
    pub async fn get_rc_mana(&self, account: &str) -> Result<RCInfo, XylemError> {
        if account.is_empty() {
            return Err(XylemError::SerializationError(
                "account name cannot be empty".to_string(),
            ));
        }

        let resp = self
            .call(
                "rc_api",
                "find_rc_accounts",
                serde_json::json!({ "accounts": [account] }),
            )
            .await?;

        let rc_accounts = resp
            .get("rc_accounts")
            .or_else(|| resp.get("result"))
            .and_then(|v| v.as_array())
            .ok_or_else(|| XylemError::RpcError("invalid RC accounts response".to_string()))?;

        if rc_accounts.is_empty() {
            return Err(XylemError::RpcError(format!(
                "no RC data found for account {}",
                account
            )));
        }

        let rc_account = &rc_accounts[0];
        let manabar = rc_account.get("rc_manabar");

        let max_rc = rc_account
            .get("max_rc")
            .and_then(|v| {
                v.as_i64()
                    .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
            })
            .unwrap_or(0);

        let last_mana = manabar
            .and_then(|m| m.get("current_mana"))
            .and_then(|v| {
                v.as_i64()
                    .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
                    .or_else(|| v.as_f64().map(|f| f as i64))
            })
            .unwrap_or(0);

        let last_update_time = manabar
            .and_then(|m| m.get("last_update_time"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let mut current_mana = last_mana;
        if last_update_time > 0 && max_rc > 0 {
            let diff = (current_time - last_update_time).max(0);
            let regenerated = (diff as f64 * max_rc as f64) / (5.0 * 24.0 * 60.0 * 60.0);
            current_mana = ((last_mana as f64 + regenerated) as i64).min(max_rc);
        }

        let last_percent = if max_rc > 0 {
            (last_mana as f64 / max_rc as f64) * 100.0
        } else {
            0.0
        };

        let current_percent = if max_rc > 0 {
            (current_mana as f64 / max_rc as f64) * 100.0
        } else {
            0.0
        };

        Ok(RCInfo {
            last_mana,
            current_mana,
            max_mana: max_rc,
            last_update_time,
            last_percent,
            current_percent,
        })
    }

    /// Calculate current Resource Credit percentage.
    pub async fn calculate_rc_mana(&self, account_data: &AccountData) -> Result<f64, XylemError> {
        let info = self.get_rc_mana(&account_data.name).await?;
        Ok(info.current_percent)
    }

    /// Calculate the real-time Voting Power percentage of an account.
    pub fn calculate_vp_mana(&self, account_data: &AccountData) -> f64 {
        let max_mana = 10000.0;
        let mut current_mana = account_data.voting_power;

        if account_data.voting_manabar.current_mana > 0.0 {
            current_mana = account_data.voting_manabar.current_mana;
        }

        if account_data.voting_manabar.last_update_time > 0 {
            let current_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let diff = (current_time - account_data.voting_manabar.last_update_time).max(0);
            let regenerated = (diff as f64 * max_mana) / (5.0 * 24.0 * 60.0 * 60.0);
            current_mana += regenerated;
            if current_mana > max_mana {
                current_mana = max_mana;
            }
        }

        (current_mana / max_mana) * 100.0
    }

    /// Stream blocks starting from start_block (or latest/irreversible if 0) indefinitely.
    pub fn stream_blocks(
        self: std::sync::Arc<Self>,
        start_block: u32,
        mode: StreamingMode,
    ) -> tokio::sync::mpsc::Receiver<Result<Block, XylemError>> {
        let (tx, rx) = tokio::sync::mpsc::channel(10);
        let client = self.clone();

        tokio::spawn(async move {
            let mut current = match client.get_dynamic_global_properties().await {
                Ok(props) => {
                    if mode == StreamingMode::Irreversible {
                        props.last_irreversible_block_num
                    } else {
                        props.head_block_number
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e)).await;
                    return;
                }
            };

            let mut seen = if start_block > 0 {
                if start_block > current {
                    let _ = tx
                        .send(Err(XylemError::RpcError(format!(
                            "start block {} cannot be in the future (current: {})",
                            start_block, current
                        ))))
                        .await;
                    return;
                }
                start_block
            } else {
                current
            };

            loop {
                // Poll properties
                match client.get_dynamic_global_properties().await {
                    Ok(props) => {
                        current = if mode == StreamingMode::Irreversible {
                            props.last_irreversible_block_num
                        } else {
                            props.head_block_number
                        };
                    }
                    Err(e) => {
                        if tx.send(Err(e)).await.is_err() {
                            break;
                        }
                        tokio::time::sleep(Duration::from_secs(3)).await;
                        continue;
                    }
                }

                while seen <= current {
                    match client.get_block(seen).await {
                        Ok(block) => {
                            if tx.send(Ok(block)).await.is_err() {
                                return;
                            }
                            seen += 1;
                        }
                        Err(e) => {
                            if tx.send(Err(e)).await.is_err() {
                                return;
                            }
                            tokio::time::sleep(Duration::from_secs(1)).await;
                        }
                    }
                }

                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        });

        rx
    }

    /// Stream applied operations starting from start_block (or latest/irreversible if 0), filtered by operation type.
    pub fn stream_operations(
        self: std::sync::Arc<Self>,
        start_block: u32,
        mode: StreamingMode,
        filter: Vec<String>,
    ) -> tokio::sync::mpsc::Receiver<Result<AppliedOperation, XylemError>> {
        let (tx, rx) = tokio::sync::mpsc::channel(100);
        let client = self.clone();

        tokio::spawn(async move {
            let mut current = match client.get_dynamic_global_properties().await {
                Ok(props) => {
                    if mode == StreamingMode::Irreversible {
                        props.last_irreversible_block_num
                    } else {
                        props.head_block_number
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e)).await;
                    return;
                }
            };

            let mut seen = if start_block > 0 {
                if start_block > current {
                    let _ = tx
                        .send(Err(XylemError::RpcError(format!(
                            "start block {} cannot be in the future (current: {})",
                            start_block, current
                        ))))
                        .await;
                    return;
                }
                start_block
            } else {
                current
            };

            let filter_set: std::collections::HashSet<String> = filter.into_iter().collect();

            loop {
                // Poll properties
                match client.get_dynamic_global_properties().await {
                    Ok(props) => {
                        current = if mode == StreamingMode::Irreversible {
                            props.last_irreversible_block_num
                        } else {
                            props.head_block_number
                        };
                    }
                    Err(e) => {
                        if tx.send(Err(e)).await.is_err() {
                            break;
                        }
                        tokio::time::sleep(Duration::from_secs(3)).await;
                        continue;
                    }
                }

                while seen <= current {
                    match client.get_ops_in_block(seen, false).await {
                        Ok(ops) => {
                            for op in ops {
                                let op_name = op.op.0.as_str();
                                if (filter_set.is_empty() || filter_set.contains(op_name))
                                    && tx.send(Ok(op)).await.is_err()
                                {
                                    return;
                                }
                            }
                            seen += 1;
                        }
                        Err(e) => {
                            if tx.send(Err(e)).await.is_err() {
                                return;
                            }
                            tokio::time::sleep(Duration::from_secs(1)).await;
                        }
                    }
                }

                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        });

        rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn test_client_failover() {
        // Start a local server 1 that returns 502
        let listener1 = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port1 = listener1.local_addr().unwrap().port();
        let url1 = format!("http://127.0.0.1:{}", port1);

        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener1.accept().await {
                let mut buf = [0; 1024];
                let _ = stream.read(&mut buf).await;
                let response = "HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n";
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });

        // Start a local server 2 that returns JSON-RPC -32603, then success
        let listener2 = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port2 = listener2.local_addr().unwrap().port();
        let url2 = format!("http://127.0.0.1:{}", port2);

        let call_count = Arc::new(AtomicU32::new(0));
        let call_count_clone = call_count.clone();

        tokio::spawn(async move {
            // First call (fails with JSON-RPC error)
            if let Ok((mut stream, _)) = listener2.accept().await {
                let mut buf = [0; 1024];
                let _ = stream.read(&mut buf).await;
                call_count_clone.fetch_add(1, Ordering::Relaxed);
                let response_body = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32603,"message":"Internal error"}}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    response_body.len(),
                    response_body
                );
                let _ = stream.write_all(response.as_bytes()).await;
            }
            // Second call (succeeds)
            if let Ok((mut stream, _)) = listener2.accept().await {
                let mut buf = [0; 1024];
                let _ = stream.read(&mut buf).await;
                call_count_clone.fetch_add(1, Ordering::Relaxed);
                let response_body = r#"{"jsonrpc":"2.0","id":1,"result":"success"}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    response_body.len(),
                    response_body
                );
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });

        let client = Client::new(vec![url1, url2], 2);
        let res = client.call("test", "method", json!([])).await.unwrap();
        assert_eq!(res.as_str().unwrap(), "success");
        assert_eq!(call_count.load(Ordering::Relaxed), 2);
    }
}
