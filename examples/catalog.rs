use originchaindb::{Client, Error, FtsSearchOptions, VectorSearchOptions};
use serde_json::{json, Map, Value};

async fn run() -> Result<(), Error> {
    let db = Client::from_env()?;
    // section:setup
    let schema = "sdk_examples.products";
    db.schemas().register(include_str!("products.toml")).await?;
    // endsection

    // section:rows
    db.rows()
        .put(
            schema,
            Map::from_iter([
                ("id".into(), json!("p2")),
                ("name".into(), json!("Gadget")),
                ("description".into(), json!("A red gadget")),
            ]),
        )
        .await?;
    db.rows()
        .put(
            schema,
            Map::from_iter([
                ("id".into(), json!("p1")),
                ("name".into(), json!("Widget")),
                ("description".into(), json!("A small blue widget")),
                ("related_to".into(), json!("p2")),
            ]),
        )
        .await?;
    let product: Value = db.rows().get(schema, "p1").await?;
    println!("{product}");
    // endsection

    // section:sql
    let result = db
        .sql()
        .run(
            "SELECT id, name FROM sdk_examples.products WHERE id = $1",
            vec![json!("p1")],
        )
        .await?;
    for row in &result.rows {
        println!("{} {}", row["id"], row["name"]);
    }
    // endsection

    // section:vector
    let embedding = [0.1, 0.2, 0.3, 0.4];
    db.vector()
        .put(
            schema,
            "p1",
            &embedding,
            Some(Map::from_iter([("category".into(), json!("tools"))])),
        )
        .await?;
    let hits = db
        .vector()
        .search(
            schema,
            &embedding,
            VectorSearchOptions {
                k: 5,
                ..Default::default()
            },
        )
        .await?;
    for hit in hits {
        println!("{} {}", hit.id, hit.score);
    }
    // endsection

    // section:fts
    db.fts()
        .index(schema, "description", "p1", "A small blue widget")
        .await?;
    let matches = db
        .fts()
        .search(
            schema,
            "description",
            "blue widget",
            FtsSearchOptions {
                k: 5,
                ..Default::default()
            },
        )
        .await?;
    for hit in matches.hits {
        println!("{} {}", hit.doc_id, hit.score);
    }
    // endsection

    // section:graph
    let neighbors = db.graph().neighbors(schema, "p1", "related").await?;
    println!("{neighbors}");
    // endsection
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let result = run().await;
    if let Err(Error::Http {
        status, request_id, ..
    }) = &result
    {
        eprintln!("Request failed: HTTP {status}, request ID {request_id:?}");
    }
    result
}
