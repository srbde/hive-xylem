use crate::errors::XylemError;
use ripemd::{Digest as RipeDigest, Ripemd160};
use secp256k1::{Message, Secp256k1, SecretKey};
use sha2::Sha256;

/// Decode WIF private key returning raw 32-byte private key.
pub fn decode_wif(wif: &str) -> Result<Vec<u8>, XylemError> {
    let decoded = bs58::decode(wif)
        .with_check(None)
        .into_vec()
        .map_err(|e| XylemError::WifError(format!("invalid base58check format: {}", e)))?;

    if decoded.is_empty() {
        return Err(XylemError::WifError("empty WIF payload".to_string()));
    }

    if decoded[0] != 0x80 {
        return Err(XylemError::WifError(format!(
            "invalid WIF version byte: expected 0x80, got 0x{:02x}",
            decoded[0]
        )));
    }

    // A WIF key has a 1-byte version prefix (0x80) + 32-byte private key + optional 1-byte compression flag (0x01)
    let priv_bytes = if decoded.len() == 33 {
        decoded[1..33].to_vec()
    } else if decoded.len() == 34 && decoded[33] == 0x01 {
        decoded[1..33].to_vec()
    } else {
        return Err(XylemError::WifError(format!(
            "invalid private key payload length: {}",
            decoded.len() - 1
        )));
    };

    Ok(priv_bytes)
}

/// Derive Hive public key string ("STM...") from WIF.
pub fn wif_to_public_key(wif: &str) -> Result<String, XylemError> {
    let priv_bytes = decode_wif(wif)?;
    let secp = Secp256k1::new();
    let secret_key = SecretKey::from_slice(&priv_bytes)
        .map_err(|e| XylemError::CryptoError(format!("invalid private key: {}", e)))?;

    let pub_key = secp256k1::PublicKey::from_secret_key(&secp, &secret_key);
    let pub_bytes = pub_key.serialize(); // 33-byte compressed public key

    // RIPEMD160 checksum
    let mut hasher = Ripemd160::new();
    hasher.update(&pub_bytes);
    let checksum = hasher.finalize();

    let mut payload = pub_bytes.to_vec();
    payload.extend_from_slice(&checksum[0..4]);

    let base58_str = bs58::encode(payload).into_string();
    Ok(format!("STM{}", base58_str))
}

/// Sign transaction bytes using WIF and chain_id.
pub fn sign_transaction_bytes(
    tx_bytes: &[u8],
    wif: &str,
    chain_id: &str,
) -> Result<String, XylemError> {
    let priv_bytes = decode_wif(wif)?;
    let chain_id_bytes = hex::decode(chain_id)
        .map_err(|e| XylemError::HexError(format!("invalid chain_id hex: {}", e)))?;

    let mut hasher = Sha256::new();
    hasher.update(&chain_id_bytes);
    hasher.update(tx_bytes);
    let digest = hasher.finalize();

    let secp = Secp256k1::new();
    let secret_key = SecretKey::from_slice(&priv_bytes)
        .map_err(|e| XylemError::CryptoError(format!("invalid private key: {}", e)))?;

    let message = Message::from_digest_slice(&digest)
        .map_err(|e| XylemError::CryptoError(format!("invalid digest: {}", e)))?;

    let sig = secp.sign_ecdsa_recoverable(&message, &secret_key);
    let (rec_id, compact) = sig.serialize_compact();

    let recovery_byte = 27 + 4 + rec_id.to_i32() as u8;

    let mut final_sig = Vec::with_capacity(65);
    final_sig.push(recovery_byte);
    final_sig.extend_from_slice(&compact);

    Ok(hex::encode(final_sig))
}

#[cfg(test)]
mod tests {
    use super::*;

    // A standard test WIF key
    fn test_wif() -> &'static str {
        "5J3mBbAH58CpQ3Y5RNJpUKPE62SQ5tfcvU2JpbnkeyhfsYB1Jcn"
    }

    #[test]
    fn test_decode_wif() {
        let decoded = decode_wif(test_wif()).unwrap();
        assert_eq!(decoded.len(), 32);
    }

    #[test]
    fn test_wif_to_public_key() {
        let pub_key = wif_to_public_key(test_wif()).unwrap();
        assert!(pub_key.starts_with("STM"));
    }
}
