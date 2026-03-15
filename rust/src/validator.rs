use crate::models::ValidationResult;

#[derive(Clone)]
pub struct PineScriptValidator;

impl PineScriptValidator {
    pub fn new() -> Self {
        Self
    }

    pub fn validate(&self, code: &str, script_type: &str) -> ValidationResult {
        let mut result = ValidationResult::new();

        self.check_version(code, &mut result);
        self.check_declaration(code, &mut result, script_type);
        self.check_inputs(code, &mut result);

        if script_type == "strategy" {
            self.check_risk_management(code, &mut result);
        }

        self.check_visual_signals(code, &mut result);
        self.check_citation_header(code, &mut result, script_type);
        self.check_repaint_guard(code, &mut result);

        result
    }

    fn check_version(&self, code: &str, result: &mut ValidationResult) {
        if !regex_match(r"(?m)^\s*//\s*@version\s*=\s*6", code) {
            result.fail("Missing or incorrect version pragma. Must start with //@version=6");
        }
    }

    fn check_declaration(&self, code: &str, result: &mut ValidationResult, script_type: &str) {
        if script_type == "indicator" {
            if !code.contains("indicator(") {
                result.fail("No indicator() declaration found.");
            }
        } else if !code.contains("strategy(") {
            result.fail("No strategy() declaration found.");
        }
    }

    fn check_inputs(&self, code: &str, result: &mut ValidationResult) {
        if !code.contains("input.") && !code.contains("input(") {
            result.warn("No input.*() calls found. Parameters should be user-tunable.");
        }
    }

    fn check_risk_management(&self, code: &str, result: &mut ValidationResult) {
        let has_sl = regex_match(r"(?i)stop_loss|stop\\.loss|sl_pct|sl_atr", code);
        let has_tp = regex_match(r"(?i)take_profit|take\\.profit|tp_pct|tp_atr", code);
        let has_exit = code.contains("strategy.exit(");

        if !has_sl {
            result.warn("No stop-loss parameter detected.");
        }
        if !has_tp {
            result.warn("No take-profit parameter detected.");
        }
        if !has_exit {
            result.fail("No strategy.exit() call found. Risk management exits are required.");
        }
    }

    fn check_visual_signals(&self, code: &str, result: &mut ValidationResult) {
        if !code.contains("plotshape") && !code.contains("plotchar") && !code.contains("plot(") {
            result.warn("No plotshape(), plotchar(), or plot() found. Visual signals are recommended.");
        }
    }

    fn check_citation_header(&self, code: &str, result: &mut ValidationResult, script_type: &str) {
        let decl = if script_type == "indicator" { "indicator(" } else { "strategy(" };
        let header = code.splitn(2, decl).next().unwrap_or("");
        let filtered: String = header
            .lines()
            .filter(|line| !line.trim().starts_with("//@version"))
            .collect::<Vec<_>>()
            .join("\n");

        if !filtered.contains("Source") && !filtered.contains('@') {
            result.warn("Citation header with tweet author not detected.");
        }
    }

    fn check_repaint_guard(&self, code: &str, result: &mut ValidationResult) {
        if code.contains("request.security(")
            && !code.contains("lookahead")
        {
            result.warn(
                "security()/request.security() used without explicit lookahead parameter. This may cause repainting.",
            );
        }
    }
}

fn regex_match(pattern: &str, haystack: &str) -> bool {
    use regex::Regex;
    Regex::new(pattern)
        .map(|re| re.is_match(haystack))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strategy_sample() -> &'static str {
        "//@version=6\nstrategy(\"Demo\", overlay=true)\nplot(1)\nentry = input.float(1.0, \"Entry\")\nstrategy.exit(\"Exit\", \"L\", stop=1, limit=2)\nif barstate.isconfirmed\n    strategy.entry(\"long\", strategy.long)"
    }

    #[test]
    fn validate_strategy() {
        let v = PineScriptValidator::new().validate(strategy_sample(), "strategy");
        assert!(v.valid);
    }

    #[test]
    fn reject_missing_version() {
        let code = "strategy(\"Demo\")\n";
        let result = PineScriptValidator::new().validate(code, "strategy");
        assert!(!result.valid);
    }
}
