"""Model-free tests for Presidio tuned on Gaze's own corpus."""

from __future__ import annotations

import argparse
import contextlib
import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

import corpus  # noqa: E402
import pool  # noqa: E402
import produce  # noqa: E402
import search  # noqa: E402
import space  # noqa: E402
import tune  # noqa: E402

score = corpus.compare.score


def uid_in(split: str, prefix: str) -> str:
    """A document id that split_for_id assigns to `split`."""
    for n in range(10_000):
        uid = f"{prefix}-{n}"
        if corpus.split_for_id(uid) == split:
            return uid
    raise AssertionError("no id found")


def document(uid: str, text: str, gold: list[tuple[str, str]], language: str = "en") -> object:
    spans = []
    for value, label in gold:
        start = text.encode().index(value.encode())
        spans.append(score.Span(start, start + len(value.encode()), label))
    return score.Document(uid, text, language, "", "synthetic", tuple(spans))


class BlankSpacyEngine:
    """A model-free NLP engine: spaCy's blank English tokenizer, no NER."""

    def __new__(cls):
        import spacy
        from presidio_analyzer.nlp_engine import SpacyNlpEngine

        engine = SpacyNlpEngine(models=[{"lang_code": "en", "model_name": "blank"}])
        engine.nlp = {"en": spacy.blank("en")}
        return engine


class ReplayEqualsEngineTest(unittest.TestCase):
    """The recorded pass replayed offline must equal AnalyzerEngine.analyze."""

    TEXT = ("Call 030 1234567 or mail jane.doe@example.org; account DE89 3704 0044 0532 0130 00, "
            "ref 1234 5678, id AB123456, see jane.doe@example.org")

    def recognizers(self) -> list[tuple[str, object]]:
        from presidio_analyzer import Pattern, PatternRecognizer

        digits = PatternRecognizer(supported_entity="NUMBER", name="Digits",
                                   patterns=[Pattern("run", r"\d{3,}", 0.2), Pattern("long", r"\d{4} \d{4}", 0.5)],
                                   context=["ref", "account"])
        grouped = PatternRecognizer(supported_entity="NUMBER", name="Grouped",
                                    patterns=[Pattern("group", r"\d{4}(?: \d{4})+", 0.6)])
        ident = PatternRecognizer(supported_entity="ID", name="Ident", patterns=[Pattern("id", r"[A-Z]{2}\d{6}", 0.4)])
        return [("Digits", digits), ("Grouped", grouped), ("Ident", ident),
                ("Email", pool.custom_recognizer(
                    {"name": "Email", "entity": "EMAIL_ADDRESS", "case_sensitive": False,
                     "patterns": [("mail", r"\b[\w.]+@[\w.]+\.org\b", 0.9)], "context": []}, "en"))]

    def engine(self, members, thresholds, context: str) -> object:
        from presidio_analyzer import AnalyzerEngine, RecognizerRegistry

        registry = RecognizerRegistry(supported_languages=["en"])
        for unit, recognizer in members:
            recognizer.score_thresholds = {"default": 0.0, **{
                key.split("|", 1)[1]: value for key, value in thresholds.items() if key.startswith(unit + "|")}}
            registry.add_recognizer(recognizer)
        return AnalyzerEngine(nlp_engine=BlankSpacyEngine(), registry=registry, supported_languages=["en"],
                              context_aware_enhancer=pool.enhancer(context), default_score_threshold=0.0)

    def test_every_subset_threshold_and_allow_list(self) -> None:
        members = self.recognizers()
        recorder = self.engine(members, {}, "default")
        units = {recognizer.id: unit for unit, recognizer in members}
        enhancers = {mode: pool.enhancer(mode) for mode in space.CONTEXT_MODES}
        rows = produce.record_document(recorder, units, enhancers, self.TEXT, "en", True)
        found = [search.Found(s, e, entity, unit, tuple(scores)) for s, e, entity, unit, *scores in rows]
        doc = search.Doc(uid_in("validation", "replay"), "C", "en",
                         document("x", self.TEXT, []), tuple(found))
        mapping = {"NUMBER": (), "ID": (), "EMAIL_ADDRESS": ("EMAIL",)}
        replayer = search.Replayer([doc], mapping, mapping)
        cases = [
            ({"Digits", "Grouped", "Ident", "Email"}, {}, "default", []),
            ({"Digits", "Ident"}, {"Digits|NUMBER": 0.3}, "off", []),
            ({"Digits", "Grouped"}, {"Grouped|NUMBER": 0.7}, "wide", ["1234 5678"]),
            ({"Email", "Ident"}, {"Ident|ID": 0.4}, "default", ["jane.doe@example.org"]),
        ]
        for active, thresholds, context, allow in cases:
            with self.subTest(active=sorted(active), context=context):
                config = {"artifact_ner": "none", "extra": {key: "off" for key in space.EXTRA_NER},
                          "scope": {unit: ("all" if unit in active else "off") for unit, _ in members},
                          "context": context, "thresholds": thresholds, "allow_list": allow}
                live = self.engine([(u, r) for u, r in self.recognizers() if u in active], thresholds, context)
                expected = live.analyze(text=self.TEXT, language="en", score_threshold=None,
                                        allow_list=allow or None, allow_list_match="exact")
                with mock.patch.object(pool, "predefined_units", return_value={}):
                    replayed = replayer.kept(0, config, replayer.included(0, config))
                self.assertEqual(sorted((r.start, r.end, r.entity_type, r.score) for r in replayed),
                                 sorted((r.start, r.end, r.entity_type, r.score) for r in expected))


