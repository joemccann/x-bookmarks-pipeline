"""
Cerebras vs xAI Grok — Classification Evaluation

Tests both providers on the same tweet samples using the project's actual
classification prompts, then generates an HTML report with side-by-side results.
"""
from __future__ import annotations

import json
import os
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path

import httpx

# ---------------------------------------------------------------------------
# Ensure project root is on sys.path so we can import prompts
# ---------------------------------------------------------------------------
PROJECT_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(PROJECT_ROOT))

from dotenv import load_dotenv

load_dotenv(PROJECT_ROOT / ".env")

from src.prompts.classification_prompts import FINANCE_TEXT_CLASSIFICATION_PROMPT

# ---------------------------------------------------------------------------
# Test tweets with expected classifications
# ---------------------------------------------------------------------------
TEST_TWEETS = [
    {
        "text": "BTC breakout above $42k, RSI oversold on 4h. Target $45k, SL $40k",
        "expected_finance": True,
        "expected_category": "finance",
        "expected_subcategory": "crypto",
        "label": "Crypto trading signal",
    },
    {
        "text": "AAPL earnings beat expectations, stock up 5% after hours",
        "expected_finance": True,
        "expected_category": "finance",
        "expected_subcategory": "equities",
        "label": "Equities / earnings",
    },
    {
        "text": "Just deployed our new ML model to production, 3x faster inference",
        "expected_finance": False,
        "expected_category": "technology",
        "expected_subcategory": "AI",
        "label": "Technology / AI",
    },
    {
        "text": "Best pasta carbonara recipe! Secret is using guanciale not bacon",
        "expected_finance": False,
        "expected_category": "other",
        "expected_subcategory": "general",
        "label": "Food / general",
    },
    {
        "text": "New study shows ocean temperatures rising faster than predicted",
        "expected_finance": False,
        "expected_category": "science",
        "expected_subcategory": "climate",
        "label": "Science / climate",
    },
    {
        "text": "Lakers beat Celtics 112-108 in overtime thriller",
        "expected_finance": False,
        "expected_category": "sports",
        "expected_subcategory": "basketball",
        "label": "Sports / basketball",
    },
]


# ---------------------------------------------------------------------------
# Result container
# ---------------------------------------------------------------------------
@dataclass
class ClassifyResult:
    provider: str
    model: str
    tweet_label: str
    tweet_text: str
    expected_finance: bool
    expected_category: str
    expected_subcategory: str
    actual_finance: bool | None = None
    actual_category: str = ""
    actual_subcategory: str = ""
    confidence: float = 0.0
    summary: str = ""
    response_time_ms: float = 0.0
    json_parsed: bool = False
    error: str = ""
    raw_response: str = ""


# ---------------------------------------------------------------------------
# Generic OpenAI-compatible chat call
# ---------------------------------------------------------------------------
def classify_tweet(
    base_url: str,
    api_key: str,
    model: str,
    tweet_text: str,
    timeout: float = 60.0,
) -> tuple[dict, float, str]:
    """
    Send the classification prompt + tweet to a chat-completions endpoint.
    Returns (parsed_dict, elapsed_ms, raw_content).
    """
    payload = {
        "model": model,
        "messages": [
            {"role": "system", "content": FINANCE_TEXT_CLASSIFICATION_PROMPT},
            {"role": "user", "content": tweet_text},
        ],
        "temperature": 0.2,
        "max_tokens": 512,
    }
    headers = {
        "Authorization": f"Bearer {api_key}",
        "Content-Type": "application/json",
    }

    t0 = time.perf_counter()
    with httpx.Client(timeout=timeout) as client:
        resp = client.post(
            f"{base_url}/chat/completions",
            headers=headers,
            json=payload,
        )
        resp.raise_for_status()
    elapsed_ms = (time.perf_counter() - t0) * 1000

    data = resp.json()
    raw = data["choices"][0]["message"]["content"]

    # Parse JSON (handle markdown fences)
    cleaned = raw.strip()
    if cleaned.startswith("```"):
        lines = cleaned.split("\n")
        lines = [l for l in lines if not l.strip().startswith("```")]
        cleaned = "\n".join(lines)
    parsed = json.loads(cleaned)
    return parsed, elapsed_ms, raw


