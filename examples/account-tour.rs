use hive_xylem::account::Account;
use hive_xylem::Client;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=====================================================");
    println!("   🌿 XYLEM RUST HIVE SDK - ACCOUNT TOUR 🌿");
    println!("=====================================================");

    // 1. Initialize Client
    let nodes = vec![
        "https://api.hive.blog".to_string(),
        "https://api.syncad.com".to_string(),
    ];
    let api = Arc::new(Client::new(nodes, 30));

    // 2. Initialize Account
    let username = "thecrazygm";
    let mut acc = Account::new(username, Some(api));

    println!("Fetching account details for @{}...", username);
    acc.refresh().await?;

    // 3. Get Reputation
    let reputation = acc.reputation().await?;
    println!("✓ Reputation: {:.3}", reputation);

    // 4. Get Voting Power
    let vp = acc.vp().await?;
    println!("✓ Voting Power: {:.2}%", vp);

    // 5. Get Resource Credit Info
    let rc = acc.rc().await?;
    println!("✓ Resource Credit: {:.2}%", rc);

    let rc_info = acc.rc_info().await?;
    println!("  Max RC:     {}", rc_info.max_mana);
    println!("  Current RC: {}", rc_info.current_mana);

    // 6. Get Account Keys
    println!("\nRetrieving account public keys and weights...");
    if let Some(posting_auth) = acc.posting() {
        println!("✓ Posting Threshold: {}", posting_auth.weight_threshold);
        for (key, weight) in acc.posting_keys() {
            println!("  - Key: {} (Weight: {})", key, weight);
        }
    }
    if let Some(memo_k) = acc.memo_key() {
        println!("✓ Memo Key:          {}", memo_k);
    }
    let all_keys = acc.get_keys();
    println!(
        "✓ All Keys count:    owner={}, active={}, posting={}",
        all_keys.owner.len(),
        all_keys.active.len(),
        all_keys.posting.len()
    );

    // 6. Build Follow / Unfollow Transactions
    println!("\nConstructing operations for follow/unfollow...");
    let follow_tx = acc.follow("srbde").await?;
    println!("✓ Follow transaction constructed (ID: {})", follow_tx.id()?);
    let dict = follow_tx.to_dict();
    println!("  Operations: {:?}", dict["operations"]);

    let unfollow_tx = acc.unfollow("srbde").await?;
    println!(
        "✓ Unfollow transaction constructed (ID: {})",
        unfollow_tx.id()?
    );

    println!("\n=====================================================");
    println!("   🌿 XYLEM ACCOUNT TOUR COMPLETED SUCCESSFULLY 🌿");
    println!("=====================================================");
    Ok(())
}
