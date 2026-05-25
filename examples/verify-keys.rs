use std::env;
use xylem::crypto::wif_to_public_key;
use xylem::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wif = match env::var("ACTIVE_WIF") {
        Ok(val) => val,
        Err(_) => {
            eprintln!("ACTIVE_WIF environment variable is not set");
            std::process::exit(1);
        }
    };

    // Derive the public key from the WIF key
    let derived_pub = wif_to_public_key(&wif)?;

    println!("=== Xylem Rust Hive SDK - Key Verification ===");
    println!("Derived Active Public Key: {}\n", derived_pub);

    // Instantiate the client
    let client = Client::new(vec!["https://api.hive.blog".to_string()], 30);

    println!("Looking up account names for the derived public key...");
    let refs = client
        .get_key_references(std::slice::from_ref(&derived_pub))
        .await?;
    let account_name = if let Some(first_ref) = refs.first() {
        println!("✓ Public key is registered to account: @{}", first_ref);
        first_ref.clone()
    } else {
        println!("⚠️ Public key is not registered to any account. Falling back to @thecrazygm");
        "thecrazygm".to_string()
    };

    println!("\nQuerying blockchain for @{}...", account_name);
    let accounts = client.get_accounts(&[account_name]).await?;
    if accounts.is_empty() {
        eprintln!("Account not found");
        std::process::exit(1);
    }

    let acc = &accounts[0];
    println!("✓ Account Name:  @{}", acc.name);
    println!("  HIVE Balance:  {}", acc.balance);
    println!("  HBD Balance:   {}", acc.hbd_balance);
    println!("  Vesting:       {}", acc.vesting_shares);
    println!("  Voting Power:  {:.2}%", acc.voting_power / 100.0);

    println!("\n=== Example Completed Successfully ===");
    Ok(())
}
