//! Connect with [`Client::from_env`] using `OC_URL` and `OC_TOKEN`.
//! Clients are cheap to clone and share a connection pool. Dropping a request
//! future stops waiting locally; it does not prove a database write was canceled.
//! No operation is replayed automatically.

mod resources;
mod search;
mod vector;
use reqwest::{Method, Url};
pub use resources::{
    Fts, FtsSearchOptions, Graph, Rows, Schemas, Sql, Vector, VectorSearchOptions,
};
pub use search::{FtsHit, FtsResult};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{fmt, sync::Arc, time::Duration};
pub use vector::VectorHit;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const RESPONSE_LIMIT: usize = 32 * 1024 * 1024;

/// A reusable database client. Debug output deliberately excludes credentials.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}
struct Inner {
    url: String,
    token: String,
    http: reqwest::Client,
    limit: tokio::sync::Semaphore,
}
impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OriginChainDB Client")
    }
}

/// Configure timeouts and the maximum number of concurrent HTTP requests.
pub struct ClientBuilder {
    url: String,
    token: String,
    timeout: Duration,
    max_in_flight: usize,
}
impl ClientBuilder {
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
    pub fn max_in_flight(mut self, max: usize) -> Self {
        self.max_in_flight = max;
        self
    }
    pub fn build(self) -> Result<Client, Error> {
        validate(&self.url, &self.token)?;
        if self.timeout.is_zero() || self.max_in_flight == 0 {
            return Err(Error::Configuration(
                "timeout and max_in_flight must be positive",
            ));
        }
        let http = reqwest::Client::builder()
            .timeout(self.timeout)
            .connect_timeout(self.timeout.min(Duration::from_secs(10)))
            .redirect(reqwest::redirect::Policy::none())
            .pool_max_idle_per_host(20)
            .user_agent(concat!("originchaindb-rust/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(Error::Transport)?;
        Ok(Client {
            inner: Arc::new(Inner {
                url: self.url.trim_end_matches('/').to_owned(),
                token: self.token,
                http,
                limit: tokio::sync::Semaphore::new(self.max_in_flight),
            }),
        })
    }
}

fn validate(address: &str, token: &str) -> Result<(), Error> {
    let url = Url::parse(address).map_err(|_| Error::Configuration("invalid database URL"))?;
    let host = url
        .host_str()
        .ok_or(Error::Configuration("database URL needs a host"))?;
    let loopback = host == "localhost"
        || host
            .trim_matches(['[', ']'])
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    if (url.scheme() != "https" && !(url.scheme() == "http" && loopback))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || address.bytes().any(|c| c <= 32 || c == 127 || c == b'\\')
    {
        return Err(Error::Configuration(
            "use an HTTPS database URL copied from Connect; HTTP is only allowed on loopback",
        ));
    }
    if token.is_empty() || token.bytes().any(|c| !(33..127).contains(&c)) {
        return Err(Error::Configuration(
            "token must be nonempty printable ASCII without spaces",
        ));
    }
    Ok(())
}

impl Client {
    pub fn builder(url: impl Into<String>, token: impl Into<String>) -> ClientBuilder {
        ClientBuilder {
            url: url.into(),
            token: token.into(),
            timeout: Duration::from_secs(60),
            max_in_flight: 100,
        }
    }
    pub fn new(url: impl Into<String>, token: impl Into<String>) -> Result<Self, Error> {
        Self::builder(url, token).build()
    }
    pub fn from_env() -> Result<Self, Error> {
        Self::new(
            std::env::var("OC_URL").map_err(|_| Error::Configuration("OC_URL is required"))?,
            std::env::var("OC_TOKEN").map_err(|_| Error::Configuration("OC_TOKEN is required"))?,
        )
    }
    pub fn sql(&self) -> Sql<'_> {
        Sql(self)
    }
    pub fn rows(&self) -> Rows<'_> {
        Rows(self)
    }
    pub fn schemas(&self) -> Schemas<'_> {
        Schemas(self)
    }
    pub fn vector(&self) -> Vector<'_> {
        Vector(self)
    }
    pub fn fts(&self) -> Fts<'_> {
        Fts(self)
    }
    pub fn graph(&self) -> Graph<'_> {
        Graph(self)
    }
    pub async fn usage(&self) -> Result<Value, Error> {
        self.request(Method::GET, "usage", &[], None, None).await
    }

    pub async fn query_metrics(
        &self,
        minutes: usize,
        bucket_seconds: usize,
    ) -> Result<Value, Error> {
        self.request(
            Method::GET,
            "query-metrics",
            &[
                ("minutes", minutes.to_string()),
                ("bucket_seconds", bucket_seconds.to_string()),
            ],
            None,
            None,
        )
        .await
    }

    async fn request(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<Value>,
        text: Option<&str>,
    ) -> Result<Value, Error> {
        let _permit = self
            .inner
            .limit
            .acquire()
            .await
            .map_err(|_| Error::Protocol("client closed"))?;
        let mut request = self
            .inner
            .http
            .request(method, format!("{}/{path}", self.inner.url))
            .bearer_auth(&self.inner.token)
            .header("Accept", "application/json")
            .query(query);
        if let Some(body) = body {
            request = request.json(&body);
        }
        if let Some(text) = text {
            request = request
                .header("Content-Type", "text/plain")
                .body(text.to_owned());
        }
        let mut response = request.send().await.map_err(Error::Transport)?;
        let status = response.status().as_u16();
        let header = |name| {
            response
                .headers()
                .get(name)
                .and_then(|s| s.to_str().ok())
                .map(str::to_owned)
        };
        let request_id = header("X-OC-Request-Id");
        let retry_after = header("Retry-After");
        let is_text = header("Content-Type").is_some_and(|s| s.starts_with("text/"));
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(Error::Transport)? {
            if bytes.len() + chunk.len() > RESPONSE_LIMIT {
                return Err(Error::Protocol("response exceeds 32 MiB"));
            }
            bytes.extend_from_slice(&chunk);
        }
        let parsed = if bytes.is_empty() {
            Ok(Value::Null)
        } else {
            serde_json::from_slice(&bytes)
        };
        if !(200..300).contains(&status) {
            return Err(Error::Http {
                status,
                request_id,
                retry_after,
                body: parsed.unwrap_or_else(|_| {
                    Value::String(String::from_utf8_lossy(&bytes).into_owned())
                }),
            });
        }
        match parsed {
            Ok(value) => Ok(value),
            Err(_) if is_text => Ok(Value::String(String::from_utf8_lossy(&bytes).into_owned())),
            Err(_) => Err(Error::Protocol("response is not valid JSON")),
        }
    }
}

