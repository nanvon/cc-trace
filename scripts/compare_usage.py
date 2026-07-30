#!/usr/bin/env python3
"""Compare CC Trace usage.db with ccbar's read-only usage rollup."""

from __future__ import annotations

import argparse
import json
import re
import sqlite3
from collections import defaultdict
from datetime import date, datetime
from decimal import Decimal
from pathlib import Path
from typing import Any


APPLE_REFERENCE_UNIX_SECONDS = 978_307_200
NANOS_PER_USD = Decimal("1000000000")
TOKEN_FIELDS = (
    "uncached_input_tokens",
    "output_tokens",
    "cache_read_input_tokens",
    "cache_write_input_tokens",
    "request_count",
    "total_tokens",
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cc-trace-db", required=True, type=Path)
    parser.add_argument("--ccbar-rollup", required=True, type=Path)
    parser.add_argument("--day", help="Local calendar day (YYYY-MM-DD)")
    parser.add_argument("--output", type=Path)
    return parser.parse_args()


def normalize_model(model: str | None) -> str:
    value = (model or "").strip().lower()
    for prefix in ("openai/", "deepseek/"):
        if value.startswith(prefix):
            value = value[len(prefix) :]
            break
    value = value.split("@", 1)[0]
    return re.sub(r"-(?:\d{8}|\d{4}-\d{2}-\d{2})$", "", value)


def empty_row() -> dict[str, Any]:
    return {
        "uncached_input_tokens": 0,
        "output_tokens": 0,
        "cache_read_input_tokens": 0,
        "cache_write_input_tokens": 0,
        "request_count": 0,
        "total_tokens": 0,
        "cost_usd": Decimal(0),
        "unpriced_entries": 0,
        "has_unpriced_usage": False,
    }


def add_row(target: dict[str, Any], source: dict[str, Any]) -> None:
    for field in TOKEN_FIELDS:
        target[field] += source[field]
    target["cost_usd"] += source["cost_usd"]
    target["unpriced_entries"] += source["unpriced_entries"]
    target["has_unpriced_usage"] = (
        target["has_unpriced_usage"] or source["has_unpriced_usage"]
    )


def load_cc_trace(path: Path) -> tuple[dict[tuple[str, str, str, str], dict[str, Any]], str | None]:
    connection = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    try:
        rows = connection.execute(
            """
            SELECT day_local,
                   source,
                   COALESCE(model, ''),
                   speed,
                   SUM(uncached_input_tokens),
                   SUM(output_tokens),
                   SUM(cache_read_input_tokens),
                   SUM(cache_write_5m_input_tokens + cache_write_1h_input_tokens),
                   COUNT(*),
                   SUM(
                     uncached_input_tokens
                     + output_tokens
                     + cache_read_input_tokens
                     + cache_write_5m_input_tokens
                     + cache_write_1h_input_tokens
                   ),
                   COALESCE(SUM(api_equivalent_cost_nanos), 0),
                   SUM(CASE WHEN api_equivalent_cost_nanos IS NULL THEN 1 ELSE 0 END)
              FROM usage_entries
             GROUP BY day_local, source, COALESCE(model, ''), speed
            """
        ).fetchall()
        fingerprints = connection.execute(
            """
            SELECT DISTINCT pricing_fingerprint
              FROM usage_entries
             WHERE pricing_fingerprint IS NOT NULL
             ORDER BY pricing_fingerprint
            """
        ).fetchall()
    finally:
        connection.close()

    result: dict[tuple[str, str, str, str], dict[str, Any]] = {}
    for row in rows:
        key = (row[0], row[1], normalize_model(row[2]), row[3])
        item = empty_row()
        item.update(
            {
                "uncached_input_tokens": int(row[4]),
                "output_tokens": int(row[5]),
                "cache_read_input_tokens": int(row[6]),
                "cache_write_input_tokens": int(row[7]),
                "request_count": int(row[8]),
                "total_tokens": int(row[9]),
                "cost_usd": Decimal(int(row[10])) / NANOS_PER_USD,
                "unpriced_entries": int(row[11]),
                "has_unpriced_usage": int(row[11]) > 0,
            }
        )
        if key in result:
            add_row(result[key], item)
        else:
            result[key] = item

    fingerprint = fingerprints[0][0] if len(fingerprints) == 1 else None
    return result, fingerprint


def ccbar_day(raw_value: int | float) -> str:
    unix_seconds = float(raw_value) + APPLE_REFERENCE_UNIX_SECONDS
    return datetime.fromtimestamp(unix_seconds).date().isoformat()


def load_ccbar(
    path: Path,
) -> tuple[dict[tuple[str, str, str, str], dict[str, Any]], str | None, str | None]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    result: dict[tuple[str, str, str, str], dict[str, Any]] = {}
    for bucket in payload.get("buckets", []):
        key = (
            ccbar_day(bucket["day"]),
            str(bucket["app"]),
            normalize_model(bucket.get("model")),
            str(bucket["speed"]),
        )
        item = empty_row()
        item.update(
            {
                "uncached_input_tokens": int(bucket["inputTokens"]),
                "output_tokens": int(bucket["outputTokens"]),
                "cache_read_input_tokens": int(bucket["cacheReadTokens"]),
                "cache_write_input_tokens": int(bucket["cacheCreationTokens"]),
                "request_count": int(bucket["requestCount"]),
                "total_tokens": (
                    int(bucket["inputTokens"])
                    + int(bucket["outputTokens"])
                    + int(bucket["cacheReadTokens"])
                    + int(bucket["cacheCreationTokens"])
                ),
                "cost_usd": Decimal(str(bucket["costUSD"])),
                "unpriced_entries": 0,
                "has_unpriced_usage": bool(bucket.get("hasUnpricedUsage", False)),
            }
        )
        if key in result:
            add_row(result[key], item)
        else:
            result[key] = item

    updated_at = payload.get("updatedAt")
    updated_iso = (
        datetime.fromtimestamp(float(updated_at) + APPLE_REFERENCE_UNIX_SECONDS).astimezone().isoformat()
        if updated_at is not None
        else None
    )
    return result, payload.get("pricingFingerprint"), updated_iso


def choose_day(
    requested: str | None,
    trace: dict[tuple[str, str, str, str], dict[str, Any]],
    ccbar: dict[tuple[str, str, str, str], dict[str, Any]],
) -> str:
    if requested:
        return date.fromisoformat(requested).isoformat()
    today = date.today().isoformat()
    trace_days = {key[0] for key in trace if key[0] < today}
    ccbar_days = {key[0] for key in ccbar if key[0] < today}
    common = sorted(trace_days & ccbar_days)
    if not common:
        raise RuntimeError("no completed local calendar day exists in both data sets")
    return common[-1]


def decimal_string(value: Decimal) -> str:
    return format(value.normalize(), "f")


def public_row(row: dict[str, Any]) -> dict[str, Any]:
    return {
        **{field: row[field] for field in TOKEN_FIELDS},
        "cost_usd": decimal_string(row["cost_usd"]),
        "unpriced_entries": row["unpriced_entries"],
        "has_unpriced_usage": row["has_unpriced_usage"],
    }


def source_totals(
    rows: dict[tuple[str, str, str, str], dict[str, Any]],
    selected_day: str,
) -> dict[str, dict[str, Any]]:
    totals: dict[str, dict[str, Any]] = defaultdict(empty_row)
    for (day_key, source, _model, _speed), row in rows.items():
        if day_key == selected_day:
            add_row(totals[source], row)
    return {source: public_row(row) for source, row in sorted(totals.items())}


def compare(
    selected_day: str,
    trace: dict[tuple[str, str, str, str], dict[str, Any]],
    ccbar: dict[tuple[str, str, str, str], dict[str, Any]],
) -> tuple[list[dict[str, Any]], dict[str, int]]:
    keys = sorted(
        {
            key
            for key in trace.keys() | ccbar.keys()
            if key[0] == selected_day
        }
    )
    details: list[dict[str, Any]] = []
    counts: dict[str, int] = defaultdict(int)

    for key in keys:
        trace_row = trace.get(key)
        ccbar_row = ccbar.get(key)
        if trace_row is None:
            classification = "missing_in_cc_trace"
        elif ccbar_row is None:
            classification = "missing_in_ccbar"
        elif any(trace_row[field] != ccbar_row[field] for field in TOKEN_FIELDS):
            classification = "token_mismatch"
        else:
            tolerance = Decimal("0.000000001") * max(
                trace_row["request_count"], ccbar_row["request_count"], 1
            )
            comparable_cost = (
                trace_row["unpriced_entries"] == 0
                and not ccbar_row["has_unpriced_usage"]
            )
            if comparable_cost and abs(trace_row["cost_usd"] - ccbar_row["cost_usd"]) <= tolerance:
                classification = "exact"
            elif comparable_cost:
                classification = "tokens_match_cost_mismatch"
            else:
                classification = "tokens_match_cost_not_comparable"

        counts[classification] += 1
        if classification != "exact":
            day_key, source, model, speed = key
            details.append(
                {
                    "day": day_key,
                    "source": source,
                    "model": model or None,
                    "speed": speed,
                    "classification": classification,
                    "cc_trace": public_row(trace_row) if trace_row else None,
                    "ccbar": public_row(ccbar_row) if ccbar_row else None,
                }
            )

    return details, dict(sorted(counts.items()))


def main() -> None:
    args = parse_args()
    trace, trace_fingerprint = load_cc_trace(args.cc_trace_db)
    ccbar, ccbar_fingerprint, ccbar_updated_at = load_ccbar(args.ccbar_rollup)
    selected_day = choose_day(args.day, trace, ccbar)
    details, counts = compare(selected_day, trace, ccbar)
    report = {
        "selected_day": selected_day,
        "ccbar_updated_at": ccbar_updated_at,
        "pricing": {
            "cc_trace_fingerprint": trace_fingerprint,
            "ccbar_fingerprint": ccbar_fingerprint,
            "same_fingerprint": trace_fingerprint == ccbar_fingerprint,
        },
        "row_classification_counts": counts,
        "cc_trace_totals_by_source": source_totals(trace, selected_day),
        "ccbar_totals_by_source": source_totals(ccbar, selected_day),
        "differences": details,
    }
    encoded = json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True)
    print(encoded)
    if args.output:
        args.output.write_text(encoded + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
