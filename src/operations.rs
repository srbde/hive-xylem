use crate::errors::XylemError;
use crate::types::AssetAmount;
use serde_json::{json, Value};

/// Helper to serialize a u64 as LEB128 varint bytes
pub fn serialize_varint(mut val: u64) -> Vec<u8> {
    let mut buf = Vec::new();
    while val >= 0x80 {
        buf.push(((val & 0x7f) | 0x80) as u8);
        val >>= 7;
    }
    buf.push(val as u8);
    buf
}

/// Helper to deserialize a LEB128 varint from bytes
pub fn deserialize_varint(bytes: &[u8], pos: &mut usize) -> Result<u64, XylemError> {
    let mut result: u64 = 0;
    let mut shift = 0;
    loop {
        if *pos >= bytes.len() {
            return Err(XylemError::SerializationError(
                "unexpected end of input reading varint".to_string(),
            ));
        }
        let byte = bytes[*pos];
        *pos += 1;
        result |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 64 {
            return Err(XylemError::SerializationError(
                "varint too long".to_string(),
            ));
        }
    }
    Ok(result)
}

/// Helper to serialize a string with a varint length prefix
pub fn serialize_string(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(&serialize_varint(s.len() as u64));
    buf.extend_from_slice(s.as_bytes());
}

/// Helper to deserialize a string with a varint length prefix
pub fn deserialize_string(bytes: &[u8], pos: &mut usize) -> Result<String, XylemError> {
    let len = deserialize_varint(bytes, pos)? as usize;
    if *pos + len > bytes.len() {
        return Err(XylemError::SerializationError(
            "unexpected end of input reading string".to_string(),
        ));
    }
    let s = std::str::from_utf8(&bytes[*pos..*pos + len])
        .map_err(|e| XylemError::SerializationError(format!("invalid UTF-8: {}", e)))?;
    *pos += len;
    Ok(s.to_string())
}

/// Helper to serialize a list of strings with a varint length prefix
pub fn serialize_string_array(buf: &mut Vec<u8>, arr: &[String]) {
    buf.extend_from_slice(&serialize_varint(arr.len() as u64));
    for s in arr {
        serialize_string(buf, s);
    }
}

/// Helper to deserialize a list of strings with a varint length prefix
pub fn deserialize_string_array(bytes: &[u8], pos: &mut usize) -> Result<Vec<String>, XylemError> {
    let len = deserialize_varint(bytes, pos)? as usize;
    let mut result = Vec::with_capacity(len);
    for _ in 0..len {
        result.push(deserialize_string(bytes, pos)?);
    }
    Ok(result)
}

/// Deserialize a single operation from bytes. The op ID must already be consumed.
pub fn deserialize_op(
    op_id: u64,
    bytes: &[u8],
    pos: &mut usize,
) -> Result<Box<dyn Operation>, XylemError> {
    match op_id {
        0 => Vote::from_bytes(bytes, pos),
        1 => Comment::from_bytes(bytes, pos),
        2 => Transfer::from_bytes(bytes, pos),
        18 => CustomJson::from_bytes(bytes, pos),
        _ => Err(XylemError::SerializationError(format!(
            "unsupported operation ID: {}",
            op_id
        ))),
    }
}

pub trait Operation: std::fmt::Debug + Send {
    /// Convert to JSON representation: (op_name, op_body)
    fn to_dict(&self) -> (String, Value);
    /// Serialize operation to binary bytes
    fn to_bytes(&self) -> Result<Vec<u8>, XylemError>;
    /// Deserialize operation from binary bytes (after op ID has been read).
    /// Default implementation returns unsupported error.
    fn from_bytes(bytes: &[u8], pos: &mut usize) -> Result<Box<dyn Operation>, XylemError>
    where
        Self: Sized,
    {
        let _ = (bytes, pos);
        Err(XylemError::SerializationError(
            "deserialization not implemented for this operation".to_string(),
        ))
    }
}

#[derive(Debug, Clone)]
pub struct Vote {
    pub voter: String,
    pub author: String,
    pub permlink: String,
    pub weight: i16,
}

