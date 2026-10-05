use crate::crypto::decode_wif;
use crate::errors::XylemError;
use crate::operations::{serialize_string, serialize_varint};
use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use cbc::{Decryptor, Encryptor};
use secp256k1::{PublicKey, Secp256k1, SecretKey};
use sha2::{Digest as ShaDigest, Sha256, Sha512};

type Aes256CbcEnc = Encryptor<aes::Aes256>;
type Aes256CbcDec = Decryptor<aes::Aes256>;

pub fn parse_public_key(pub_key_str: &str) -> Result<(PublicKey, Vec<u8>), XylemError> {
    let mut trimmed = pub_key_str;
    if pub_key_str.len() > 3 && (&pub_key_str[0..3] == "STM" || &pub_key_str[0..3] == "TST") {
        trimmed = &pub_key_str[3..];
    }
    let decoded = bs58::decode(trimmed)
        .into_vec()
        .map_err(|e| XylemError::Base58Error(e.to_string()))?;
    if decoded.len() < 33 {
        return Err(XylemError::SerializationError(
            "invalid public key length".to_string(),
        ));
    }
    let raw_pub = decoded[0..33].to_vec();
    let pub_key =
        PublicKey::from_slice(&raw_pub).map_err(|e| XylemError::CryptoError(e.to_string()))?;
    Ok((pub_key, raw_pub))
}

pub fn deserialize_varint(buf: &[u8]) -> Result<(u64, usize), XylemError> {
    let mut val = 0u64;
    let mut shift = 0;
    let mut idx = 0;
    loop {
        if idx >= buf.len() {
            return Err(XylemError::SerializationError(
                "unexpected end of varint".to_string(),
            ));
        }
        let b = buf[idx];
        idx += 1;
        val |= ((b & 0x7f) as u64) << shift;
        if b & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 64 {
            return Err(XylemError::SerializationError(
                "varint overflow".to_string(),
            ));
        }
    }
    Ok((val, idx))
}

/// X coordinate of an ECDH shared point.
///
/// `secp256k1::ecdh::shared_secret_point` returns `X ‖ Y` (64 bytes, no prefix
/// byte). Uncompressed SEC1 public keys are 65 bytes and do start with `0x04`;
/// that prefix is not present here, so the X coordinate is the first 32 bytes.
fn shared_secret_x(shared_point: &[u8; 64]) -> &[u8] {
    &shared_point[0..32]
}

/// Encrypt a memo if it starts with "#".
pub fn encode(
    sender_wif: &str,
    recipient_pub_key_str: &str,
    memo: &str,
) -> Result<String, XylemError> {
    encode_with_nonce(sender_wif, recipient_pub_key_str, memo, rand::random())
}

