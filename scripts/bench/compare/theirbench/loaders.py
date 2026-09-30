"""Load competitors' own benchmarks into the Gaze byte scorer's documents.

Report-only: these sets are never used to design or tune Gaze rules. Nothing
here writes document text or spans anywhere; callers keep aggregates only.
"""

from __future__ import annotations

import hashlib
import json
import subprocess
import sys
from pathlib import Path
from typing import Mapping, Sequence

BENCH = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(BENCH))

import gaze_bench_score as score  # noqa: E402

LABEL_MAPS = Path(__file__).with_name("label-maps.json")
PRESIDIO_RESEARCH = {
    "repository": "microsoft/presidio-research",
    "commit": "6db3769a3388b4075b93ab2229c5e0b9c30137f7",
    "file": "data/synth_dataset_v2.json",
    "sha256": "ec08a771ba8135314cafb60752b2295212222ba3a4cd75d73811839c699e0012",
    "documents": 1500,
    "licence": "MIT code; Fake Name Generator identities CC-BY-SA-3.0-US",
}
PII_TRACE = {
    "repository": "perplexity-ai/PII-TRACE",
    "revision": "1c3eb67bbd43e8571b4433a9d3d76271f9970eb9",
    "file": "data/train.parquet",
    "sha256": "1f37039ca2a9c7a93d7b755e1603d5192424c61e61fcbf2ce9405f26705fd670",
    "conversations": 500,
    "messages": 4500,
    "gold_spans": 2653,
    "licence": "MIT",
    "scope": "the only published part of PII-TRACE: 500 English conversations; the paper's "
             "13,148-conversation, 13-language set (1,922-document test split) is not public",
}


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def checkout_commit(checkout: Path) -> str:
    return subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=checkout, text=True).strip()


def load_presidio_research(checkout: Path) -> tuple[list[score.Document], dict[str, object]]:
    if checkout_commit(checkout) != PRESIDIO_RESEARCH["commit"]:
        raise ValueError(f"presidio-research checkout is not at {PRESIDIO_RESEARCH['commit']}")
    path = checkout / PRESIDIO_RESEARCH["file"]
    digest = sha256(path)
    if digest != PRESIDIO_RESEARCH["sha256"]:
        raise ValueError(f"{PRESIDIO_RESEARCH['file']}: SHA-256 {digest} differs from its pin")
    documents = []
    for index, row in enumerate(json.loads(path.read_text(encoding="utf-8"))):
        text = row["full_text"]
        offsets = score.char_to_byte_offsets(text)
        spans = []
        for span in row["spans"]:
            start, end = span["start_position"], span["end_position"]
            if not 0 <= start < end <= len(text) or text[start:end] != span["entity_value"]:
                raise ValueError(f"document {index}: gold span does not match its text")
            spans.append(score.Span(offsets[start], offsets[end], span["entity_type"]))
        documents.append(score.Document(
            uid=f"presidio-research/{index}", text=text, language="en", region="",
            source_dataset="presidio-research", spans=tuple(spans),
        ))
    if len(documents) != PRESIDIO_RESEARCH["documents"]:
        raise ValueError("presidio-research document count differs from its pin")
    return documents, {**PRESIDIO_RESEARCH, "sha256": digest}


def load_label_maps() -> dict[str, object]:
    return json.loads(LABEL_MAPS.read_text(encoding="utf-8"))


def compose_mapping(
    tool: str,
    tool_map: Mapping[str, Sequence[str]],
    benchmark: str,
    maps: Mapping[str, object] | None = None,
) -> dict[str, tuple[str, ...]]:
    """Map each tool label to the benchmark's native gold labels it covers.

    A tool label covers a gold label when their canonical label sets share at
    least one label, the same generous overlap rule the main comparison uses.
    """
    maps = maps or load_label_maps()
    gold = maps["gold"][benchmark]
    identity = maps.get("native_gold", {}).get(benchmark, {}).get(tool)
    if identity is not None:
        # The tool's own labels ARE this benchmark's labels (its author trained on that taxonomy),
        # so the canonical detour would wrongly drop classes such as other_pii.
        if set(identity) != set(tool_map) or any(set(v) - set(gold) for v in identity.values()):
            raise ValueError(f"{tool}: native_gold must name exactly the tool's labels and known {benchmark} labels")
        return {label: tuple(sorted(natives)) for label, natives in identity.items()}
    extension = maps["tool_extension"].get(tool, {})
    known = set(maps["extension_labels"])
    unknown = sorted({label for labels in gold.values() for label in labels}
                     - known - _main_corpus_labels())
    if unknown:
        raise ValueError(f"{benchmark}: gold maps to unknown canonical labels {unknown}")
    stray = sorted(set(extension) - set(tool_map))
    if stray:
        raise ValueError(f"{tool}: extension names labels the tool never emits: {stray}")
    composed = {}
    for label, canonical in tool_map.items():
        covered = set(canonical) | set(extension.get(label, ()))
        composed[label] = tuple(sorted(native for native, targets in gold.items() if covered & set(targets)))
    return composed


