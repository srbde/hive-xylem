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

    pub fn from_bytes(bytes: &[u8], pos: &mut usize) -> Result<Self, XylemError> {
        if *pos + 16 > bytes.len() {
            return Err(XylemError::SerializationError(
                "unexpected end reading asset amount".to_string(),
            ));
        }
        let satoshis = i64::from_le_bytes([
            bytes[*pos],
            bytes[*pos + 1],
            bytes[*pos + 2],
            bytes[*pos + 3],
            bytes[*pos + 4],
            bytes[*pos + 5],
            bytes[*pos + 6],
            bytes[*pos + 7],
        ]);
        *pos += 8;
        let precision = bytes[*pos];
        *pos += 1;
        let symbol_bytes = &bytes[*pos..*pos + 7];
        *pos += 7;
        let symbol = std::str::from_utf8(symbol_bytes)
            .map_err(|e| XylemError::SerializationError(format!("invalid symbol UTF-8: {}", e)))?
            .trim_end_matches('\0')
            .to_string();
        let display_symbol = match symbol.as_str() {
            "STEEM" => "HIVE",
            "SBD" => "HBD",
            other => other,
        };
        let value = satoshis as f64 / 10f64.powi(precision as i32);
        Ok(AssetAmount {
            value,
            symbol: display_symbol.to_string(),
        })
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
    pub owner: Authority,
    pub active: Authority,
    pub posting: Authority,
    pub memo_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    pub previous: String,
    pub timestamp: HiveTime,
    pub witness: String,
    pub transaction_merkle_root: String,
}

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Authority {
    pub weight_threshold: u32,
    pub account_auths: HashMap<String, u16>,
    pub key_auths: HashMap<String, u16>,
}

impl Serialize for Authority {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        struct WireAuthority<'a> {
            weight_threshold: u32,
            account_auths: Vec<(&'a String, &'a u16)>,
            key_auths: Vec<(&'a String, &'a u16)>,
        }

        let mut account_auths: Vec<(&String, &u16)> = self.account_auths.iter().collect();
        account_auths.sort_by(|a, b| a.0.cmp(b.0));

        let mut key_auths: Vec<(&String, &u16)> = self.key_auths.iter().collect();
        key_auths.sort_by(|a, b| a.0.cmp(b.0));

        let wire = WireAuthority {
            weight_threshold: self.weight_threshold,
            account_auths,
            key_auths,
        };

        wire.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Authority {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawAuthority {
            weight_threshold: u32,
            account_auths: serde_json::Value,
            key_auths: serde_json::Value,
        }

        let raw = RawAuthority::deserialize(deserializer)?;

        let account_auths = parse_auths(raw.account_auths).map_err(de::Error::custom)?;
        let key_auths = parse_auths(raw.key_auths).map_err(de::Error::custom)?;

        Ok(Authority {
            weight_threshold: raw.weight_threshold,
            account_auths,
            key_auths,
        })
    }
}

fn parse_auths(val: serde_json::Value) -> Result<HashMap<String, u16>, String> {
    match val {
        serde_json::Value::Object(map) => {
            let mut res = HashMap::new();
            for (k, v) in map {
                let weight = v
                    .as_u64()
                    .ok_or_else(|| "invalid weight: not a number".to_string())?
                    as u16;
                res.insert(k, weight);
            }
            Ok(res)
        }
        serde_json::Value::Array(arr) => {
            let mut res = HashMap::new();
            for item in arr {
                let pair = item
                    .as_array()
                    .ok_or_else(|| "expected array of pairs".to_string())?;
                if pair.len() != 2 {
                    return Err("expected pair of [name/key, weight]".to_string());
                }
                let k = pair[0]
                    .as_str()
                    .ok_or_else(|| "key is not a string".to_string())?
                    .to_string();
                let weight = pair[1]
                    .as_u64()
                    .ok_or_else(|| "weight is not a number".to_string())?
                    as u16;
                res.insert(k, weight);
            }
            Ok(res)
        }
        _ => Err("expected object or array for authority auths".to_string()),
    }
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

impl OperationTuple {
    /// Extracts a typed transfer operation, returning `Ok(None)` for unrelated operations.
    pub fn transfer(&self) -> Result<Option<crate::operations::Transfer>, XylemError> {
        let Some(data) = self.matching_object("transfer")? else {
            return Ok(None);
        };

        let from = required_string(data, "from")?;
        let to = required_string(data, "to")?;
        let amount = required_string(data, "amount")?;
        AssetAmount::parse(&amount).map_err(|err| XylemError::MalformedAmount(err.to_string()))?;
        let memo = match data.get("memo") {
            None => String::new(),
            Some(value) => value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| XylemError::WrongOperationFieldType("memo".to_string()))?,
        };

        Ok(Some(crate::operations::Transfer {
            from,
            to,
            amount,
            memo,
        }))
    }

    /// Extracts a typed custom JSON operation, returning `Ok(None)` for unrelated operations.
    pub fn custom_json(&self) -> Result<Option<crate::operations::CustomJson>, XylemError> {
        let Some(data) = self.matching_object("custom_json")? else {
            return Ok(None);
        };

        Ok(Some(crate::operations::CustomJson {
            id: required_string(data, "id")?,
            json: required_string(data, "json")?,
            required_auths: required_string_array(data, "required_auths")?,
            required_posting_auths: required_string_array(data, "required_posting_auths")?,
        }))
    }

    pub fn custom_json_id(&self) -> Option<String> {
        if self.0 == "custom_json" {
            self.1
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        } else {
            None
        }
    }

    fn matching_object(
        &self,
        operation_type: &str,
    ) -> Result<Option<&serde_json::Map<String, serde_json::Value>>, XylemError> {
        if self.0 != operation_type {
            return Ok(None);
        }
        self.1.as_object().map(Some).ok_or_else(|| {
            XylemError::MalformedOperationTuple("operation value is not an object".to_string())
        })
    }
}

