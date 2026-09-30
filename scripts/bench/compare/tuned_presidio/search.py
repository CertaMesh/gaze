"""Offline replay of candidate Presidio configurations, and the declared search.

A candidate's predictions for a document are rebuilt from the recorded raw
findings (`produce.py pool`) exactly as `AnalyzerEngine.analyze` would finish
them: keep the results of the configuration's recognizers, apply its per
recognizer and entity thresholds to the score under its context mode, run
Presidio's own `EntityRecognizer.remove_duplicates`, drop allow-listed texts,
then resolve overlaps with the comparison's resolver
(`compare.resolved_presidio_spans`). Each recognizer's results depend only on the
text and the NLP artifacts, so a recorded pass can stand in for any subset of it;
`tune.py measure` checks this by running the chosen configurations live.

`select` reads validation records and validation documents only. It never opens
a test-half file, and every document it scores passes `corpus.require_validation`.
"""

from __future__ import annotations

import copy
import json
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Iterable, Mapping, Sequence

import corpus
import pool
import space
from comparison_metrics import ComparisonMetrics, f_beta

MODES = tuple(space.CONTEXT_MODES)
CUSTOM_UNITS = tuple(spec["name"] for spec in space.CUSTOM_RECOGNIZERS) + (space.PHONE_WIDE["name"],)


@dataclass(frozen=True)
class Found:
    start: int
    end: int
    entity: str
    unit: str
    scores: tuple[float, float, float]  # one per context mode, in MODES order


@dataclass
class Doc:
    uid: str
    layer: str
    language: str
    document: object  # the contract-applied score.Document
    results: tuple[Found, ...]


def read_pool(pool_dir: Path, passes: Sequence[str], split: str) -> dict[str, list[Found]]:
    """uid -> recorded results of every pass, for one half."""
    found: dict[str, list[Found]] = {}
    for name in passes:
        path = pool_dir / f"{name}.{split}.jsonl"
        with path.open(encoding="utf-8") as handle:
            for line in handle:
                row = json.loads(line)
                if split == "validation":
                    corpus.require_validation([row["uid"]])
                found.setdefault(row["uid"], []).extend(
                    Found(start, end, entity, unit, tuple(scores))
                    for start, end, entity, unit, *scores in row["r"])
    return found


def make_docs(layers: Mapping[str, Sequence], found: Mapping[str, list[Found]],
              contracts: Mapping[str, object]) -> list[Doc]:
    docs = []
    for layer, documents in layers.items():
        contract = contracts["agentic" if layer in {"A", "D", "R"} else "v3"]
        for document in documents:
            applied = corpus.compare.score.apply_scored_label_contract([document], contract)[0]
            results = tuple(sorted(found.get(document.uid, ()), key=lambda f: (f.start, f.end, f.entity, f.unit)))
            docs.append(Doc(document.uid, layer, document.language, applied, results))
    return docs


# --------------------------------------------------------------------------
# Configurations


def start_config(name: str) -> dict[str, object]:
    natives = pool.predefined_units()
    if name == "presidio-default":
        # compare.py's presidio-all row: default registry for five languages plus the nine German recognizers.
        from presidio_analyzer import RecognizerRegistry

        default = RecognizerRegistry(supported_languages=list(space.LANGUAGES))
        default.load_predefined_recognizers(languages=list(space.LANGUAGES))
        on = {type(r).__name__ for r in default.recognizers} | set(space.PRESIDIO_ALL_GERMAN)
        scopes = {unit: "native" if unit in on else "off" for unit in natives}
        scopes.update({unit: "off" for unit in CUSTOM_UNITS})
        return {"artifact_ner": "spacy", "extra": {key: "off" for key in space.EXTRA_NER},
                "scope": scopes, "context": "default", "thresholds": {},
                "allow_min_documents": None, "allow_list": []}
    if name == "everything":
        scopes = {unit: "all" for unit in natives}
        scopes.update({unit: "all" for unit in CUSTOM_UNITS})
        return {"artifact_ner": "spacy", "extra": {key: "all" for key in space.EXTRA_NER},
                "scope": scopes, "context": "default", "thresholds": {},
                "allow_min_documents": None, "allow_list": []}
    raise ValueError(f"unknown start {name}")


# --------------------------------------------------------------------------
# Replay


