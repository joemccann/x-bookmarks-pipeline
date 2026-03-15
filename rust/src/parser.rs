use serde_json::Value;

pub fn sanitize_path(value: &str) -> String {
    let lowered = value.to_lowercase();
    let trimmed = lowered.trim();
    let with_spaces = trimmed.replace(' ', "_");
    let sanitized: String = with_spaces
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-')
        .collect();
    if sanitized.is_empty() {
        "unknown".to_string()
    } else {
        sanitized
    }
}

pub fn parse_chart_json(raw: Option<&str>) -> Option<Value> {
    let text = raw?.trim();
    if text.is_empty() {
        return None;
    }

    let cleaned = if text.starts_with("```") {
        let mut lines: Vec<&str> = text.lines().collect();
        if !lines.is_empty() {
            lines.remove(0);
        }
        if let Some(last) = lines.last().cloned() {
            if last.trim() == "```" {
                lines.pop();
            }
        }
        lines
            .into_iter()
            .filter(|line| !line.trim().starts_with("```"))
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string()
    } else {
        text.to_string()
    };

    serde_json::from_str(&cleaned).ok()
}
