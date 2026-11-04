# OriginChainDB for Rust

The official Rust SDK for working with OriginChainDB through SQL, rows,
vector search, full-text search, and graph relationships. Use one database client
for these operations, with native Rust types and conventions.

[![CI](https://github.com/originchaindb/originchaindb-rust/actions/workflows/ci.yml/badge.svg)](https://github.com/originchaindb/originchaindb-rust/actions/workflows/ci.yml)

**Requirements:** Rust 1.88+ and Tokio. **Package:** `originchaindb`.

> This README describes the **1.0.0-rc.1 source candidate**. It has not been
> published to package registries. Follow the source installation below; an older
> registry package may expose a different API.

[Install](#installation) | [Connect](#connect-and-run-your-first-query) |
[Examples](#product-catalog-example) | [Results](#understanding-sql-results) |
[Errors](#handling-errors) | [Source](#source-layout)

## Installation

Create an application with `cargo new myapp`, then add these entries to the
`[dependencies]` section of its `Cargo.toml`. The Git revision pins the verified SDK:

```toml
[dependencies]
originchaindb = { git = "https://github.com/originchaindb/originchaindb-rust", rev = "fb0dc4964efd92cef711148b27c9d5020a1500f9" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
serde_json = "1"
```

The client is asynchronous and uses Reqwest with Rustls for HTTPS.

Pin a reviewed source commit in your application or build process. See the
[changelog](CHANGELOG.md) before updating.

## Connect and run your first query

Copy the complete database address from **Connect** into `OC_URL`, and the
database access token into `OC_TOKEN`. The SDK sends the token as a bearer
credential. It uses the address as supplied; do not add or derive a tenant path.

macOS/Linux shell:

```sh
export OC_URL="<database HTTPS address from Connect>"
export OC_TOKEN="<database access token>"
```

PowerShell:

```powershell
$env:OC_URL = "<database HTTPS address from Connect>"
$env:OC_TOKEN = "<database access token>"
```

Replace the placeholders, keep credentials out of source control, and use a
secret manager for deployed applications. HTTPS is required except for local
loopback development. SQL tools use a separate database login from Connect.

Put this in `src/main.rs` and run `cargo run`.

```rust
use originchaindb::Client;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = Client::from_env()?;
    let result = db.sql().run("SELECT $1 AS value", vec![json!(7)]).await?;
    println!("{:?}", result.rows);
    Ok(())
}
```

The query needs no tables. Its returned row contains `value` equal to `7`.
`$1` binds the first parameter; `$2` binds the second. Bind values instead of
interpolating application input into SQL. Parameters do not substitute table or
column names; those identifiers must come from trusted application code.

## Product catalog example

The complete [catalog program](examples/catalog.rs) creates a small product catalog and
demonstrates the same sequence shown below. It uses the included
[products.toml](examples/products.toml) schema and writes two rows plus search
data under `sdk_examples.products`. **Run it against a development database.**
Your token needs permission to register schemas and read/write the relevant data.

Run from the SDK checkout root after installation and setting the two variables:

```sh
git clone https://github.com/originchaindb/originchaindb-rust.git
cd originchaindb-rust
cargo run --example catalog
```

The sections below are excerpts from that complete program. Rust snippets belong inside an async function returning `Result<(), Error>`, after creating `db`.
Required imports are `originchaindb::{Client, Error, FtsSearchOptions, VectorSearchOptions}` and `serde_json::{json, Map, Value}`; the complete file includes them.

### Register the schema

The schema declares string columns `id`, `name`, `description`, and `related_to`.
`id` is the primary key. The `related` relation connects a product to the product
named by `related_to`. Registration reads the TOML file; it does not require
you to construct a schema request manually.

```rust
let schema = "sdk_examples.products";
db.schemas().register(include_str!("products.toml")).await?;
```

### Write and read rows

Create the referenced product first, then the product pointing to it. `put`
writes the supplied row; `get` retrieves it by primary key. The example prints
the row for `p1`, named `Widget`.

```rust
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
```

For multiple rows, the rows namespace also provides a batch-put operation.
Treat its response as a batch report, not as a promise of transaction atomicity.

### Query with SQL

Use `sql.run` (with the language's casing) for a complete statement result.
`execute` is an alias; `query` is intended for SELECT results.

```rust
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
```

Use the same parameter binding for INSERT, UPDATE, and DELETE. Read the
server-reported write count rather than assuming a successful request changed a row.

### Search by vector similarity

Store an embedding with its product ID, then request up to five nearest hits.
The default metric is cosine. These four numbers are a small illustrative vector;
generate real embeddings with your chosen model, and use that same model and
dimension for stored vectors and queries. The SDK does not generate embeddings.

```rust
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
```

Each hit exposes an ID, score, optional metadata, and the original hit. Do not
assume the score is a probability; its interpretation depends on the metric.

### Search text

Index the description explicitly, then use ranked BM25 search. Search defaults
to ten hits; the example asks for five. Boolean and phrase modes, highlighting,
fuzzy matching, and facets are available through the language's search options.

```rust
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
```

The result contains `hits`, optional `facets`, and the original response.
Hits have a document ID and score; unranked modes use zero for the score.
Writing a row does not imply this example's separate full-text or vector index
entry has been updated: update the relevant index when your application changes data.

### Follow graph relationships

The schema's `related` relation and the `p1.related_to = p2` row value create the
connection used here. The result includes the neighbor `p2`.

```rust
let neighbors = db.graph().neighbors(schema, "p1", "related").await?;
println!("{neighbors}");
```

The graph namespace also supports breadth-first traversal and Cypher queries.
Cypher parameters bind **by name**; SQL parameters bind **by position**.
See the [API guide](docs/api.md) and source links below for exact signatures.

## Understanding SQL results

Rows are `Vec<Map<String, Value>>`; arbitrary-precision JSON numbers are retained. SQL returns a result object rather than only an array:

| Member | Meaning |
| --- | --- |
| `kind` | Statement kind, such as select, insert, update or delete |
| `rows` | Returned rows, including RETURNING rows when the server supplies them |
| `columns` | Column names, when supplied |
| `column_types` | Column type metadata, when supplied |
| `rows_affected` | Server-reported write count; missing means unknown |
| `rows_buffered` | Buffered count, when supplied; it is not proof of a committed write |
| `results` | Individual results for a multi-statement response |
| `raw` | Original response, including additional server fields |

Write counts use `Option<u64>`; `None` means the server did not report
a count. Empty rows are not evidence of a failed statement: a write may return
only counts. Conversely, a malformed response is an error, not an empty success.

Numeric and temporal **strings** remain strings. Convert them deliberately in
your application when you know the column type. Unsupported write-count ranges
raise a protocol error instead of silently rounding.

Multi-statement batches can partially succeed. A failed batch exposes the
available statement results through its error. Do not assume batches are atomic
or replay an entire batch without checking which statements completed.

## Timeouts and client lifetime

The default request timeout is 60 seconds. Use the builder for configuration:

```rust
use std::{env, time::Duration};
use originchaindb::Client;

let db = Client::builder(env::var("OC_URL")?, env::var("OC_TOKEN")?)
    .timeout(Duration::from_secs(10))
    .max_in_flight(32)
    .build()?;
```

This fragment belongs inside a function returning a compatible `Result`.
Clone `Client` to share its connection pool across tasks; cloning does not create
a new pool. Dropping a request future stops waiting locally.

Requests are **not retried automatically by the SDK by default**, and redirects
are not followed. A timeout or disconnect can happen after a write committed.
Canceling the local wait does not prove rollback. Only retry an operation when
your application can safely reconcile or repeat it.

## Handling errors

```rust
use originchaindb::Error;
use serde_json::json;

match db.sql().run("SELECT $1 AS value", vec![json!(7)]).await {
    Ok(result) => println!("{:?}", result.rows),
    Err(Error::Http { status, request_id, .. }) => {
        eprintln!("HTTP {status}, request ID {request_id:?}");
    }
    Err(Error::Batch { result }) => {
        eprintln!("Partial batch: {} statements", result.results.len());
    }
    Err(error) => return Err(error),
}
```

Use this inside an async function returning `Result<_, Error>`.
Other variants are `Configuration`, `Protocol`, and `Transport`.
`error.is_timeout()` distinguishes a transport timeout.

| Symptom | What to check |
| --- | --- |
| 401 or 403 | Token validity, expiry, database access and operation grants |
| 404 | Complete database address, schema name and resource ID |
| 400 or 422 | SQL syntax, parameter order, schema fields and vector dimensions |
| 429 | Capacity/rate limits and any server-provided retry timing |
| Timeout or 5xx after a write | Check the outcome before retrying |
| Empty search results | Indexed data, query terms, dimensions, filters and feature availability |

Include the SDK version and request ID in a support report. Redact credentials
and application data. Available database features and token permissions still
determine which operations can run.

## Application integration

The [API v1 reference](docs/api/v1/README.md) covers typed row conversion,
rotating credentials, local request diagnostics, native HTTP configuration,
and cancellation. The [complete integration example](examples/integration.rs) shows
these features together using a read-only query. See the
[migration guide](docs/migration-v1.md) when upgrading an earlier application.

## Source layout

| Path | Purpose |
| --- | --- |
| [src/lib.rs](src/lib.rs) | Client, builder, transport, SQL result and error types |
| [src/resources.rs](src/resources.rs) | Operation services and search options |
| [src/search.rs](src/search.rs), [src/vector.rs](src/vector.rs) | Search result decoding |
| [Cargo.toml](Cargo.toml) | Features, dependencies and minimum Rust version |
| [examples/](examples/) | Quickstart, product catalog and schema |
| [tests/contract/run.py](tests/contract/run.py) | Shared offline HTTP conformance fixture |
| [docs/api.md](docs/api.md), [docs/contract.md](docs/contract.md) | Native API notes and shared behavior |

All eight SDKs share the same core concepts and HTTP contract. Method casing,
JSON types and concurrency follow each language. Historical Python/TypeScript
helpers are broader than this common core; do not assume every advanced helper
exists in every SDK.

## Development and support

From the checkout root:

```sh
cargo fmt --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
python tests/contract/run.py -- cargo run --locked --example contract
cargo package --locked --allow-dirty
```

The contract runner requires Python 3.10+ for testing only and uses a local
fixture; it does not require database credentials. Application runtime
requirements are listed at the top. Live examples use your selected database.

Read [Contributing](CONTRIBUTING.md) before sending a change. Report bugs through
[GitHub issues](https://github.com/originchaindb/originchaindb-rust/issues); report security concerns
privately as described in [Security](SECURITY.md). See [LICENSE](LICENSE) for MIT terms.
