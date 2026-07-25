use crate::safety::{ParseError, SafetyCategory, SafetyLevel, SafetyParser, SafetyVerdict};

pub struct ShieldGemmaParser;

impl ShieldGemmaParser {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ShieldGemmaParser {
    fn default() -> Self {
        Self::new()
    }
}

fn map_category(key: &str) -> Option<SafetyCategory> {
    match key.trim().to_lowercase().as_str() {
        "harassment" => Some(SafetyCategory::Violence),
        "hate_speech" | "hate speech" | "hatespeech" => Some(SafetyCategory::Profanity),
        "sexually_explicit" | "sexually explicit" | "sexual" => Some(SafetyCategory::SexualContent),
        "dangerous_content" | "dangerous content" | "dangerous" => {
            Some(SafetyCategory::IllegalActivity)
        }
        _ => None,
    }
}

impl SafetyParser for ShieldGemmaParser {
    fn parse(&self, raw_output: &str, model_id: &str) -> Result<SafetyVerdict, ParseError> {
        let raw = raw_output.trim();

        let parsed: serde_json::Value = serde_json::from_str(raw)
            .map_err(|e| ParseError::ParseError(format!("invalid JSON: {e}")))?;

        let obj = parsed
            .as_object()
            .ok_or_else(|| ParseError::ParseError("expected JSON object".into()))?;

        let mut categories = Vec::new();
        let mut is_unsafe = false;

        for (key, value) in obj {
            let verdict_str = match value {
                serde_json::Value::String(s) => s.to_lowercase(),
                serde_json::Value::Bool(b) => {
                    if *b {
                        "yes".to_string()
                    } else {
                        "no".to_string()
                    }
                }
                serde_json::Value::Number(n) => {
                    if n.as_f64().unwrap_or(0.0) > 0.5 {
                        "yes".to_string()
                    } else {
                        "no".to_string()
                    }
                }
                _ => continue,
            };

            if verdict_str == "yes" || verdict_str == "y" {
                is_unsafe = true;
                if let Some(cat) = map_category(key) {
                    categories.push(cat);
                }
            }
        }

        let verdict = if is_unsafe {
            SafetyLevel::Unsafe
        } else {
            SafetyLevel::Safe
        };

        Ok(SafetyVerdict {
            verdict,
            categories,
            explanation: Some(raw.to_string()),
            model_id: model_id.to_string(),
            raw_output: raw.to_string(),
        })
    }

    fn name(&self) -> &str {
        "shield_gemma"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_safe() {
        let parser = ShieldGemmaParser::new();
        let output = r#"{"harassment": "No", "hate_speech": "No", "sexually_explicit": "No", "dangerous_content": "No"}"#;
        let v = parser.parse(output, "shield-gemma-2b").unwrap();
        assert_eq!(v.verdict, SafetyLevel::Safe);
        assert!(v.categories.is_empty());
    }

    #[test]
    fn parse_unsafe_single_category() {
        let parser = ShieldGemmaParser::new();
        let output = r#"{"harassment": "No", "dangerous_content": "Yes"}"#;
        let v = parser.parse(output, "shield-gemma-2b").unwrap();
        assert_eq!(v.verdict, SafetyLevel::Unsafe);
        assert!(v.categories.contains(&SafetyCategory::IllegalActivity));
        assert_eq!(v.categories.len(), 1);
    }

    #[test]
    fn parse_unsafe_multiple_categories() {
        let parser = ShieldGemmaParser::new();
        let output = r#"{"harassment": "Yes", "hate_speech": "Yes", "sexually_explicit": "Yes"}"#;
        let v = parser.parse(output, "shield-gemma-2b").unwrap();
        assert_eq!(v.verdict, SafetyLevel::Unsafe);
        assert!(v.categories.contains(&SafetyCategory::Violence));
        assert!(v.categories.contains(&SafetyCategory::Profanity));
        assert!(v.categories.contains(&SafetyCategory::SexualContent));
    }

    #[test]
    fn parse_boolean_values() {
        let parser = ShieldGemmaParser::new();
        let output = r#"{"harassment": true, "dangerous_content": false}"#;
        let v = parser.parse(output, "shield-gemma-2b").unwrap();
        assert_eq!(v.verdict, SafetyLevel::Unsafe);
        assert!(v.categories.contains(&SafetyCategory::Violence));
    }

    #[test]
    fn parse_invalid_json() {
        let parser = ShieldGemmaParser::new();
        let result = parser.parse("not json", "shield-gemma-2b");
        assert!(result.is_err());
    }

    #[test]
    fn parser_name() {
        assert_eq!(ShieldGemmaParser::new().name(), "shield_gemma");
    }
}