fn required_string(
    data: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<String, XylemError> {
    let value = data
        .get(field)
        .ok_or_else(|| XylemError::MissingOperationField(field.to_string()))?;
    let value = value
        .as_str()
        .ok_or_else(|| XylemError::WrongOperationFieldType(field.to_string()))?;
    if value.is_empty() {
        return Err(XylemError::MissingOperationField(field.to_string()));
    }
    Ok(value.to_string())
}

fn required_string_array(
    data: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<Vec<String>, XylemError> {
    let value = data
        .get(field)
        .ok_or_else(|| XylemError::MissingOperationField(field.to_string()))?;
    let values = value
        .as_array()
        .ok_or_else(|| XylemError::MalformedAuthorizationArray(field.to_string()))?;
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            value.as_str().map(str::to_owned).ok_or_else(|| {
                XylemError::MalformedAuthorizationArray(format!("{}[{}]", field, index))
            })
        })
        .collect()
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_operation_tuple_custom_json_id() {
        let json_data = json!(["custom_json", {"id": "hiveidentity", "json": "{}", "required_posting_auths": ["alice"]}]);
        let ot: OperationTuple = serde_json::from_value(json_data).unwrap();
        assert_eq!(ot.custom_json_id(), Some("hiveidentity".to_string()));

        let json_data_other =
            json!(["transfer", {"from": "alice", "to": "bob", "amount": "1.000 HIVE"}]);
        let ot_other: OperationTuple = serde_json::from_value(json_data_other).unwrap();
        assert_eq!(ot_other.custom_json_id(), None);

        let json_data_missing = json!(["custom_json", {"json": "{}"}]);
        let ot_missing: OperationTuple = serde_json::from_value(json_data_missing).unwrap();
        assert_eq!(ot_missing.custom_json_id(), None);
    }

    #[test]
    fn test_operation_tuple_transfer_helpers() {
        for value in [
            json!(["transfer", {"from": "alice", "to": "bob", "amount": "1.000 HIVE", "memo": "hi"}]),
            json!({"type": "transfer", "value": {"from": "alice", "to": "bob", "amount": "2.500 HBD"}}),
        ] {
            let operation: OperationTuple = serde_json::from_value(value).unwrap();
            let transfer = operation.transfer().unwrap().unwrap();
            assert_eq!(transfer.from, "alice");
            assert_eq!(transfer.to, "bob");
            assert!(transfer.amount == "1.000 HIVE" || transfer.amount == "2.500 HBD");
            assert!(transfer.memo == "hi" || transfer.memo.is_empty());
        }

        let unrelated: OperationTuple = serde_json::from_value(json!([
            "vote",
            {"voter": "alice"}
        ]))
        .unwrap();
        assert!(unrelated.transfer().unwrap().is_none());

        for field in ["from", "to", "amount"] {
            let mut data = json!({"from": "alice", "to": "bob", "amount": "1.000 HIVE"});
            data.as_object_mut().unwrap().remove(field);
            let operation = OperationTuple("transfer".to_string(), data);
            assert!(matches!(
                operation.transfer(),
                Err(XylemError::MissingOperationField(ref name)) if name == field
            ));
        }

        let wrong_type = OperationTuple(
            "transfer".to_string(),
            json!({"from": "alice", "to": "bob", "amount": 1.0}),
        );
        assert!(matches!(
            wrong_type.transfer(),
            Err(XylemError::WrongOperationFieldType(ref name)) if name == "amount"
        ));

        let malformed_amount = OperationTuple(
            "transfer".to_string(),
            json!({"from": "alice", "to": "bob", "amount": "not-an-amount"}),
        );
        assert!(matches!(
            malformed_amount.transfer(),
            Err(XylemError::MalformedAmount(_))
        ));
    }

    #[test]
    fn test_operation_tuple_custom_json_helper() {
        for value in [
            json!(["custom_json", {"id": "x/hiveidentity", "required_auths": ["alice"], "required_posting_auths": [], "json": "{\"ok\":true}"}]),
            json!({"type": "custom_json", "value": {"id": "x/hivebridge", "required_auths": [], "required_posting_auths": ["bob"], "json": "payload"}}),
        ] {
            let operation: OperationTuple = serde_json::from_value(value).unwrap();
            let custom_json = operation.custom_json().unwrap().unwrap();
            assert!(!custom_json.id.is_empty());
            assert!(!custom_json.json.is_empty());
            assert!(
                custom_json.required_auths.is_empty()
                    || custom_json.required_auths == vec!["alice".to_string()]
            );
            assert!(
                custom_json.required_posting_auths.is_empty()
                    || custom_json.required_posting_auths == vec!["bob".to_string()]
            );
        }

        let missing_id = OperationTuple(
            "custom_json".to_string(),
            json!({"json": "{}", "required_auths": [], "required_posting_auths": []}),
        );
        assert!(matches!(
            missing_id.custom_json(),
            Err(XylemError::MissingOperationField(ref name)) if name == "id"
        ));

        let malformed_auths = OperationTuple(
            "custom_json".to_string(),
            json!({"id": "id", "json": "{}", "required_auths": ["alice", 1], "required_posting_auths": []}),
        );
        assert!(matches!(
            malformed_auths.custom_json(),
            Err(XylemError::MalformedAuthorizationArray(ref name)) if name == "required_auths[1]"
        ));

        let unrelated: OperationTuple = serde_json::from_value(json!([
            "transfer",
            {"from": "alice", "to": "bob", "amount": "1.000 HIVE"}
        ]))
        .unwrap();
        assert!(unrelated.custom_json().unwrap().is_none());
    }

    #[test]
    fn test_authority_json() {
        // Deserialization from wire format
        let json_data = json!({
            "weight_threshold": 2,
            "account_auths": [["bob", 1]],
            "key_auths": [["STM5key1111111111111111111111111111111111111111111111", 2]]
        });
        let auth: Authority = serde_json::from_value(json_data).unwrap();
        assert_eq!(auth.weight_threshold, 2);
        assert_eq!(auth.account_auths.get("bob"), Some(&1));
        assert_eq!(
            auth.key_auths
                .get("STM5key1111111111111111111111111111111111111111111111"),
            Some(&2)
        );

        // Serialization
        let mut account_auths = HashMap::new();
        account_auths.insert("carol".to_string(), 2);
        account_auths.insert("bob".to_string(), 1);
        let mut key_auths = HashMap::new();
        key_auths.insert("STM5keyone".to_string(), 1);

        let auth_orig = Authority {
            weight_threshold: 3,
            account_auths,
            key_auths,
        };

        let serialized = serde_json::to_value(&auth_orig).unwrap();
        let expected = json!({
            "weight_threshold": 3,
            "account_auths": [["bob", 1], ["carol", 2]],
            "key_auths": [["STM5keyone", 1]]
        });
        assert_eq!(serialized, expected);
    }

    #[test]
    fn test_account_data_deserialization() {
        let json_data = json!({
            "name": "alice",
            "voting_power": 10000.0,
            "voting_manabar": {
                "current_mana": 1000000.0,
                "last_update_time": 1718888888
            },
            "last_vote_time": "2026-06-09T12:00:00",
            "balance": "100.000 HIVE",
            "hbd_balance": "50.000 HBD",
            "vesting_shares": "1000000.000000 VESTS",
            "created": "2026-06-09T12:00:00",
            "owner": {
                "weight_threshold": 1,
                "account_auths": [],
                "key_auths": [["STM5ownerkey1111111111111111111111111111111111111111", 1]]
            },
            "active": {
                "weight_threshold": 2,
                "account_auths": [["bob", 1]],
                "key_auths": [["STM5activekey111111111111111111111111111111111111111", 2]]
            },
            "posting": {
                "weight_threshold": 1,
                "account_auths": [],
                "key_auths": [["STM5postingkey11111111111111111111111111111111111111", 1]]
            },
            "memo_key": "STM5memokey1111111111111111111111111111111111111111111"
        });

        let acc: AccountData = serde_json::from_value(json_data).unwrap();
        assert_eq!(acc.name, "alice");
        assert_eq!(
            acc.memo_key,
            "STM5memokey1111111111111111111111111111111111111111111"
        );
        assert_eq!(acc.owner.weight_threshold, 1);
        assert_eq!(
            acc.owner
                .key_auths
                .get("STM5ownerkey1111111111111111111111111111111111111111"),
            Some(&1)
        );
        assert_eq!(acc.active.weight_threshold, 2);
        assert_eq!(acc.active.account_auths.get("bob"), Some(&1));
        assert_eq!(acc.posting.weight_threshold, 1);
    }
}
