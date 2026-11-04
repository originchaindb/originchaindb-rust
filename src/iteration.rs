use crate::{Error, Rows};
use futures_util::{stream, Stream, StreamExt, TryStreamExt};
use serde_json::{Map, Value};

/// One acknowledged input range. Earlier results remain valid after a later error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchResult {
    pub offset: u64,
    pub count: usize,
    pub inserted: usize,
}
#[derive(Debug, Clone)]
pub struct BatchOptions {
    pub batch_size: usize,
    pub max_bytes: usize,
}
impl Default for BatchOptions {
    fn default() -> Self {
        Self {
            batch_size: 1000,
            max_bytes: 1024 * 1024,
        }
    }
}
/// An unconfirmed range. `unknown` means the request may have committed.
#[derive(Debug)]
pub struct BatchWriteError {
    pub offset: u64,
    pub count: usize,
    pub outcome: &'static str,
    pub source: Error,
}
impl std::fmt::Display for BatchWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "batch at input offset {} failed ({})",
            self.offset, self.outcome
        )
    }
}
impl std::error::Error for BatchWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}
#[derive(Debug, Clone)]
pub struct PageOptions {
    pub page_size: usize,
    pub after: Option<String>,
}
impl Default for PageOptions {
    fn default() -> Self {
        Self {
            page_size: 1000,
            after: None,
        }
    }
}
#[derive(Debug, Clone)]
pub struct RowPage {
    pub rows: Vec<Map<String, Value>>,
    pub next_cursor: Option<String>,
}

impl<'a> Rows<'a> {
    /// Lazily write bounded atomic JSON batches. Dropping the stream stops future
    /// requests; dropping an in-flight write does not prove rollback.
    pub fn write_batches<I>(
        &self,
        schema: &'a str,
        rows: I,
        options: BatchOptions,
    ) -> impl Stream<Item = Result<BatchResult, BatchWriteError>> + 'a
    where
        I: IntoIterator<Item = Map<String, Value>>,
        I::IntoIter: 'a,
    {
        let client = self.0;
        let input = rows.into_iter();
        stream::try_unfold(
            (input, None, 0_u64, false),
            move |(mut input, mut pending, offset, done)| {
                let options = options.clone();
                async move {
                    let fail = |count, outcome, source| BatchWriteError {
                        offset,
                        count,
                        outcome,
                        source,
                    };
                    if options.batch_size == 0
                        || options.batch_size > 10000
                        || options.max_bytes < 2
                        || options.max_bytes > 8 * 1024 * 1024
                    {
                        return Err(fail(
                            0,
                            "not_sent",
                            Error::Configuration("invalid batch bounds"),
                        ));
                    }
                    crate::segment(schema).map_err(|e| fail(0, "not_sent", e))?;
                    if done {
                        return Ok(None);
                    }
                    let mut batch = Vec::new();
                    let mut size = 2;
                    let mut ended = false;
                    while batch.len() < options.batch_size {
                        // Give cancellation a polling point even for expensive synchronous producers.
                        tokio::task::yield_now().await;
                        let row = match pending.take().or_else(|| input.next()) {
                            Some(row) => row,
                            None => {
                                ended = true;
                                break;
                            }
                        };
                        let bytes = serde_json::to_vec(&row)
                            .map_err(|_| {
                                fail(
                                    batch.len() + 1,
                                    "not_sent",
                                    Error::Configuration("row cannot be encoded"),
                                )
                            })?
                            .len();
                        if bytes + 2 > options.max_bytes {
                            return Err(fail(
                                batch.len() + 1,
                                "not_sent",
                                Error::Configuration("row exceeds max_bytes"),
                            ));
                        }
                        if !batch.is_empty() && size + 1 + bytes > options.max_bytes {
                            pending = Some(row);
                            break;
                        }
                        size += bytes + usize::from(!batch.is_empty());
                        batch.push(row);
                    }
                    if batch.is_empty() {
                        return Ok(None);
                    }
                    let count = batch.len();
                    let value = client
                        .rows()
                        .put_batch(schema, batch)
                        .await
                        .map_err(|e| fail(count, "unknown", e))?;
                    let inserted = value
                        .get("inserted")
                        .and_then(Value::as_u64)
                        .filter(|n| *n <= count as u64)
                        .ok_or_else(|| {
                            fail(count, "unknown", Error::Protocol("invalid inserted count"))
                        })? as usize;
                    Ok(Some((
                        BatchResult {
                            offset,
                            count,
                            inserted,
                        },
                        (input, pending, offset + count as u64, ended),
                    )))
                }
            },
        )
    }

    /// Keyset pages for a unique immutable non-null string key with binary ordering.
    /// This is not a database snapshot. No page is fetched until the stream is polled.
    pub fn pages(
        &self,
        schema: &'a str,
        key: &'a str,
        options: PageOptions,
    ) -> impl Stream<Item = Result<RowPage, Error>> + 'a {
        let client = self.0;
        stream::try_unfold((options.after, false), move |(after, done)| async move {
            let valid = |name: &str| {
                let mut bytes = name.bytes();
                bytes
                    .next()
                    .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
                    && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
            };
            let parts: Vec<_> = schema.split('.').collect();
            if parts.len() > 2
                || !parts.iter().all(|s| valid(s))
                || !valid(key)
                || options.page_size == 0
                || options.page_size > 10000
            {
                return Err(Error::Configuration("invalid page options"));
            }
            if done {
                return Ok(None);
            }
            let table = parts
                .iter()
                .map(|s| format!("\"{s}\""))
                .collect::<Vec<_>>()
                .join(".");
            let condition = if after.is_some() {
                format!(" WHERE \"{key}\" > $1")
            } else {
                String::new()
            };
            let sql = format!(
                "SELECT * FROM {table}{condition} ORDER BY \"{key}\" ASC LIMIT {}",
                options.page_size
            );
            let params = after.iter().cloned().map(Value::String).collect();
            let rows = client.sql().query(&sql, params).await?.rows;
            if rows.len() > options.page_size {
                return Err(Error::Protocol("page exceeds requested size"));
            }
            let mut last = after.as_deref();
            for row in &rows {
                let value = row
                    .get(key)
                    .and_then(Value::as_str)
                    .ok_or(Error::Protocol("page key must be a string"))?;
                if last.is_some_and(|last| value <= last) {
                    return Err(Error::Protocol("page key did not advance"));
                }
                last = Some(value);
            }
            if rows.is_empty() {
                return Ok(None);
            }
            let next = if rows.len() < options.page_size {
                None
            } else {
                last.map(str::to_owned)
            };
            let done = next.is_none();
            Ok(Some((
                RowPage {
                    rows,
                    next_cursor: next.clone(),
                },
                (next, done),
            )))
        })
    }

    /// Flatten one bounded page at a time without prefetching.
    pub fn scan(
        &self,
        schema: &'a str,
        key: &'a str,
        options: PageOptions,
    ) -> impl Stream<Item = Result<Map<String, Value>, Error>> + 'a {
        self.pages(schema, key, options)
            .map_ok(|page| stream::iter(page.rows.into_iter().map(Ok)))
            .try_flatten()
            .fuse()
    }
}
