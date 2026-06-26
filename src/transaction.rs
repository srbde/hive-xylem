use crate::errors::XylemError;
use crate::operations::{deserialize_op, deserialize_varint, Operation};
use crate::types::{Authority, HiveTime};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub struct Transaction {
    pub ref_block_num: u16,
    pub ref_block_prefix: u32,
    pub expiration: HiveTime,
    pub operations: Vec<Box<dyn Operation>>,
    pub signatures: Vec<String>,
}

impl Transaction {
    pub fn new(ref_block_num: u16, ref_block_prefix: u32, expiration: HiveTime) -> Self {
        Transaction {
            ref_block_num,
            ref_block_prefix,
            expiration,
            operations: Vec::new(),
            signatures: Vec::new(),
        }
    }

    /// Deserialize transaction from binary bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, XylemError> {
        let mut pos = 0;

        if bytes.len() < 14 {
            return Err(XylemError::SerializationError(
                "transaction too short".to_string(),
            ));
        }

        let ref_block_num = u16::from_le_bytes([bytes[0], bytes[1]]);
        pos += 2;
        let ref_block_prefix = u32::from_le_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]);
        pos += 4;
        let exp_seconds = u32::from_le_bytes([bytes[6], bytes[7], bytes[8], bytes[9]]);
        pos += 4;
        let expiration = HiveTime(
            chrono::DateTime::from_timestamp(exp_seconds as i64, 0)
                .map(|dt| dt.naive_utc())
                .ok_or_else(|| {
                    XylemError::SerializationError("invalid expiration timestamp".to_string())
                })?,
        );

        let ops_count = deserialize_varint(bytes, &mut pos)? as usize;
        let mut operations: Vec<Box<dyn Operation>> = Vec::with_capacity(ops_count);
        for _ in 0..ops_count {
            let op_id = deserialize_varint(bytes, &mut pos)?;
            operations.push(deserialize_op(op_id, bytes, &mut pos)?);
        }

        let _extensions_count = bytes[pos];

        Ok(Transaction {
            ref_block_num,
            ref_block_prefix,
            expiration,
            operations,
            signatures: Vec::new(),
        })
    }

    pub fn append_op(&mut self, op: Box<dyn Operation>) {
        self.operations.push(op);
    }

    /// Serialize transaction structure to standard wire protocol binary bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>, XylemError> {
        let mut buf = Vec::new();
        // Write ref_block_num (u16 little-endian)
        buf.extend_from_slice(&self.ref_block_num.to_le_bytes());
        // Write ref_block_prefix (u32 little-endian)
        buf.extend_from_slice(&self.ref_block_prefix.to_le_bytes());
        // Write expiration (u32 Unix timestamp little-endian)
        let exp_seconds = self.expiration.0.and_utc().timestamp() as u32;
        buf.extend_from_slice(&exp_seconds.to_le_bytes());

        // Write operations length (varint)
        buf.extend_from_slice(&crate::operations::serialize_varint(
            self.operations.len() as u64
        ));

        // Write operations bytes
        for op in &self.operations {
            buf.extend_from_slice(&op.to_bytes()?);
        }

        // Write extensions length (0) -> u8 0
        buf.push(0);

        Ok(buf)
    }

    /// Sign transaction with private WIF key and chain ID.
    pub fn sign(&mut self, wif: &str, chain_id: &str) -> Result<(), XylemError> {
        let tx_bytes = self.to_bytes()?;
        let signature = crate::crypto::sign_transaction_bytes(&tx_bytes, wif, chain_id)?;
        self.signatures.push(signature);
        Ok(())
    }

    /// Calculate the transaction ID (first 20 bytes of SHA-256 of serialized bytes).
    pub fn id(&self) -> Result<String, XylemError> {
        let tx_bytes = self.to_bytes()?;
        let mut hasher = Sha256::new();
        hasher.update(&tx_bytes);
        let digest = hasher.finalize();
        Ok(hex::encode(&digest[0..20]))
    }

    /// Convert transaction to dictionary form for JSON-RPC broadcast.
    pub fn to_dict(&self) -> Value {
        let mut ops_array = Vec::new();
        for op in &self.operations {
            let (name, body) = op.to_dict();
            ops_array.push(serde_json::json!([name, body]));
        }

        serde_json::json!({
            "ref_block_num": self.ref_block_num,
            "ref_block_prefix": self.ref_block_prefix,
            "expiration": self.expiration,
            "operations": Value::Array(ops_array),
            "extensions": Value::Array(Vec::new()),
            "signatures": self.signatures
        })
    }

    /// Sign transaction with multiple private WIF keys.
    pub fn sign_many(&mut self, wifs: &[&str], chain_id: &str) -> Result<(), XylemError> {
        for wif in wifs {
            self.sign(wif, chain_id)?;
        }
        Ok(())
    }

    /// Verify if the accumulated signatures satisfy the provided authority's threshold.
    pub fn verify_authority(&self, auth: &Authority, chain_id: &str) -> Result<bool, XylemError> {
        if self.signatures.is_empty() {
            return Err(XylemError::CryptoError(
                "transaction has no signatures to verify".to_string(),
            ));
        }

        let tx_bytes = self.to_bytes()?;
        let chain_bytes = hex::decode(chain_id)
            .map_err(|e| XylemError::HexError(format!("invalid chain_id hex: {}", e)))?;

        let mut hasher = Sha256::new();
        hasher.update(&chain_bytes);
        hasher.update(&tx_bytes);
        let digest = hasher.finalize();

        let mut recovered_keys = std::collections::HashSet::new();
        for sig in &self.signatures {
            let pub_key_str = crate::crypto::recover_key_from_signature(sig, &digest)?;
            recovered_keys.insert(pub_key_str);
        }

        let mut total_weight = 0u32;
        for (key_str, weight) in &auth.key_auths {
            if recovered_keys.contains(key_str) {
                total_weight += *weight as u32;
            }
        }

        Ok(total_weight >= auth.weight_threshold)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::{Transfer, Vote};
    use crate::types::HiveTime;
    use chrono::NaiveDateTime;

    #[test]
    fn test_transaction_serialization() {
        let dt = NaiveDateTime::parse_from_str("2026-05-25T10:00:00", "%Y-%m-%dT%H:%M:%S").unwrap();
        let mut tx = Transaction::new(1234, 56789, HiveTime(dt));

        let vote = Vote {
            voter: "alice".to_string(),
            author: "bob".to_string(),
            permlink: "hello-world".to_string(),
            weight: 10000,
        };
        tx.append_op(Box::new(vote));

        let bytes = tx.to_bytes().unwrap();
        assert!(!bytes.is_empty());

        let dict = tx.to_dict();
        assert_eq!(dict["ref_block_num"], 1234);
        assert_eq!(dict["ref_block_prefix"], 56789);
        assert_eq!(dict["operations"][0][0], "vote");
    }

    #[test]
    fn test_transaction_id() {
        let dt = NaiveDateTime::parse_from_str("2026-05-25T10:00:00", "%Y-%m-%dT%H:%M:%S").unwrap();
        let tx = Transaction::new(1234, 56789, HiveTime(dt));
        let id = tx.id().unwrap();
        assert_eq!(id.len(), 40); // 20 bytes hex string is 40 chars
    }

    #[test]
    fn test_verify_authority() {
        let dt = NaiveDateTime::parse_from_str("2026-05-25T10:00:00", "%Y-%m-%dT%H:%M:%S").unwrap();
        let mut tx = Transaction::new(1234, 56789, HiveTime(dt));

        let vote = Vote {
            voter: "alice".to_string(),
            author: "bob".to_string(),
            permlink: "hello-world".to_string(),
            weight: 10000,
        };
        tx.append_op(Box::new(vote));

        let wif = "5J3mBbAH58CpQ3Y5RNJpUKPE62SQ5tfcvU2JpbnkeyhfsYB1Jcn";
        let pub_key = crate::crypto::wif_to_public_key(wif).unwrap();
        let chain_id = "0000000000000000000000000000000000000000000000000000000000000000";

        tx.sign(wif, chain_id).unwrap();

        let mut key_auths = std::collections::HashMap::new();
        key_auths.insert(pub_key, 1u16);

        let auth = Authority {
            weight_threshold: 1,
            account_auths: std::collections::HashMap::new(),
            key_auths,
        };

        let verified = tx.verify_authority(&auth, chain_id).unwrap();
        assert!(verified);
    }

    #[test]
    fn test_transfer_roundtrip() {
        let dt = NaiveDateTime::parse_from_str("2026-05-25T10:00:00", "%Y-%m-%dT%H:%M:%S").unwrap();
        let mut tx = Transaction::new(1234, 56789, HiveTime(dt));

        let transfer = Transfer {
            from: "alice".to_string(),
            to: "bob".to_string(),
            amount: "1.000 HIVE".to_string(),
            memo: "test memo".to_string(),
        };
        tx.append_op(Box::new(transfer));

        let bytes = tx.to_bytes().unwrap();
        let tx2 = Transaction::from_bytes(&bytes).unwrap();

        assert_eq!(tx2.ref_block_num, 1234);
        assert_eq!(tx2.ref_block_prefix, 56789);
        assert_eq!(tx2.operations.len(), 1);

        let dict = tx2.to_dict();
        let ops = dict["operations"].as_array().unwrap();
        assert_eq!(ops[0][0], "transfer");
        assert_eq!(ops[0][1]["from"], "alice");
        assert_eq!(ops[0][1]["to"], "bob");
        assert_eq!(ops[0][1]["memo"], "test memo");
    }

    #[test]
    fn test_vote_roundtrip() {
        let dt = NaiveDateTime::parse_from_str("2026-05-25T10:00:00", "%Y-%m-%dT%H:%M:%S").unwrap();
        let mut tx = Transaction::new(100, 200, HiveTime(dt));

        let vote = Vote {
            voter: "alice".to_string(),
            author: "bob".to_string(),
            permlink: "hello-world".to_string(),
            weight: 5000,
        };
        tx.append_op(Box::new(vote));

        let bytes = tx.to_bytes().unwrap();
        let tx2 = Transaction::from_bytes(&bytes).unwrap();

        assert_eq!(tx2.ref_block_num, 100);
        assert_eq!(tx2.ref_block_prefix, 200);
        assert_eq!(tx2.operations.len(), 1);

        let dict = tx2.to_dict();
        let ops = dict["operations"].as_array().unwrap();
        assert_eq!(ops[0][0], "vote");
        assert_eq!(ops[0][1]["voter"], "alice");
        assert_eq!(ops[0][1]["author"], "bob");
        assert_eq!(ops[0][1]["permlink"], "hello-world");
        assert_eq!(ops[0][1]["weight"], 5000);
    }

    #[test]
    fn test_comment_roundtrip() {
        use crate::operations::Comment;

        let dt = NaiveDateTime::parse_from_str("2026-05-25T10:00:00", "%Y-%m-%dT%H:%M:%S").unwrap();
        let mut tx = Transaction::new(100, 200, HiveTime(dt));

        let comment = Comment {
            parent_author: "".to_string(),
            parent_permlink: "test-parent".to_string(),
            author: "alice".to_string(),
            permlink: "my-post".to_string(),
            title: "My Title".to_string(),
            body: "Hello world, this is a test post body.".to_string(),
            json_metadata: "{\"tags\":[\"test\"]}".to_string(),
        };
        tx.append_op(Box::new(comment));

        let bytes = tx.to_bytes().unwrap();
        let tx2 = Transaction::from_bytes(&bytes).unwrap();

        assert_eq!(tx2.operations.len(), 1);
        let dict = tx2.to_dict();
        let ops = dict["operations"].as_array().unwrap();
        assert_eq!(ops[0][0], "comment");
        assert_eq!(ops[0][1]["parent_author"], "");
        assert_eq!(ops[0][1]["parent_permlink"], "test-parent");
        assert_eq!(ops[0][1]["author"], "alice");
        assert_eq!(ops[0][1]["permlink"], "my-post");
        assert_eq!(ops[0][1]["title"], "My Title");
        assert_eq!(ops[0][1]["body"], "Hello world, this is a test post body.");
        assert_eq!(ops[0][1]["json_metadata"], "{\"tags\":[\"test\"]}");
    }

    #[test]
    fn test_custom_json_roundtrip() {
        use crate::operations::CustomJson;

        let dt = NaiveDateTime::parse_from_str("2026-05-25T10:00:00", "%Y-%m-%dT%H:%M:%S").unwrap();
        let mut tx = Transaction::new(100, 200, HiveTime(dt));

        let cj = CustomJson {
            id: "follow".to_string(),
            json: r#"["follow",{"follower":"alice","following":"bob","what":["blog"]}]"#
                .to_string(),
            required_auths: vec![],
            required_posting_auths: vec!["alice".to_string()],
        };
        tx.append_op(Box::new(cj));

        let bytes = tx.to_bytes().unwrap();
        let tx2 = Transaction::from_bytes(&bytes).unwrap();

        assert_eq!(tx2.operations.len(), 1);
        let dict = tx2.to_dict();
        let ops = dict["operations"].as_array().unwrap();
        assert_eq!(ops[0][0], "custom_json");
        assert_eq!(ops[0][1]["id"], "follow");
        assert_eq!(
            ops[0][1]["json"],
            r#"["follow",{"follower":"alice","following":"bob","what":["blog"]}]"#
        );
        assert_eq!(
            ops[0][1]["required_posting_auths"],
            serde_json::json!(["alice"])
        );
    }

    #[test]
    fn test_multiple_operations_roundtrip() {
        let dt = NaiveDateTime::parse_from_str("2026-05-25T10:00:00", "%Y-%m-%dT%H:%M:%S").unwrap();
        let mut tx = Transaction::new(500, 99999, HiveTime(dt));

        let vote = Vote {
            voter: "alice".to_string(),
            author: "bob".to_string(),
            permlink: "post-1".to_string(),
            weight: 10000,
        };
        tx.append_op(Box::new(vote));

        let transfer = Transfer {
            from: "alice".to_string(),
            to: "bob".to_string(),
            amount: "2.500 HIVE".to_string(),
            memo: "pay".to_string(),
        };
        tx.append_op(Box::new(transfer));

        let bytes = tx.to_bytes().unwrap();
        let tx2 = Transaction::from_bytes(&bytes).unwrap();

        assert_eq!(tx2.ref_block_num, 500);
        assert_eq!(tx2.ref_block_prefix, 99999);
        assert_eq!(tx2.operations.len(), 2);

        let dict = tx2.to_dict();
        let ops = dict["operations"].as_array().unwrap();
        assert_eq!(ops[0][0], "vote");
        assert_eq!(ops[1][0], "transfer");
    }

    #[test]
    fn test_from_bytes_too_short() {
        let result = Transaction::from_bytes(&[0u8; 5]);
        assert!(result.is_err());
    }

    #[test]
    fn test_from_bytes_invalid_op_id() {
        let dt = NaiveDateTime::parse_from_str("2026-05-25T10:00:00", "%Y-%m-%dT%H:%M:%S").unwrap();
        let mut tx = Transaction::new(100, 200, HiveTime(dt));
        // Vote with empty strings to create minimal bytes, then corrupt the op ID
        let vote = Vote {
            voter: "a".to_string(),
            author: "b".to_string(),
            permlink: "c".to_string(),
            weight: 1,
        };
        tx.append_op(Box::new(vote));

        let mut bytes = tx.to_bytes().unwrap();
        // The op ID is after ref_block_num(2) + ref_block_prefix(4) + expiration(4) + ops_count(1) = offset 11
        // The op ID byte for Vote is 0x00; change it to an unsupported ID
        bytes[11] = 99;

        let result = Transaction::from_bytes(&bytes);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_memo_transfer_roundtrip() {
        let dt = NaiveDateTime::parse_from_str("2026-05-25T10:00:00", "%Y-%m-%dT%H:%M:%S").unwrap();
        let mut tx = Transaction::new(100, 200, HiveTime(dt));

        let transfer = Transfer {
            from: "alice".to_string(),
            to: "bob".to_string(),
            amount: "10.000 HBD".to_string(),
            memo: "".to_string(),
        };
        tx.append_op(Box::new(transfer));

        let bytes = tx.to_bytes().unwrap();
        let tx2 = Transaction::from_bytes(&bytes).unwrap();

        let dict = tx2.to_dict();
        let ops = dict["operations"].as_array().unwrap();
        assert_eq!(ops[0][1]["amount"], "10.000 HBD");
        assert_eq!(ops[0][1]["memo"], "");
    }

    #[test]
    fn test_large_varint_roundtrip() {
        let dt = NaiveDateTime::parse_from_str("2026-05-25T10:00:00", "%Y-%m-%dT%H:%M:%S").unwrap();
        let mut tx = Transaction::new(65535, u32::MAX, HiveTime(dt));

        let vote = Vote {
            voter: "longvotername123".to_string(),
            author: "longauthorname456".to_string(),
            permlink: "this-is-a-really-long-permlink-that-tests-varint-encoding-at-higher-values"
                .to_string(),
            weight: -10000,
        };
        tx.append_op(Box::new(vote));

        let bytes = tx.to_bytes().unwrap();
        let tx2 = Transaction::from_bytes(&bytes).unwrap();

        assert_eq!(tx2.ref_block_num, 65535);
        assert_eq!(tx2.ref_block_prefix, u32::MAX);

        let dict = tx2.to_dict();
        let ops = dict["operations"].as_array().unwrap();
        assert_eq!(ops[0][1]["voter"], "longvotername123");
        assert_eq!(ops[0][1]["weight"], -10000);
    }
}