# ---------------------------------------------------------------------------
# Run all tests for one provider/model
# ---------------------------------------------------------------------------
def run_eval(
    provider_name: str,
    base_url: str,
    api_key: str,
    model: str,
) -> list[ClassifyResult]:
    results: list[ClassifyResult] = []
    for tweet in TEST_TWEETS:
        r = ClassifyResult(
            provider=provider_name,
            model=model,
            tweet_label=tweet["label"],
            tweet_text=tweet["text"],
            expected_finance=tweet["expected_finance"],
            expected_category=tweet["expected_category"],
            expected_subcategory=tweet["expected_subcategory"],
        )
        try:
            parsed, elapsed, raw = classify_tweet(base_url, api_key, model, tweet["text"])
            r.json_parsed = True
            r.response_time_ms = elapsed
            r.raw_response = raw
            r.actual_finance = parsed.get("is_finance", None)
            r.actual_category = parsed.get("category", "")
            r.actual_subcategory = parsed.get("subcategory", "")
            r.confidence = parsed.get("confidence", 0.0)
            r.summary = parsed.get("summary", "")
        except httpx.HTTPStatusError as e:
            r.error = f"HTTP {e.response.status_code}: {e.response.text[:300]}"
            r.raw_response = r.error
        except json.JSONDecodeError as e:
            r.error = f"JSON parse error: {e}"
            r.json_parsed = False
        except Exception as e:
            r.error = str(e)[:300]

        print(f"  [{provider_name}/{model}] {tweet['label']}: "
              f"{'OK' if not r.error else 'ERR'} "
              f"({r.response_time_ms:.0f}ms)")
        results.append(r)

        # Brief pause to avoid rate limits
        time.sleep(0.3)

    return results


# ---------------------------------------------------------------------------
# HTML report generation
# ---------------------------------------------------------------------------
def accuracy_stats(results: list[ClassifyResult]) -> dict:
    total = len(results)
    if total == 0:
        return {"finance_acc": 0, "category_acc": 0, "avg_ms": 0, "json_rate": 0, "total": 0}

    finance_correct = sum(
        1 for r in results
        if r.actual_finance is not None and r.actual_finance == r.expected_finance
    )
    category_correct = sum(
        1 for r in results
        if r.actual_category.lower() == r.expected_category.lower()
    )
    avg_ms = sum(r.response_time_ms for r in results) / total
    json_ok = sum(1 for r in results if r.json_parsed)

    return {
        "finance_acc": finance_correct / total * 100,
        "category_acc": category_correct / total * 100,
        "avg_ms": avg_ms,
        "json_rate": json_ok / total * 100,
        "total": total,
    }


