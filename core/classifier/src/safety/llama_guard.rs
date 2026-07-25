use crate::safety::{ParseError, SafetyCategory, SafetyLevel, SafetyParser, SafetyVerdict};

pub struct LlamaGuardParser;

impl LlamaGuardParser {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LlamaGuardParser {
    fn default() -> Self {
        Self::new()
    }
}

fn map_s_code(code: &str) -> Result<SafetyCategory, ParseError> {
    match code.trim().to_uppercase().as_str() {
        "S1" => Ok(SafetyCategory::Violence),
        "S2" => Ok(SafetyCategory::SexualContent),
        "S3" => Ok(SafetyCategory::IllegalActivity),
        "S4" => Ok(SafetyCategory::Profanity),
        "S5" => Ok(SafetyCategory::SelfHarm),
        "S6" => Ok(SafetyCategory::Pii),
        "S7" => Ok(SafetyCategory::MinorSafety),
        _ => Err(ParseError::UnknownCategory(code.to_string())),
    }
}

impl SafetyParser for LlamaGuardParser {
    fn parse(&self, raw_output: &str, model_id: &str) -> Result<SafetyVerdict, ParseError> {
        let raw = raw_output.trim();
        let mut lines = raw.lines();

        let first = lines
            .next()
            .ok_or_else(|| ParseError::ParseError("empty output".into()))?;

        let verdict = match first.trim().to_lowercase().as_str() {
            "safe" => SafetyLevel::Safe,
            "unsafe" => SafetyLevel::Unsafe,
            other => return Err(ParseError::UnknownVerdict(other.to_string())),
        };

        let mut categories = Vec::new();
        if let Some(second) = lines.next() {
            let codes = second.trim();
            if !codes.is_empty() {
                for code in codes.split(',') {
                    let code = code.trim();
                    if !code.is_empty() {
                        categories.push(map_s_code(code)?);
                    }
                }
            }
        }

        Ok(SafetyVerdict {
            verdict,
            categories,
            explanation: None,
            model_id: model_id.to_string(),
            raw_output: raw.to_string(),
        })
    }

    fn name(&self) -> &str {
        "llama_guard"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_safe() {
        let parser = LlamaGuardParser::new();
        let v = parser.parse("safe", "llama-guard-3-8b").unwrap();
        assert_eq!(v.verdict, SafetyLevel::Safe);
        assert!(v.categories.is_empty());
    }

    #[test]
    fn parse_unsafe_with_s_codes() {
        let parser = LlamaGuardParser::new();
        let output = "unsafe\nS1,S3,S6";
        let v = parser.parse(output, "llama-guard-3-8b").unwrap();
        assert_eq!(v.verdict, SafetyLevel::Unsafe);
        assert!(v.categories.contains(&SafetyCategory::Violence));
        assert!(v.categories.contains(&SafetyCategory::IllegalActivity));
        assert!(v.categories.contains(&SafetyCategory::Pii));
    }

    #[test]
    fn parse_unsafe_single_category() {
        let parser = LlamaGuardParser::new();
        let output = "unsafe\nS5";
        let v = parser.parse(output, "llama-guard-3-8b").unwrap();
        assert_eq!(v.verdict, SafetyLevel::Unsafe);
        assert!(v.categories.contains(&SafetyCategory::SelfHarm));
        assert_eq!(v.categories.len(), 1);
    }

    #[test]
    fn parse_unknown_s_code() {
        let parser = LlamaGuardParser::new();
        let output = "unsafe\nS99";
        let result = parser.parse(output, "llama-guard-3-8b");
        assert!(result.is_err());
    }

    #[test]
    fn parse_unknown_verdict() {
        let parser = LlamaGuardParser::new();
        let result = parser.parse("maybe", "llama-guard-3-8b");
        assert!(result.is_err());
    }

    #[test]
    fn parser_name() {
        assert_eq!(LlamaGuardParser::new().name(), "llama_guard");
    }
}
