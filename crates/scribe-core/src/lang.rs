//! The UI language, shared by the app and the prompt and style texts.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    En,
    Fr,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_lowercase() {
        assert_eq!(serde_json::to_value(Language::Fr).unwrap(), serde_json::json!("fr"));
        assert_eq!(serde_json::from_str::<Language>("\"en\"").unwrap(), Language::En);
    }
}
