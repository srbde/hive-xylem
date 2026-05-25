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

/// Helper to serialize a string with a varint length prefix
pub fn serialize_string(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(&serialize_varint(s.len() as u64));
    buf.extend_from_slice(s.as_bytes());
}

/// Helper to serialize a list of strings with a varint length prefix
pub fn serialize_string_array(buf: &mut Vec<u8>, arr: &[String]) {
    buf.extend_from_slice(&serialize_varint(arr.len() as u64));
    for s in arr {
        serialize_string(buf, s);
    }
}

pub trait Operation: std::fmt::Debug {
    /// Convert to JSON representation: (op_name, op_body)
    fn to_dict(&self) -> (String, Value);
    /// Serialize operation to binary bytes
    fn to_bytes(&self) -> Result<Vec<u8>, XylemError>;
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
}