impl Operation for Vote {
    fn to_dict(&self) -> (String, Value) {
        (
            "vote".to_string(),
            json!({
                "voter": self.voter,
                "author": self.author,
                "permlink": self.permlink,
                "weight": self.weight
            }),
        )
    }

    fn to_bytes(&self) -> Result<Vec<u8>, XylemError> {
        let mut buf = Vec::new();
        // Op ID is 0
        buf.extend_from_slice(&serialize_varint(0));
        serialize_string(&mut buf, &self.voter);
        serialize_string(&mut buf, &self.author);
        serialize_string(&mut buf, &self.permlink);
        buf.extend_from_slice(&self.weight.to_le_bytes());
        Ok(buf)
    }

    fn from_bytes(bytes: &[u8], pos: &mut usize) -> Result<Box<dyn Operation>, XylemError> {
        let voter = deserialize_string(bytes, pos)?;
        let author = deserialize_string(bytes, pos)?;
        let permlink = deserialize_string(bytes, pos)?;
        if *pos + 2 > bytes.len() {
            return Err(XylemError::SerializationError(
                "unexpected end reading vote weight".to_string(),
            ));
        }
        let weight = i16::from_le_bytes([bytes[*pos], bytes[*pos + 1]]);
        *pos += 2;
        Ok(Box::new(Vote {
            voter,
            author,
            permlink,
            weight,
        }))
    }
}

#[derive(Debug, Clone)]
pub struct Transfer {
    pub from: String,
    pub to: String,
    pub amount: String, // e.g. "1.000 HIVE"
    pub memo: String,
}

impl Operation for Transfer {
    fn to_dict(&self) -> (String, Value) {
        (
            "transfer".to_string(),
            json!({
                "from": self.from,
                "to": self.to,
                "amount": self.amount,
                "memo": self.memo
            }),
        )
    }

    fn to_bytes(&self) -> Result<Vec<u8>, XylemError> {
        let mut buf = Vec::new();
        // Op ID is 2
        buf.extend_from_slice(&serialize_varint(2));
        serialize_string(&mut buf, &self.from);
        serialize_string(&mut buf, &self.to);

        let asset = AssetAmount::parse(&self.amount)?;
        buf.extend_from_slice(&asset.to_bytes()?);

        serialize_string(&mut buf, &self.memo);
        Ok(buf)
    }

    fn from_bytes(bytes: &[u8], pos: &mut usize) -> Result<Box<dyn Operation>, XylemError> {
        let from = deserialize_string(bytes, pos)?;
        let to = deserialize_string(bytes, pos)?;
        let asset = AssetAmount::from_bytes(bytes, pos)?;
        let memo = deserialize_string(bytes, pos)?;
        Ok(Box::new(Transfer {
            from,
            to,
            amount: asset.to_string(),
            memo,
        }))
    }
}

#[derive(Debug, Clone)]
pub struct Comment {
    pub parent_author: String,
    pub parent_permlink: String,
    pub author: String,
    pub permlink: String,
    pub title: String,
    pub body: String,
    pub json_metadata: String,
}

impl Operation for Comment {
    fn to_dict(&self) -> (String, Value) {
        (
            "comment".to_string(),
            json!({
                "parent_author": self.parent_author,
                "parent_permlink": self.parent_permlink,
                "author": self.author,
                "permlink": self.permlink,
                "title": self.title,
                "body": self.body,
                "json_metadata": self.json_metadata
            }),
        )
    }

    fn to_bytes(&self) -> Result<Vec<u8>, XylemError> {
        let mut buf = Vec::new();
        // Op ID is 1
        buf.extend_from_slice(&serialize_varint(1));
        serialize_string(&mut buf, &self.parent_author);
        serialize_string(&mut buf, &self.parent_permlink);
        serialize_string(&mut buf, &self.author);
        serialize_string(&mut buf, &self.permlink);
        serialize_string(&mut buf, &self.title);
        serialize_string(&mut buf, &self.body);
        serialize_string(&mut buf, &self.json_metadata);
        Ok(buf)
    }

