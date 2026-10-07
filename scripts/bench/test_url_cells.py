"""Model-free specification of source-byte URL gold, never detector output."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import url_cells as urls

PINNED_URL_SHA256 = {'dev': '7d122c171d3e74443198542daccd3b2e5ebe73e491e0923c2439864a0988bbaa', 'test': '09bb55bb163bf59303b3bd2197b15b19873697924aaad98e8e1d45f093aefb1e'}


class RawSpanTests(unittest.TestCase):
    def test_compact_json_keeps_escaped_spelling_and_unicode_byte_offsets(self):
        raw = r"https:\/\/portal.example.invalid\/users\/alice\/"
        text, gold, decoys = urls.render("test", "tool_json", (raw,))
        expected = 'é:{"account":{"link":"' + raw + '"},"amount":81.9,"status":"open"}'
        self.assertEqual(text, expected)
        self.assertEqual([(g.start, g.end, g.label, g.value) for g in gold],
                         [(23, 23 + len(raw.encode()), "URL", raw)])
        self.assertEqual(decoys, ())

    def test_surfaces_keep_quotes_punctuation_and_repeats_outside_gold(self):
        value = "https://portal.example.invalid/users/alice"
        cases = {
            "html_double": ('é:<a href="', '">Account</a> units=81.9'),
            "html_single": ("é:<a href='", "' rel='next'>Account</a> units=81.9"),
            "markdown": ('é:[account](', ') count=81.9'),
            "prose": ('é:Account link (', '). Count 81.9 stays visible.'),
            "log_kv": ('é:event=account link="', '" units=81.9 result=open'),
        }
        for surface, (prefix, suffix) in cases.items():
            with self.subTest(surface=surface):
                self.assertIn(surface, urls.TEMPLATES)
                text, gold, _ = urls.render("test", surface, (value,))
                self.assertEqual(text, prefix + value + suffix)
                self.assertEqual(text.encode()[gold[0].start:gold[0].end], value.encode())
        text, gold, _ = urls.render("test", "tool_json", (value, value))
        expected = ('é:{"account":{"link":"' + value + '","again":"' + value
                    + '"},"amount":81.9,"status":"open"}')
        self.assertEqual(text, expected)
        self.assertEqual(len(gold), 2)
        self.assertNotEqual(gold[0].start, gold[1].start)
        self.assertEqual([text.encode()[g.start:g.end].decode() for g in gold], [value, value])


class GeneratedCellTests(unittest.TestCase):
    def test_generated_layers_cover_raw_spellings_and_benign_counterweights(self):
        records = urls.generate("test")
        self.assertEqual({r.layer for r in records}, {"A", "D", "R"})
        positives = [r for r in records if r.layer == "A"]
        negatives = [r for r in records if r.layer == "D"]
        for surface in ("tool_json", "html_double", "html_single", "markdown", "prose", "log_kv"):
            self.assertTrue(any(r.surface == "url_" + surface for r in positives))
            self.assertTrue(any(r.surface == "url_" + surface for r in negatives))
        values = {g.value for r in positives for g in r.gold}
        for prefix in ("https://", "http://", "HTTPS://", "www.", r"https:\/\/", r"https:\//", r"https:/\/"):
            self.assertTrue(any(v.startswith(prefix) for v in values), prefix)
        self.assertTrue(any(v.endswith(r"\/") for v in values))
        self.assertTrue(any("?" in v and "#" in v for v in values))
        self.assertTrue(any("'" in v for v in values))
        self.assertTrue(any("%22%3C%3E%7B%7D%5C" in v for v in values))
        self.assertEqual({r.family for r in negatives}, {
            "url_bare_host", "url_bare_path", "url_scheme_fragment", "url_www_fragment",
            "url_filename", "url_numeric", "url_version"})
        for record in records:
            self.assertTrue(record.gold if record.layer != "D" else not record.gold)
            for span in (*record.gold, *record.decoys):
                self.assertEqual(record.text.encode()[span.start:span.end].decode(), span.value)
            if record.layer == "D":
                self.assertEqual(len(record.decoys), 1)
                self.assertNotIn("https://", record.decoys[0].value.rstrip("/"))
        repeats = [r for r in records if r.layer == "R"]
        self.assertTrue(any(len({g.value for g in r.gold}) == 2 for r in repeats))
        self.assertTrue(any(len({g.value for g in r.gold}) == 1 for r in repeats))
        for record in repeats:
            self.assertEqual(len(record.gold), 2)
            self.assertEqual(len(record.decoys), 1)
            self.assertLess(record.gold[0].end, record.gold[1].start)

    def test_bad_raw_spans_and_missing_counterweights_fail_closed(self):
        from dataclasses import replace
        from agentic_layers import LayerError
        records = urls.generate("test")
        corrupted = list(records)
        original = corrupted[0]
        corrupted[0] = replace(original, gold=(replace(original.gold[0], end=original.gold[0].end + 1),))
        with self.assertRaisesRegex(LayerError, "source bytes"):
            urls.check(corrupted, "test")
        with self.assertRaisesRegex(LayerError, "population"):
            urls.check([r for r in records if r.family != "url_bare_path"], "test")
        with self.assertRaisesRegex(LayerError, "partition"):
            urls.check(records, "dev")

    def test_escaped_query_apostrophe_and_host_only_values_are_gold(self):
        records = urls.generate("test")
        expected = {"url_escaped_query_fragment", "url_escaped_apostrophe", "url_escaped_www",
                    "url_host_https", "url_host_www", "url_http_uppercase"}
        self.assertTrue(expected <= {r.family for r in records if r.layer == "A"})
        for record in records:
            if record.family == "url_escaped_query_fragment":
                self.assertIn(r"?next=\/orders", record.gold[0].value)
                self.assertIn("#settings", record.gold[0].value)
            if record.family == "url_escaped_apostrophe":
                self.assertIn(r"\/O'Brien?q=O'Brien", record.gold[0].value)
                self.assertNotEqual(record.surface, "url_html_single")
        bare_paths = [r.decoys[0].value for r in records if r.family == "url_bare_path"]
        self.assertTrue(any(r"\/" in value for value in bare_paths))

    def test_seeded_partitions_are_disjoint_and_raw_records_are_deterministic(self):
        import agentic_layers as agentic
        dev, test = urls.generate("dev"), urls.generate("test")
        for partition, records in (("dev", dev), ("test", test)):
            self.assertEqual(agentic.corpus_bytes(records), agentic.corpus_bytes(urls.generate(partition)))
            import hashlib
            self.assertEqual(hashlib.sha256(agentic.corpus_bytes(records)).hexdigest(), PINNED_URL_SHA256[partition])
            self.assertTrue(all(r.partition == partition for r in records))
            self.assertTrue(all(r.uid.startswith(f"agentic-{partition}-") for r in records))
            self.assertTrue(all(r.template.endswith("/" + partition) for r in records))
            self.assertEqual(len({r.uid for r in records}), len(records))
        for field in ("text", "template", "group", "uid"):
            self.assertFalse({getattr(r, field) for r in dev} & {getattr(r, field) for r in test}, field)
        self.assertFalse({g.value for r in dev for g in r.gold} & {g.value for r in test for g in r.gold})
        for templates in urls.TEMPLATES.values():
            self.assertNotEqual(templates["dev"], templates["test"])
        with self.assertRaisesRegex(agentic.LayerError, "partition"):
            urls.generate("training")

    def test_records_fit_existing_schema_and_inactive_v9_contract_rejects_url(self):
        import hashlib
        import json
        import agentic_layers as agentic
        import gaze_bench_score as score
        root = Path(agentic.__file__).resolve().parents[2]
        records = urls.generate("test")
        expected_fields = {"id", "partition", "layer", "family", "surface", "validity", "group",
                           "template", "language", "region", "text", "gold"}
        for record in records:
            row = json.loads(json.dumps(record.to_json()))
            self.assertEqual(set(row), expected_fields | ({"decoys"} if record.decoys else set()))
            self.assertTrue(all(set(g) == {"start", "end", "label", "value"} for g in row["gold"]))
            document = record.to_document()
            self.assertEqual(document.cell.split("|"), [record.layer, record.family, record.surface, record.validity])
            self.assertEqual([(s.start, s.end, s.label) for s in document.spans],
                             [(g.start, g.end, "URL") for g in record.gold])
        documents = [r.to_document() for r in records]
        # Layer C has already ruled on URL; reuse that label, never credentials.
        for contract in (score.SCORED_LABEL_CONTRACT_V1,
                         score.load_scored_label_contract(root / "docs/reference/benchmarks/scored-labels-v2.json")):
            applied = score.apply_scored_label_contract(documents, contract)
            self.assertEqual([d.spans for d in applied], [d.spans for d in documents])
        # The committed v9 profile predates URL; the ambient profile may score it.
        inactive_path = root / "docs/reference/benchmarks/scored-labels-agentic-generator-v9.json"
        inactive_raw = inactive_path.read_bytes()
        self.assertEqual(len(inactive_raw), 11285)
        self.assertEqual(hashlib.sha256(inactive_raw).hexdigest(),
                         "f9d0cfff6ebc2feac3bd73567b5800af2f38ef1df9e7e10e5e7e64ebd2e93543")
        self.assertEqual(json.loads(inactive_raw)["corpus"]["generator_version"], 9)
        inactive = agentic.load_contract(root, version=9)
        self.assertEqual(inactive.path, inactive_path.relative_to(root).as_posix())
        self.assertEqual(inactive.sha256, hashlib.sha256(inactive_raw).hexdigest())
        self.assertNotIn("URL", inactive.scored_labels | inactive.excluded_labels)
        with self.assertRaisesRegex(score.ScoredLabelContractError, "URL"):
            score.apply_scored_label_contract(documents, inactive)

    def test_current_contract_scores_url_and_preserves_complete_source_gold(self):
        import json
        import agentic_layers as agentic
        import gaze_bench_score as score
        root = Path(agentic.__file__).resolve().parents[2]
        current = agentic.load_contract(root)
        profile = json.loads((root / agentic.SCORED_LABELS_PATH).read_bytes())
        self.assertEqual(profile["corpus"]["generator_version"], agentic.GENERATOR_VERSION)
        self.assertIn("URL", current.scored_labels)
        self.assertNotIn("URL", current.excluded_labels)
        for partition in ("dev", "test"):
            with self.subTest(partition=partition):
                records = urls.generate_extended(partition)
                documents = [r.to_document() for r in records]
                expected_spans = [tuple(score.Span(g.start, g.end, "URL") for g in r.gold)
                                  for r in records]
                self.assertGreater(sum(map(len, expected_spans)), 0)
                try:
                    applied = score.apply_scored_label_contract(documents, current)
                except score.ScoredLabelContractError as error:
                    self.fail(f"current profile must accept URL gold: {error}")
                self.assertEqual([d.spans for d in applied], expected_spans)
                self.assertEqual([(d.uid, d.text, d.cell) for d in applied],
                                 [(d.uid, d.text, d.cell) for d in documents])
                self.assertTrue(all(not d.excluded_spans for d in applied))

    def test_existing_scorer_prices_raw_leaks_surrounds_and_repeat_near_misses(self):
        import gaze_bench_score as score
        records = urls.generate("test")
        record = next(r for r in records if r.layer == "A" and r.family == "url_terminal_slash" and r.surface == "url_tool_json")
        document = record.to_document()
        gold = document.spans[0]
        exact = score.MetricAccumulator()
        exact.add(document, [gold])
        self.assertEqual(exact.result()["utf8_bytes"]["leaked"], 0)
        self.assertEqual(exact.result()["utf8_bytes"]["false_positive"], 0)
        truncated = score.MetricAccumulator()
        truncated.add(document, [score.Span(gold.start, gold.end - 2, "URL")])
        self.assertEqual(truncated.result()["utf8_bytes"]["leaked"], 2)
        oversized = score.MetricAccumulator()
        oversized.add(document, [score.Span(gold.start, len(document.text.encode()), "URL")])
        self.assertGreater(oversized.result()["utf8_bytes"]["false_positive"], 0)
        repeated = next(r for r in records if r.layer == "R" and r.family == "url_repeat_mixed")
        self.assertNotEqual(repeated.gold[0].value, repeated.gold[1].value)
        repeated_doc = repeated.to_document()
        decoy = repeated.decoys[0]
        accumulator = score.MetricAccumulator()
        accumulator.add(repeated_doc, [*repeated_doc.spans, score.Span(decoy.start, decoy.end, "URL")])
        self.assertEqual(accumulator.result()["utf8_bytes"]["false_positive"], len(decoy.value.encode()))
        negative = next(r for r in records if r.layer == "D" and r.family == "url_bare_path")
        accumulator = score.MetricAccumulator()
        d = negative.decoys[0]
        accumulator.add(negative.to_document(), [score.Span(d.start, d.end, "URL")])
        self.assertEqual(accumulator.result()["utf8_bytes"]["false_positive"], len(d.value.encode()))

    def test_reference_urls_are_gold_and_single_quoted_html_is_valid(self):
        from html.parser import HTMLParser
        from agentic_layers import LayerError
        class Links(HTMLParser):
            def __init__(self):
                super().__init__()
                self.values = []
            def handle_starttag(self, tag, attrs):
                self.values.extend(v for k, v in attrs if k in {"href", "data-again"})
        for record in urls.generate("test"):
            if record.family in {"url_reference_docs", "url_reference_repo"}:
                self.assertEqual(record.layer, "A")
                self.assertEqual(len(record.gold), 1)
            if record.surface.startswith("url_html_"):
                parser = Links()
                parser.feed(record.text)
                spans = record.decoys if record.layer == "D" else record.gold
                self.assertEqual(parser.values, [g.value for g in spans])
        with self.assertRaisesRegex(LayerError, "ambiguous"):
            urls.render("test", "html_single", ("https://portal.example.invalid/O'Brien",))

    def test_partition_metadata_matches_requested_partition(self):
        for partition in ("dev", "test"):
            records = urls.generate(partition)
            self.assertEqual({r.partition for r in records}, {partition})
            self.assertTrue(all(r.text.startswith("ß:" if partition == "dev" else "é:") for r in records))

    def test_each_declared_spelling_has_literal_boundary_evidence(self):
        expected_prefixes = {
            "url_escaped": r"https:\/\/portal.example.invalid\/users\/",
            "url_scheme_left": r"https:\//portal.example.invalid/users/",
            "url_scheme_right": r"https:/\/portal.example.invalid/users/",
            "url_escaped_www_scheme": r"https:\/\/www.example.invalid\/users\/",
            "url_escaped_uppercase": r"HTTP:\/\/PORTAL.EXAMPLE.INVALID\/USERS\/",
        }
        for record in urls.generate("test"):
            if record.layer == "A" and record.family in expected_prefixes:
                self.assertTrue(record.gold[0].value.startswith(expected_prefixes[record.family]), record.uid)

    def test_each_partition_has_no_duplicate_source_documents(self):
        for partition in ("dev", "test"):
            records = urls.generate(partition)
            self.assertEqual(len({r.text for r in records}), len(records))

    def test_cells_have_a_unique_prefix_for_future_historical_filtering(self):
        self.assertTrue(all(r.surface.startswith("url_") for r in urls.generate("test")))
