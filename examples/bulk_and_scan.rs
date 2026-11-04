// Run against a development database; writes sdk_iteration.items.
use futures_util::TryStreamExt;
use originchaindb::{BatchOptions, Client, PageOptions};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = Client::from_env()?;
    let schema = "sdk_iteration.items";
    db.schemas().register(include_str!("items.toml")).await?;
    let rows = "abcde".chars().map(|id| {
        json!({"id":id.to_string(),"name":format!("Item {id}")})
            .as_object()
            .unwrap()
            .clone()
    });
    // section:bulk
    let mut batches = Box::pin(db.rows().write_batches(
        schema,
        rows,
        BatchOptions {
            batch_size: 2,
            ..Default::default()
        },
    ));
    while let Some(batch) = batches.try_next().await? {
        println!("{} {} {}", batch.offset, batch.count, batch.inserted);
    }
    // endsection
    // section:scan
    let mut scan = Box::pin(db.rows().scan(
        schema,
        "id",
        PageOptions {
            page_size: 2,
            ..Default::default()
        },
    ));
    while let Some(row) = scan.try_next().await? {
        println!("{} {}", row["id"], row["name"]);
    }
    // endsection
    // Save a checkpoint only after successfully processing the complete page.
    let mut pages = Box::pin(db.rows().pages(
        schema,
        "id",
        PageOptions {
            page_size: 2,
            after: Some("b".into()),
        },
    ));
    while let Some(page) = pages.try_next().await? {
        for row in page.rows {
            println!("resumed {}", row["id"]);
        }
        println!("next cursor {:?}", page.next_cursor);
    }
    Ok(())
}
