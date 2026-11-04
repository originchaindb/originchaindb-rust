# API v1: Rust

This reference describes the 1.0 source candidate's shared core and application
integration APIs. See the [installation and data examples](../../../README.md),
[migration guide](../../migration-v1.md), and [source](../../../src/lib.rs)
for complete signatures, native types and language-specific helpers.

## Core operations

| Resource | Entry points |
| --- | --- |
| SQL | `sql().run / sql().query / sql().execute` |
| Schemas | `schemas().list / schemas().get / schemas().register` |
| Rows | `rows().get / rows().put / rows().put_batch`  `write_batches / pages / scan` |
| Vectors | `vector().put / vector().search / vector().delete` |
| Full-text search | `fts().index / fts().search` |
| Graph | `graph().query / graph().neighbors / graph().bfs` |
| Usage and query metrics | `usage / query_metrics` |

`run`/`RunAsync` accepts any supported SQL statement and returns the complete
statement envelope. `query`/`QueryAsync` requires a SELECT result. `execute` is
an alias for the complete result path. Bind values with positional parameters;
identifiers must come from trusted application code.

Rows use a schema name and primary key. Schema registration accepts TOML text.
Vector search returns IDs and scores; full-text search returns hits plus metadata
when supplied by the server. Graph operations use the relations declared in the
schema. The README has complete setup and query examples for each resource.

## Bounded iteration

`write_batches` yields BatchResult (offset, count, inserted); `pages` yields RowPage
(rows, `next_cursor`); `scan` yields individual rows. Their iterators are lazy,
sequential, and do not automatically retry writes. `BatchWriteError` identifies an
unconfirmed input range and its original cause. Read the [bulk and pagination
guide](../../bulk-and-pagination.md) for exact limits, supported key types,
cancellation and checkpoint semantics.

## Configure an application

`Client::builder_with_token_provider(url, provider).on_request(observer).build()`

Existing static-token constructors remain supported. Choose one credential source.
Copy the entire database address from Connect; no tenant path is inferred.

Use ClientBuilder::http_client_builder(reqwest::ClientBuilder) for proxy and trust roots. The SDK reapplies its timeout, user agent and redirect refusal. Client clones share the connection pool.

The [complete integration example](../../../examples/integration.rs) uses a read-only
parameterized query, typed products, a token provider and a local observer. It
needs `OC_URL` and `OC_TOKEN`, but no schema setup or embedding service:

```sh
cargo run --example integration
```

## Rotating credentials

An async closure returns Result<String, Error>. It must be Send, Sync and static; the returned future is Send. The request deadline includes credential resolution.

The SDK validates every returned token and sends it unchanged as a bearer value.
No prefix is required. It does not implement a refresh protocol or automatically
retry after a 401/403. The next application call resolves the provider again.
Provider failures do not send an HTTP request and do not expose the provider's
exception message in the SDK's error text. Callbacks must not log credentials.

The example reads the process environment on each call. External secret rotation
does not update an existing process environment automatically; a deployed service
should return the current value from its own secret manager/cache.

## Typed rows

`result.rows_as::<Product>()`

DeserializeOwned models use Serde rules, including serde attributes and custom deserializers. JSON precision remains intact before conversion. RowDecodeError.row identifies a failed row and Error::source exposes the native cause.

Conversion makes no network request, preserves the raw result, and returns no
partial list on failure. It does not invent missing execution counts or coerce
exact decimal strings through floating point. Application-selected conversions
can change precision, so choose model types appropriate to your schema.

## Local request observation

The optional observer receives method, operation family, elapsed time
(`duration (std::time::Duration)`), HTTP status, server request ID, and error category.
It receives no database URL, schema name, SQL text, query values, headers, response
body or credentials. It does not enable a telemetry exporter. Forward these
fields into your existing logs, metrics or tracing system when needed.

Observers run once per logical call. They should be short and nonblocking;
ordinary callback failures are isolated from the database result. Status is
absent (zero in Go) if no response headers arrived. A successful HTTP event may
be followed by a result-decoding, batch or model-conversion failure.

Common failure categories include authentication, permission, rate_limit,
timeout, canceled, transport, protocol, and credential. Native error types and
fields remain available: `Error::Credential; RowDecodeError.row`. The README explains HTTP errors,
partial batches, and request IDs.

## Cancellation and ownership

Dropping a request future stops local work. Deadline coverage includes waiting for a concurrency permit, token lookup and HTTP work. The observer reports canceled when an in-progress future is dropped.

Stopping the local wait does not prove that a server-side write was canceled.
Automatic SDK retries are disabled by default. Inspect a failed batch's partial
result before deciding how the application should recover.

## Compatibility checks

CI compiles/runs the native consumer, shared 24-request HTTP contract, nine-request
resilience contract, 21-request bounded iteration contract, public API baseline, and applicable language tests. Those consumers exercise the
public core API and fail when a required method, type or behavior is removed.
The shared fixtures are pinned to the [organization contract](https://github.com/originchaindb/.github/tree/073b0396db55c903ca114f15fab8cf83c4b92e3d/sdk-contract/v1).
Changes to either fixture must be coordinated across all eight SDKs.

These checks cover the shared core. Changes to additional helpers also require
their native tests and API review. Fixtures model authentication failures; they
do not certify a deployed token lifecycle or an untested server version.

See [live verification](../../live-testing.md) for the manually dispatched database check.