class Replayer:
    """Predictions and per-document metrics of a configuration, memoized per document."""

    def __init__(self, docs: Sequence[Doc], mapping: Mapping[str, Sequence[str]],
                 typed_mapping: Mapping[str, Sequence[str]]) -> None:
        from presidio_anonymizer import AnonymizerEngine

        self.docs = list(docs)
        self.mapping = mapping
        self.typed_mapping = typed_mapping
        self.anonymizer = AnonymizerEngine()
        self._cache: list[dict[tuple, tuple[int, int, int, int, int]]] = [{} for _ in self.docs]
        self._allow_ids: dict[frozenset, int] = {}

    def included(self, index: int, config: Mapping[str, object]) -> tuple[int, ...]:
        doc = self.docs[index]
        mode = MODES.index(config["context"])
        thresholds = config["thresholds"]
        return tuple(
            i for i, found in enumerate(doc.results)
            if pool.unit_active(config, found.unit, doc.language)
            and found.scores[mode] >= thresholds.get(pool.threshold_key(found.unit, found.entity), 0.0)
        )

    def kept(self, index: int, config: Mapping[str, object], chosen: tuple[int, ...]) -> list:
        """Presidio's post-threshold steps: de-duplicate, then the allow list."""
        from presidio_analyzer import EntityRecognizer, RecognizerResult

        doc = self.docs[index]
        mode = MODES.index(config["context"])
        results = [RecognizerResult(doc.results[i].entity, doc.results[i].start, doc.results[i].end,
                                    doc.results[i].scores[mode]) for i in chosen]
        results = EntityRecognizer.remove_duplicates(results)
        allow = set(config["allow_list"])
        if allow:
            text = doc.document.text
            results = [r for r in results if text[r.start:r.end] not in allow]
        return results

    def predict(self, index: int, config: Mapping[str, object]) -> list:
        doc = self.docs[index]
        chosen = self.included(index, config)
        return corpus.compare.resolved_presidio_spans(self.anonymizer, doc.document.text,
                                                      self.kept(index, config, chosen))

    def metrics(self, index: int, config: Mapping[str, object]) -> tuple[int, int, int, int, int]:
        """(leaked bytes, false-positive bytes, char tp, char fp, char fn) of one document."""
        allow = frozenset(config["allow_list"])
        key = (config["context"], self.included(index, config), self._allow_ids.setdefault(allow, len(self._allow_ids)))
        cached = self._cache[index].get(key)
        if cached is None:
            cell = ComparisonMetrics(self.mapping, None, self.typed_mapping)
            cell.add(self.docs[index].document, self.predict(index, config))
            cached = (cell.leaked_bytes, cell.false_positive_bytes, cell.char_tp, cell.char_fp, cell.char_fn)
            self._cache[index][key] = cached
        return cached


def objective_key(objective: str, totals: Sequence[int]) -> tuple:
    leaked, fp, tp, cfp, cfn = totals
    if objective == "leak-first":
        return (leaked, fp)
    if objective == "f2":
        precision = tp / (tp + cfp) if tp + cfp else 0.0
        recall = tp / (tp + cfn) if tp + cfn else 0.0
        return (-f_beta(precision, recall, 2), leaked)
    raise ValueError(f"unknown objective {objective}")


def f2_of(totals: Sequence[int]) -> float:
    return -objective_key("f2", totals)[0]


# --------------------------------------------------------------------------
# Coordinate descent


