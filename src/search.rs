use crate::Error;
use serde_json::Value;

/// Uniform hit for ranked, boolean and phrase search. Unranked scores are zero.
#[derive(Debug, Clone)]
pub struct FtsHit {
    pub doc_id: String,
    pub score: f64,
    pub highlights: Option<Value>,
    pub raw: Value,
}

#[derive(Debug, Clone)]
pub struct FtsResult {
    pub hits: Vec<FtsHit>,
    pub facets: Option<Value>,
    pub raw: Value,
}
impl FtsResult {
    pub(crate) fn decode(value: Value) -> Result<Self, Error> {
        let items = if value.is_object() {
            value
                .get("hits")
                .ok_or(Error::Protocol("missing FTS hits"))?
        } else {
            &value
        };
        let items = items
            .as_array()
            .ok_or(Error::Protocol("FTS hits must be an array"))?;
        let mut hits = Vec::with_capacity(items.len());
        for item in items {
            if let Some(id) = item.as_str() {
                hits.push(FtsHit {
                    doc_id: id.into(),
                    score: 0.0,
                    highlights: None,
                    raw: item.clone(),
                });
            } else {
                let doc_id = item
                    .get("doc_id")
                    .and_then(Value::as_str)
                    .ok_or(Error::Protocol("invalid FTS hit"))?;
                let score = match item.get("score") {
                    Some(number) => number
                        .as_f64()
                        .filter(|n| n.is_finite())
                        .ok_or(Error::Protocol("invalid FTS score"))?,
                    None => 0.0,
                };
                hits.push(FtsHit {
                    doc_id: doc_id.into(),
                    score,
                    highlights: item.get("highlights").cloned(),
                    raw: item.clone(),
                });
            }
        }
        Ok(Self {
            hits,
            facets: value.get("facets").cloned(),
            raw: value,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn accepts_all_search_modes_and_retains_metadata() {
        for value in [
            json!(["one"]),
            json!([{"doc_id":"one","score":1.0}]),
            json!({"hits":[{"doc_id":"one","score":1.0}],"facets":{"tag":[]}}),
        ] {
            let result = FtsResult::decode(value.clone()).unwrap();
            assert_eq!(result.hits[0].doc_id, "one");
            assert_eq!(result.raw, value);
        }
        for value in [
            json!(null),
            json!({}),
            json!([42]),
            json!([{"doc_id":"one","score":"invalid"}]),
        ] {
            assert!(FtsResult::decode(value).is_err());
        }
    }
}