def generate_html(all_results: dict[str, list[ClassifyResult]], output_path: Path) -> None:
    """Generate an HTML report comparing all providers."""

    provider_stats = {}
    for name, results in all_results.items():
        provider_stats[name] = accuracy_stats(results)

    providers = list(all_results.keys())

    # Build per-tweet comparison rows
    tweet_rows = ""
    for i, tweet in enumerate(TEST_TWEETS):
        tweet_rows += f"""
        <tr class="tweet-header">
            <td colspan="{1 + len(providers)}" class="tweet-text">
                <strong>Tweet {i+1}:</strong> {tweet['label']}<br>
                <span class="tweet-content">"{tweet['text']}"</span><br>
                <span class="expected">Expected: is_finance={tweet['expected_finance']},
                category={tweet['expected_category']}, subcategory={tweet['expected_subcategory']}</span>
            </td>
        </tr>
        <tr>
            <td class="metric-label">is_finance</td>"""

        for name in providers:
            r = all_results[name][i]
            match = r.actual_finance == r.expected_finance if r.actual_finance is not None else False
            css = "match" if match else "mismatch"
            val = str(r.actual_finance) if r.actual_finance is not None else "ERROR"
            tweet_rows += f'<td class="{css}">{val}</td>'
        tweet_rows += "</tr>"

        tweet_rows += f"""
        <tr>
            <td class="metric-label">category</td>"""
        for name in providers:
            r = all_results[name][i]
            match = r.actual_category.lower() == r.expected_category.lower()
            css = "match" if match else "mismatch"
            tweet_rows += f'<td class="{css}">{r.actual_category or "N/A"}</td>'
        tweet_rows += "</tr>"

        tweet_rows += f"""
        <tr>
            <td class="metric-label">subcategory</td>"""
        for name in providers:
            r = all_results[name][i]
            match = r.actual_subcategory.lower() == r.expected_subcategory.lower()
            css = "match" if match else "partial"
            tweet_rows += f'<td class="{css}">{r.actual_subcategory or "N/A"}</td>'
        tweet_rows += "</tr>"

        tweet_rows += f"""
        <tr>
            <td class="metric-label">confidence</td>"""
        for name in providers:
            r = all_results[name][i]
            tweet_rows += f'<td>{r.confidence:.2f}</td>'
        tweet_rows += "</tr>"

        tweet_rows += f"""
        <tr>
            <td class="metric-label">response time</td>"""
        for name in providers:
            r = all_results[name][i]
            tweet_rows += f'<td>{r.response_time_ms:.0f} ms</td>'
        tweet_rows += "</tr>"

        tweet_rows += f"""
        <tr>
            <td class="metric-label">summary</td>"""
        for name in providers:
            r = all_results[name][i]
            summary_text = r.summary[:120] if r.summary else (r.error[:120] if r.error else "N/A")
            tweet_rows += f'<td class="summary-cell">{summary_text}</td>'
        tweet_rows += "</tr>"

        tweet_rows += '<tr class="spacer"><td colspan="99"></td></tr>'

    # Provider headers
    provider_headers = ""
    for name in providers:
        model = all_results[name][0].model if all_results[name] else "?"
        provider_headers += f'<th>{name}<br><span class="model-name">{model}</span></th>'

    # Summary table
    summary_rows = ""
    metrics = [
        ("is_finance Accuracy", "finance_acc", "%"),
        ("Category Accuracy", "category_acc", "%"),
        ("Avg Response Time", "avg_ms", " ms"),
        ("JSON Parse Rate", "json_rate", "%"),
    ]
    for label, key, unit in metrics:
        summary_rows += f"<tr><td class='metric-label'>{label}</td>"
        vals = [provider_stats[name][key] for name in providers]
        for j, name in enumerate(providers):
            val = provider_stats[name][key]
            # Highlight best value
            if key == "avg_ms":
                is_best = val == min(vals) and val > 0
            else:
                is_best = val == max(vals)
            css = "best-val" if is_best else ""
            summary_rows += f'<td class="{css}">{val:.1f}{unit}</td>'
        summary_rows += "</tr>"

    # Recommendation logic
    recommendation = _build_recommendation(providers, provider_stats)

    html = f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Cerebras vs xAI Grok — Classification Eval</title>
<style>
  :root {{
    --bg: #0d1117;
    --card: #161b22;
    --border: #30363d;
    --text: #e6edf3;
    --text-dim: #8b949e;
    --green: #3fb950;
    --red: #f85149;
    --blue: #58a6ff;
    --yellow: #d29922;
    --purple: #bc8cff;
  }}
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  body {{
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Helvetica, Arial, sans-serif;
    background: var(--bg);
    color: var(--text);
    padding: 2rem;
    line-height: 1.5;
  }}
  h1 {{
    font-size: 1.8rem;
    margin-bottom: 0.5rem;
    color: var(--blue);
  }}
  h2 {{
    font-size: 1.3rem;
    margin: 2rem 0 1rem;
    color: var(--purple);
    border-bottom: 1px solid var(--border);
    padding-bottom: 0.5rem;
  }}
  .subtitle {{
    color: var(--text-dim);
    margin-bottom: 2rem;
  }}
  table {{
    width: 100%;
    border-collapse: collapse;
    margin: 1rem 0;
    background: var(--card);
    border-radius: 8px;
    overflow: hidden;
  }}
  th, td {{
    padding: 0.6rem 1rem;
    text-align: left;
    border-bottom: 1px solid var(--border);
    font-size: 0.9rem;
  }}
  th {{
    background: #1c2128;
    font-weight: 600;
    color: var(--blue);
  }}
  .model-name {{
    font-size: 0.75rem;
    color: var(--text-dim);
    font-weight: normal;
  }}
  .metric-label {{
    font-weight: 600;
    color: var(--text-dim);
    min-width: 130px;
  }}
  .tweet-header td {{
    background: #1c2128;
    border-top: 2px solid var(--border);
    padding-top: 1rem;
  }}
  .tweet-text {{ color: var(--text); }}
  .tweet-content {{
    font-style: italic;
    color: var(--text-dim);
    font-size: 0.85rem;
  }}
  .expected {{
    color: var(--yellow);
    font-size: 0.8rem;
  }}
  .match {{
    color: var(--green);
    font-weight: 600;
  }}
  .mismatch {{
    color: var(--red);
    font-weight: 600;
  }}
  .partial {{
    color: var(--yellow);
  }}
  .best-val {{
    color: var(--green);
    font-weight: 700;
  }}
  .summary-cell {{
    font-size: 0.8rem;
    color: var(--text-dim);
    max-width: 300px;
  }}
  .spacer td {{
    height: 0.5rem;
    border: none;
    background: var(--bg);
  }}
  .recommendation {{
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 1.5rem;
    margin: 2rem 0;
  }}
  .recommendation h3 {{
    color: var(--blue);
    margin-bottom: 0.8rem;
  }}
  .recommendation p {{
    margin-bottom: 0.5rem;
  }}
  .rec-verdict {{
    font-size: 1.1rem;
    font-weight: 700;
    padding: 0.5rem 0;
  }}
  .timestamp {{
    color: var(--text-dim);
    font-size: 0.8rem;
    margin-top: 2rem;
    text-align: center;
  }}