/// Encrypt a memo with a caller-supplied nonce.
///
/// Same rules as [`encode`]. A fixed nonce lets an existing Hive memo be
/// reproduced and checked against other clients.
pub fn encode_with_nonce(
    sender_wif: &str,
    recipient_pub_key_str: &str,
    memo: &str,
    nonce: u64,
) -> Result<String, XylemError> {
    if !memo.starts_with('#') {
        return Ok(memo.to_string());
    }
    let memo_text = &memo[1..];

    // Decode sender private WIF key
    let priv_bytes = decode_wif(sender_wif)?;
    let secp = Secp256k1::new();
    let sender_priv =
        SecretKey::from_slice(&priv_bytes).map_err(|e| XylemError::CryptoError(e.to_string()))?;
    let sender_pub = PublicKey::from_secret_key(&secp, &sender_priv);
    let sender_pub_bytes = sender_pub.serialize(); // 33-byte compressed

    // Parse recipient public key
    let (recipient_pub, recipient_pub_bytes) = parse_public_key(recipient_pub_key_str)?;

    // Derive shared secret X-coordinate of P = sender_priv * recipient_pub.
    // shared_secret_point returns X ‖ Y (64 bytes, no prefix byte).
    let shared_point = secp256k1::ecdh::shared_secret_point(&recipient_pub, &sender_priv);
    let shared_x = shared_secret_x(&shared_point);

    // S = sha512(shared_x)
    let mut hasher = Sha512::new();
    hasher.update(shared_x);
    let s = hasher.finalize();

    // ebuf = nonce + S
    let mut ebuf = Vec::new();
    ebuf.extend_from_slice(&nonce.to_le_bytes());
    ebuf.extend_from_slice(&s);

    // encryption_key = sha512(ebuf)
    let mut hasher = Sha512::new();
    hasher.update(&ebuf);
    let encryption_key = hasher.finalize();

    let tag = &encryption_key[0..32];
    let iv = &encryption_key[32..48];

    // checksum = sha256(encryption_key)[0..4]
    let mut hasher = Sha256::new();
    hasher.update(encryption_key);
    let hash = hasher.finalize();
    let check32 = u32::from_le_bytes(hash[0..4].try_into().unwrap());

    // plaintext: varint length of memoText + memoText bytes
    let mut plaintext = Vec::new();
    serialize_string(&mut plaintext, memo_text);

    // Encrypt using AES-256-CBC
    let msg_len = plaintext.len();
    let padded_len = (msg_len / 16 + 1) * 16;
    let mut encrypt_buf = vec![0u8; padded_len];
    encrypt_buf[..msg_len].copy_from_slice(&plaintext);

    let cipher = Aes256CbcEnc::new_from_slices(tag, iv)
        .map_err(|e| XylemError::CryptoError(e.to_string()))?;
    let ciphertext = cipher
        .encrypt_padded_mut::<Pkcs7>(&mut encrypt_buf, msg_len)
        .map_err(|e| XylemError::CryptoError(e.to_string()))?
        .to_vec();

    // Envelope: sender_pub + recipient_pub + nonce + check32 + varint(ciphertext_len) + ciphertext
    let mut envelope = Vec::new();
    envelope.extend_from_slice(&sender_pub_bytes);
    envelope.extend_from_slice(&recipient_pub_bytes);
    envelope.extend_from_slice(&nonce.to_le_bytes());
    envelope.extend_from_slice(&check32.to_le_bytes());
    envelope.extend_from_slice(&serialize_varint(ciphertext.len() as u64));
    envelope.extend_from_slice(&ciphertext);

    Ok(format!("#{}", bs58::encode(envelope).into_string()))
}

