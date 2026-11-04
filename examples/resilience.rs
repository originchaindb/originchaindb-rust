use originchaindb::{Client, Error, RequestEvent};
use serde::Deserialize;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("OC_URL")?;
    let token = std::env::var("OC_TOKEN")?;
    let client = Client::new(&url, &token)?;
    for (name, expected) in [
        ("expired", 401),
        ("revoked", 401),
        ("forbidden", 403),
        ("limited", 429),
    ] {
        let error = client
            .sql()
            .run(&format!("fault:{name}"), vec![])
            .await
            .unwrap_err();
        match error {
            Error::Http {
                status,
                request_id,
                retry_after,
                ..
            } => {
                assert_eq!(status, expected);
                assert_eq!(
                    request_id.as_deref(),
                    Some("84f92822-5837-432e-981a-7b24ad65e16c")
                );
                if name == "limited" {
                    assert_eq!(retry_after.as_deref(), Some("2"));
                }
            }
            other => panic!("unexpected error {other}"),
        }
    }
    assert!(client.sql().run("fault:interrupted", vec![]).await.is_err());
    let slow = Client::builder(&url, &token)
        .timeout(Duration::from_millis(150))
        .build()?;
    assert!(slow
        .sql()
        .run("fault:timeout", vec![])
        .await
        .unwrap_err()
        .is_timeout());
    let calls = Arc::new(AtomicUsize::new(0));
    let events = Arc::new(Mutex::new(Vec::<RequestEvent>::new()));
    let source = calls.clone();
    let collected = events.clone();
    let rotating = Client::builder_with_token_provider(&url, move || {
        let next = source.fetch_add(1, Ordering::SeqCst) + 1;
        async move { Ok(format!("rotate-{next}")) }
    })
    .on_request(move |e| collected.lock().unwrap().push(e))
    .http_client_builder(reqwest::Client::builder())
    .build()?;
    assert_eq!(
        rotating
            .sql()
            .run("fault:rotate", vec![])
            .await
            .unwrap_err()
            .status(),
        Some(401)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let result = rotating.sql().run("fault:rotate", vec![]).await?;
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    {
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].error_kind.as_deref(), Some("authentication"));
        assert!(
            events[1].error_kind.is_none()
                && events[1].operation == "sql"
                && events[1].duration > Duration::ZERO
        );
        let debug = format!("{events:?}");
        assert!(!debug.contains("rotate-") && !debug.contains(&url));
    }
    #[derive(Deserialize)]
    struct Product {
        id: i64,
        amount: String,
    }
    let products: Vec<Product> = result.rows_as()?;
    assert_eq!(products[0].id, 9007199254740993);
    assert_eq!(products[0].amount, "1.00000000000000001");
    #[derive(Debug, Deserialize)]
    struct Bad {
        #[serde(rename = "id")]
        _id: u8,
    }
    assert_eq!(result.rows_as::<Bad>().unwrap_err().row, 0);
    let invalid =
        Client::builder_with_token_provider(&url, || async { Ok("bad\nheader".into()) }).build()?;
    assert_eq!(
        invalid
            .sql()
            .run("fault:invalid-provider", vec![])
            .await
            .unwrap_err()
            .kind(),
        "credential"
    );
    let observed = Client::builder(&url, &token)
        .on_request(|_| panic!("observer"))
        .build()?;
    // Panic hooks run even when caught. Suppress this deliberately broken observer's hook.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let observed_result = observed.sql().run("fault:observer", vec![]).await;
    std::panic::set_hook(hook);
    assert_eq!(observed_result?.rows.len(), 1);

    let began = Arc::new(tokio::sync::Notify::new());
    let signal = began.clone();
    let collected = events.clone();
    let pending = Client::builder_with_token_provider(&url, move || {
        let signal = signal.clone();
        async move {
            signal.notify_one();
            std::future::pending::<Result<String, Error>>().await
        }
    })
    .on_request(move |e| collected.lock().unwrap().push(e))
    .build()?;
    let task = tokio::spawn(async move { pending.usage().await });
    began.notified().await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(
        events.lock().unwrap().last().unwrap().error_kind.as_deref(),
        Some("canceled")
    );
    println!("Rust resilience assertions passed");
    Ok(())
}
