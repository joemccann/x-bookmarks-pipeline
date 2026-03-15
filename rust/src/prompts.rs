pub const FINANCE_TEXT_CLASSIFICATION_PROMPT: &str = "Classify tweet. JSON: is_finance(bool),confidence(0-1),category(finance|technology|science|politics|entertainment|sports|health|education|other),subcategory(crypto|equities|forex|commodities|macro|options|AI|machine_learning|web_dev|mobile|cybersecurity|physics|biology|climate|us_politics|world_politics|movies|music|gaming|football|basketball|mma|nutrition|mental_health|fitness|tutorials|research|general),has_trading_pattern(bool),has_visual_data(false),detected_topic,summary. is_finance=true for trading/TA/FA/crypto. has_trading_pattern=true for entry/exit/levels.";

pub const FINANCE_IMAGE_CLASSIFICATION_PROMPT: &str = "Classify image(s). JSON: is_finance(bool),confidence(0-1),category(finance|technology|science|politics|entertainment|sports|health|education|other),subcategory(crypto|equities|forex|commodities|macro|options|AI|machine_learning|web_dev|mobile|cybersecurity|physics|biology|climate|us_politics|world_politics|movies|music|gaming|football|basketball|mma|nutrition|mental_health|fitness|tutorials|research|general),has_trading_pattern(bool),has_visual_data(bool),detected_topic,summary. has_visual_data=true for charts/tables/graphs. is_finance=true for financial charts.";

pub const CLAUDE_PLANNING_SYSTEM_PROMPT: &str = r#"Pine Script v6 plan from tweet. Compact JSON.\
\
"strategy"(entry/exit/SL/TP) or "indicator"(visualize). Fields: script_type,title,ticker(BTCUSDT),direction,timeframe(D),indicators(2+),indicator_params,entry_conditions,exit_conditions,risk_management{sl_type,sl_value,tp_type,tp_value,size_pct}. Indicators: omit entry/exit+risk. Use numbers from tweet."#;

pub const GROK_PINESCRIPT_SYSTEM_PROMPT: &str = r#"```pinescript only. No comments except //Source:@author //Date:date after //@version=6.\
strategy(overlay=true,percent_of_equity,10) or indicator(overlay=true) per type.\
input.*() group=. var/varip. SL+TP inputs, strategy.entry()+exit(). Indicators: no strategy.*.\
plotshape/plot. barstate.isconfirmed."#;

pub const VISION_ANALYSIS_PROMPT: &str = r#"ONLY compact JSON (no fences). Fields: image_type,description,asset{ticker,name},chart_analysis{timeframe,chart_type,trend_direction,patterns[]},price_levels{current,support[],resistance[],all_visible[]},indicators[{name,value,signal}],annotations[],tabular_data{headers[],rows[]},statistics{key_values{}}. Extract ALL visible numbers. Omit inapplicable fields."#;

pub const CATEGORIZE_OUTPUT_SYSTEM: &str = r#"Return JSON only. Schema: {\"category\": string, \"confidence\": float, \"rationale\": string}."#;

pub const ANALYZE_OUTPUT_SYSTEM: &str = r#"Return JSON only. Schema: {\"signal\": string, \"summary\": string, \"indicators\": [string], \"confidence\": float}."#;

pub const CODEGEN_OUTPUT_SYSTEM: &str = r#"Produce valid Pine Script v6. Return only JSON with fields: pine_script string, confidence float, notes array[string]. pine_script must start with //@version=6 and contain strategy() or indicator()."#;

pub const GENERIC_ERROR_PLACEHOLDER: &str = "Unable to parse LLM JSON";