/// Decrypt a memo if it starts with "#".
pub fn decode(wif: &str, memo: &str) -> Result<String, XylemError> {
    if !memo.starts_with('#') {
        return Ok(memo.to_string());
    }
    let memo_base58 = &memo[1..];

    let decoded = bs58::decode(memo_base58)
        .into_vec()
        .map_err(|e| XylemError::Base58Error(e.to_string()))?;

    if decoded.len() < 33 + 33 + 8 + 4 {
        return Err(XylemError::SerializationError(
            "invalid encrypted memo payload length".to_string(),
        ));
    }

    let from_bytes = &decoded[0..33];
    let to_bytes = &decoded[33..66];

    let nonce_bytes = &decoded[66..74];
    let nonce = u64::from_le_bytes(nonce_bytes.try_into().unwrap());

    let check32_bytes = &decoded[74..78];
    let check32 = u32::from_le_bytes(check32_bytes.try_into().unwrap());

    let (cipher_len, varint_size) = deserialize_varint(&decoded[78..])?;
    let cipher_start = 78 + varint_size;
    let cipher_end = cipher_start + cipher_len as usize;

    if decoded.len() < cipher_end {
        return Err(XylemError::SerializationError(
            "unexpected end of ciphertext payload".to_string(),
        ));
    }
    let ciphertext = &decoded[cipher_start..cipher_end];

    // Decode WIF
    let priv_bytes = decode_wif(wif)?;
    let secp = Secp256k1::new();
    let my_priv =
        SecretKey::from_slice(&priv_bytes).map_err(|e| XylemError::CryptoError(e.to_string()))?;
    let my_pub = PublicKey::from_secret_key(&secp, &my_priv);
    let my_pub_bytes = my_pub.serialize();

    let other_pub_bytes = if my_pub_bytes.as_slice() == from_bytes {
        to_bytes
    } else {
        from_bytes
    };

    let other_pub = PublicKey::from_slice(other_pub_bytes)
        .map_err(|e| XylemError::CryptoError(e.to_string()))?;

    // Derive shared secret. shared_secret_point returns X ‖ Y (64 bytes, no prefix byte).
    let shared_point = secp256k1::ecdh::shared_secret_point(&other_pub, &my_priv);
    let shared_x = shared_secret_x(&shared_point);

    // S = sha512(shared_x)
    let mut hasher = Sha512::new();
    hasher.update(shared_x);
    let s = hasher.finalize();

    // ebuf = nonce + S
    let mut ebuf = Vec::new();
    ebuf.extend_from_slice(&nonce.to_le_bytes());
    ebuf.extend_from_slice(&s);

    // encryption_key = sha512(ebuf)
    let mut hasher = Sha512::new();
    hasher.update(&ebuf);
    let encryption_key = hasher.finalize();

    let tag = &encryption_key[0..32];
    let iv = &encryption_key[32..48];

    // Verify Checksum
    let mut hasher = Sha256::new();
    hasher.update(encryption_key);
    let hash = hasher.finalize();
    let expected_check32 = u32::from_le_bytes(hash[0..4].try_into().unwrap());
    if expected_check32 != check32 {
        return Err(XylemError::CryptoError(
            "checksum verification failed (invalid key or tampered payload)".to_string(),
        ));
    }

    // Decrypt using AES-256-CBC
    let mut decrypt_buf = ciphertext.to_vec();
    let cipher = Aes256CbcDec::new_from_slices(tag, iv)
        .map_err(|e| XylemError::CryptoError(e.to_string()))?;
    let plaintext = cipher
        .decrypt_padded_mut::<Pkcs7>(&mut decrypt_buf)
        .map_err(|e| XylemError::CryptoError(e.to_string()))?
        .to_vec();

    // Parse string length prefix
    if let Ok((memo_len, varint_len)) = deserialize_varint(&plaintext) {
        if varint_len + memo_len as usize <= plaintext.len() {
            let memo_bytes = &plaintext[varint_len..varint_len + memo_len as usize];
            if let Ok(memo_str) = std::str::from_utf8(memo_bytes) {
                return Ok(format!("#{}", memo_str));
            }
        }
    }

    // Fallback to raw string
    let fallback_str =
        String::from_utf8(plaintext).map_err(|e| XylemError::SerializationError(e.to_string()))?;
    Ok(format!("#{}", fallback_str))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sender_wif() -> &'static str {
        "5J3mBbAH58CpQ3Y5RNJpUKPE62SQ5tfcvU2JpbnkeyhfsYB1Jcn"
    }

    fn recipient_pub() -> &'static str {
        "STM5kQ1uy2CGNSwibSeYyLELWFng3HTyYVSsQd4Bjd4sWfqgKgtgJ"
    }

    #[test]
    fn test_memo_encryption_decryption() {
        let memo = "#Hello secure world!";
        let encrypted = encode(sender_wif(), recipient_pub(), memo).unwrap();
        assert!(encrypted.starts_with('#'));

        let decrypted = decode(sender_wif(), &encrypted).unwrap();
        assert_eq!(decrypted, memo);
    }

    #[test]
    fn test_memo_ecdh_hive_vector() {
        // X coordinate of sender_wif * recipient_pub, checked independently by
        // scaling the recipient point (see upstream srbde/hive-xylem#9).
        let priv_bytes = decode_wif(sender_wif()).unwrap();
        let sender_priv = SecretKey::from_slice(&priv_bytes).unwrap();
        let (recipient, _) = parse_public_key(recipient_pub()).unwrap();
        let shared_point = secp256k1::ecdh::shared_secret_point(&recipient, &sender_priv);
        assert_eq!(shared_point.len(), 64);
        assert_eq!(
            hex::encode(shared_secret_x(&shared_point)),
            "41aec6a617873711cdc9db5da4e6a4f6e237fb28a3beadfc43c097e8c7193884"
        );

        // Ciphertext computed outside this crate (Node secp256k1 ECDH + SHA-512
        // + AES-256-CBC) for memo "#Hello secure world!" and nonce
        // 0x0102030405060708. A self-round-trip cannot catch a shared-secret
        // slice error; this vector can.
        let memo = "#Hello secure world!";
        let encrypted =
            encode_with_nonce(sender_wif(), recipient_pub(), memo, 0x0102_0304_0506_0708).unwrap();
        assert_eq!(
            encrypted,
            "#AiXWKsmWbHTz5h2nrjKcFob567v8z1buixTLW6AkJSApkFxWBhp9aNefPRrgsx35N2Y4HkopNi6dgFevetUa4hvButYJNDMaHWfaHxii4nBdTqPTSRgoAPnmsq3mHeUxJHkLkcmX2CraksY4EUKNk1R"
        );
        assert_eq!(decode(sender_wif(), &encrypted).unwrap(), memo);
    }

    #[test]
    fn test_plaintext_memo_unchanged() {
        let memo = "not encrypted";
        assert_eq!(encode(sender_wif(), recipient_pub(), memo).unwrap(), memo);
        assert_eq!(decode(sender_wif(), memo).unwrap(), memo);
    }
}
