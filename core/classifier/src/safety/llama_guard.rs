use crate::safety::{
    parse_safety_category, ParseError, SafetyCategory, SafetyLevel, SafetyParser, SafetyVerdict,
};

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

fn map_s_code(code: &str) -> Option<SafetyCategory> {
    match parse_safety_category(code) {
        SafetyCategory::Custom(_) => None,
        known => Some(known),
    }
}

impl SafetyParser for LlamaGuardParser {
    fn parse(&self, raw_output: &str, model_id: &str) -> Result<SafetyVerdict, ParseError> {
        let raw = raw_output.trim();
        let mut lines = raw.lines();

        let Some(first) = lines.next() else {
            return Ok(SafetyVerdict::unknown(model_id, raw));
        };

        let verdict = match first.trim().to_lowercase().as_str() {
            "safe" => SafetyLevel::Safe,
            "unsafe" => SafetyLevel::Unsafe,
            _ => return Ok(SafetyVerdict::unknown(model_id, raw)),
        };

        let mut categories = Vec::new();
        if let Some(second) = lines.next() {
            let codes = second.trim();
            if !codes.is_empty() {
                for code in codes.split(',') {
                    let code = code.trim();
                    if !code.is_empty() {
                        match map_s_code(code) {
                            Some(cat) => categories.push(cat),
                            None => return Ok(SafetyVerdict::unknown(model_id, raw)),
                        }
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
    fn parse_unknown_s_code_returns_unknown() {
        let parser = LlamaGuardParser::new();
        let output = "unsafe\nS99";
        let v = parser.parse(output, "llama-guard-3-8b").unwrap();
        assert_eq!(v.verdict, SafetyLevel::Unknown);
        assert!(v.categories.is_empty());
    }

    #[test]
    fn parse_unknown_verdict_returns_unknown() {
        let parser = LlamaGuardParser::new();
        let v = parser.parse("maybe", "llama-guard-3-8b").unwrap();
        assert_eq!(v.verdict, SafetyLevel::Unknown);
    }

    #[test]
    fn parse_empty_output_returns_unknown() {
        let parser = LlamaGuardParser::new();
        let v = parser.parse("", "llama-guard-3-8b").unwrap();
        assert_eq!(v.verdict, SafetyLevel::Unknown);
    }

    #[test]
    fn parser_name() {
        assert_eq!(LlamaGuardParser::new().name(), "llama_guard");
    }
}
