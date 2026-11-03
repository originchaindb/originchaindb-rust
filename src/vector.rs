use crate::Error;
use serde_json::{Map, Value};

/// A vector match. Both server ID spellings normalize to id.
#[derive(Debug, Clone)]
pub struct VectorHit {
    pub id: String,
    pub score: f64,
    pub metadata: Option<Map<String, Value>>,
    pub raw: Value,
}
impl VectorHit {
    pub(crate) fn decode(value: Value) -> Result<Vec<Self>, Error> {
        value
            .as_array()
            .ok_or(Error::Protocol("vector hits must be an array"))?
            .iter()
            .map(|item| {
                let id = item
                    .get("id")
                    .or_else(|| item.get("vec_id"))
                    .and_then(Value::as_str)
                    .ok_or(Error::Protocol("invalid vector ID"))?;
                let score = item
                    .get("score")
                    .and_then(Value::as_f64)
                    .filter(|n| n.is_finite())
                    .ok_or(Error::Protocol("invalid vector score"))?;
                Ok(Self {
                    id: id.into(),
                    score,
                    metadata: item.get("metadata").and_then(Value::as_object).cloned(),
                    raw: item.clone(),
                })
            })
            .collect()
    }
}
