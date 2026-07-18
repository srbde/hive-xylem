use std::fmt;

#[derive(Debug)]
pub enum XylemError {
    WifError(String),
    Base58Error(String),
    CryptoError(String),
    HexError(String),
    SerializationError(String),
    RpcError(String),
    HttpError(String),
    JsonError(String),
    MalformedOperationTuple(String),
    MissingOperationField(String),
    WrongOperationFieldType(String),
    MalformedAmount(String),
    MalformedAuthorizationArray(String),
}

impl std::error::Error for XylemError {}

impl fmt::Display for XylemError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            XylemError::WifError(s) => write!(f, "WIF error: {}", s),
            XylemError::Base58Error(s) => write!(f, "Base58 error: {}", s),
            XylemError::CryptoError(s) => write!(f, "Cryptography error: {}", s),
            XylemError::HexError(s) => write!(f, "Hex error: {}", s),
            XylemError::SerializationError(s) => write!(f, "Serialization error: {}", s),
            XylemError::RpcError(s) => write!(f, "RPC error: {}", s),
            XylemError::HttpError(s) => write!(f, "HTTP error: {}", s),
            XylemError::JsonError(s) => write!(f, "JSON error: {}", s),
            XylemError::MalformedOperationTuple(s) => {
                write!(f, "malformed operation tuple: {}", s)
            }
            XylemError::MissingOperationField(s) => {
                write!(f, "missing operation field: {}", s)
            }
            XylemError::WrongOperationFieldType(s) => {
                write!(f, "wrong operation field type: {}", s)
            }
            XylemError::MalformedAmount(s) => write!(f, "malformed operation amount: {}", s),
            XylemError::MalformedAuthorizationArray(s) => {
                write!(f, "malformed authorization array: {}", s)
            }
        }
    }
}

impl From<hex::FromHexError> for XylemError {
    fn from(err: hex::FromHexError) -> Self {
        XylemError::HexError(err.to_string())
    }
}

impl From<reqwest::Error> for XylemError {
    fn from(err: reqwest::Error) -> Self {
        XylemError::HttpError(err.to_string())
    }
}

impl From<serde_json::Error> for XylemError {
    fn from(err: serde_json::Error) -> Self {
        XylemError::JsonError(err.to_string())
    }
}

impl From<secp256k1::Error> for XylemError {
    fn from(err: secp256k1::Error) -> Self {
        XylemError::CryptoError(err.to_string())
    }
}