/// Typed failure. Display excludes payloads and tokens; inspect fields explicitly.
#[derive(Debug)]
pub enum Error {
    Configuration(&'static str),
    Transport(reqwest::Error),
    Protocol(&'static str),
    Http {
        status: u16,
        request_id: Option<String>,
        retry_after: Option<String>,
        body: Value,
    },
    Batch {
        result: Box<SqlResult>,
    },
}
impl Error {
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Http { status, .. } => Some(*status),
            Self::Batch { .. } => Some(200),
            _ => None,
        }
    }
    pub fn is_timeout(&self) -> bool {
        matches!(self, Self::Transport(e) if e.is_timeout())
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(message) | Self::Protocol(message) => f.write_str(message),
            Self::Transport(_) => {
                f.write_str("OriginChainDB transport error; write outcome may be unknown")
            }
            Self::Http { status, .. } => write!(f, "OriginChainDB HTTP {status}"),
            Self::Batch { .. } => f.write_str("SQL batch contains a failed statement"),
        }
    }
}
impl std::error::Error for Error {}

/// Complete SQL result; JSON numeric precision and unknown fields are retained.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqlResult {
    pub kind: String,
    #[serde(default)]
    pub rows: Vec<Map<String, Value>>,
    pub columns: Option<Vec<String>>,
    pub column_types: Option<Map<String, Value>>,
    pub rows_affected: Option<u64>,
    pub rows_buffered: Option<u64>,
    #[serde(default)]
    pub results: Vec<SqlResult>,
    #[serde(default)]
    pub stopped_on_error: bool,
    #[serde(skip)]
    pub raw: Value,
}
impl SqlResult {
    fn decode(value: Value) -> Result<Self, Error> {
        let mut normalized = value.clone();
        let object = normalized
            .as_object_mut()
            .ok_or(Error::Protocol("SQL result must be an object"))?;
        if !object.contains_key("kind") && object.contains_key("error") {
            object.insert("kind".into(), Value::String("error".into()));
        }
        let children = object
            .remove("results")
            .unwrap_or_else(|| Value::Array(vec![]));
        let results = children
            .as_array()
            .ok_or(Error::Protocol("SQL batch results must be an array"))?
            .iter()
            .cloned()
            .map(Self::decode)
            .collect::<Result<Vec<_>, _>>()?;
        if !object.contains_key("columns") {
            if let Some(columns) = object.get("returning").cloned() {
                object.insert("columns".into(), columns);
            }
        }
        if !object.contains_key("rows_affected") {
            if object.get("kind").and_then(Value::as_str) == Some("insert") {
                if let Some(count) = object.get("inserted").cloned() {
                    object.insert("rows_affected".into(), count);
                }
            } else if object.get("kind").and_then(Value::as_str) == Some("select") {
                object.insert("rows_affected".into(), Value::from(0));
            }
        }
        let mut decoded: Self = serde_json::from_value(normalized)
            .map_err(|_| Error::Protocol("invalid SQL result envelope"))?;
        decoded.results = results;
        decoded.raw = value;
        Ok(decoded)
    }
    fn failed(&self) -> bool {
        self.stopped_on_error
            || self.kind == "error"
            || self.raw.get("error").is_some()
            || self.results.iter().any(Self::failed)
    }
}

fn segment(value: &str) -> Result<String, Error> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.bytes().any(|b| b < 32 || b == 127)
    {
        return Err(Error::Configuration(
            "path name must be nonempty and contain no control characters",
        ));
    }
    const ESCAPE: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'~');
    Ok(percent_encoding::utf8_percent_encode(value, ESCAPE).to_string())
}