def _main_corpus_labels() -> set[str]:
    raw = json.loads((BENCH / "compare/label-map.json").read_text(encoding="utf-8"))
    return {label for table in raw.values() for labels in table.values() for label in labels}


def bio_spans(tokens: Sequence[str], labels: Sequence[str]) -> list[tuple[int, int, str]]:
    """Character spans over " ".join(tokens), with seqeval's chunk rules:
    an I- tag that does not continue a same-type chunk starts a new one."""
    if len(tokens) != len(labels):
        raise ValueError("token and label counts differ")
    spans: list[list[object]] = []
    position, previous = 0, "O"
    for token, tag in zip(tokens, labels):
        start, end = position, position + len(token)
        position = end + 1
        kind = tag[2:] if tag[:2] in ("B-", "I-") else None
        if tag != "O" and kind is None:
            raise ValueError(f"unsupported tag {tag!r}")
        continues = tag.startswith("I-") and previous != "O" and previous[2:] == kind
        if kind is not None and continues:
            spans[-1][1] = end
        elif kind is not None:
            spans.append([start, end, kind])
        previous = tag
    return [(start, end, kind) for start, end, kind in spans]


def load_piibench_commercial(data: Path) -> tuple[dict[str, list[score.Document]], dict[str, object]]:
    manifest = json.loads((data / "manifest.json").read_text(encoding="utf-8"))
    path = data / "data/test_5k.jsonl"
    if sha256(path) != manifest["files"]["test_5k.jsonl"]:
        raise ValueError("test_5k.jsonl differs from its build manifest")
    documents = []
    for index, line in enumerate(path.read_text(encoding="utf-8").splitlines()):
        row = json.loads(line)
        text = " ".join(row["tokens"])
        offsets = score.char_to_byte_offsets(text)
        spans = tuple(score.Span(offsets[s], offsets[e], label)
                      for s, e, label in bio_spans(row["tokens"], row["labels"]))
        documents.append(score.Document(
            uid=f"piibench-commercial/{index}", text=text, language="en", region="",
            source_dataset=f"piibench/{row['source']}", spans=spans,
        ))
    identity = {"piibench": manifest["piibench"], "sources": manifest["sources"],
                "excluded": manifest["excluded"], "published_full_mix": manifest["published_full_mix"],
                "test_5k_sha256": manifest["files"]["test_5k.jsonl"], "documents": len(documents)}
    return {"test": documents}, identity


def pii_trace_messages(path: Path) -> list[tuple[str, str, list[tuple[int, int, str]]]]:
    """(uid, text, character-offset gold spans) per message: user then assistant, in file order."""
    import pyarrow.parquet as pq

    digest = sha256(path)
    if digest != PII_TRACE["sha256"]:
        raise ValueError(f"{path.name}: SHA-256 {digest} differs from its pin")
    messages = []
    for row in pq.read_table(path).to_pylist():
        by_message: dict[tuple[int, str], list[dict]] = {}
        for span in row["spans"]:
            by_message.setdefault((span["turn"], span["source"]), []).append(span)
        for turn in row["turns"]:
            for source in ("user", "assistant"):
                text = turn[source]
                spans = []
                for span in by_message.pop((turn["turn"], source), []):
                    start, end = span["start"], span["end"]
                    if not 0 <= start < end <= len(text) or text[start:end] != span["text"]:
                        raise ValueError(f"{row['id']}: gold span does not match its text")
                    spans.append((start, end, span["label"]))
                messages.append((f"pii-trace/{row['id']}/{turn['turn']}/{source}", text, spans))
        if by_message:
            raise ValueError(f"{row['id']}: gold spans point at a message that does not exist")
    if (len(messages), sum(len(spans) for _, _, spans in messages)) != (PII_TRACE["messages"], PII_TRACE["gold_spans"]):
        raise ValueError("PII-TRACE message or span count differs from its pin")
    return messages


def load_pii_trace(path: Path) -> tuple[dict[str, list[score.Document]], dict[str, object]]:
    """One document per message, user and assistant, keyed like the dataset's own span identity
    (conversation, turn, source). All 2,653 gold spans sit in user messages; assistant messages
    carry none, so a detection there is a false positive under the dataset's exact-span protocol."""
    documents = []
    for uid, text, spans in pii_trace_messages(path):
        offsets = score.char_to_byte_offsets(text)
        documents.append(score.Document(
            uid=uid, text=text, language="en", region="", source_dataset="pii-trace",
            spans=tuple(score.Span(offsets[start], offsets[end], label) for start, end, label in spans),
        ))
    return {"test": documents}, {**PII_TRACE, "sha256": sha256(path), "documents": len(documents)}
