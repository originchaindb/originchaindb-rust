# OriginChainDB for Rust

The official Rust client for SQL, vector search, full-text search, and graph queries.

[![CI](https://github.com/originchaindb/originchaindb-rust/actions/workflows/ci.yml/badge.svg)](https://github.com/originchaindb/originchaindb-rust/actions/workflows/ci.yml)

## Get started

Requires **Rust 1.88+ with Tokio**. This is the unpublished 1.0 release candidate. Build from
source and pin a reviewed commit; registry publication is currently on hold.

In your application's `Cargo.toml`, replace `<commit>` with a reviewed revision:

```toml
[dependencies]
originchaindb = { git = "https://github.com/originchaindb/originchaindb-rust", rev = "<commit>" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
serde_json = "1"
```

Copy your database address and access token from Connect into `OC_URL` and
`OC_TOKEN`. Keep tokens in your server's environment or secret manager.

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

Reuse the client for your application's lifetime. Pass values as parameters;
do not interpolate application data into query text.

## One API family

Use `sql`, `schemas`, `rows`, `vector`, `fts`, and `graph` for the same database
concepts across every OriginChainDB SDK. Names use this language's normal casing.

- SQL results include returned rows, confirmed counts, metadata and batch results.
- Full-text search provides the same hit structure for ranked and unranked queries.
- Access tokens are sent as bearer credentials to the exact database address.
- Failed writes are not retried automatically.

[Using the client](docs/api.md) Â· [Behavior](docs/contract.md) Ã‚Â· [Contributing](CONTRIBUTING.md) Ã‚Â·
[Changes](CHANGELOG.md) Ã‚Â· [Security](SECURITY.md)

## Development

```sh
cargo fmt --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
python tests/contract/run.py -- cargo run --locked --example contract
cargo package --locked --allow-dirty
```

The HTTP contract runner requires Python 3.10+ for testing only. No database
credentials are needed for the offline suite. Native runtime and dependency
requirements above apply to applications.
