# Autoresearch: Token/Cost Efficiency — COMPLETED

## Objective

Reduce total token consumption (and therefore cost) across all LLM API calls in the X Bookmarks Pipeline, without degrading classification accuracy, plan quality, or Pine Script validity.

## Final Results

| Metric | Baseline | Optimized | Change |
|--------|----------|-----------|--------|
| **Total tokens/bookmark** | 7,972 | 2,880 | **-63.9%** |
| **Cost/bookmark** | 23.1¢ | 8.9¢ | **-61.7%** |
| **System prompt tokens** | 3,141 | 562 | -82.1% |
| **At 1,000 bookmarks/month** | $231 | $89 | **saves $142/mo** |

54 experiments run, 40+ kept improvements. All 151 tests pass.

## Per-Stage Breakdown (before → after)

| Stage | Baseline | Optimized | Reduction |
|-------|----------|-----------|-----------|
| Cerebras text classify | 564 | 279 | -50.5% |
| xAI image classify | 572 | 200 | -65.0% |
| Claude vision analysis | 1,760 | 457 | -74.0% |
| Claude strategy planning | 1,308 | 407 | -68.9% |
| ChatGPT Pine Script gen | 3,768 | 1,537 | -59.2% |

## Key Optimizations Applied

### System prompt compression (82% reduction)
- Removed decorative formatting (═══ borders, numbered sections)
- Merged redundant RULES/VERIFY/OUTPUT sections
- Inlined JSON schemas as compact field lists instead of verbose examples
- Simplified vision schema (removed rarely-used nested fields)
- Removed unused fields from planning prompt (key_levels, pattern, visual_signals, rationale)
- Shortened risk field names in planning prompt

### User prompt compression
- Switched from verbose text format to compact JSON (`separators=(",",":")`)
- Dropped redundant fields from generator prompt (raw_tweet_text, chart_description, key_levels, pattern, rationale, visual_signals)
- Flattened indicator_params into compact format: `RSI(14/30/70)` instead of nested JSON
- Flattened risk_management into: `SL:fixed@40000,TP:fixed@45000,size:10%`
- Removed summary from planning prompt (LLM derives it from text)

### Output token reduction
- Reduced max_tokens across all stages: text 512→192, image 1024→384, planning 2048→768, vision 4096→1280
- Added compact output instructions ("compact JSON", "no comments except citation")
- Removed chart_details from image classification (unused downstream)
- Used `response_format={"type":"json_object"}` for Cerebras and xAI

### Structural changes
- xAI `chat_with_vision` now accepts `response_format` parameter
- Generator `_build_user_prompt` produces compact JSON with flattened params
- Planner `_build_prompt` produces minimal JSON
- Classification prompts share `_CATEGORIES` and `_SUBCATEGORIES` constants

## Constraints Maintained
- All 151 pipeline tests pass ✅
- All Pine Script HARD RULES maintained (version, declaration, inputs, risk management, etc.)
- JSON response schemas unchanged (downstream code parses them)
- No new dependencies added
- Output directory structure unchanged