    fn from_bytes(bytes: &[u8], pos: &mut usize) -> Result<Box<dyn Operation>, XylemError> {
        let parent_author = deserialize_string(bytes, pos)?;
        let parent_permlink = deserialize_string(bytes, pos)?;
        let author = deserialize_string(bytes, pos)?;
        let permlink = deserialize_string(bytes, pos)?;
        let title = deserialize_string(bytes, pos)?;
        let body = deserialize_string(bytes, pos)?;
        let json_metadata = deserialize_string(bytes, pos)?;
        Ok(Box::new(Comment {
            parent_author,
            parent_permlink,
            author,
            permlink,
            title,
            body,
            json_metadata,
        }))
    }
}

#[derive(Debug, Clone)]
pub struct CustomJson {
    pub id: String,
    pub json: String,
    pub required_auths: Vec<String>,
    pub required_posting_auths: Vec<String>,
}

impl Operation for CustomJson {
    fn to_dict(&self) -> (String, Value) {
        (
            "custom_json".to_string(),
            json!({
                "id": self.id,
                "json": self.json,
                "required_auths": self.required_auths,
                "required_posting_auths": self.required_posting_auths
            }),
        )
    }

    fn to_bytes(&self) -> Result<Vec<u8>, XylemError> {
        let mut buf = Vec::new();
        // Op ID is 18
        buf.extend_from_slice(&serialize_varint(18));
        serialize_string_array(&mut buf, &self.required_auths);
        serialize_string_array(&mut buf, &self.required_posting_auths);
        serialize_string(&mut buf, &self.id);
        serialize_string(&mut buf, &self.json);
        Ok(buf)
    }

    fn from_bytes(bytes: &[u8], pos: &mut usize) -> Result<Box<dyn Operation>, XylemError> {
        let required_auths = deserialize_string_array(bytes, pos)?;
        let required_posting_auths = deserialize_string_array(bytes, pos)?;
        let id = deserialize_string(bytes, pos)?;
        let json = deserialize_string(bytes, pos)?;
        Ok(Box::new(CustomJson {
            id,
            json,
            required_auths,
            required_posting_auths,
        }))
    }
}

#[derive(Debug, Clone)]
pub struct Follow {
    pub follower: String,
    pub following: String,
    pub what: Vec<String>,
}

impl Operation for Follow {
    fn to_dict(&self) -> (String, Value) {
        let follow_json = json!([
            "follow",
            {
                "follower": self.follower,
                "following": self.following,
                "what": self.what
            }
        ]);
        (
            "custom_json".to_string(),
            json!({
                "id": "follow",
                "json": follow_json.to_string(),
                "required_auths": Vec::<String>::new(),
                "required_posting_auths": vec![self.follower.clone()]
            }),
        )
    }

    fn to_bytes(&self) -> Result<Vec<u8>, XylemError> {
        let (_, op_body) = self.to_dict();
        let custom_json = CustomJson {
            id: op_body["id"].as_str().unwrap().to_string(),
            json: op_body["json"].as_str().unwrap().to_string(),
            required_auths: vec![],
            required_posting_auths: vec![self.follower.clone()],
        };
        custom_json.to_bytes()
    }
}

pub fn serialize_public_key(buf: &mut Vec<u8>, pub_key_str: &str) -> Result<(), XylemError> {
    if pub_key_str.is_empty() {
        buf.extend(std::iter::repeat_n(0, 33));
        return Ok(());
    }
    let mut trimmed = pub_key_str;
    if pub_key_str.len() > 3 && (pub_key_str.starts_with("STM") || pub_key_str.starts_with("TST")) {
        trimmed = &pub_key_str[3..];
    }
    let decoded = bs58::decode(trimmed)
        .into_vec()
        .map_err(|e| XylemError::SerializationError(format!("invalid public key base58: {}", e)))?;
    if decoded.len() < 33 {
        return Err(XylemError::SerializationError(format!(
            "invalid public key length: {}",
            decoded.len()
        )));
    }
    buf.extend_from_slice(&decoded[..33]);
    Ok(())
}

