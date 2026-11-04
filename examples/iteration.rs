use futures_util::{StreamExt, TryStreamExt};
use originchaindb::{BatchOptions, Client, PageOptions};
use serde_json::{json, Map, Value};
use std::{cell::Cell, rc::Rc};

fn source(count: Rc<Cell<usize>>) -> impl Iterator<Item = Map<String, Value>> {
    "abcde".chars().map(move |id| {
        count.set(count.get() + 1);
        json!({"id":id.to_string()}).as_object().unwrap().clone()
    })
}
fn options() -> BatchOptions {
    BatchOptions {
        batch_size: 2,
        ..Default::default()
    }
}
fn pages() -> PageOptions {
    PageOptions {
        page_size: 2,
        ..Default::default()
    }
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = Client::from_env()?;
    let consumed = Rc::new(Cell::new(0));
    let mut batches = Box::pin(db.rows().write_batches(
        "bulk.ok",
        source(consumed.clone()),
        options(),
    ));
    assert_eq!(consumed.get(), 0);
    let first = batches.next().await.unwrap()?;
    assert_eq!((first.offset, first.count, first.inserted), (0, 2, 2));
    assert_eq!(consumed.get(), 2);
    let rest = batches.try_collect::<Vec<_>>().await?;
    assert_eq!(
        rest.iter()
            .map(|b| (b.offset, b.count, b.inserted))
            .collect::<Vec<_>>(),
        vec![(2, 2, 2), (4, 1, 1)]
    );
    for (name, offset, count) in [("fail", 2, 4), ("lost", 0, 2), ("unknown", 0, 2)] {
        let consumed = Rc::new(Cell::new(0));
        let schema = format!("bulk.{name}");
        let error = db
            .rows()
            .write_batches(&schema, source(consumed.clone()), options())
            .try_collect::<Vec<_>>()
            .await
            .unwrap_err();
        assert_eq!(
            (error.offset, error.count, error.outcome),
            (offset, 2, "unknown")
        );
        assert_eq!(consumed.get(), count);
    }
    let reports = db
        .rows()
        .write_batches(
            "bulk.bytes",
            "abc".chars().map(|id| {
                json!({"id":id.to_string(),"value":"x".repeat(20)})
                    .as_object()
                    .unwrap()
                    .clone()
            }),
            BatchOptions {
                max_bytes: 64,
                ..Default::default()
            },
        )
        .try_collect::<Vec<_>>()
        .await?;
    assert_eq!(
        reports.iter().map(|b| b.count).collect::<Vec<_>>(),
        vec![1, 1, 1]
    );
    let consumed = Rc::new(Cell::new(0));
    {
        let mut stream = Box::pin(db.rows().write_batches(
            "bulk.stop",
            source(consumed.clone()),
            options(),
        ));
        stream.next().await.unwrap()?;
    }
    assert_eq!(consumed.get(), 2);
    assert!(db
        .rows()
        .write_batches("bulk.never", vec![], BatchOptions::default())
        .try_collect::<Vec<_>>()
        .await?
        .is_empty());
    let error = db
        .rows()
        .write_batches(
            "bulk.never",
            source(Rc::new(Cell::new(0))),
            BatchOptions {
                max_bytes: 4,
                ..Default::default()
            },
        )
        .try_collect::<Vec<_>>()
        .await
        .unwrap_err();
    assert_eq!((error.offset, error.outcome), (0, "not_sent"));
    assert!(db
        .rows()
        .write_batches(
            "bulk.never",
            vec![],
            BatchOptions {
                batch_size: 0,
                ..Default::default()
            }
        )
        .try_collect::<Vec<_>>()
        .await
        .is_err());
    // Dropping an unpolled stream neither consumes input nor sends a request.
    let consumed = Rc::new(Cell::new(0));
    drop(
        db.rows()
            .write_batches("bulk.never", source(consumed.clone()), options()),
    );
    assert_eq!(consumed.get(), 0);
    let rows = db
        .rows()
        .scan("scan.items", "id", pages())
        .try_collect::<Vec<_>>()
        .await?;
    assert_eq!(
        rows.iter()
            .map(|r| r["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["a", "b", "c", "d", "e"]
    );
    {
        let mut stream = Box::pin(db.rows().scan("scan.stop", "id", pages()));
        assert_eq!(stream.next().await.unwrap()?["id"], "a");
    }
    let result = db
        .rows()
        .pages(
            "scan.resume",
            "id",
            PageOptions {
                after: Some("b".into()),
                ..pages()
            },
        )
        .try_collect::<Vec<_>>()
        .await?;
    assert_eq!(
        result
            .iter()
            .map(|p| p.next_cursor.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("d"), None]
    );
    for name in ["bad", "numeric", "oversize"] {
        assert!(db
            .rows()
            .pages(&format!("scan.{name}"), "id", pages())
            .try_collect::<Vec<_>>()
            .await
            .is_err());
    }
    assert!(db
        .rows()
        .scan("scan.empty", "id", pages())
        .try_collect::<Vec<_>>()
        .await?
        .is_empty());
    assert!(db
        .rows()
        .scan("scan.items;DELETE", "id", pages())
        .try_collect::<Vec<_>>()
        .await
        .is_err());
    println!("Rust iteration assertions passed");
    Ok(())
}
