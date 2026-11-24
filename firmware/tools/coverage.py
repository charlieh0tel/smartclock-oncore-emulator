"""Renders docs/coverage.json as docs/coverage.html, the emulator's coverage by instrument.

A cell that is a Yes or a Partial must name the repository file and the heading
that records it; the heading is checked to exist, so the page cannot claim more
than the repository holds.

    uv run python coverage.py           # write docs/coverage.html
    uv run python coverage.py --check   # fail if it is stale or a claim has no record
"""

import argparse
import html
import json
import re
import sys
from pathlib import Path
from typing import Any

REPO = Path(__file__).resolve().parents[2]
DOCS = REPO / "docs"
SOURCE = DOCS / "coverage.json"
PAGE = DOCS / "coverage.html"
REPOSITORY = "https://github.com/charlieh0tel/smartclock-oncore-emulator"
BLOB = f"{REPOSITORY}/blob/main/"
GENERATOR = "firmware/tools/coverage.py"
# The statuses that claim a run, so must cite where it is recorded.
CLAIMS = ("yes", "partial")
LABELS = {
    "yes": "Yes",
    "partial": "Partial",
    "in-review": "In review",
    "not-yet": "Not yet",
    "not-planned": "Not planned",
}
HEADING = re.compile(r"^#{1,6}\s+(.*?)\s*$")


def slug(heading: str) -> str:
    """GitHub's anchor for a Markdown heading: lower case, punctuation but - and _
    dropped, spaces to -."""
    kept = re.sub(r"[^\w\- ]", "", heading.strip().lower())
    return kept.replace(" ", "-")


def headings(path: Path) -> set[str]:
    """A Markdown file's headings, outside fenced code."""
    found, fenced = set(), False
    for line in path.read_text().splitlines():
        if line.startswith("```"):
            fenced = not fenced
        elif not fenced and (m := HEADING.match(line)):
            found.add(m[1])
    return found


def problems(data: dict[str, Any]) -> list[str]:
    """Each cell's fault: an unknown status, a claim without its record, or a record
    that is not there; and a figure whose picture is not there."""
    keys = [c["key"] for c in data["columns"]]
    out = []
    if not (REPO / data["figure"]["path"]).is_file():
        out.append(f"figure: no file {data['figure']['path']}")
    for row in data["rows"]:
        if sorted(row["cells"]) != sorted(keys):
            out.append(f"{row['image']}: cells {sorted(row['cells'])}, columns {sorted(keys)}")
            continue
        for key in keys:
            cell, where = row["cells"][key], f"{row['image']}, {key}"
            if cell["status"] not in data["statuses"]:
                out.append(f"{where}: status {cell['status']!r}")
            if "path" not in cell:
                if cell["status"] in CLAIMS:
                    out.append(f"{where}: {cell['status']} with no record")
                continue
            path = REPO / cell["path"]
            if not path.is_file():
                out.append(f"{where}: no file {cell['path']}")
            elif cell["section"] not in headings(path):
                out.append(f"{where}: no heading {cell['section']!r} in {cell['path']}")
    return out


def cell_html(cell: dict[str, Any]) -> str:
    label = LABELS[cell["status"]]
    if "path" in cell:
        href = f"{BLOB}{cell['path']}#{slug(cell['section'])}"
        label = f'<a href="{html.escape(href)}">{label}</a>'
    note = f'<span class="note">{html.escape(cell["note"])}</span>' if "note" in cell else ""
    return f'<td class="{cell["status"]}">{label}{note}</td>'


def figure_html(figure: dict[str, str]) -> str:
    """The picture, its path given from the repository's root, linked from the page in docs/;
    its caption links the repository, as the page is the repository's home page."""
    src = (REPO / figure["path"]).relative_to(DOCS).as_posix()
    return (
        f'<figure><img src="{html.escape(src)}" alt="{html.escape(figure["alt"])}">'
        f"<figcaption>{html.escape(figure['caption'])} "
        f'<a href="{REPOSITORY}">{REPOSITORY.removeprefix("https://")}</a></figcaption></figure>'
    )


