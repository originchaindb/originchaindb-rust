use originchaindb::Client;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = Client::from_env()?;
    let result = db.sql().run("SELECT $1 AS value", vec![json!(7)]).await?;
    println!("{:?}", result.rows);
    Ok(())
}
