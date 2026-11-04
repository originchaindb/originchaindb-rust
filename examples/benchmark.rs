use originchaindb::Client;
use serde_json::json;
use std::time::Instant;

async fn query(client: &Client, mode: &str) -> Result<f64, originchaindb::Error> {
    let start = Instant::now();
    let result = client.sql().query(&format!("bench:{mode}"), vec![]).await?;
    assert_eq!(result.rows.len(), if mode == "large" { 2048 } else { 1 });
    Ok(start.elapsed().as_secs_f64() * 1000.0)
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::from_env()?;
    let mut report = serde_json::Map::new();
    for _ in 0..10 {
        query(&client, "warm").await?;
    }
    for (mode, count) in [("sequential", 100), ("concurrent", 64), ("large", 10)] {
        let start = Instant::now();
        let mut values = Vec::new();
        if mode == "concurrent" {
            let mut tasks = tokio::task::JoinSet::new();
            for _ in 0..8 {
                let client = client.clone();
                tasks.spawn(async move {
                    let mut values = Vec::new();
                    for _ in 0..8 {
                        values.push(query(&client, "concurrent").await?);
                    }
                    Ok::<_, originchaindb::Error>(values)
                });
            }
            while let Some(result) = tasks.join_next().await {
                values.extend(result??);
            }
        } else {
            for _ in 0..count {
                values.push(query(&client, mode).await?);
            }
        }
        report.insert(format!("{mode}_ms"), json!(values));
        report.insert(
            format!("{mode}_rps"),
            json!(f64::from(count) / start.elapsed().as_secs_f64()),
        );
    }
    println!("OC_BENCHMARK {}", serde_json::to_string(&report)?);
    Ok(())
}