pub fn serialize_authority(
    buf: &mut Vec<u8>,
    auth: &crate::types::Authority,
) -> Result<(), XylemError> {
    buf.extend_from_slice(&auth.weight_threshold.to_le_bytes());

    let mut acc_names: Vec<&String> = auth.account_auths.keys().collect();
    acc_names.sort();

    buf.extend_from_slice(&serialize_varint(acc_names.len() as u64));
    for name in acc_names {
        serialize_string(buf, name);
        let weight = auth.account_auths.get(name).unwrap();
        buf.extend_from_slice(&weight.to_le_bytes());
    }

    let mut key_strs: Vec<&String> = auth.key_auths.keys().collect();
    key_strs.sort();

    buf.extend_from_slice(&serialize_varint(key_strs.len() as u64));
    for key in key_strs {
        serialize_public_key(buf, key)?;
        let weight = auth.key_auths.get(key).unwrap();
        buf.extend_from_slice(&weight.to_le_bytes());
    }

    Ok(())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct BeneficiaryRoute {
    pub account: String,
    pub weight: u16,
}

#[derive(Debug, Clone)]
pub struct CommentPayoutBeneficiaries {
    pub beneficiaries: Vec<BeneficiaryRoute>,
}

impl CommentPayoutBeneficiaries {
    pub fn to_bytes(&self) -> Result<Vec<u8>, XylemError> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&serialize_varint(self.beneficiaries.len() as u64));
        for b in &self.beneficiaries {
            serialize_string(&mut buf, &b.account);
            buf.extend_from_slice(&b.weight.to_le_bytes());
        }
        Ok(buf)
    }
}

#[derive(Debug, Clone)]
pub enum CommentExtension {
    Beneficiaries(CommentPayoutBeneficiaries),
}

#[derive(Debug, Clone)]
pub struct CommentOptions {
    pub author: String,
    pub permlink: String,
    pub max_accepted_payout: String,
    pub percent_hbd: u16,
    pub allow_votes: bool,
    pub allow_curation_rewards: bool,
    pub extensions: Vec<CommentExtension>,
}

impl Operation for CommentOptions {
    fn to_dict(&self) -> (String, Value) {
        let mut exts_array = Vec::new();
        for ext in &self.extensions {
            match ext {
                CommentExtension::Beneficiaries(b) => {
                    exts_array.push(json!([
                        0,
                        { "beneficiaries": b.beneficiaries }
                    ]));
                }
            }
        }

        (
            "comment_options".to_string(),
            json!({
                "author": self.author,
                "permlink": self.permlink,
                "max_accepted_payout": self.max_accepted_payout,
                "percent_hbd": self.percent_hbd,
                "allow_votes": self.allow_votes,
                "allow_curation_rewards": self.allow_curation_rewards,
                "extensions": exts_array
            }),
        )
    }

    fn to_bytes(&self) -> Result<Vec<u8>, XylemError> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&serialize_varint(19)); // ID 19
        serialize_string(&mut buf, &self.author);
        serialize_string(&mut buf, &self.permlink);

        let asset = AssetAmount::parse(&self.max_accepted_payout)?;
        buf.extend_from_slice(&asset.to_bytes()?);

        buf.extend_from_slice(&self.percent_hbd.to_le_bytes());
        buf.push(if self.allow_votes { 1 } else { 0 });
        buf.push(if self.allow_curation_rewards { 1 } else { 0 });

        buf.extend_from_slice(&serialize_varint(self.extensions.len() as u64));
        for ext in &self.extensions {
            match ext {
                CommentExtension::Beneficiaries(b) => {
                    buf.extend_from_slice(&serialize_varint(0)); // variant ID 0
                    buf.extend_from_slice(&b.to_bytes()?);
                }
            }
        }

        Ok(buf)
    }
}

#[derive(Debug, Clone)]
pub struct DeleteComment {
    pub author: String,
    pub permlink: String,
}

impl Operation for DeleteComment {
    fn to_dict(&self) -> (String, Value) {
        (
            "delete_comment".to_string(),
            json!({
                "author": self.author,
                "permlink": self.permlink
            }),
        )
    }

    fn to_bytes(&self) -> Result<Vec<u8>, XylemError> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&serialize_varint(17)); // ID 17
        serialize_string(&mut buf, &self.author);
        serialize_string(&mut buf, &self.permlink);
        Ok(buf)
    }
}