def render(data: dict[str, Any]) -> str:
    columns = data["columns"]
    # Each group's title and how many columns it spans, in order.
    groups: list[tuple[str, int]] = []
    for c in columns:
        if groups and groups[-1][0] == c["group"]:
            groups[-1] = (c["group"], groups[-1][1] + 1)
        else:
            groups.append((c["group"], 1))
    group_row = "".join(f'<th colspan="{n}">{html.escape(g)}</th>' for g, n in groups)
    title_row = "".join(f"<th>{html.escape(c['title'])}</th>" for c in columns)
    rows = "\n".join(
        f'<tr><th scope="row">{html.escape(r["image"])}</th>'
        + "".join(cell_html(r["cells"][c["key"]]) for c in columns)
        + "</tr>"
        for r in data["rows"]
    )
    legend = "".join(
        f"<li><b>{LABELS[k]}</b>: {html.escape(v)}</li>" for k, v in data["statuses"].items()
    )
    means = "".join(
        f"<li><b>{html.escape(c['title'])}</b>: {html.escape(c['means'])}</li>" for c in columns
    )
    title = html.escape(data["title"])
    return f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
:root {{ --bg: #fbfbf9; --ink: #22231f; --muted: #6a6b63; --rule: #d6d5ce;
  --yes: #dff3e6; --partial: #fbf0d3; --review: #e2e9f6; --no: #f1f0eb; color-scheme: light dark; }}
@media (prefers-color-scheme: dark) {{ :root {{ --bg: #171816; --ink: #e9e8e2;
  --muted: #a3a299; --rule: #3b3c37; --yes: #1c3a28; --partial: #3d3418; --review: #1f2b40;
  --no: #22231f; }} }}
body {{ background: var(--bg); color: var(--ink); font: 15px/1.5 system-ui, sans-serif;
  max-width: 1100px; margin: 0 auto; padding: 24px 16px; }}
p, li {{ max-width: 75ch; }}
.scroll {{ overflow-x: auto; }}
table {{ border-collapse: collapse; font-size: 14px; }}
th, td {{ border: 1px solid var(--rule); padding: 6px 8px; text-align: left; vertical-align: top; }}
thead th {{ background: var(--no); }}
td.yes {{ background: var(--yes); }}
td.partial {{ background: var(--partial); }}
td.in-review {{ background: var(--review); }}
td.not-yet, td.not-planned {{ color: var(--muted); }}
.note {{ display: block; color: var(--muted); font-size: 12px; }}
a {{ color: inherit; }}
footer {{ color: var(--muted); font-size: 13px; margin-top: 24px; }}
figure {{ margin: 0 0 16px; max-width: 75ch; }}
figure img {{ display: block; width: 100%; height: auto; }}
figcaption {{ color: var(--muted); font-size: 13px; }}
</style>
</head>
<body>
<h1>{title}</h1>
{figure_html(data["figure"])}
<p>{html.escape(data["intro"])}</p>
<div class="scroll">
<table>
<thead>
<tr><th rowspan="2">Image</th>{group_row}</tr>
<tr>{title_row}</tr>
</thead>
<tbody>
{rows}
</tbody>
</table>
</div>
<h2>Marks</h2>
<ul>{legend}</ul>
<h2>The columns</h2>
<ul>{means}</ul>
<footer>Generated by <code>{GENERATOR}</code> from <code>docs/coverage.json</code>.</footer>
</body>
</html>
"""


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--check", action="store_true", help="fail if stale or unrecorded")
    args = parser.parse_args()
    data = json.loads(SOURCE.read_text())
    if faults := problems(data):
        print("\n".join(faults), file=sys.stderr)
        return 1
    page = render(data)
    if args.check:
        if not PAGE.is_file() or PAGE.read_text() != page:
            print(f"{PAGE.relative_to(REPO)} is stale: run {GENERATOR}", file=sys.stderr)
            return 1
        return 0
    PAGE.write_text(page)
    return 0


if __name__ == "__main__":
    sys.exit(main())