class Search:
    def __init__(self, replayer: Replayer, objective: str, log: Callable[[dict], None]) -> None:
        corpus.require_validation([doc.uid for doc in replayer.docs])
        self.replayer = replayer
        self.objective = objective
        self.log = log
        docs = replayer.docs
        self.by_unit: dict[str, set[int]] = {}
        self.by_threshold: dict[str, set[int]] = {}
        self.scores: dict[str, tuple[set[float], ...]] = {}
        for index, doc in enumerate(docs):
            for found in doc.results:
                key = pool.threshold_key(found.unit, found.entity)
                self.by_unit.setdefault(found.unit, set()).add(index)
                self.by_threshold.setdefault(key, set()).add(index)
                for mode, value in zip(self.scores.setdefault(key, tuple(set() for _ in MODES)), found.scores):
                    mode.add(value)
        self.all_docs = set(range(len(docs)))
        self.evaluations = 0

    def totals(self, config: Mapping[str, object]) -> tuple[list[tuple], list[int]]:
        per_doc = [self.replayer.metrics(i, config) for i in range(len(self.replayer.docs))]
        return per_doc, [sum(values) for values in zip(*per_doc)] if per_doc else [0] * 5

    def run(self, start: str) -> tuple[dict[str, object], list[int]]:
        self.config = start_config(start)
        per_doc, totals = self.totals(self.config)
        self._record(start, 0, "start", totals, True)
        for round_number in range(1, space.SEARCH["max_rounds"] + 1):
            improved = False
            # `dimensions` reads self.config lazily, so each dimension varies the current incumbent.
            for label, affected, alternatives in self.dimensions():
                best = None
                for move, candidate in alternatives:
                    new_totals, new_rows = self._delta(candidate, per_doc, totals, affected)
                    better = objective_key(self.objective, new_totals) < objective_key(
                        self.objective, (best[1] if best else totals))
                    self._record(start, round_number, f"{label}={move}", new_totals, False)
                    if better:
                        best = (candidate, new_totals, new_rows, move)
                if best is not None:
                    self.config, totals = best[0], best[1]
                    for index, row in best[2].items():
                        per_doc[index] = row
                    self._record(start, round_number, f"accept {label}={best[3]}", totals, True)
                    improved = True
            if not improved:
                break
        return self.config, totals

    def _delta(self, candidate: Mapping[str, object], per_doc: list[tuple], totals: list[int],
               affected: Iterable[int]) -> tuple[list[int], dict[int, tuple]]:
        new_totals = list(totals)
        rows = {}
        for index in affected:
            row = self.replayer.metrics(index, candidate)
            if row != per_doc[index]:
                rows[index] = row
                for k in range(5):
                    new_totals[k] += row[k] - per_doc[index][k]
        self.evaluations += 1
        return new_totals, rows

    def _record(self, start: str, round_number: int, move: str, totals: Sequence[int], accepted: bool) -> None:
        self.log({"objective": self.objective, "start": start, "round": round_number, "move": move,
                  "leaked_bytes": totals[0], "false_positive_bytes": totals[1],
                  "char_f2": round(f2_of(totals), 6), "accepted": accepted})

    def dimensions(self):
        """Yield (label, affected documents, [(move, candidate config)]) in the declared order.

        Each dimension is built from `self.config` when it is reached, so a move
        accepted earlier in the round is already in place.
        """
        def variant(**changes: object) -> dict[str, object]:
            candidate = copy.deepcopy(self.config)
            candidate.update(changes)
            return candidate

        ner_docs = self.by_unit.get(pool.SPACY_UNIT, set()) | self.by_unit.get(pool.DSLIM_UNIT, set())
        yield ("artifact_ner", ner_docs,
               [(value, variant(artifact_ner=value)) for value in space.ARTIFACT_NER if value != self.config["artifact_ner"]])
        for key, unit in pool.EXTRA_UNITS.items():
            yield (f"extra[{key}]", self.by_unit.get(unit, set()),
                   [(scope, variant(extra={**self.config["extra"], key: scope}))
                    for scope in space.EXTRA_NER_SCOPES if scope != self.config["extra"][key]])
        natives = pool.predefined_units()
        for unit in sorted(natives):
            if unit not in self.by_unit:
                continue  # no validation result: every scope scores the same
            scopes = [s for s in space.PATTERN_SCOPES if s != self.config["scope"][unit]
                      and not (s == "native" and not natives[unit])]
            yield (f"scope[{unit}]", self.by_unit[unit],
                   [(scope, variant(scope={**self.config["scope"], unit: scope})) for scope in scopes])
        for unit in CUSTOM_UNITS:
            if unit not in self.by_unit:
                continue
            other = "off" if self.config["scope"][unit] == "all" else "all"
            yield (f"scope[{unit}]", self.by_unit[unit], [(other, variant(scope={**self.config["scope"], unit: other}))])
        yield ("context", self.all_docs,
               [(mode, variant(context=mode)) for mode in MODES if mode != self.config["context"]])
        for key in sorted(self.by_threshold):
            unit = key.split("|", 1)[0]
            docs = self.by_threshold[key]
            if not any(pool.unit_active(self.config, unit, self.replayer.docs[i].language) for i in docs):
                continue
            current = self.config["thresholds"].get(key, 0.0)
            scores = self.scores[key][MODES.index(self.config["context"])]
            # A value no recorded score separates from the current one keeps every result as is.
            values = [value for value in space.THRESHOLD_GRID if value != current and any(
                min(value, current) <= score < max(value, current) for score in scores)]
            yield (f"threshold[{key}]", docs,
                   [(value, variant(thresholds={**self.config["thresholds"], key: value})) for value in values])
        yield ("allow_list", self.all_docs, self.allow_alternatives(self.config))

    def allow_alternatives(self, config: Mapping[str, object]) -> list[tuple[object, dict[str, object]]]:
        """Allow lists learned from the incumbent's validation false positives."""
        score = corpus.compare.score
        base = dict(config, allow_list=[])
        seen: dict[str, set[int]] = {}
        touches_gold: set[str] = set()
        for index, doc in enumerate(self.replayer.docs):
            text = doc.document.text
            offsets = score.char_to_byte_offsets(text)
            gold = score.merge_intervals((s.start, s.end) for s in doc.document.spans)
            for result in self.replayer.kept(index, base, self.replayer.included(index, base)):
                value = text[result.start:result.end]
                span = [(offsets[result.start], offsets[result.end])]
                if score.intersection_length(gold, span):
                    touches_gold.add(value)
                else:
                    seen.setdefault(value, set()).add(index)
        alternatives = []
        for minimum in space.ALLOW_LIST_MIN_DOCUMENTS:
            if minimum == config["allow_min_documents"]:
                continue
            allow = [] if minimum is None else sorted(
                value for value, docs in seen.items() if len(docs) >= minimum and value not in touches_gold)
            alternatives.append((minimum, dict(config, allow_min_documents=minimum, allow_list=allow)))
        return alternatives
