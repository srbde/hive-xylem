use chrono::Utc;
use std::env;
use std::time::Duration;
use xylem::operations::Transfer;
use xylem::types::HiveTime;
use xylem::{Client, Transaction};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wif = match env::var("ACTIVE_WIF") {
        Ok(val) => val,
        Err(_) => {
            eprintln!("ACTIVE_WIF environment variable is not set");
            std::process::exit(1);
        }
    };

    // Initialize the client
    let client = Client::new(vec!["https://api.hive.blog".to_string()], 30);

    // Get dynamic global properties to get TaPoS parameters
    println!("Fetching global properties...");
    let props = client.get_dynamic_global_properties().await?;
    let ref_block_num = (props.head_block_number & 0xFFFF) as u16;

    // Extract block prefix from head block ID
    let prefix_bytes = hex::decode(&props.head_block_id[8..16])?;
    let ref_block_prefix = u32::from_le_bytes(prefix_bytes.try_into().unwrap());

    // Create transaction set to expire in 1 minute
    let expiration = HiveTime(Utc::now().naive_utc() + chrono::Duration::minutes(1));
    let mut tx = Transaction::new(ref_block_num, ref_block_prefix, expiration);

    // Append the transfer operation with the xylem emoji memo
    tx.append_op(Box::new(Transfer {
        from: "thecrazygm".to_string(),
        to: "ecoinstant".to_string(),
        amount: "0.001 HIVE".to_string(),
        memo: "Sent with Xylem 🧬".to_string(),
    }));

    // Sign the transaction
    println!("Signing transaction offline...");
    let mainnet_chain_id = "beeab0de00000000000000000000000000000000000000000000000000000000";
    tx.sign(&wif, mainnet_chain_id)?;

    let trx_id = tx.id()?;
    println!("Transaction ID derived: {}", trx_id);

    // Broadcast
    println!("Broadcasting transaction to Hive network...");
    let response = client.broadcast_transaction(&tx).await?;
    println!("✓ Broadcast Response: {}\n", response);

    // Polling for block inclusion
    print!("Polling for block inclusion");
    let mut found_tx = None;
    for _ in 0..15 {
        let res = client
            .call(
                "condenser_api",
                "get_transaction",
                serde_json::json!([trx_id]),
            )
            .await;
        if let Ok(tx_val) = res {
            if !tx_val.is_null() && tx_val.get("block_num").is_some() {
                found_tx = Some(tx_val);
                break;
            }
        }
        print!(".");
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    println!();

    if let Some(tx_data) = found_tx {
        println!("🎉 SUCCESS! Transaction found in block!");
        println!(
            "Full Tx Details: {}",
            serde_json::to_string_pretty(&tx_data)?
        );
    } else {
        println!("⚠️ Transaction not found within polling timeout.");
    }

    Ok(())
}
