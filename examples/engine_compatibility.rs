//! Private integration worker. Credentials arrive over stdin and are never logged.
use futures_util::TryStreamExt;
use originchaindb::{BatchOptions, Client, PageOptions};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let current = Arc::new(Mutex::new(String::from("unset")));
    let calls = Arc::new(AtomicUsize::new(0));
    let token = current.clone();
    let counter = calls.clone();
    let db = Client::builder_with_token_provider(std::env::var("OC_URL")?, move || {
        counter.fetch_add(1, Ordering::SeqCst);
        let value = token.lock().unwrap().clone();
        async move { Ok(value) }
    })
    .build()?;
    for line in std::io::stdin().lock().lines() {
        let cmd: Value = serde_json::from_str(&line?)?;
        *current.lock().unwrap() = cmd["token"].as_str().ok_or("missing token")?.into();
        let before = calls.load(Ordering::SeqCst);
        let mut status = 200;
        match cmd["action"].as_str().ok_or("missing action")? {
            "query" => {
                let params = cmd
                    .get("params")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                match db
                    .sql()
                    .run(cmd["sql"].as_str().ok_or("missing SQL")?, params)
                    .await
                {
                    Ok(result) => {
                        assert_eq!(cmd["status"], 200);
                        assert_eq!(
                            result.rows.len(),
                            cmd.get("rows").and_then(Value::as_u64).unwrap_or(0) as usize
                        );
                    }
                    Err(error) => {
                        status = error.status().ok_or("unexpected non-HTTP failure")?;
                        assert_ne!(cmd["status"], 200);
                        assert_eq!(cmd["status"], status);
                    }
                }
            }
            "bulk" => {
                let schema = cmd["schema"].as_str().ok_or("missing schema")?;
                let rows = "abcde".chars().enumerate().map(|(value, id)| {
                    json!({"id":id.to_string(),"value":value})
                        .as_object()
                        .unwrap()
                        .clone()
                });
                let reports = db
                    .rows()
                    .write_batches(
                        schema,
                        rows,
                        BatchOptions {
                            batch_size: 2,
                            ..Default::default()
                        },
                    )
                    .try_collect::<Vec<_>>()
                    .await?;
                assert_eq!(
                    reports
                        .iter()
                        .map(|b| (b.offset, b.count, b.inserted))
                        .collect::<Vec<_>>(),
                    vec![(0, 2, 2), (2, 2, 2), (4, 1, 1)]
                );
            }
            "scan" => {
                let rows = db
                    .rows()
                    .scan(
                        cmd["schema"].as_str().ok_or("missing schema")?,
                        "id",
                        PageOptions {
                            page_size: 2,
                            ..Default::default()
                        },
                    )
                    .try_collect::<Vec<_>>()
                    .await?;
                assert_eq!(
                    rows.iter()
                        .map(|r| r["id"].as_str().unwrap())
                        .collect::<Vec<_>>(),
                    vec!["a", "b", "c", "d", "e"]
                );
            }
            _ => return Err("unknown compatibility action".into()),
        }
        let requests = calls.load(Ordering::SeqCst) - before;
        assert_eq!(cmd["requests"], requests);
        println!("{}", json!({"ok":true,"status":status,"requests":requests}));
        std::io::stdout().flush()?;
    }
    Ok(())
}
