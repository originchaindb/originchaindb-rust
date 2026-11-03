use crate::{segment, Client, Error, FtsResult, SqlResult, VectorHit};
use reqwest::Method;
use serde_json::{json, Map, Value};

pub struct Sql<'a>(pub(crate) &'a Client);
impl Sql<'_> {
    pub async fn run(&self, statement: &str, params: Vec<Value>) -> Result<SqlResult, Error> {
        if statement.trim().is_empty() {
            return Err(Error::Configuration("SQL statement must not be empty"));
        }
        let result = SqlResult::decode(
            self.0
                .request(
                    Method::POST,
                    "sql",
                    &[],
                    Some(json!({"sql": statement, "params": params})),
                    None,
                )
                .await?,
        )?;
        if result.failed() {
            return Err(Error::Batch {
                result: Box::new(result),
            });
        }
        Ok(result)
    }
    pub async fn query(&self, statement: &str, params: Vec<Value>) -> Result<SqlResult, Error> {
        let result = self.run(statement, params).await?;
        if result.kind != "select" {
            return Err(Error::Protocol(
                "sql.query expected SELECT; use sql.run for other statements",
            ));
        }
        Ok(result)
    }
    pub async fn execute(&self, statement: &str, params: Vec<Value>) -> Result<SqlResult, Error> {
        self.run(statement, params).await
    }
}

pub struct Schemas<'a>(pub(crate) &'a Client);
impl Schemas<'_> {
    pub async fn list(&self) -> Result<Value, Error> {
        self.0
            .request(Method::GET, "schemas", &[], None, None)
            .await
    }
    pub async fn get(&self, name: &str) -> Result<Value, Error> {
        self.0
            .request(
                Method::GET,
                &format!("schemas/{}", segment(name)?),
                &[],
                None,
                None,
            )
            .await
    }
    pub async fn register(&self, toml: &str) -> Result<Value, Error> {
        self.0
            .request(Method::POST, "schemas", &[], None, Some(toml))
            .await
    }
}

pub struct Rows<'a>(pub(crate) &'a Client);
impl Rows<'_> {
    pub async fn get(&self, schema: &str, key: &str) -> Result<Value, Error> {
        self.0
            .request(
                Method::GET,
                &format!("rows/{}/{}", segment(schema)?, segment(key)?),
                &[],
                None,
                None,
            )
            .await
    }
    pub async fn put(&self, schema: &str, row: Map<String, Value>) -> Result<Value, Error> {
        self.0
            .request(
                Method::POST,
                &format!("rows/{}", segment(schema)?),
                &[],
                Some(Value::Object(row)),
                None,
            )
            .await
    }
    pub async fn put_batch(
        &self,
        schema: &str,
        rows: Vec<Map<String, Value>>,
    ) -> Result<Value, Error> {
        self.0
            .request(
                Method::POST,
                &format!("rows/{}/_batch", segment(schema)?),
                &[],
                Some(json!(rows)),
                None,
            )
            .await
    }
}

pub struct Vector<'a>(pub(crate) &'a Client);

/// Vector search settings. Defaults to ten neighbors using cosine distance.
#[derive(Debug, Clone)]
pub struct VectorSearchOptions {
    pub k: usize,
    pub metric: String,
    pub filter: Option<Value>,
}
impl Default for VectorSearchOptions {
    fn default() -> Self {
        Self {
            k: 10,
            metric: "cosine".into(),
            filter: None,
        }
    }
}
impl Vector<'_> {
    pub async fn put(
        &self,
        table: &str,
        id: &str,
        embedding: &[f64],
        metadata: Option<Map<String, Value>>,
    ) -> Result<Value, Error> {
        if embedding.is_empty() || embedding.iter().any(|v| !v.is_finite()) {
            return Err(Error::Configuration(
                "embedding must contain finite numbers",
            ));
        }
        let mut body = json!({"id": id, "embedding": embedding, "dim": embedding.len()});
        if let Some(metadata) = metadata {
            body["metadata"] = json!(metadata);
        }
        self.0
            .request(
                Method::POST,
                &format!("vector/{}/put", segment(table)?),
                &[],
                Some(body),
                None,
            )
            .await
    }
    pub async fn search(
        &self,
        table: &str,
        query: &[f64],
        options: VectorSearchOptions,
    ) -> Result<Vec<VectorHit>, Error> {
        if query.is_empty() || query.iter().any(|v| !v.is_finite()) || options.k == 0 {
            return Err(Error::Configuration(
                "query must contain finite numbers and k must be positive",
            ));
        }
        let mut body =
            json!({"query": query, "dim": query.len(), "k": options.k, "metric": options.metric});
        if let Some(filter) = options.filter {
            body["filter"] = filter;
        }
        let value = self
            .0
            .request(
                Method::POST,
                &format!("vector/{}/topk", segment(table)?),
                &[],
                Some(body),
                None,
            )
            .await?;
        VectorHit::decode(value)
    }
    pub async fn delete(&self, table: &str, id: &str) -> Result<Value, Error> {
        self.0
            .request(
                Method::DELETE,
                &format!("vector/{}/{}", segment(table)?, segment(id)?),
                &[],
                None,
                None,
            )
            .await
    }
}

