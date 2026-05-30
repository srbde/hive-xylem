use hive_xylem::memo;
use hive_xylem::{Client, StreamingMode};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    println!("=====================================================");
    println!("   🌿 XYLEM RUST HIVE SDK - COMPLETE TOUR 🌿");
    println!("=====================================================");
    println!("This example showcases the features of the Xylem Rust");
    println!("library (queries, ECIES memo, offline signing, and");
    println!("async block/operation streaming).");
    println!("=====================================================");
    println!();

    // 1. Initialize Client with multiple nodes for failover
    let nodes = vec![
        "https://api.hive.blog".to_string(),
        "https://api.syncad.com".to_string(),
    ];
    let client = Arc::new(Client::new(nodes, 30));

    // 2. Fetch dynamic global properties
    println!("🌐 [PHASE 1] Fetching Blockchain Global Properties...");
    match client.get_dynamic_global_properties().await {
        Ok(props) => {
            println!("✓ Head Block:         {}", props.head_block_number);
            println!("  Head Block ID:      {}", props.head_block_id);
            println!("  Time:               {}", props.time.0);
            println!(
                "  Last Irreversible:  {}",
                props.last_irreversible_block_num
            );
        }
        Err(e) => println!("✗ Failed to fetch properties: {:?}", e),
    }
    println!();

    // 3. Fetch node config
    println!("⚙️ [PHASE 2] Fetching Node Config...");
    match client.get_config().await {
        Ok(config) => {
            if let Some(version) = config.get("HIVE_BLOCKCHAIN_VERSION") {
                println!("✓ Hive Version:       {}", version);
            } else {
                println!("✓ Config received (HIVE_BLOCKCHAIN_VERSION not found)");
            }
        }
        Err(e) => println!("✗ Failed to fetch config: {:?}", e),
    }
    println!();

    // 4. Fetch chain properties
    println!("⛓️ [PHASE 3] Fetching Chain Properties...");
    match client.get_chain_properties().await {
        Ok(props) => {
            println!("✓ Account Create Fee: {}", props.account_creation_fee);
            println!("  Max Block Size:     {}", props.maximum_block_size);
            println!(
                "  HBD Interest Rate:  {}%",
                props.hbd_interest_rate as f64 / 100.0
            );
        }
        Err(e) => println!("✗ Failed to fetch chain properties: {:?}", e),
    }
    println!();

    // 5. Fetch median history price
    println!("💰 [PHASE 4] Fetching Current Median History Price...");
    match client.get_current_median_history_price().await {
        Ok(price) => {
            println!("✓ Current Price:      {} = {}", price.base, price.quote);
        }
        Err(e) => println!("✗ Failed to fetch median price: {:?}", e),
    }
    println!();

    // 6. Fetch account history for @thecrazygm
    println!("📜 [PHASE 5] Fetching Account History for @thecrazygm...");
    match client.get_account_history("thecrazygm", -1, 3).await {
        Ok(history) => {
            for item in history {
                println!(
                    "✓ Seq {}: Trx ID = {}, Virtual = {}",
                    item.seq, item.op.trx_id, item.op.virtual_op
                );
                println!("  Op Name: {}", item.op.op.0);
            }
        }
        Err(e) => println!("✗ Failed to fetch history: {:?}", e),
    }
    println!();

    // 7. Fetch vesting delegations for @thecrazygm
    println!("🤝 [PHASE 6] Fetching Vesting Delegations for @thecrazygm...");
    match client.get_vesting_delegations("thecrazygm", "", 3).await {
        Ok(delegations) => {
            println!("✓ Found {} delegations", delegations.len());
            for d in delegations {
                println!(
                    "  - To: {}, Vesting Shares: {}",
                    d.delegatee, d.vesting_shares
                );
            }
        }
        Err(e) => println!("✗ Failed to fetch delegations: {:?}", e),
    }
    println!();

    // 8. Fetch block header and full block
    println!("📦 [PHASE 7] Fetching Block & Header details...");
    let target_block = 85_000_000;
    match client.get_block_header(target_block).await {
        Ok(header) => {
            println!(
                "✓ Block {} Header: Witness = {}, Prev = {}",
                target_block, header.witness, header.previous
            );
        }
        Err(e) => println!("✗ Failed to fetch block header: {:?}", e),
    }

    match client.get_block(target_block).await {
        Ok(block) => {
            println!(
                "✓ Block {} Details: Transactions = {}, Signing Key = {}",
                target_block,
                block.transactions.len(),
                block.signing_key
            );
        }
        Err(e) => println!("✗ Failed to fetch block: {:?}", e),
    }
    println!();

    // 9. Fetch Resource Credits info
    println!("⚡ [PHASE 8] Fetching Resource Credit Information for @thecrazygm...");
    match client.get_rc_mana("thecrazygm").await {
        Ok(info) => {
            println!("✓ RC Mana details:");
            println!("  Max RC:          {}", info.max_mana);
            println!(
                "  Current RC:      {} ({:.2}%)",
                info.current_mana, info.current_percent
            );
        }
        Err(e) => println!("✗ Failed to fetch RC mana: {:?}", e),
    }
    println!();

    // 10. ECIES Memo Encryption and Decryption
    println!("🔐 [PHASE 9] ECIES Memo Encryption & Decryption...");
    let sender_wif = "5J3mBbAH58CpQ3Y5RNJpUKPE62SQ5tfcvU2JpbnkeyhfsYB1Jcn";
    let recipient_wif = "5J3mBbAH58CpQ3Y5RNJpUKPE62SQ5tfcvU2JpbnkeyhfsYB1Jcn"; // standard WIF for testing
    let recipient_pub = hive_xylem::crypto::wif_to_public_key(recipient_wif).unwrap();

    let secret_memo = "#This is a secret memo! 🤫";
    println!("Raw Memo:        {}", secret_memo);

    match memo::encode(sender_wif, &recipient_pub, secret_memo) {
        Ok(encrypted) => {
            println!("Encrypted:       {}", encrypted);

            // Decrypt it using recipient's WIF
            match memo::decode(recipient_wif, &encrypted) {
                Ok(decrypted) => {
                    println!("Decrypted:       {}", decrypted);
                    assert_eq!(secret_memo, decrypted);
                    println!("✓ ECIES encryption/decryption verified successfully!");
                }
                Err(e) => println!("✗ Decryption failed: {:?}", e),
            }
        }
        Err(e) => println!("✗ Encryption failed: {:?}", e),
    }
    println!();

    // 11. Stream blocks (listen to 3 blocks)
    println!("🌊 [PHASE 10] Streaming blocks (latest 3)...");
    let mut block_rx = client.clone().stream_blocks(0, StreamingMode::Latest);
    let mut count = 0;
    while let Some(res) = block_rx.recv().await {
        match res {
            Ok(block) => {
                println!(
                    "✓ Streamed Block: Witness = {}, Prev = {}",
                    block.witness, block.previous
                );
                count += 1;
                if count >= 3 {
                    break;
                }
            }
            Err(e) => {
                println!("✗ Stream error: {:?}", e);
                break;
            }
        }
    }
    println!();

    // 12. Stream operations (listen to 5 vote/transfer operations)
    println!("🌊 [PHASE 11] Streaming operations (latest 5 votes/transfers)...");
    let mut op_rx = client.clone().stream_operations(
        0,
        StreamingMode::Latest,
        vec!["vote".to_string(), "transfer".to_string()],
    );
    let mut op_count = 0;
    while let Some(res) = op_rx.recv().await {
        match res {
            Ok(op) => {
                println!("✓ Streamed Op: Name = {}, Block = {}", op.op.0, op.block);
                op_count += 1;
                if op_count >= 5 {
                    break;
                }
            }
            Err(e) => {
                println!("✗ Stream error: {:?}", e);
                break;
            }
        }
    }

    println!();
    println!("=====================================================");
    println!("   🌿 XYLEM COMPLETE TOUR COMPLETED SUCCESSFULLY 🌿");
    println!("=====================================================");
}
