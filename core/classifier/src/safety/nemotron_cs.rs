use crate::safety::{ParseError, SafetyCategory, SafetyLevel, SafetyParser, SafetyVerdict};

pub struct NemotronContentSafetyParser;

impl NemotronContentSafetyParser {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NemotronContentSafetyParser {
    fn default() -> Self {
        Self::new()
    }
}

fn map_category(cat: &str) -> SafetyCategory {
    let cat = cat.trim();
    match cat.to_lowercase().as_str() {
        "criminal planning/confessions" => SafetyCategory::IllegalActivity,
        "pii/privacy" => SafetyCategory::Pii,
        "violence" => SafetyCategory::Violence,
        "sexual content" => SafetyCategory::SexualContent,
        "self-harm" => SafetyCategory::SelfHarm,
        "hate speech" => SafetyCategory::Custom("HateSpeech".to_string()),
        "malware/cybersecurity" => SafetyCategory::Injection,
        "fraud/deception" => SafetyCategory::Financial,
        "prompt injection" => SafetyCategory::Injection,
        _ => SafetyCategory::Custom(cat.to_string()),
    }
}

fn unknown_verdict(raw: &str, model_id: &str) -> SafetyVerdict {
    SafetyVerdict {
        verdict: SafetyLevel::Unknown,
        categories: Vec::new(),
        explanation: None,
        model_id: model_id.to_string(),
        raw_output: raw.to_string(),
    }
}

impl SafetyParser for NemotronContentSafetyParser {
    fn parse(&self, raw_output: &str, model_id: &str) -> Result<SafetyVerdict, ParseError> {
        let raw = raw_output.trim();
        if raw.is_empty() {
            return Ok(unknown_verdict(raw, model_id));
        }

        let mut verdict = None;
        let mut categories = Vec::new();

        for line in raw.lines() {
            let line = line.trim();
            if let Some(value) = line.strip_prefix("User Safety:") {
                verdict = match value.trim().to_lowercase().as_str() {
                    "safe" => Some(SafetyLevel::Safe),
                    "unsafe" => Some(SafetyLevel::Unsafe),
                    _ => None,
                };
            } else if let Some(value) = line.strip_prefix("Safety Categories:") {
                for cat in value.split(',') {
                    let cat = cat.trim();
                    if !cat.is_empty() {
                        categories.push(map_category(cat));
                    }
                }
            }
        }

        let verdict = match verdict {
            Some(v) => v,
            None => return Ok(unknown_verdict(raw, model_id)),
        };

        Ok(SafetyVerdict {
            verdict,
            categories,
            explanation: None,
            model_id: model_id.to_string(),
            raw_output: raw.to_string(),
        })
    }

    fn name(&self) -> &str {
        "nemotron_cs"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_safe() {
        let parser = NemotronContentSafetyParser::new();
        let v = parser
            .parse("User Safety: safe", "nemotron-3.5-content-safety")
            .unwrap();
        assert_eq!(v.verdict, SafetyLevel::Safe);
        assert!(v.categories.is_empty());
    }

    #[test]
    fn parse_unsafe_single_category() {
        let parser = NemotronContentSafetyParser::new();
        let output = "User Safety: unsafe\nSafety Categories: Violence";
        let v = parser.parse(output, "nemotron-3.5-content-safety").unwrap();
        assert_eq!(v.verdict, SafetyLevel::Unsafe);
        assert_eq!(v.categories, vec![SafetyCategory::Violence]);
    }

    #[test]
    fn parse_unsafe_multiple_categories() {
        let parser = NemotronContentSafetyParser::new();
        let output =
            "User Safety: unsafe\nSafety Categories: Criminal Planning/Confessions, PII/Privacy";
        let v = parser.parse(output, "nemotron-3.5-content-safety").unwrap();
        assert_eq!(v.verdict, SafetyLevel::Unsafe);
        assert!(v.categories.contains(&SafetyCategory::IllegalActivity));
        assert!(v.categories.contains(&SafetyCategory::Pii));
        assert_eq!(v.categories.len(), 2);
    }

    #[test]
    fn parse_category_mappings() {
        let parser = NemotronContentSafetyParser::new();
        let output = "User Safety: unsafe\nSafety Categories: Sexual Content, Self-Harm, Hate Speech, Malware/Cybersecurity, Fraud/Deception, Prompt Injection";
        let v = parser.parse(output, "nemotron-3.5-content-safety").unwrap();
        assert!(v.categories.contains(&SafetyCategory::SexualContent));
        assert!(v.categories.contains(&SafetyCategory::SelfHarm));
        assert!(v
            .categories
            .contains(&SafetyCategory::Custom("HateSpeech".to_string())));
        assert!(v.categories.contains(&SafetyCategory::Injection));
        assert!(v.categories.contains(&SafetyCategory::Financial));
    }

    #[test]
    fn parse_unknown_category_falls_back_to_custom() {
        let parser = NemotronContentSafetyParser::new();
        let output = "User Safety: unsafe\nSafety Categories: Something New";
        let v = parser.parse(output, "nemotron-3.5-content-safety").unwrap();
        assert_eq!(v.verdict, SafetyLevel::Unsafe);
        assert_eq!(
            v.categories,
            vec![SafetyCategory::Custom("Something New".to_string())]
        );
    }

    #[test]
    fn parse_malformed_output_returns_unknown() {
        let parser = NemotronContentSafetyParser::new();
        let v = parser
            .parse("garbage output", "nemotron-3.5-content-safety")
            .unwrap();
        assert_eq!(v.verdict, SafetyLevel::Unknown);
        assert!(v.categories.is_empty());
    }

    #[test]
    fn parse_empty_input_returns_unknown() {
        let parser = NemotronContentSafetyParser::new();
        let v = parser.parse("", "nemotron-3.5-content-safety").unwrap();
        assert_eq!(v.verdict, SafetyLevel::Unknown);
        assert!(v.categories.is_empty());
    }

    #[test]
    fn parser_name() {
        assert_eq!(NemotronContentSafetyParser::new().name(), "nemotron_cs");
    }
}