pub struct Fts<'a>(pub(crate) &'a Client);

/// Ranked search by default; boolean and phrase modes are also supported.
#[derive(Debug, Clone)]
pub struct FtsSearchOptions {
    pub mode: String,
    pub k: usize,
    pub highlight: bool,
    pub fuzzy: Option<usize>,
    pub facets: Option<Vec<String>>,
}
impl Default for FtsSearchOptions {
    fn default() -> Self {
        Self {
            mode: "bm25".into(),
            k: 10,
            highlight: false,
            fuzzy: None,
            facets: None,
        }
    }
}
impl Fts<'_> {
    pub async fn index(
        &self,
        table: &str,
        field: &str,
        id: &str,
        text: &str,
    ) -> Result<Value, Error> {
        self.0
            .request(
                Method::POST,
                &format!("fts/{}/{}", segment(table)?, segment(field)?),
                &[],
                Some(json!({"doc_id": id, "text": text})),
                None,
            )
            .await
    }
    pub async fn search(
        &self,
        table: &str,
        field: &str,
        query: &str,
        options: FtsSearchOptions,
    ) -> Result<FtsResult, Error> {
        if options.k == 0 || !["bm25", "boolean", "phrase"].contains(&options.mode.as_str()) {
            return Err(Error::Configuration("invalid full-text search options"));
        }
        let mut args = vec![("q", query.into()), ("mode", options.mode.clone())];
        if options.mode == "bm25" {
            args.push(("k", options.k.to_string()));
        }
        if options.highlight {
            args.push(("highlight", "true".into()));
        }
        if let Some(fuzzy) = options.fuzzy {
            args.push(("fuzzy", fuzzy.to_string()));
        }
        if let Some(facets) = options.facets {
            args.push(("facets", facets.join(",")));
        }
        let value = self
            .0
            .request(
                Method::GET,
                &format!("fts/{}/{}", segment(table)?, segment(field)?),
                &args,
                None,
                None,
            )
            .await?;
        FtsResult::decode(value)
    }
}

pub struct Graph<'a>(pub(crate) &'a Client);
impl Graph<'_> {
    pub async fn query(&self, cypher: &str, params: Map<String, Value>) -> Result<Value, Error> {
        self.0
            .request(
                Method::POST,
                "cypher",
                &[],
                Some(json!({"cypher": cypher, "params": params})),
                None,
            )
            .await
    }
    pub async fn neighbors(&self, schema: &str, key: &str, relation: &str) -> Result<Value, Error> {
        self.0
            .request(
                Method::GET,
                &format!("graph/{}/neighbors", segment(schema)?),
                &[("pk", key.into()), ("rel", relation.into())],
                None,
                None,
            )
            .await
    }
    pub async fn bfs(
        &self,
        schema: &str,
        key: &str,
        relation: &str,
        max_depth: usize,
    ) -> Result<Value, Error> {
        self.0
            .request(
                Method::GET,
                &format!("graph/{}/bfs", segment(schema)?),
                &[
                    ("pk", key.into()),
                    ("rel", relation.into()),
                    ("max_depth", max_depth.to_string()),
                ],
                None,
                None,
            )
            .await
    }
}