#[derive(Debug, Clone)]
pub struct TransferToVesting {
    pub from: String,
    pub to: String,
    pub amount: String,
}

impl Operation for TransferToVesting {
    fn to_dict(&self) -> (String, Value) {
        (
            "transfer_to_vesting".to_string(),
            json!({
                "from": self.from,
                "to": self.to,
                "amount": self.amount
            }),
        )
    }

    fn to_bytes(&self) -> Result<Vec<u8>, XylemError> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&serialize_varint(3)); // ID 3
        serialize_string(&mut buf, &self.from);
        serialize_string(&mut buf, &self.to);
        let asset = AssetAmount::parse(&self.amount)?;
        buf.extend_from_slice(&asset.to_bytes()?);
        Ok(buf)
    }
}

#[derive(Debug, Clone)]
pub struct WithdrawVesting {
    pub account: String,
    pub vesting_shares: String,
}

impl Operation for WithdrawVesting {
    fn to_dict(&self) -> (String, Value) {
        (
            "withdraw_vesting".to_string(),
            json!({
                "account": self.account,
                "vesting_shares": self.vesting_shares
            }),
        )
    }

    fn to_bytes(&self) -> Result<Vec<u8>, XylemError> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&serialize_varint(4)); // ID 4
        serialize_string(&mut buf, &self.account);
        let asset = AssetAmount::parse(&self.vesting_shares)?;
        buf.extend_from_slice(&asset.to_bytes()?);
        Ok(buf)
    }
}

#[derive(Debug, Clone)]
pub struct DelegateVestingShares {
    pub delegator: String,
    pub delegatee: String,
    pub vesting_shares: String,
}

impl Operation for DelegateVestingShares {
    fn to_dict(&self) -> (String, Value) {
        (
            "delegate_vesting_shares".to_string(),
            json!({
                "delegator": self.delegator,
                "delegatee": self.delegatee,
                "vesting_shares": self.vesting_shares
            }),
        )
    }

    fn to_bytes(&self) -> Result<Vec<u8>, XylemError> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&serialize_varint(40)); // ID 40
        serialize_string(&mut buf, &self.delegator);
        serialize_string(&mut buf, &self.delegatee);
        let asset = AssetAmount::parse(&self.vesting_shares)?;
        buf.extend_from_slice(&asset.to_bytes()?);
        Ok(buf)
    }
}

#[derive(Debug, Clone)]
pub struct ClaimRewardBalance {
    pub account: String,
    pub reward_hive: String,
    pub reward_hbd: String,
    pub reward_vests: String,
}

impl Operation for ClaimRewardBalance {
    fn to_dict(&self) -> (String, Value) {
        (
            "claim_reward_balance".to_string(),
            json!({
                "account": self.account,
                "reward_hive": self.reward_hive,
                "reward_hbd": self.reward_hbd,
                "reward_vests": self.reward_vests
            }),
        )
    }

    fn to_bytes(&self) -> Result<Vec<u8>, XylemError> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&serialize_varint(39)); // ID 39
        serialize_string(&mut buf, &self.account);

        let asset_hive = AssetAmount::parse(&self.reward_hive)?;
        buf.extend_from_slice(&asset_hive.to_bytes()?);

        let asset_hbd = AssetAmount::parse(&self.reward_hbd)?;
        buf.extend_from_slice(&asset_hbd.to_bytes()?);

        let asset_vests = AssetAmount::parse(&self.reward_vests)?;
        buf.extend_from_slice(&asset_vests.to_bytes()?);

        Ok(buf)
    }
}

#[derive(Debug, Clone)]
pub struct RecurrentTransfer {
    pub from: String,
    pub to: String,
    pub amount: String,
    pub memo: String,
    pub recurrence: u16,
    pub executions: u16,
}

impl Operation for RecurrentTransfer {
    fn to_dict(&self) -> (String, Value) {
        (
            "recurrent_transfer".to_string(),
            json!({
                "from": self.from,
                "to": self.to,
                "amount": self.amount,
                "memo": self.memo,
                "recurrence": self.recurrence,
                "executions": self.executions,
                "extensions": Vec::<Value>::new()
            }),
        )
    }

