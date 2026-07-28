use crate::safety::{
    parse_safety_category, ParseError, SafetyCategory, SafetyLevel, SafetyParser, SafetyVerdict,
};

pub struct GraniteGuardianParser;

impl GraniteGuardianParser {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GraniteGuardianParser {
    fn default() -> Self {
        Self::new()
    }
}

fn map_category(cat: &str) -> Option<SafetyCategory> {
    match parse_safety_category(cat) {
        SafetyCategory::Custom(_) => None,
        known => Some(known),
    }
}

fn parse_structured_json(raw: &str) -> Option<(SafetyLevel, Vec<SafetyCategory>)> {
    let parsed: serde_json::Value = serde_json::from_str(raw).ok()?;
    let obj = parsed.as_object()?;

    let verdict_str = obj
        .get("verdict")
        .or_else(|| obj.get("prediction"))
        .or_else(|| obj.get("decision"))?
        .as_str()?;

    let verdict = match verdict_str.to_lowercase().as_str() {
        "safe" => SafetyLevel::Safe,
        "unsafe" | "harmful" => SafetyLevel::Unsafe,
        _ => return None,
    };

    let categories = obj
        .get("categories")
        .or_else(|| obj.get("labels"))
        .or_else(|| obj.get("risk_categories"));

    let categories: Vec<SafetyCategory> = match categories {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().and_then(map_category))
            .collect(),
        Some(serde_json::Value::String(s)) => s
            .split(',')
            .filter_map(|c| map_category(c.trim()))
            .collect(),
        _ => Vec::new(),
    };

    Some((verdict, categories))
}

fn parse_text_output(raw: &str) -> Option<(SafetyLevel, Vec<SafetyCategory>)> {
    let text = raw.trim().to_lowercase();

    if text.contains("safe") && !text.contains("unsafe") {
        return Some((SafetyLevel::Safe, Vec::new()));
    }

    if !text.contains("unsafe") && !text.contains("harmful") {
        return None;
    }

    let categories: Vec<SafetyCategory> = text
        .lines()
        .skip(1)
        .flat_map(|line| line.split(',').flat_map(|c| map_category(c.trim())))
        .collect();

    Some((SafetyLevel::Unsafe, categories))
}

impl SafetyParser for GraniteGuardianParser {
    fn parse(&self, raw_output: &str, model_id: &str) -> Result<SafetyVerdict, ParseError> {
        let raw = raw_output.trim();

        let Some((verdict, categories)) =
            parse_structured_json(raw).or_else(|| parse_text_output(raw))
        else {
            return Ok(SafetyVerdict::unknown(model_id, raw));
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
        "granite_guardian"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_safe_short() {
        let parser = GraniteGuardianParser::new();
        let v = parser.parse("safe", "granite-guardian-3.0").unwrap();
        assert_eq!(v.verdict, SafetyLevel::Safe);
        assert!(v.categories.is_empty());
    }

    #[test]
    fn parse_unsafe_with_categories() {
        let parser = GraniteGuardianParser::new();
        let output = "unsafe\nharm, pii";
        let v = parser.parse(output, "granite-guardian-3.0").unwrap();
        assert_eq!(v.verdict, SafetyLevel::Unsafe);
        assert!(v.categories.contains(&SafetyCategory::Violence));
        assert!(v.categories.contains(&SafetyCategory::Pii));
    }

    #[test]
    fn parse_unsafe_json() {
        let parser = GraniteGuardianParser::new();
        let output = r#"{"verdict": "unsafe", "categories": ["harm", "injection"]}"#;
        let v = parser.parse(output, "granite-guardian-3.0").unwrap();
        assert_eq!(v.verdict, SafetyLevel::Unsafe);
        assert!(v.categories.contains(&SafetyCategory::Violence));
        assert!(v.categories.contains(&SafetyCategory::Injection));
    }

    #[test]
    fn parse_safe_json() {
        let parser = GraniteGuardianParser::new();
        let output = r#"{"verdict": "safe"}"#;
        let v = parser.parse(output, "granite-guardian-3.0").unwrap();
        assert_eq!(v.verdict, SafetyLevel::Safe);
    }

    #[test]
    fn parse_unknown_output_returns_unknown() {
        let parser = GraniteGuardianParser::new();
        let v = parser
            .parse("garbage output", "granite-guardian-3.0")
            .unwrap();
        assert_eq!(v.verdict, SafetyLevel::Unknown);
        assert!(v.categories.is_empty());
    }

    #[test]
    fn parser_name() {
        assert_eq!(GraniteGuardianParser::new().name(), "granite_guardian");
    }
}
