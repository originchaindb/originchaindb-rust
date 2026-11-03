# Using the client

Create `db` as shown in the [quickstart](../README.md). These independent examples
assume an existing `demo.items` schema and an appropriate search index.

```rust
let result = db.sql().run("SELECT $1 AS value", vec![serde_json::json!(7)]).await?;
let row = db.rows().get("demo.items", "one").await?;
let vectors = db.vector().search("demo.items", &[1.0, 0.0], Default::default()).await?;
let text = db.fts().search("demo.items", "body", "hello", Default::default()).await?;
```

## Results, errors, and lifetime

The fragment belongs inside an async function returning `Result`.
`SqlResult.rows` contains `serde_json::Value`; arbitrary-precision JSON numbers
are retained. `rows_affected` is `Option<u64>`. Vector hits expose `id`, `score`,
`metadata`, and `raw`. Full-text results expose `hits`, `facets`, and `raw`.

Match `Error::Http { status, request_id, .. }`, `Error::Batch { result }`,
`Error::Protocol`, or `Error::Transport` as needed.

Use `Client::builder(url, token).timeout(Duration::from_secs(10)).build()?`
with `std::time::Duration`. Cloned clients share the pool. Dropping a request
future cancels the local wait; it does not establish whether a write committed.

See the [shared behavior contract](contract.md) for parameter binding, batch
semantics, defaults, and retry behavior. The [contract consumer](../examples/contract.rs)
exercises the full common API against a local fixture.
