"""Static SVG benchmark panels in the model-launch style, plus the model-card tables.

Everything here is a pure function of committed JSON: Gaze release rows (passed in
as `GazeRow`s from `release-history.json`), `comparison.json` (own corpus,
competitors) and `their-benchmarks.json` (third-party sets). The output is one
light and one dark SVG (no external fonts, no scripts) that a `<picture>` element
switches on `prefers-color-scheme`.

Only released, tagged Gaze versions are ever drawn. The comparison report's
"Gaze main" run and the third-party files' `gaze-full` row measure an untagged
tree, so this module never reads them. A `GazeRow` whose version is not `vX.Y.Z`
raises `ChartError`, and a third-party Gaze row is read only from the optional
`gaze_releases` map, keyed by such a version.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Any, Mapping, Sequence
from xml.sax.saxutils import escape

TAG = re.compile(r"v\d+\.\d+\.\d+")

#: Third-party sets score only labels every tool can emit, so a tool is never
#: penalised for a label it has no output for.
THIRD_PARTY = (
    ("presidio-research", "Presidio Research"),
    ("piibench-commercial", "PIIBench-commercial"),
)
SHORT_NAMES = {
    "presidio": "Presidio",
    "datafog-core": "DataFog core",
    "datafog-python": "DataFog spaCy",
    "scrubadub": "scrubadub",
    "gliner": "GLiNER",
    "opf": "OPF",
}


class ChartError(Exception):
    """The committed data cannot produce a truthful chart."""


@dataclass(frozen=True)
class GazeRow:
    """One released Gaze default, already reduced to the two chart numbers."""

    version: str
    protected: float  # percent of gold PII bytes that did not leak
    fp_bytes: int  # gold-gap-adjusted false-positive bytes on the own corpus

    def __post_init__(self) -> None:
        if not TAG.fullmatch(self.version):
            raise ChartError(f"Gaze bars must be tagged releases, got {self.version!r}")

    @property
    def name(self) -> str:
        return "Gaze " + self.version[1:].rsplit(".", 1)[0]


@dataclass(frozen=True)
class Bar:
    name: str
    protected: float | None  # None: not measured yet
    fp_per_1k: float | None
    gaze: bool = False


@dataclass(frozen=True)
class Panel:
    title: str
    dataset: str  # short "docs, split" line shown under the title
    bars: tuple[Bar, ...]


def _protected(leaked: float, gold: float) -> float:
    return 100.0 * (1.0 - leaked / gold)


def own_panel(
    gaze: Sequence[GazeRow], comparison: Mapping[str, Any],
    declared: Mapping[str, str], gold_bytes: int,
) -> Panel:
    layer = comparison["corpus"]["layers"]["C"]["documents"]
    totals = {
        tool["contracts"]["v3"]["C"]["metrics"]["common_intersection"]["full"]["total_bytes"]
        for tool in comparison["tools"].values()
        if "v3" in tool["contracts"]
    }
    if len(totals) != 1:
        raise ChartError("layer C corpus bytes differ between tools")
    corpus_bytes = totals.pop()
    bars = [Bar(row.name, row.protected, 1000.0 * row.fp_bytes / corpus_bytes, gaze=True) for row in gaze]
    for key, name in declared.items():
        cell = comparison["tools"][name]["contracts"]["v3"]["C"]
        fp = cell["false_positive_bytes_after_gold_gap"]
        fp = cell["false_positive_bytes"] if fp is None else fp
        bars.append(
            Bar(SHORT_NAMES[key], _protected(cell["leaked_bytes"], gold_bytes),
                1000.0 * fp / corpus_bytes)
        )
    return Panel("Own corpus", f"{layer:,} docs", tuple(bars))


def third_party_panel(
    bench: Mapping[str, Any], title: str, declared: Mapping[str, str],
    latest_tag: str,
) -> Panel:
    """Competitors from their declared rows; Gaze only from tagged rows.

    `gaze_releases` (optional) maps a tag to a `common_intersection` block. Until
    the tag is measured on the set, its slot reads "pending" rather than borrowing
    the untagged `gaze-full` run.
    """
    tagged = bench.get("gaze_releases", {})
    for tag in tagged:
        if not TAG.fullmatch(tag):
            raise ChartError(f"gaze_releases key {tag!r} is not a release tag")
    label = "Gaze " + latest_tag[1:].rsplit(".", 1)[0]
    if latest_tag in tagged:
        bars = [_third_party_bar(label, tagged[latest_tag]["common_intersection"], True)]
    else:
        bars = [Bar(label, None, None, gaze=True)]
    for key, name in declared.items():
        bars.append(_third_party_bar(
            SHORT_NAMES[key], bench["rows"][name]["common_intersection"], False))
    split = next(iter(bench["splits"]))
    docs = bench["splits"][split]["documents"]
    return Panel(title, f"{docs:,} docs, {split} split", tuple(bars))


def _third_party_bar(name: str, block: Mapping[str, Any], gaze: bool) -> Bar:
    gold = block["leaked_bytes"] + block["true_positive_bytes"]
    return Bar(
        name, _protected(block["leaked_bytes"], gold),
        1000.0 * block["false_positive_bytes"] / block["total_bytes"], gaze=gaze,
    )


def panels(
    gaze: Sequence[GazeRow], comparison: Mapping[str, Any],
    their: Mapping[str, Any], declared: Mapping[str, str], gold_bytes: int,
) -> list[Panel]:
    if not gaze:
        raise ChartError("no tagged Gaze release row to chart")
    latest = gaze[-1].version
    return [own_panel(gaze, comparison, declared, gold_bytes)] + [
        third_party_panel(their[key], title, declared, latest) for key, title in THIRD_PARTY
    ]


# --------------------------------------------------------------------------
# SVG
# --------------------------------------------------------------------------

THEMES = {
    "light": dict(text="#1f2328", sub="#59636e", grid="#d1d9e0", accent="#0969da",
                  grey="#a8b1bb", panel="#f6f8fa"),
    "dark": dict(text="#f0f6fc", sub="#9198a1", grid="#3d444d", accent="#4493f8",
                 grey="#6e7681", panel="#151b23"),
}
FONT = "-apple-system,BlinkMacSystemFont,'Segoe UI',Helvetica,Arial,sans-serif"
WIDTH, GAP, PAD, PANEL_H, NAME_PX = 1200, 12, 12, 300, 10
#: Conservative glyph advance (em) used to prove neighbouring names do not touch.
GLYPH_EM = 0.55


def name_lines(name: str) -> list[str]:
    return name.split(" ", 1) if " " in name else [name]


def slot_width(bar_count: int) -> float:
    panel_w = (WIDTH - GAP * 4) / 3
    return (panel_w - 2 * PAD) / bar_count


def label_overlap(names: Sequence[str], bar_count: int) -> str | None:
    """A neighbouring pair whose estimated names would touch, or None."""
    slot = slot_width(bar_count)

    def half(name: str) -> float:
        return max(len(line) for line in name_lines(name)) * NAME_PX * GLYPH_EM / 2

    for left, right in zip(names, names[1:]):
        if half(left) + half(right) > slot:
            return f"{left!r} and {right!r}"
    return None


def _bar_svg(t: Mapping[str, str], bar: Bar, value: float | None, fmt: str,
             cx: float, bw: float, bot: float, ph: float, vmax: float) -> list[str]:
    out = []
    if value is None:
        out.append(
            f'<rect x="{cx - bw / 2:.1f}" y="{bot - 6:.1f}" width="{bw:.1f}" height="6" rx="2" '
            f'fill="none" stroke="{t["accent"]}" stroke-width="1.5" stroke-dasharray="3 2"/>'
        )
        out.append(f'<text x="{cx:.1f}" y="{bot - 12:.1f}" font-size="11" font-weight="700" '
                   f'text-anchor="middle" fill="{t["accent"]}">pending</text>')
        return out
    bh = ph * min(value / vmax, 1.0)
    fill = t["accent"] if bar.gaze else t["grey"]
    out.append(f'<rect x="{cx - bw / 2:.1f}" y="{bot - bh:.1f}" width="{bw:.1f}" '
               f'height="{max(bh, 1):.1f}" rx="3" fill="{fill}"/>')
    colour = t["accent"] if bar.gaze else t["text"]
    out.append(f'<text x="{cx:.1f}" y="{bot - bh - 6:.1f}" font-size="12" font-weight="700" '
               f'text-anchor="middle" fill="{colour}">{fmt.format(value)}</text>')
    return out


def _panel_svg(t: Mapping[str, str], x: float, y: float, w: float, panel: Panel,
               metric: str, better: str, fmt: str, pick: Any, vmax: float) -> list[str]:
    o = [
        f'<rect x="{x:.1f}" y="{y}" width="{w:.1f}" height="{PANEL_H}" rx="10" fill="{t["panel"]}"/>',
        f'<text x="{x + 16:.1f}" y="{y + 28}" font-size="16" font-weight="700" fill="{t["text"]}">{escape(panel.title)}</text>',
        f'<text x="{x + w - 16:.1f}" y="{y + 28}" font-size="11" text-anchor="end" fill="{t["sub"]}">{better}</text>',
        f'<text x="{x + 16:.1f}" y="{y + 48}" font-size="12" fill="{t["sub"]}">{escape(metric)} · {escape(panel.dataset)}</text>',
    ]
    top, bot = y + 82, y + PANEL_H - 46
    ph = bot - top
    slot = (w - 2 * PAD) / len(panel.bars)
    bw = min(slot * 0.62, 38)
    for frac in (0.0, 0.5, 1.0):
        gy = bot - ph * frac
        dash = "" if frac == 0 else ' stroke-dasharray="3 4"'
        o.append(f'<line x1="{x + PAD:.1f}" x2="{x + w - PAD:.1f}" y1="{gy:.1f}" y2="{gy:.1f}" '
                 f'stroke="{t["grid"]}" stroke-width="1"{dash}/>')
    for i, bar in enumerate(panel.bars):
        cx = x + PAD + slot * (i + 0.5)
        o += _bar_svg(t, bar, pick(bar), fmt, cx, bw, bot, ph, vmax)
        weight = "600" if bar.gaze else "400"
        colour = t["text"] if bar.gaze else t["sub"]
        for j, line in enumerate(name_lines(bar.name)):
            o.append(f'<text x="{cx:.1f}" y="{bot + 16 + 12 * j}" font-size="{NAME_PX}" '
                     f'font-weight="{weight}" text-anchor="middle" fill="{colour}">{escape(line)}</text>')
    return o


def figure_svg(theme: str, panel_set: Sequence[Panel], alt: str) -> str:
    """Row 1: PII protected (higher is better). Row 2: false positives (lower is better)."""
    t = THEMES[theme]
    pw = (WIDTH - GAP * 4) / 3
    height = GAP + 2 * (PANEL_H + GAP)
    o = [
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {WIDTH} {height}" '
        f'width="{WIDTH}" height="{height}" font-family="{FONT}" role="img" aria-label="{escape(alt)}">',
        f"<title>{escape(alt)}</title>",
    ]
    for row, (metric, better, fmt, pick) in enumerate((
        ("PII protected", "higher is better", "{:.1f}%", lambda b: b.protected),
        ("False-positive bytes per 1,000", "lower is better", "{:.1f}", lambda b: b.fp_per_1k),
    )):
        for col, panel in enumerate(panel_set):
            values = [pick(b) for b in panel.bars if pick(b) is not None]
            vmax = 100.0 if row == 0 else max(values) * 1.15
            o += _panel_svg(t, GAP + col * (pw + GAP), GAP + row * (PANEL_H + GAP), pw,
                            panel, metric, better, fmt, pick, vmax)
    o.append("</svg>")
    return "\n".join(o) + "\n"


def figure_files(panel_set: Sequence[Panel]) -> dict[str, str]:
    """File name (under the assets directory) -> SVG text."""
    for panel in panel_set:
        clash = label_overlap([b.name for b in panel.bars], len(panel.bars))
        if clash:
            raise ChartError(f"{panel.title}: bar names {clash} would overlap")
    alt = (
        "Benchmark panels for own corpus, Presidio Research and PIIBench-commercial: "
        "PII protected percent (higher is better) and false-positive bytes per 1,000 "
        "bytes (lower is better), Gaze release against declared competitor configurations."
    )
    return {f"benchmark-panels-{theme}.svg": figure_svg(theme, panel_set, alt) for theme in THEMES}


# --------------------------------------------------------------------------
# model-card tables
# --------------------------------------------------------------------------


def _column_names(panel_set: Sequence[Panel]) -> list[str]:
    seen: list[str] = []
    for panel in panel_set:
        for bar in panel.bars:
            if bar.name not in seen:
                seen.append(bar.name)
    return seen


def _table(panel_set: Sequence[Panel], pick: Any, fmt: str, best: Any) -> list[str]:
    columns = _column_names(panel_set)
    lines = ["| Benchmark | " + " | ".join(columns) + " |",
             "|---|" + "---:|" * len(columns)]
    for panel in panel_set:
        measured = [pick(b) for b in panel.bars if pick(b) is not None]
        top = best(measured)
        cells = []
        by_name = {b.name: b for b in panel.bars}
        for name in columns:
            bar = by_name.get(name)
            if bar is None:
                cells.append("not run")
            elif pick(bar) is None:
                cells.append("pending")
            else:
                text = fmt.format(pick(bar))
                cells.append(f"**{text}**" if pick(bar) == top else text)
        lines.append(f"| {panel.title} | " + " | ".join(cells) + " |")
    return lines


def model_card_tables(panel_set: Sequence[Panel]) -> str:
    protected = _table(panel_set, lambda b: b.protected, "{:.1f}%", max)
    false_pos = _table(panel_set, lambda b: b.fp_per_1k, "{:.1f}", min)
    return "\n".join([
        "**PII protected** (higher is better; best per row in bold):", "", *protected, "",
        "**False-positive bytes per 1,000 bytes** (lower is better; best per row in bold):",
        "", *false_pos,
    ])