    fn to_bytes(&self) -> Result<Vec<u8>, XylemError> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&serialize_varint(49)); // ID 49
        serialize_string(&mut buf, &self.from);
        serialize_string(&mut buf, &self.to);
        let asset = AssetAmount::parse(&self.amount)?;
        buf.extend_from_slice(&asset.to_bytes()?);
        serialize_string(&mut buf, &self.memo);
        buf.extend_from_slice(&self.recurrence.to_le_bytes());
        buf.extend_from_slice(&self.executions.to_le_bytes());
        buf.push(0); // extensions: empty array -> 0
        Ok(buf)
    }
}

#[derive(Debug, Clone)]
pub struct ClaimAccount {
    pub creator: String,
    pub fee: String,
}

impl Operation for ClaimAccount {
    fn to_dict(&self) -> (String, Value) {
        (
            "claim_account".to_string(),
            json!({
                "creator": self.creator,
                "fee": self.fee,
                "extensions": Vec::<Value>::new()
            }),
        )
    }

    fn to_bytes(&self) -> Result<Vec<u8>, XylemError> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&serialize_varint(22)); // ID 22
        serialize_string(&mut buf, &self.creator);
        let asset = AssetAmount::parse(&self.fee)?;
        buf.extend_from_slice(&asset.to_bytes()?);
        buf.push(0); // extensions: empty array -> 0
        Ok(buf)
    }
}

#[derive(Debug, Clone)]
pub struct CreateClaimedAccount {
    pub creator: String,
    pub new_account_name: String,
    pub owner: crate::types::Authority,
    pub active: crate::types::Authority,
    pub posting: crate::types::Authority,
    pub memo_key: String,
    pub json_metadata: String,
}

impl Operation for CreateClaimedAccount {
    fn to_dict(&self) -> (String, Value) {
        (
            "create_claimed_account".to_string(),
            json!({
                "creator": self.creator,
                "new_account_name": self.new_account_name,
                "owner": self.owner,
                "active": self.active,
                "posting": self.posting,
                "memo_key": self.memo_key,
                "json_metadata": self.json_metadata,
                "extensions": Vec::<Value>::new()
            }),
        )
    }

    fn to_bytes(&self) -> Result<Vec<u8>, XylemError> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&serialize_varint(23)); // ID 23
        serialize_string(&mut buf, &self.creator);
        serialize_string(&mut buf, &self.new_account_name);
        serialize_authority(&mut buf, &self.owner)?;
        serialize_authority(&mut buf, &self.active)?;
        serialize_authority(&mut buf, &self.posting)?;
        serialize_public_key(&mut buf, &self.memo_key)?;
        serialize_string(&mut buf, &self.json_metadata);
        buf.push(0); // extensions: empty array -> 0
        Ok(buf)
    }
}

#[derive(Debug, Clone)]
pub struct AccountUpdate {
    pub account: String,
    pub owner: Option<crate::types::Authority>,
    pub active: Option<crate::types::Authority>,
    pub posting: Option<crate::types::Authority>,
    pub memo_key: String,
    pub json_metadata: String,
}

impl Operation for AccountUpdate {
    fn to_dict(&self) -> (String, Value) {
        let mut dict = json!({
            "account": self.account,
            "memo_key": self.memo_key,
            "json_metadata": self.json_metadata
        });

        if let Some(ref o) = self.owner {
            dict["owner"] = json!(o);
        }
        if let Some(ref a) = self.active {
            dict["active"] = json!(a);
        }
        if let Some(ref p) = self.posting {
            dict["posting"] = json!(p);
        }

        ("account_update".to_string(), dict)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, XylemError> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&serialize_varint(10)); // ID 10
        serialize_string(&mut buf, &self.account);

        if let Some(ref o) = self.owner {
            buf.push(1);
            serialize_authority(&mut buf, o)?;
        } else {
            buf.push(0);
        }

        if let Some(ref a) = self.active {
            buf.push(1);
            serialize_authority(&mut buf, a)?;
        } else {
            buf.push(0);
        }

        if let Some(ref p) = self.posting {
            buf.push(1);
            serialize_authority(&mut buf, p)?;
        } else {
            buf.push(0);
        }

