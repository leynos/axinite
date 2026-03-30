#!/usr/bin/env python3
"""Query and rebuild upstream audit ledgers."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any


ORDERED_FIELDS: dict[str, list[str]] = {
    "severity": ["low", "medium", "medium-high", "high", "critical"],
    "size": ["small", "medium", "large", "very-large"],
    "mission": ["no", "weak", "mixed", "mixed-strong", "strong"],
    "arch": ["conflicts", "low", "partial", "direct"],
    "main": ["no", "unlikely", "maybe", "yes"],
    "need_main": ["no", "maybe", "yes"],
    "effectiveness": ["targeted", "follow-up", "strong", "comprehensive"],
}

FILTER_RE = re.compile(r"^(?P<field>[A-Za-z_][A-Za-z0-9_]*)"
                       r"(?P<op>>=|<=|!=|=|>|<|~)"
                       r"(?P<value>.+)$")


def load_records(path: Path) -> list[dict[str, Any]]:
    records: list[dict[str, Any]] = []
    with path.open(encoding="utf-8") as handle:
        for line_number, raw_line in enumerate(handle, start=1):
            line = raw_line.strip()
            if not line:
                continue
            try:
                record = json.loads(line)
            except json.JSONDecodeError as exc:
                raise SystemExit(
                    f"{path}:{line_number}: invalid JSON: {exc.msg}"
                ) from exc
            if not isinstance(record, dict):
                raise SystemExit(
                    f"{path}:{line_number}: expected JSON object per line"
                )
            records.append(record)
    return records


def normalise_scalar(value: Any) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if value is None:
        return ""
    return str(value).strip().lower().replace(" ", "-")


def compare_values(field: str, lhs: Any, op: str, rhs: str) -> bool:
    lhs_norm = normalise_scalar(lhs)
    rhs_norm = normalise_scalar(rhs)
    ordered = ORDERED_FIELDS.get(field)
    if ordered:
        if lhs_norm not in ordered:
            return False
        if rhs_norm not in ordered:
            raise SystemExit(
                f"unsupported comparison value {rhs!r} for ordered field {field!r}"
            )
        left = ordered.index(lhs_norm)
        right = ordered.index(rhs_norm)
        if op == "=":
            return left == right
        if op == "!=":
            return left != right
        if op == ">":
            return left > right
        if op == ">=":
            return left >= right
        if op == "<":
            return left < right
        if op == "<=":
            return left <= right
        raise SystemExit(f"unsupported operator {op!r} for field {field!r}")

    if op == "~":
        return rhs_norm in lhs_norm
    if op in {"=", "!="}:
        result = lhs_norm == rhs_norm
        return result if op == "=" else not result

    try:
        left_num = float(lhs_norm)
        right_num = float(rhs_norm)
    except ValueError as exc:
        raise SystemExit(
            f"field {field!r} is not ordered or numeric; use =, !=, or ~"
        ) from exc

    if op == ">":
        return left_num > right_num
    if op == ">=":
        return left_num >= right_num
    if op == "<":
        return left_num < right_num
    if op == "<=":
        return left_num <= right_num
    raise SystemExit(f"unsupported operator {op!r}")


def parse_filter(expression: str) -> tuple[str, str, str]:
    match = FILTER_RE.match(expression)
    if match is None:
        raise SystemExit(
            f"invalid filter {expression!r}; expected FIELDOPVALUE such as "
            "'severity>=medium' or 'kind=fix'"
        )
    return (
        match.group("field"),
        match.group("op"),
        match.group("value"),
    )


def filter_commits(
    records: list[dict[str, Any]], expressions: list[str]
) -> list[dict[str, Any]]:
    commits = [
        record for record in records if record.get("record_type") == "commit"
    ]
    parsed = [parse_filter(expression) for expression in expressions]
    filtered: list[dict[str, Any]] = []
    for record in commits:
        matched = True
        for field, op, value in parsed:
            if field not in record:
                matched = False
                break
            if not compare_values(field, record[field], op, value):
                matched = False
                break
        if matched:
            filtered.append(record)
    return filtered


def format_commit_markdown(record: dict[str, Any]) -> str:
    sha = record["sha"]
    kind = record["kind"]
    purpose = record["purpose"]
    audit = record["audit_text"]
    return f"- `{sha}` [{kind}] {purpose}. {audit}"


def render_markdown(records: list[dict[str, Any]]) -> str:
    meta = next(
        (record for record in records if record.get("record_type") == "meta"),
        None,
    )
    if meta is None:
        raise SystemExit("JSONL file is missing a meta record")

    commits = [
        record for record in records if record.get("record_type") == "commit"
    ]
    commits.sort(key=lambda record: record.get("order", 0))

    lines: list[str] = []
    lines.append("<!-- markdownlint-disable MD013 MD024 -->")
    lines.append("")
    lines.append(meta["title"])
    lines.append("")
    lines.append("## Scope")
    lines.append("")
    for bullet in meta["scope_bullets"]:
        lines.append(f"- {bullet}")
    lines.append("")
    lines.append("## Executive summary")
    lines.append("")
    for bullet in meta["summary_bullets"]:
        lines.append(f"- {bullet}")
    lines.append("")
    lines.append("## Full ledger")
    lines.append("")

    current_date: str | None = None
    for record in commits:
        date = record["date"]
        if date != current_date:
            if current_date is not None:
                lines.append("")
            lines.append(f"### {date}")
            lines.append("")
            current_date = date
        lines.append(format_commit_markdown(record))

    lines.append("")
    lines.append("<!-- markdownlint-enable MD013 MD024 -->")
    lines.append("")
    return "\n".join(lines)


def print_query_results(
    records: list[dict[str, Any]],
    output_format: str,
    fields: list[str],
) -> None:
    if output_format == "jsonl":
        for record in records:
            print(json.dumps(record, sort_keys=True))
        return

    if output_format == "json":
        json.dump(records, sys.stdout, indent=2, sort_keys=True)
        print()
        return

    widths: dict[str, int] = {}
    for field in fields:
        widths[field] = max(
            len(field),
            max((len(str(record.get(field, ""))) for record in records), default=0),
        )

    header = "  ".join(field.ljust(widths[field]) for field in fields)
    print(header)
    print("  ".join("-" * widths[field] for field in fields))
    for record in records:
        print(
            "  ".join(
                str(record.get(field, "")).ljust(widths[field]) for field in fields
            )
        )


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Query and rebuild upstream audit JSONL files."
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    query = subparsers.add_parser(
        "query",
        help="Filter commit records with expressions like severity>=medium.",
    )
    query.add_argument("jsonl", type=Path, help="Path to the JSONL ledger.")
    query.add_argument(
        "filters",
        nargs="*",
        help="FIELDOPVALUE expressions, for example severity>=medium kind=fix.",
    )
    query.add_argument(
        "--format",
        choices=("table", "jsonl", "json"),
        default="table",
        help="Output format.",
    )
    query.add_argument(
        "--fields",
        default="sha,date,kind,purpose",
        help="Comma-separated fields for table output.",
    )
    query.add_argument(
        "--limit",
        type=int,
        default=None,
        help="Maximum number of matching records to print.",
    )

    rebuild = subparsers.add_parser(
        "rebuild-markdown",
        help="Rebuild the markdown report from the JSONL ledger.",
    )
    rebuild.add_argument("jsonl", type=Path, help="Path to the JSONL ledger.")
    rebuild.add_argument(
        "output",
        type=Path,
        help="Markdown path to write.",
    )

    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    records = load_records(args.jsonl)

    if args.command == "query":
        filtered = filter_commits(records, args.filters)
        if args.limit is not None:
            filtered = filtered[: args.limit]
        fields = [field.strip() for field in args.fields.split(",") if field.strip()]
        print_query_results(filtered, args.format, fields)
        return 0

    if args.command == "rebuild-markdown":
        markdown = render_markdown(records)
        args.output.write_text(markdown, encoding="utf-8")
        return 0

    raise SystemExit(f"unsupported command {args.command!r}")


if __name__ == "__main__":
    raise SystemExit(main())
