use originchaindb::{Client, Error};
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize)]
struct Product {
    id: String,
    name: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder_with_token_provider(std::env::var("OC_URL")?, || async {
        // A deployed application can read from its asynchronously refreshed credential cache.
        std::env::var("OC_TOKEN").map_err(|_| Error::Credential)
    })
    // Configure proxy or root certificates on this native builder when required.
    .http_client_builder(reqwest::Client::builder())
    .on_request(|event| {
        println!(
            "{} {:?} {:?}",
            event.operation, event.status, event.duration
        )
    })
    .build()?;
    let result = client
        .sql()
        .run(
            "SELECT $1 AS id, $2 AS name",
            vec![json!("p1"), json!("Widget")],
        )
        .await?;
    let products: Vec<Product> = result.rows_as()?;
    for product in products {
        println!("{}: {}", product.id, product.name);
    }
    Ok(())
}
