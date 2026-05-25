pub mod client;
pub mod crypto;
pub mod errors;
pub mod operations;
pub mod transaction;
pub mod types;

// Re-exports for convenient usage
pub use client::Client;
pub use crypto::{decode_wif, sign_transaction_bytes, wif_to_public_key};
pub use errors::XylemError;
pub use transaction::Transaction;