</style>
</head>
<body>

<h1>Cerebras vs xAI Grok &mdash; Tweet Classification Eval</h1>
<p class="subtitle">Side-by-side comparison using the same classification prompts from the x-bookmarks-pipeline project.</p>

<h2>Summary</h2>
<table>
  <tr>
    <th>Metric</th>
    {provider_headers}
  </tr>
  {summary_rows}
</table>

<h2>Per-Tweet Results</h2>
<table>
  <tr>
    <th>Metric</th>
    {provider_headers}
  </tr>
  {tweet_rows}
</table>

<div class="recommendation">
  {recommendation}
</div>

<p class="timestamp">Generated {time.strftime('%Y-%m-%d %H:%M:%S')}</p>

</body>
</html>"""

    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(html)
    print(f"\nReport written to {output_path}")


def _build_recommendation(providers: list[str], stats: dict) -> str:
    """Build a recommendation section based on accuracy and speed stats."""
    sections = []

    # Find the best provider for each metric
    best_accuracy_provider = max(providers, key=lambda p: stats[p]["finance_acc"])
    best_speed_provider = min(
        [p for p in providers if stats[p]["avg_ms"] > 0],
        key=lambda p: stats[p]["avg_ms"],
        default=providers[0],
    )

    sections.append("<h3>Recommendation</h3>")

    # Check if any Cerebras model matches xAI accuracy
    xai_acc = stats.get("xAI Grok", {}).get("finance_acc", 0)
    cerebras_models = [p for p in providers if p.startswith("Cerebras")]

    all_cerebras_accurate = all(
        stats[p]["finance_acc"] >= xai_acc for p in cerebras_models
    )
    any_cerebras_accurate = any(
        stats[p]["finance_acc"] >= xai_acc for p in cerebras_models
    )
    any_cerebras_faster = any(
        stats[p]["avg_ms"] < stats.get("xAI Grok", {}).get("avg_ms", float("inf"))
        for p in cerebras_models
    )

    if any_cerebras_accurate and any_cerebras_faster:
        accurate_and_fast = [
            p for p in cerebras_models
            if stats[p]["finance_acc"] >= xai_acc
            and stats[p]["avg_ms"] < stats.get("xAI Grok", {}).get("avg_ms", float("inf"))
        ]
        if accurate_and_fast:
            winner = accurate_and_fast[0]
            speedup = stats["xAI Grok"]["avg_ms"] / stats[winner]["avg_ms"] if stats[winner]["avg_ms"] > 0 else 0
            sections.append(
                f'<p class="rec-verdict" style="color: var(--green);">'
                f'YES &mdash; Switch to {winner}.</p>'
                f'<p>Matches xAI Grok accuracy ({stats[winner]["finance_acc"]:.0f}% vs {xai_acc:.0f}%) '
                f'while being {speedup:.1f}x faster ({stats[winner]["avg_ms"]:.0f}ms vs '
                f'{stats["xAI Grok"]["avg_ms"]:.0f}ms avg).</p>'
            )
        else:
            sections.append(
                '<p class="rec-verdict" style="color: var(--yellow);">CONDITIONAL &mdash; '
                'Cerebras offers speed but with accuracy trade-offs.</p>'
            )
    elif any_cerebras_accurate and not any_cerebras_faster:
        sections.append(
            '<p class="rec-verdict" style="color: var(--yellow);">NO SPEED ADVANTAGE &mdash; '
            'Cerebras matches accuracy but is not faster.</p>'
        )
    elif not any_cerebras_accurate and any_cerebras_faster:
        sections.append(
            '<p class="rec-verdict" style="color: var(--red);">NOT RECOMMENDED &mdash; '
            'Cerebras is faster but less accurate on finance classification.</p>'
        )

        # Detail which missed
        for p in cerebras_models:
            sections.append(
                f'<p>{p}: {stats[p]["finance_acc"]:.0f}% finance accuracy '
                f'(vs {xai_acc:.0f}% for xAI Grok), '
                f'{stats[p]["avg_ms"]:.0f}ms avg response time.</p>'
            )
    else:
        sections.append(
            '<p class="rec-verdict" style="color: var(--red);">NOT RECOMMENDED &mdash; '
            'Cerebras is neither faster nor more accurate.</p>'
        )

    # Cost note
    sections.append(
        "<p><strong>Cost note:</strong> Cerebras free tier (llama3.1-8b / gpt-oss-120b) has "
        "rate limits but no per-token charges. The Qwen 3 235B model may incur costs. "
        "xAI Grok (grok-4-0709) is a paid API.</p>"
    )

    return "\n".join(sections)


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
def main() -> None:
    # Config
    xai_key = os.environ.get("XAI_API_KEY", "")
    cerebras_free_key = os.environ.get("CEREBRAS_API_KEY", "")
    cerebras_paid_key = os.environ.get("CEREBRAS_API_KEY_PAID", cerebras_free_key)

    if not xai_key:
        print("ERROR: XAI_API_KEY not set")
        sys.exit(1)
    if not cerebras_free_key:
        print("ERROR: CEREBRAS_API_KEY not set")
        sys.exit(1)

    all_results: dict[str, list[ClassifyResult]] = {}

    # --- xAI Grok ---
    print("\n=== xAI Grok (grok-4-0709) ===")
    all_results["xAI Grok"] = run_eval(
        provider_name="xAI Grok",
        base_url="https://api.x.ai/v1",
        api_key=xai_key,
        model="grok-4-0709",
    )

    # --- Cerebras GPT-OSS-120B (free model) ---
    print("\n=== Cerebras GPT-OSS-120B (free) ===")
    all_results["Cerebras GPT-OSS-120B"] = run_eval(
        provider_name="Cerebras GPT-OSS-120B",
        base_url="https://api.cerebras.ai/v1",
        api_key=cerebras_free_key,
        model="gpt-oss-120b",
    )

    # Check accuracy of free model
    free_stats = accuracy_stats(all_results["Cerebras GPT-OSS-120B"])
    xai_stats = accuracy_stats(all_results["xAI Grok"])

    # --- Cerebras Qwen 3 235B (paid/free key - same models available) ---
    print("\n=== Cerebras Qwen 3 235B ===")
    all_results["Cerebras Qwen3-235B"] = run_eval(
        provider_name="Cerebras Qwen3-235B",
        base_url="https://api.cerebras.ai/v1",
        api_key=cerebras_free_key,
        model="qwen-3-235b-a22b-instruct-2507",
    )

    # --- Generate Report ---
    report_path = PROJECT_ROOT / "reports" / "cerebras_eval.html"
    generate_html(all_results, report_path)

    # Print quick summary
    print("\n=== Quick Summary ===")
    for name, results in all_results.items():
        s = accuracy_stats(results)
        print(f"  {name}: finance_acc={s['finance_acc']:.0f}%, "
              f"cat_acc={s['category_acc']:.0f}%, "
              f"avg={s['avg_ms']:.0f}ms, json_ok={s['json_rate']:.0f}%")


if __name__ == "__main__":
    main()
