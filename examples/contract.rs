use originchaindb::{Client, Error};
use serde_json::{json, Map};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::from_env()?;
    let result = client
        .sql()
        .run(
            "contract:select",
            vec![json!(7), json!("quote' / 雪"), json!(null)],
        )
        .await?;
    assert_eq!(result.rows[0]["id"].to_string(), "9007199254740993");
    assert_eq!(result.rows[0]["amount"], "1.00000000000000001");
    assert_eq!(result.raw["future_field"]["kept"], true);
    let result = client.sql().run("contract:update", vec![]).await?;
    assert_eq!(result.rows_affected, Some(2));
    assert_eq!(result.rows_buffered, Some(3));
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.columns, Some(vec!["id".into()]));
    let result = client.sql().run("contract:unknown", vec![]).await?;
    assert_eq!(result.kind, "future_statement");
    assert_eq!(result.rows_affected, None);
    for name in [
        "batch",
        "unavailable",
        "unauthorized",
        "redirect",
        "malformed",
        "invalid-rows",
    ] {
        let error = client
            .sql()
            .run(&format!("contract:{name}"), vec![])
            .await
            .unwrap_err();
        if name == "batch" {
            match error {
                Error::Batch { result } => assert_eq!(result.results[0].rows_affected, Some(1)),
                _ => panic!("Expected batch error"),
            }
        } else if name == "unauthorized" {
            match error {
                Error::Http {
                    status, request_id, ..
                } => {
                    assert_eq!(status, 401);
                    assert_eq!(
                        request_id.as_deref(),
                        Some("84f92822-5837-432e-981a-7b24ad65e16c")
                    );
                }
                _ => panic!("Expected HTTP error"),
            }
        } else if name == "unavailable" {
            match error {
                Error::Http {
                    status,
                    retry_after,
                    ..
                } => {
                    assert_eq!(status, 503);
                    assert_eq!(retry_after.as_deref(), Some("0"));
                }
                _ => panic!("Expected HTTP error"),
            }
        }
    }
    client.rows().get("demo/items", "a b?/%").await?;
    client
        .rows()
        .put(
            "demo.items",
            json!({"id":"one","nullable":null})
                .as_object()
                .unwrap()
                .clone(),
        )
        .await?;
    client
        .rows()
        .put_batch(
            "demo.items",
            vec![
                json!({"id":"one"}).as_object().unwrap().clone(),
                json!({"id":"two"}).as_object().unwrap().clone(),
            ],
        )
        .await?;
    client.schemas().list().await?;
    client
        .schemas()
        .register("[schema]\nname=\"demo.items\"\n")
        .await?;
    client
        .vector()
        .put(
            "demo.items",
            "one",
            &[1.0, 0.0],
            Some(json!({"tag":"test"}).as_object().unwrap().clone()),
        )
        .await?;
    let vectors = client
        .vector()
        .search("demo.items", &[1.0, 0.0], Default::default())
        .await?;
    assert_eq!(
        vectors
            .iter()
            .map(|hit| hit.id.as_str())
            .collect::<Vec<_>>(),
        vec!["one", "two"]
    );
    client.vector().delete("demo.items", "one").await?;
    client
        .fts()
        .index("demo.items", "body", "one", "hello world")
        .await?;
    client
        .fts()
        .search("demo.items", "body", "hello", Default::default())
        .await?;
    let phrase = client
        .fts()
        .search(
            "demo.items",
            "body",
            "phrase",
            originchaindb::FtsSearchOptions {
                mode: "phrase".into(),
                ..Default::default()
            },
        )
        .await?;
    assert_eq!(phrase.hits[0].doc_id, "two");
    assert_eq!(phrase.hits[0].score, 0.0);
    let enriched = client
        .fts()
        .search(
            "demo.items",
            "body",
            "highlight",
            originchaindb::FtsSearchOptions {
                highlight: true,
                facets: Some(vec!["tag".into()]),
                ..Default::default()
            },
        )
        .await?;
    assert!(enriched.facets.is_some() && enriched.hits[0].highlights.is_some());
    let mut params = Map::new();
    params.insert("name".into(), json!("one"));
    client
        .graph()
        .query("MATCH (n) RETURN $name", params)
        .await?;
    client
        .graph()
        .neighbors("demo.items", "one", "related")
        .await?;
    client
        .graph()
        .bfs("demo.items", "one", "related", 5)
        .await?;
    println!("Rust consumer assertions passed");
    Ok(())
}
