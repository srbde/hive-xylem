use crate::errors::XylemError;
use chrono::NaiveDateTime;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

// Hive datetime format: YYYY-MM-DDTHH:MM:SS
const HIVE_TIME_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HiveTime(pub NaiveDateTime);

impl Serialize for HiveTime {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let s = self.0.format(HIVE_TIME_FORMAT).to_string();
        serializer.serialize_str(&s)
    }
}

impl<'de> Deserialize<'de> for HiveTime {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let dt = NaiveDateTime::parse_from_str(&s, HIVE_TIME_FORMAT).map_err(de::Error::custom)?;
        Ok(HiveTime(dt))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssetAmount {
    pub value: f64,
    pub symbol: String,
}

impl AssetAmount {
    pub fn parse(s: &str) -> Result<Self, XylemError> {
        let parts: Vec<&str> = s.split_whitespace().collect();
        if parts.len() != 2 {
            return Err(XylemError::SerializationError(format!(
                "invalid asset amount format: {}",
                s
            )));
        }

        let value = parts[0]
            .parse::<f64>()
            .map_err(|e| XylemError::SerializationError(format!("invalid float value: {}", e)))?;

        Ok(AssetAmount {
            value,
            symbol: parts[1].to_string(),
        })
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, XylemError> {
        let precision = match self.symbol.as_str() {
            "HIVE" | "STEEM" => 3,
            "HBD" | "SBD" => 3,
            "VESTS" => 6,
            _ => {
                return Err(XylemError::SerializationError(format!(
                    "unknown asset symbol: {}",
                    self.symbol
                )))
            }
        };

        // Convert amount to satoshis
        let satoshis = (self.value * 10f64.powi(precision as i32)).round() as i64;

        // legacy wire symbols for signing
        let wire_symbol = match self.symbol.as_str() {
            "HIVE" => "STEEM",
            "HBD" => "SBD",
            other => other,
        };

        if wire_symbol.len() > 7 {
            return Err(XylemError::SerializationError(
                "asset symbol cannot exceed 7 characters".to_string(),
            ));
        }

        let mut buf = Vec::new();
        // Write satoshis as little-endian i64
        buf.extend_from_slice(&satoshis.to_le_bytes());
        // Write precision as u8
        buf.push(precision);

        // Write symbol padded to 7 bytes
        let symbol_bytes = wire_symbol.as_bytes();
        buf.extend_from_slice(symbol_bytes);
        buf.extend(std::iter::repeat_n(0, 7 - symbol_bytes.len()));

        Ok(buf)
    }
}

impl fmt::Display for AssetAmount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let precision = match self.symbol.as_str() {
            "HIVE" | "STEEM" => 3,
            "HBD" | "SBD" => 3,
            "VESTS" => 6,
            _ => 3,
        };
        write!(f, "{:.*} {}", precision, self.value, self.symbol)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicGlobalProperties {
    pub head_block_number: u32,
    pub head_block_id: String,
    pub time: HiveTime,
    pub last_irreversible_block_num: u32,
    pub total_vesting_fund_hive: String,
    pub total_vesting_shares: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manabar {
    #[serde(deserialize_with = "deserialize_mana_val")]
    pub current_mana: f64,
    pub last_update_time: i64,
}

// Deserialize current_mana supporting both string and numeric types robustly
fn deserialize_mana_val<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let value: serde_json::Value = Deserialize::deserialize(deserializer)?;
    match value {
        serde_json::Value::Number(num) => num
            .as_f64()
            .ok_or_else(|| de::Error::custom("invalid float number")),
        serde_json::Value::String(s) => s.parse::<f64>().map_err(de::Error::custom),
        _ => Err(de::Error::custom("unexpected mana value type")),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountData {
    pub name: String,
    pub voting_power: f64,
    pub voting_manabar: Manabar,
    pub last_vote_time: HiveTime,
    pub balance: String,
    pub hbd_balance: String,
    pub vesting_shares: String,
    pub created: HiveTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    pub previous: String,
    pub timestamp: HiveTime,
    pub witness: String,
    pub transaction_merkle_root: String,
}

use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Authority {
    pub weight_threshold: u32,
    pub account_auths: HashMap<String, u16>,
    pub key_auths: HashMap<String, u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RCInfo {
    pub last_mana: i64,
    pub current_mana: i64,
    pub max_mana: i64,
    pub last_update_time: i64,
    pub last_percent: f64,
    pub current_percent: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Price {
    pub base: String,
    pub quote: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainProperties {
    pub account_creation_fee: String,
    pub maximum_block_size: u32,
    pub hbd_interest_rate: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VestingDelegation {
    pub id: u64,
    pub delegator: String,
    pub delegatee: String,
    pub vesting_shares: String,
    pub min_delegation_time: HiveTime,
}

#[derive(Debug, Clone, Serialize)]
pub struct OperationTuple(pub String, pub serde_json::Value);

impl<'de> Deserialize<'de> for OperationTuple {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let val = serde_json::Value::deserialize(deserializer)?;
        if let Some(arr) = val.as_array() {
            if arr.len() == 2 {
                let name = arr[0]
                    .as_str()
                    .ok_or_else(|| de::Error::custom("operation name is not a string"))?
                    .to_string();
                let body = arr[1].clone();
                return Ok(OperationTuple(name, body));
            }
            return Err(de::Error::custom("invalid operation tuple array size"));
        } else if let Some(obj) = val.as_object() {
            let name = obj
                .get("type")
                .and_then(|t| t.as_str())
                .ok_or_else(|| de::Error::custom("operation object missing string field 'type'"))?
                .to_string();
            let body = obj.get("value").cloned().unwrap_or(serde_json::Value::Null);
            return Ok(OperationTuple(name, body));
        }
        Err(de::Error::custom("invalid operation tuple format"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionInBlock {
    pub ref_block_num: u16,
    pub ref_block_prefix: u32,
    pub expiration: HiveTime,
    pub operations: Vec<OperationTuple>,
    pub extensions: Vec<serde_json::Value>,
    pub signatures: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub block_id: String,
    pub previous: String,
    pub timestamp: HiveTime,
    pub witness: String,
    pub transaction_merkle_root: String,
    pub extensions: Vec<serde_json::Value>,
    pub witness_signature: String,
    pub transactions: Vec<TransactionInBlock>,
    pub transaction_ids: Vec<String>,
    pub signing_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppliedOperation {
    pub trx_id: String,
    pub block: u32,
    pub trx_in_block: u32,
    pub op_in_trx: u32,
    pub virtual_op: bool,
    pub op: OperationTuple,
}

#[derive(Debug, Clone, Serialize)]
pub struct HistoryItem {
    pub seq: u64,
    pub op: AppliedOperation,
}

impl<'de> Deserialize<'de> for HistoryItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let arr = Vec::<serde_json::Value>::deserialize(deserializer)?;
        if arr.len() != 2 {
            return Err(de::Error::custom(
                "invalid history item format: expected 2 elements",
            ));
        }
        let seq = arr[0]
            .as_u64()
            .ok_or_else(|| de::Error::custom("history item sequence is not a u64"))?;
        let op: AppliedOperation =
            serde_json::from_value(arr[1].clone()).map_err(de::Error::custom)?;
        Ok(HistoryItem { seq, op })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamingMode {
    #[serde(rename = "latest")]
    Latest,
    #[serde(rename = "irreversible")]
    Irreversible,
}
