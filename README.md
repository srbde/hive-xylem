# 🧬 Xylem

**The modern, secure, and high-performance Rust SDK for the Hive blockchain. Built for production. Made to last.**

Rust is built for ultimate memory safety, blazing-fast performance, and zero-cost abstractions. **Xylem** brings those strengths to the Hive blockchain. It is designed from the ground up to provide secure, offline transaction serialization and signing, multi-signature authority validation, and native async-first blockchain queries.

If you are building high-throughput backend services, high-speed bots, or indexers on Hive in Rust, Xylem is your foundation.

---

**Secured and Native:** Xylem uses standard, audited RustCrypto cryptography (`secp256k1`, `bs58`, `sha2`, `ripemd`) and features a native byte serialization engine. It has zero dependency on legacy RPC serialization.

---

## Why Xylem?

The Hive ecosystem deserves infrastructure that is safe, fast, and robust by default.

### 🔒 Audited Cryptography & Local Serialization

Xylem strips out deprecated RPC-based serialization (`get_transaction_hex`). In its place:

- **[secp256k1](https://crates.io/crates/secp256k1)**: Uses standard, audited Rust bindings to `libsecp256k1` to generate canonical low-S ECDSA compact signatures and recovery IDs natively.
- **[bs58](https://crates.io/crates/bs58)**: Safe, high-performance Base58/WIF key parsing and checksum verification.
- **Offline Serialization**: Encodes operations (`Vote`, `Comment`, `Transfer`, `CustomJson`, etc.) into exact consensus-compatible wire bytes locally.

### ⚡ Async First & High Performance

Xylem is designed for modern Rust concurrency models:

- **Lock-Free Concurrency**: Utilizes lock-free `AtomicUsize` for sticky node connection tracking and thread-safe failovers.
- **Async/Await Native**: Fully integrated with the `Tokio` runtime and built on top of connection-pooled `reqwest` clients.
- **Exponential Backoff & Failover**: Transparently retries failed requests and automatically rotates between nodes.

### 🔌 Ecosystem Alignment

Xylem is the Rust counterpart to **[Anther](https://github.com/srbde/hive-anther)** (Go), **[Pollen](https://github.com/srbde/hive-pollen)** (TypeScript), and **[Nectar](https://github.com/srbde/hive-nectar)** (Python). Together, they form a unified, secure foundation for building cross-platform Hive applications under the **SRBDE** umbrella.

---

## 🚀 Quick Start

Add `xylem` to your `Cargo.toml`:

```toml
[dependencies]
xylem = { git = "https://github.com/srbde/hive-xylem.git" }
tokio = { version = "1.0", features = ["full"] }
```

### Read Account Data

```rust
use xylem::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the client with public Hive nodes
    let client = Client::new(vec!["https://api.hive.blog".to_string()], 30);

    println!("Querying account...");
    let accounts = client.get_accounts(&["thecrazygm".to_string()]).await?;

    if let Some(acc) = accounts.first() {
        println!("Account:      @{}", acc.name);
        println!("HIVE Balance: {}", acc.balance);
        println!("HBD Balance:  {}", acc.hbd_balance);
        println!("Voting Power: {:.2}%", acc.voting_power / 100.0);
    }

    Ok(())
}
```

### Sign and Broadcast a Transaction

```rust
use xylem::{Client, Transaction};
use xylem::operations::Transfer;
use xylem::types::HiveTime;
use chrono::Utc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new(vec!["https://api.hive.blog".to_string()], 30);

    // Get dynamic global properties to get TaPoS parameters
    let props = client.get_dynamic_global_properties().await?;
    let ref_block_num = (props.head_block_number & 0xFFFF) as u16;

    // Extract block prefix from head block ID
    let prefix_bytes = hex::decode(&props.head_block_id[8..16])?;
    let ref_block_prefix = u32::from_le_bytes(prefix_bytes.try_into().unwrap());

    // Create transaction set to expire in 1 minute
    let expiration = HiveTime(Utc::now().naive_utc() + chrono::Duration::minutes(1));
    let mut tx = Transaction::new(ref_block_num, ref_block_prefix, expiration);

    // Append a transfer operation
    tx.append_op(Box::new(Transfer {
        from: "youraccount".to_string(),
        to: "recipient".to_string(),
        amount: "1.000 HIVE".to_string(),
        memo: "Sent with Xylem 🧬".to_string(),
    }));

    // Sign transaction with WIF active key and default mainnet chain ID
    let active_wif = "your-active-private-key-wif";
    let mainnet_chain_id = "beeab0de00000000000000000000000000000000000000000000000000000000";
    tx.sign(active_wif, mainnet_chain_id)?;

    // Broadcast the signed transaction to the blockchain
    let response = client.broadcast_transaction(&tx).await?;
    println!("Broadcast Result: {}", response);

    Ok(())
}
```

---

## 🏗️ Building & Testing

Xylem uses standard Cargo toolchain commands:

```bash
# Build the crate
cargo build

# Run unit tests
cargo test

# Run key verification example
ACTIVE_WIF="your_wif_key" cargo run --example verify-keys
```

---

---

## 📜 Standing on Shoulders

Xylem is a completely original Rust library designed from the ground up to bring Hive development to the Rust ecosystem. It was built using the TAPOS headers, transaction signatures, and cryptographic standards verified by the SRBDE team to ensure 100% mathematical consensus compatibility with the Hive blockchain.

---

## 🌐 Built by SRBDE

**Xylem** is developed and maintained by the **Sustainable Resource and Business Development Enterprise (SRBDE)** — an open-source infrastructure organization building tools and platforms for communities that build things together.

We apply the logic of agricultural sustainability to software: the goal is always to return more to the ecosystem than we extract.

- **Open source is our value, not just our business model.**
- **Our commercial products fund our open-source core. The open work is the mission.**

### Explore the Ecosystem

| Project                                               | Description                       |
| ----------------------------------------------------- | --------------------------------- |
| [Pollen](https://github.com/srbde/hive-pollen)             | The modern Hive TypeScript SDK    |
| [Anther](https://github.com/srbde/hive-anther)             | The modern Hive Go SDK            |
| [Xylem](https://github.com/srbde/hive-xylem)               | The modern Hive Rust SDK          |
| [Nectar](https://github.com/srbde/hive-nectar)        | The modern Hive Python SDK        |
| [nectarengine](https://github.com/srbde/nectarengine) | The Hive-Engine sidechain library |
| [ecoinstats.net](https://ecoinstats.net)              | SRBDE corporate hub               |
| [thecrazygm.com](https://thecrazygm.com)              | Open gaming tools & TTRPGs        |

---

## 🤝 Contributing

Audits, forks, and pull requests are welcome. **Xylem** is built to last for the decade, not the quarter. If you find a security issue, please open a private advisory rather than a public issue.