        serialize_public_key(&mut buf, &self.memo_key)?;
        serialize_string(&mut buf, &self.json_metadata);

        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Authority;
    use std::collections::HashMap;

    #[test]
    fn test_operations_serialization() {
        // Test DeleteComment
        let op = DeleteComment {
            author: "alice".to_string(),
            permlink: "test-post".to_string(),
        };
        let bytes = op.to_bytes().unwrap();
        assert!(!bytes.is_empty());
        let (name, dict) = op.to_dict();
        assert_eq!(name, "delete_comment");
        assert_eq!(dict["author"], "alice");

        // Test TransferToVesting
        let op = TransferToVesting {
            from: "alice".to_string(),
            to: "bob".to_string(),
            amount: "10.000 HIVE".to_string(),
        };
        let bytes = op.to_bytes().unwrap();
        assert!(!bytes.is_empty());

        // Test WithdrawVesting
        let op = WithdrawVesting {
            account: "alice".to_string(),
            vesting_shares: "100.000000 VESTS".to_string(),
        };
        let bytes = op.to_bytes().unwrap();
        assert!(!bytes.is_empty());

        // Test DelegateVestingShares
        let op = DelegateVestingShares {
            delegator: "alice".to_string(),
            delegatee: "bob".to_string(),
            vesting_shares: "50.000000 VESTS".to_string(),
        };
        let bytes = op.to_bytes().unwrap();
        assert!(!bytes.is_empty());

        // Test ClaimRewardBalance
        let op = ClaimRewardBalance {
            account: "alice".to_string(),
            reward_hive: "1.000 HIVE".to_string(),
            reward_hbd: "2.000 HBD".to_string(),
            reward_vests: "3.000000 VESTS".to_string(),
        };
        let bytes = op.to_bytes().unwrap();
        assert!(!bytes.is_empty());

        // Test RecurrentTransfer
        let op = RecurrentTransfer {
            from: "alice".to_string(),
            to: "bob".to_string(),
            amount: "5.000 HIVE".to_string(),
            memo: "monthly allowance".to_string(),
            recurrence: 24,
            executions: 12,
        };
        let bytes = op.to_bytes().unwrap();
        assert!(!bytes.is_empty());

        // Test ClaimAccount
        let op = ClaimAccount {
            creator: "alice".to_string(),
            fee: "0.000 HIVE".to_string(),
        };
        let bytes = op.to_bytes().unwrap();
        assert!(!bytes.is_empty());

        // Test CreateClaimedAccount
        let auth = Authority {
            weight_threshold: 1,
            account_auths: HashMap::new(),
            key_auths: HashMap::new(),
        };
        let op = CreateClaimedAccount {
            creator: "alice".to_string(),
            new_account_name: "charlie".to_string(),
            owner: auth.clone(),
            active: auth.clone(),
            posting: auth.clone(),
            memo_key: "STM11111111111111111111111111111111111111111111111111".to_string(),
            json_metadata: "{}".to_string(),
        };
        let bytes = op.to_bytes().unwrap();
        assert!(!bytes.is_empty());

        // Test AccountUpdate
        let op = AccountUpdate {
            account: "alice".to_string(),
            owner: Some(auth.clone()),
            active: None,
            posting: Some(auth),
            memo_key: "STM11111111111111111111111111111111111111111111111111".to_string(),
            json_metadata: "{}".to_string(),
        };
        let bytes = op.to_bytes().unwrap();
        assert!(!bytes.is_empty());

        // Test CommentOptions
        let op = CommentOptions {
            author: "alice".to_string(),
            permlink: "test-post".to_string(),
            max_accepted_payout: "1000000.000 HBD".to_string(),
            percent_hbd: 10000,
            allow_votes: true,
            allow_curation_rewards: true,
            extensions: vec![CommentExtension::Beneficiaries(
                CommentPayoutBeneficiaries {
                    beneficiaries: vec![BeneficiaryRoute {
                        account: "bob".to_string(),
                        weight: 5000,
                    }],
                },
            )],
        };
        let bytes = op.to_bytes().unwrap();
        assert!(!bytes.is_empty());
    }
}