class SplitGuardTest(unittest.TestCase):
    def test_search_refuses_a_test_half_document(self) -> None:
        doc = search.Doc(uid_in("test", "guard"), "C", "en", document("x", "text", []), ())
        with self.assertRaises(corpus.SplitGuardError):
            search.Search(search.Replayer([doc], {}, {}), "leak-first", lambda row: None)

    def test_validation_records_carrying_a_test_id_are_refused(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            Path(tmp, "base.validation.jsonl").write_text(
                json.dumps({"uid": uid_in("test", "leak"), "layer": "C", "r": []}) + "\n", encoding="utf-8")
            with self.assertRaises(corpus.SplitGuardError):
                search.read_pool(Path(tmp), ["base"], "validation")

    def test_select_never_opens_the_test_half(self) -> None:
        """Runs the whole selection over a synthetic corpus whose test-half records are
        unreadable (directories) and whose layers mix both halves."""
        text_hit = "Mail anna@example.org or call +49 30 1234567 today."
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            pool_dir = root / "pool"
            pool_dir.mkdir()
            layers = {layer: [] for layer in corpus.LAYERS}
            rows = []
            for n in range(4):
                for split in ("validation", "test"):
                    uid = uid_in(split, f"sel{n}")
                    layers["C"].append(document(uid, text_hit, [("anna@example.org", "EMAIL"),
                                                                ("+49 30 1234567", "PHONENUMBER")]))
                    if split == "validation":
                        start = text_hit.index("anna@")
                        phone = text_hit.index("+49")
                        rows.append({"uid": uid, "layer": "C", "r": [
                            [start, start + 16, "EMAIL_ADDRESS", "EmailRecognizer", 1.0, 1.0, 1.0],
                            [phone, phone + 14, "PHONE_NUMBER", "CustomPhoneRecognizer", 0.5, 0.5, 0.5],
                            [0, 4, "PERSON", pool.SPACY_UNIT, 0.85, 0.85, 0.85]]})
            space_sha = hashlib.sha256((Path(tune.HERE) / "space.py").read_bytes()).hexdigest()
            for name in tune.POOL_PASSES:
                body = "".join(json.dumps(row) + "\n" for row in rows) if name == "base" else ""
                (pool_dir / f"{name}.validation.jsonl").write_text(body, encoding="utf-8")
                (pool_dir / f"{name}.test.jsonl").mkdir()  # opening it raises
                (pool_dir / f"{name}.meta.json").write_text(json.dumps({
                    "space_sha256": space_sha, "harness_dirty": False, "smoke": None, "models": {},
                    "harness_revision": "x",
                    "sha256": {"validation": hashlib.sha256(body.encode()).hexdigest(), "test": "unread"},
                }), encoding="utf-8")
            comparison = {"corpus": {}, "contracts": {}}
            patches = [
                mock.patch.object(corpus, "read_comparison", return_value=comparison),
                mock.patch.object(corpus, "load_measured", return_value=layers),
                mock.patch.object(corpus, "measured_agentic_contract", lambda _c: contextlib.nullcontext()),
                mock.patch.object(tune, "SEARCH_LOG", root / "log.jsonl.gz"),
                mock.patch.object(tune, "SELECTION", root / "selection.json"),
                mock.patch.object(tune, "git_state", lambda: {"harness_revision": "x", "harness_dirty": False}),
                mock.patch.dict(space.SEARCH, {"max_rounds": 2}),
                mock.patch.dict("os.environ", {"PYTHONHASHSEED": "0"}),
            ]
            with contextlib.ExitStack() as stack:
                for patch in patches:
                    stack.enter_context(patch)
                tune.select(argparse.Namespace(dataset=root / "unused.parquet", pool=pool_dir))
            selection = json.loads((root / "selection.json").read_text(encoding="utf-8"))
            self.assertEqual(selection["validation_documents"], {"C": 4, "A": 0, "D": 0, "R": 0})
            for objective in space.OBJECTIVES:
                choice = selection["choices"][objective]
                final = choice["finals"][choice["start"]]
                self.assertEqual(final["validation"]["leaked_bytes"], 0, objective)
                self.assertEqual(final["config"]["scope"]["CustomPhoneRecognizer"], "all", objective)
            # The spaCy PERSON false positive (4 bytes per document) is searched away.
            self.assertEqual(selection["choices"]["leak-first"]["finals"]["everything"]["validation"]
                             ["false_positive_bytes"], 0)


class DeclarationTest(unittest.TestCase):
    def test_every_pool_entity_has_a_reviewed_label_mapping(self) -> None:
        mapping = tune.tuned_mapping()
        entities = pool.pool_entities() | set(space.GLINER_LABELS.values())
        self.assertEqual(sorted(entities - set(mapping)), [])

    def test_every_openmed_model_label_is_mapped(self) -> None:
        """OpenMed's labels, read from its model card configuration's label names."""
        labels = {"api_key", "http_cookie", *pool.openmed_mapping()}
        mapping = tune.tuned_mapping()
        openmed = pool.openmed_mapping()
        for label in labels:
            self.assertIn(openmed.get(label, label), mapping, label)

    def test_gliner_label_set_covers_every_pool_entity(self) -> None:
        extras = pool.Extras(pool.ModelPaths(spacy={}), pool.pool_entities())
        values = set(extras.gliner_mapping().values())
        self.assertEqual(sorted(pool.pool_entities() - values), [])

    def test_custom_patterns_compile_case_as_declared(self) -> None:
        recognizer = pool.custom_recognizer(
            next(spec for spec in space.CUSTOM_RECOGNIZERS if spec["name"] == "CustomStreetRecognizer"), "en")
        found = recognizer.analyze("She lives at 42 Elmwood Crescent in Boise.", ["STREET_ADDRESS"])
        self.assertIn("42 Elmwood Crescent", {"She lives at 42 Elmwood Crescent in Boise."[r.start:r.end] for r in found})
        self.assertEqual(recognizer.analyze("she lives at 42 elmwood crescent", ["STREET_ADDRESS"]), [])


class ReportTest(unittest.TestCase):
    @staticmethod
    def rows(f2s: dict[str, float]) -> dict:
        return {name: {"contracts": {"v3": {"C": {"metrics": {"product_coverage": {"test": {
            "char_level": {"f2": value}}}}}}}} for name, value in f2s.items()}

    def test_chart_shows_the_better_tuned_row_and_ties_keep_the_authors_setup(self) -> None:
        names = tune.ROWS
        chosen = tune.chart_choice(self.rows({names["authors"]: 0.7, names["leak-first"]: 0.6, names["f2"]: 0.8}))
        self.assertEqual(chosen["row"], names["f2"])
        tied = tune.chart_choice(self.rows({names["authors"]: 0.8, names["leak-first"]: 0.8, names["f2"]: 0.8}))
        self.assertEqual(tied["row"], names["authors"])

    def test_anchor_reports_every_byte_mismatch(self) -> None:
        cell = {"leaked_bytes": 5, "false_positive_bytes": 7, "gold_gap_protected_bytes": 0,
                "false_positive_bytes_after_gold_gap": None, "documents": 1}
        committed = {version: {layer: dict(cell) for layer in corpus.LAYERS} for version in corpus.compare.CONTRACTS}
        measured = json.loads(json.dumps(committed))
        self.assertTrue(tune.anchor({"contracts": measured}, {"tools": {"presidio-all": {"contracts": committed}}})["equal"])
        measured["v2"]["A"]["leaked_bytes"] = 6
        result = tune.anchor({"contracts": measured}, {"tools": {"presidio-all": {"contracts": committed}}})
        self.assertEqual(result["mismatches"], ["v2/A/leaked_bytes: 6 != 5"])


class ChartBarTest(unittest.TestCase):
    def test_own_panel_presidio_bar_becomes_the_chosen_tuned_row(self) -> None:
        sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
        import benchmark_charts as charts

        def cell(f2: float, leaked: int) -> dict:
            return {"false_positive_bytes_after_gold_gap": 10, "false_positive_bytes": 12, "skipped_documents": 0,
                    "metrics": {"product_coverage": {"full": {
                        "char_level": {"f2": f2}, "leaked_bytes": leaked, "false_positive_bytes": 12,
                        "total_bytes": 1000}}}}
        comparison = {
            "corpus": {"layers": {"C": {"documents": 4}}},
            "heldout_split": {"layers": {"C": {"validation": {}, "test": {}}}},
            "tools": {"presidio-strong": {"contracts": {"v3": {"C": cell(0.5, 90)}}},
                      "gliner": {"contracts": {"v3": {"C": cell(0.6, 80)}}}},
            "presidio_tuned": {"chart": {"row": "presidio-tuned-own-f2"},
                               "rows": {"presidio-tuned-own-f2": {"contracts": {"v3": {"C": cell(0.7, 40)}}}}},
        }
        panel = charts.own_panel([], comparison, {"presidio": "presidio-strong", "gliner": "gliner"}, "C")
        presidio, gliner = panel.bars
        self.assertEqual((presidio.name, presidio.column, presidio.f2, presidio.leaked),
                         ("Presidio (tuned)", "Presidio", 0.7, 40))
        self.assertEqual((gliner.name, gliner.f2), ("GLiNER", 0.6))
        self.assertIn("validation half", panel.caption)
        del comparison["presidio_tuned"]
        self.assertEqual(charts.own_panel([], comparison, {"presidio": "presidio-strong"}, "C").bars[0].f2, 0.5)


if __name__ == "__main__":
    unittest.main()
