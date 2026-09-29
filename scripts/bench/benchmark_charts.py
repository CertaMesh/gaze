"""Static SVG benchmark panels in the model-launch style, plus the model-card tables.

The headline metric is character-level F2 (see `METRIC_DEFINITION`), read from the
comparison report, the third-party files and `release-char-level.json`; every
tool is scored by the same `ComparisonMetrics`, so none is computed here.

Everything here is a pure function of committed JSON: Gaze release rows (passed in
as `GazeRow`s from `release-history.json`), `comparison.json` (own corpus,
competitors) and `their-benchmarks.json` (third-party sets). The output is one
light and one dark SVG (no external fonts, no scripts) that a `<picture>` element
switches on `prefers-color-scheme`.

Only released, tagged Gaze versions are ever drawn. The comparison report's
"Gaze main" run and the third-party files' `gaze-full` row measure an untagged
tree, so this module never reads them. A `GazeRow` whose version is not `vX.Y.Z`
raises `ChartError`, and a third-party Gaze row is read only from a `gaze-vX.Y.Z`
row of the benchmark (the shared rule lives in `tagged_gaze.py`).
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Mapping, Sequence
from xml.sax.saxutils import escape

from tagged_gaze import TAG, UntaggedGazeError, require_tag

#: (benchmark key, panel title, the vendor's own headline metric for that set).
THIRD_PARTY = (
    ("presidio-research", "Presidio Research", "F2, binary PII vs O (presidio-evaluator)"),
    ("piibench-commercial", "PIIBench-commercial", "span F1, exact span + type (PIIBench seqeval)"),
)
SHORT_NAMES = {
    "presidio": "Presidio",
    "datafog-core": "DataFog core",
    "datafog-python": "DataFog spaCy",
    "scrubadub": "scrubadub",
    "gliner": "GLiNER",
    "opf": "OPF",
}
METRIC = "Character-level F2 (β=2, label-agnostic, micro)"
#: One sentence, shown wherever the headline number is (README and benchmark page).
METRIC_DEFINITION = (
    "F2 counts Unicode code points (not grapheme clusters) inside the "
    "merged byte spans of each document, ignores labels, pools every document (micro), "
    "weights recall four times precision, scores 0 when precision and recall are both 0 "
    "(0/0 = 0), and counts all of a skipped document's gold characters as missed."
)


#: Why the F2 row and the false-positive row can differ in what they count as a false positive.
FP_NOTE = (
    "F2 counts every false-positive character for every tool; the false-positive row "
    "(bytes redacted that are not PII, per 1,000 bytes of the scored documents) also credits "
    "a protected repeat of a labelled value on the own corpus (contract v3). Every tool is "
    "treated identically within each row, and the third-party sets have no such credit."
)


class ChartError(Exception):
    """The committed data cannot produce a truthful chart."""


@dataclass(frozen=True)
class GazeRow:
    """One released Gaze default, reduced to the chart numbers."""

    version: str
    f2: float  # every charted release has a recorded character-level measurement
    leaked_bytes: int
    fp_bytes: int  # gold-gap-adjusted false-positive bytes on the own corpus
    total_bytes: int  # bytes of the documents scored (same block as f2 and leaked_bytes)
    refused: int = 0  # documents Gaze failed closed on instead of cleaning

    def __post_init__(self) -> None:
        try:
            require_tag(self.version, "Gaze bar")
        except UntaggedGazeError as error:
            raise ChartError(str(error)) from error

    @property
    def name(self) -> str:
        return "Gaze " + self.version[1:].rsplit(".", 1)[0]

    @property
    def fp_per_1k(self) -> float:
        return 1000.0 * self.fp_bytes / self.total_bytes


@dataclass(frozen=True)
class Bar:
    name: str
    f2: float | None  # None: not measured yet
    leaked: int | None  # leaked PII bytes, printed under the bar
    fp_per_1k: float | None
    gaze: bool = False


@dataclass(frozen=True)
class Panel:
    title: str
    dataset: str  # dataset and split line under the metric
    labels: str  # which gold labels are scored
    bars: tuple[Bar, ...]
    vendor_metric: str = ""  # the set's own headline metric (third-party sets)
    skipped: int = 0  # documents a declared competitor skipped (their gold counts as missed)
    refused: tuple[tuple[str, int], ...] = ()  # (Gaze bar name, refused documents)
    documents: int = 0


@dataclass(frozen=True)
class View:
    """Every per-panel number of one tool, read from ONE metrics block.

    A block's `total_bytes` counts the bytes its own view scores (the
    common-intersection view drops other labels' bytes), so a rate must divide
    that block's false positives by that block's total, never another view's.
    """

    f2: float
    leaked: int
    fp: int
    total_bytes: int

    @classmethod
    def of(cls, block: Mapping[str, Any], fp: int | None = None) -> "View":
        """`fp` overrides the block's false positives with a same-view adjusted count
        (the v3 gold-gap credit lives on the layer cell, not in the block)."""
        return cls(
            block["char_level"]["f2"], block["leaked_bytes"],
            block["false_positive_bytes"] if fp is None else fp, block["total_bytes"],
        )

    @property
    def fp_per_1k(self) -> float:
        return 1000.0 * self.fp / self.total_bytes


def own_panel(
    gaze: Sequence[GazeRow], comparison: Mapping[str, Any],
    declared: Mapping[str, str], corpus_name: str,
) -> Panel:
    layer = comparison["corpus"]["layers"]["C"]["documents"]
    splits = " + ".join(comparison["heldout_split"]["layers"]["C"])
    skipped = 0
    bars = [
        Bar(row.name, row.f2, row.leaked_bytes, row.fp_per_1k, gaze=True)
        for row in gaze
    ]
    for key, name in declared.items():
        cell = comparison["tools"][name]["contracts"]["v3"]["C"]
        fp = cell["false_positive_bytes_after_gold_gap"]
        fp = cell["false_positive_bytes"] if fp is None else fp
        skipped += cell["skipped_documents"]
        view = View.of(cell["metrics"]["product_coverage"]["full"], fp=fp)
        bars.append(Bar(SHORT_NAMES[key], view.f2, view.leaked, view.fp_per_1k))
    return Panel(
        "Own corpus", f"{corpus_name} · {layer:,} docs, {splits}",
        "Scored labels v3: the labels Gaze commits to detect", tuple(bars),
        skipped=skipped, refused=tuple((row.name, row.refused) for row in gaze),
        documents=layer,
    )


def third_party_panel(
    bench: Mapping[str, Any], title: str, vendor_metric: str,
    declared: Mapping[str, str], latest_tag: str,
) -> Panel:
    """Competitors from their declared rows; Gaze only from a tagged row.

    A `gaze-vX.Y.Z` row of the benchmark is a tagged run. Until the latest tag is
    measured on the set, its slot reads "pending" rather than borrowing the
    untagged `gaze-full` run.
    """
    for tool in bench["rows"]:
        if tool.startswith("gaze-v") and not TAG.fullmatch(tool[len("gaze-"):]):
            raise ChartError(f"row {tool!r} is not a gaze-vX.Y.Z release tag row")
    label = "Gaze " + latest_tag[1:].rsplit(".", 1)[0]
    tagged = bench["rows"].get(f"gaze-{latest_tag}")
    if tagged is not None:
        bars = [_third_party_bar(label, tagged, True)]
    else:
        bars = [Bar(label, None, None, None, gaze=True)]
    for key, name in declared.items():
        bars.append(_third_party_bar(SHORT_NAMES[key], bench["rows"][name], False))
    split = next(iter(bench["splits"]))
    docs = bench["splits"][split]["documents"]
    return Panel(
        title, f"{docs:,} docs, {split} split",
        "All gold labels; labels a tool cannot emit count as missed", tuple(bars),
        vendor_metric=vendor_metric, documents=docs,
    )


def _third_party_bar(name: str, row: Mapping[str, Any], gaze: bool) -> Bar:
    view = View.of(row["product_coverage"])
    return Bar(name, view.f2, view.leaked, view.fp_per_1k, gaze=gaze)


def panels(
    gaze: Sequence[GazeRow], comparison: Mapping[str, Any],
    their: Mapping[str, Any], declared: Mapping[str, str], corpus_name: str,
) -> list[Panel]:
    if not gaze:
        raise ChartError("no tagged Gaze release row to chart")
    latest = gaze[-1].version
    return [own_panel(gaze, comparison, declared, corpus_name)] + [
        third_party_panel(their[key], title, vendor, declared, latest)
        for key, title, vendor in THIRD_PARTY
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
WIDTH, GAP, PAD, PANEL_H, NAME_PX = 1200, 12, 10, 350, 10
#: Size of the small leaked-bytes figure under each top-row bar.
SMALL_PX = 9
#: Conservative glyph advance (em), wide enough for DejaVu Sans, the usual Linux
#: fallback of the font stack.
GLYPH_EM = 0.6
#: Clear space kept between neighbouring bar names.
NAME_MARGIN = 6


def name_lines(name: str) -> list[str]:
    return name.split(" ", 1) if " " in name else [name]


def panel_widths(panel_set: Sequence[Panel]) -> list[float]:
    """Panel widths proportional to bar count, so every bar slot is about as wide."""
    total = WIDTH - GAP * (len(panel_set) + 1)
    bars = sum(len(p.bars) for p in panel_set)
    return [total * len(p.bars) / bars for p in panel_set]


def slot_width(panel_w: float, bar_count: int) -> float:
    return (panel_w - 2 * PAD) / bar_count


def name_fit(name: str, slot: float) -> tuple[float, float | None]:
    """(drawn width, forced width). A name wider than its slot minus the margin is
    squeezed to that width with `textLength`, so no font can make neighbours touch."""
    natural = max(len(line) for line in name_lines(name)) * NAME_PX * GLYPH_EM
    cap = slot - NAME_MARGIN
    return (cap, cap) if natural > cap else (natural, None)


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


def _leaked_svg(t: Mapping[str, str], bar: Bar, cx: float, y: float, slot: float) -> str:
    """The small leaked-bytes figure under a top-row bar, capped to the slot like names."""
    if bar.leaked is None:
        return ""
    text = f"{bar.leaked:,}"
    natural = len(text) * SMALL_PX * GLYPH_EM
    cap = slot - NAME_MARGIN
    squeeze = f' textLength="{cap:.1f}" lengthAdjust="spacingAndGlyphs"' if natural > cap else ""
    return (f'<text x="{cx:.1f}" y="{y:.1f}" font-size="{SMALL_PX}" text-anchor="middle" '
            f'fill="{t["sub"]}"{squeeze}>{text}</text>')


def _panel_svg(t: Mapping[str, str], x: float, y: float, w: float, panel: Panel,
               metric: str, better: str, fmt: str, pick: Any, vmax: float,
               show_leaked: bool) -> list[str]:
    o = [
        f'<rect x="{x:.1f}" y="{y}" width="{w:.1f}" height="{PANEL_H}" rx="10" fill="{t["panel"]}"/>',
        f'<text x="{x + 16:.1f}" y="{y + 28}" font-size="16" font-weight="700" fill="{t["text"]}">{escape(panel.title)}</text>',
        f'<text x="{x + w - 16:.1f}" y="{y + 28}" font-size="11" text-anchor="end" fill="{t["sub"]}">{better}</text>',
        f'<text x="{x + 16:.1f}" y="{y + 48}" font-size="12" fill="{t["text"]}">{escape(metric)}</text>',
        f'<text x="{x + 16:.1f}" y="{y + 64}" font-size="11" fill="{t["sub"]}">{escape(panel.dataset)}</text>',
        f'<text x="{x + 16:.1f}" y="{y + 79}" font-size="11" fill="{t["sub"]}">{escape(panel.labels)}</text>',
    ]
    top, bot = y + 108, y + PANEL_H - 66
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
        _, forced = name_fit(bar.name, slot)
        squeeze = f' textLength="{forced:.1f}" lengthAdjust="spacingAndGlyphs"' if forced else ""
        for j, line in enumerate(name_lines(bar.name)):
            longest = line == max(name_lines(bar.name), key=len)
            o.append(f'<text x="{cx:.1f}" y="{bot + 16 + 12 * j}" font-size="{NAME_PX}" '
                     f'font-weight="{weight}" text-anchor="middle" fill="{colour}"'
                     f'{squeeze if longest else ""}>{escape(line)}</text>')
        if show_leaked:
            o.append(_leaked_svg(t, bar, cx, bot + 44, slot))
    if show_leaked:
        o.append(f'<text x="{x + 16:.1f}" y="{y + PANEL_H - 8}" font-size="{SMALL_PX}" '
                 f'fill="{t["sub"]}">Grey figure under each bar: leaked PII bytes</text>')
    return o


def figure_svg(theme: str, panel_set: Sequence[Panel], alt: str) -> str:
    """Row 1: character-level F2 (higher is better). Row 2: false positives (lower is better)."""
    t = THEMES[theme]
    widths = panel_widths(panel_set)
    height = GAP + 2 * (PANEL_H + GAP)
    o = [
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {WIDTH} {height}" '
        f'width="{WIDTH}" height="{height}" font-family="{FONT}" role="img" aria-label="{escape(alt)}">',
        f"<title>{escape(alt)}</title>",
    ]
    for row, (metric, better, fmt, pick) in enumerate((
        (METRIC, "higher is better", "{:.3f}", lambda b: b.f2),
        ("False-positive bytes per 1,000 (own scale)", "lower is better", "{:.1f}",
         lambda b: b.fp_per_1k),
    )):
        x = float(GAP)
        for panel, width in zip(panel_set, widths):
            values = [pick(b) for b in panel.bars if pick(b) is not None]
            vmax = 1.0 if row == 0 else max(values) * 1.15
            o += _panel_svg(t, x, GAP + row * (PANEL_H + GAP), width,
                            panel, metric, better, fmt, pick, vmax, show_leaked=row == 0)
            x += width + GAP
    o.append("</svg>")
    return "\n".join(o) + "\n"


def figure_files(panel_set: Sequence[Panel]) -> dict[str, str]:
    """File name (under the assets directory) -> SVG text."""
    alt = (
        "Benchmark panels for own corpus, Presidio Research and PIIBench-commercial: "
        "character-level F2 (higher is better) with leaked PII bytes under each bar, and "
        "false-positive bytes per 1,000 bytes (lower is better), Gaze release against "
        "declared competitor configurations."
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
    f2 = _table(panel_set, lambda b: b.f2, "{:.3f}", max)
    leaked = _table(panel_set, lambda b: b.leaked, "{:,}", min)
    false_pos = _table(panel_set, lambda b: b.fp_per_1k, "{:.1f}", min)
    own = panel_set[0]
    refused = ", ".join(f"{name} {count:,}" for name, count in own.refused) or "none measured"
    notes = (
        "A document a tool skips counts all its gold characters as missed and all its gold "
        f"bytes as leaked. The declared competitor configurations skipped {own.skipped:,} of "
        f"the own corpus's {own.documents:,} documents and no documents on the third-party "
        "sets. Refused documents are ones Gaze failed closed on instead of cleaning: "
        f"{refused}."
    )
    return "\n".join([
        f"**{METRIC}** (higher is better; best per row in bold):", "", *f2, "",
        "**Leaked PII bytes** (lower is better; best per row in bold):", "", *leaked, "",
        "**False-positive bytes per 1,000 bytes** (lower is better; best per row in bold):",
        "", *false_pos, "", notes,
    ])
