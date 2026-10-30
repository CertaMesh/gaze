"""Model-free provenance checks for the invisible-separator extension."""
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib
import unittest

import agentic_layers as agentic
import invisible_separator_cells as cells


class InvisibleSeparatorCellsTests(unittest.TestCase):
    def test_each_separator_has_independent_gold_repeats_and_counterweights(self):
        for partition in agentic.PARTITIONS:
            rows = cells.records(agentic, partition)
            for name, separator in cells.SEPARATORS.items():
                selected = [row for row in rows if row.surface.startswith("invisible_" + name + "_")]
                self.assertEqual({row.layer for row in selected}, {"A", "D", "R"})
                self.assertTrue({"invisible_card_twin", "invisible_tax_twin", "invisible_phone_twin",
                                 "invisible_iban_twin", "invisible_email_twin"}
                                <= {row.family for row in selected if row.layer == "D"})
                self.assertEqual({span.label for row in selected for span in row.gold},
                                 {"TAXNUM", "IBAN", "TELEPHONENUM", "CREDITCARDNUMBER", "EMAIL"})
                for row in selected:
                    self.assertIn(separator, row.text)
                    self.assertEqual(len(row.gold), {"A": 1, "D": 0, "R": 2}[row.layer])
                    for span in (*row.gold, *row.decoys):
                        self.assertEqual(row.text.encode()[span.start:span.end].decode(), span.value)
                        self.assertIn(separator, span.value)
                    if row.family == "invisible_card_twin":
                        prefix, suffix = (("Schema example: ", "; incomplete.") if partition == "dev"
                                          else ("Public format fixture = ", ". Not assigned."))
                        digits = row.text.removeprefix(prefix).removesuffix(suffix).replace(separator, "")
                        doubled = [int(digit) * (2 if index % 2 else 1)
                                   for index, digit in enumerate(reversed(digits))]
                        self.assertNotEqual(sum(n - 9 if n > 9 else n for n in doubled) % 10, 0)

    def test_cued_tax_gold_and_counterweights_participate_in_credit_gate(self):
        self.assertIn("invisible_card_twin", agentic.guard_families(agentic.GENERATOR_VERSION))
        self.assertIn("invisible_tax_twin", agentic.guard_families(agentic.GENERATOR_VERSION))
        self.assertNotIn("invisible_card_twin", agentic.guard_families(26))
        self.assertNotIn("invisible_tax_twin", agentic.guard_families(26))
        for surface in cells.CUED_SURFACES:
            self.assertTrue(agentic.invalid_twin_credited("TAXNUM", surface))

    def test_integrated_history_reconstructs_byte_identically(self):
        # v25/v26 are the existing corpus pins; v27/v28 also include structured_;
        # v29 keeps the card twins v30 made phone-free.
        pins = {
            25: ("a88025bd7c995e3633738debf8b8bab45c8c7232bf222a4d72a9af36fc67ca59",
                 "133f8452d01fd517c77087e539ba9dc253e30a1886668bfab3e1631cc9ff6c32"),
            26: ("cbc9a1bbfc899c50b35c78e0ce1e02518b3a8f5ecfeb759ec5e6dde0b6432b25",
                 "f8b9dc58de45dcc6ec8de0e911dc0d5f52682889ec912cbf4b120a03e56409bc"),
            27: ("a1b00d3d8281b54feabe1c04c9a490738bc415d9f9b94b75ae9eb1b1c172e4e1",
                 "dbee6dc0184c49df384b9f2949b29af81e58c14ae1db0043df8aadc8f1c41a81"),
            28: ("ad447042cd7403b599b217c40b77851f8c07dc59c99d19bddd7a21586ca55c99",
                 "db979087d95fce766e649ba0f4bc701a62b4c6b2e3975767348e163e942e8b9f"),
            29: ("75ff131ba6a7588a5baf71b9ed5a7dd3d44dbcbe41993bd8c2d0d8c0404ddf59",
                 "ef3a457a3c1a273cd8d64f9dbb5be2010014cc40d6664f7fdff531ff9bbdb03b"),
        }
        root = Path(__file__).resolve().parents[2]
        for index, partition in enumerate(("dev", "test")):
            current = agentic.generate(partition)
            for version, digests in pins.items():
                with self.subTest(partition=partition, version=version):
                    previous = agentic.records_as_of(version, current)
                    self.assertEqual(hashlib.sha256(agentic.corpus_bytes(previous)).hexdigest(), digests[index])
                    agentic.load_contract(root, version=version)
        frozen = root / "docs/reference/benchmarks/scored-labels-agentic-generator-v26.json"
        self.assertEqual(hashlib.sha256(frozen.read_bytes()).hexdigest(),
                         "11bb0105c99e61e254d31c9cf7d93e333ccfc79f2086499490fa37a95277a660")

    def test_partitions_share_no_new_text_or_value(self):
        dev, test = (cells.records(agentic, partition) for partition in agentic.PARTITIONS)
        self.assertFalse({row.text for row in dev} & {row.text for row in test})
        self.assertFalse({span.value for row in dev for span in row.gold}
                         & {span.value for row in test for span in row.gold})

    def test_imported_cells_and_dev_phone_correction_are_pinned(self):
        # v30 changes only the card twins; the v29 cells stay reproducible.
        pins = {True: {"dev": "fb5095cb78b0befa55733313b7846592ae6423bed176758bf2fcb1c3d060e31e",
                       "test": "5bd68b46a8c4ab4dfd0afae70c70653464d5386186790b3a06cccdc80758afa1"},
                False: {"dev": "d0ddf954642df5e32612286d9efd418447ee7ca7831754adf258cfb9b06e4f0d",
                        "test": "c16e596db95282e23fd72c02f4d04aab1de8da0afd94e532462311a56605f22f"}}
        for phone_free, digests in pins.items():
            for partition, digest in digests.items():
                rows = cells.records(agentic, partition, phone_free_card_twins=phone_free)
                self.assertEqual(len(rows), 12530)
                self.assertEqual(hashlib.sha256(agentic.corpus_bytes(rows)).hexdigest(), digest)

    def test_phone_free_card_twins_differ_only_where_a_group_started_with_zero(self):
        for partition in agentic.PARTITIONS:
            current = cells.records(agentic, partition)
            legacy = {row.uid: row for row in cells.records(agentic, partition, phone_free_card_twins=False)}
            changed = [row for row in current if row != legacy[row.uid]]
            self.assertTrue(changed)
            self.assertEqual({row.family for row in changed}, {"invisible_card_twin"})
            for row in current:
                if row.family == "invisible_card_twin":
                    groups = re.findall(r"\d{4}", row.text)
                    self.assertEqual(len(groups), 4)
                    self.assertFalse(any(group.startswith("0") for group in groups), row.uid)
                    self.assertEqual(row in changed,
                                     any(group.startswith("0") for group in re.findall(r"\d{4}", legacy[row.uid].text)))

    def test_bom_counterweights_include_leading_and_embedded_code(self):
        for partition in agentic.PARTITIONS:
            rows = cells.records(agentic, partition)
            code = [row for row in rows if row.family in {"leading_code", "embedded_code"}]
            self.assertEqual(len(code), 2 * agentic.DOCS_PER_FAMILY)
            self.assertTrue(all(row.layer == "D" and not row.gold for row in code))
            self.assertTrue(all("\ufeff" in row.text for row in code))
            self.assertTrue(all(row.text.startswith("\ufeff") for row in code if row.family == "leading_code"))

    def test_joined_overlong_values_have_gold_repeats_and_public_counterweights(self):
        for partition in agentic.PARTITIONS:
            rows = cells.records(agentic, partition)
            for name, separator in cells.JOINERS.items():
                selected = [row for row in rows if row.surface.startswith("invisible_joined_" + name + "_")]
                self.assertEqual({row.layer for row in selected}, {"A", "D", "R"})
                self.assertEqual({span.label for row in selected for span in row.gold}, {"NATIONALID", "TAXNUM"})
                for row in selected:
                    self.assertIn(separator, row.text)
                    self.assertEqual(len(row.gold), {"A": 1, "D": 0, "R": 2}[row.layer])

    def test_labelled_values_and_repeats_are_whole_source_gold(self):
        for partition in agentic.PARTITIONS:
            for row in cells.records(agentic, partition):
                if row.layer == "D" or row.surface.startswith("invisible_boundary_"):
                    continue
                first, *repeat = row.text.split("\n")
                if row.surface.endswith("_json"):
                    payload = json.loads(first)
                    fields = payload.get("result", payload)
                    expected = next(value for value in fields.values() if isinstance(value, str))
                elif partition == "dev":
                    expected = first.split(": ", 1)[1].removesuffix("; approved.")
                else:
                    expected = first.split(" = ", 1)[1].removesuffix(". End.")
                self.assertEqual([span.value for span in row.gold], [expected] * (1 + len(repeat)), row.uid)
                if repeat:
                    self.assertEqual(repeat, [("Again: " if partition == "dev" else "Copy = ") + expected])
                if row.surface.startswith("invisible_joined_"):
                    self.assertEqual(len(expected), 23)

    def test_boundary_shapes_have_whole_value_gold_and_formatting_counterweights(self):
        for partition in agentic.PARTITIONS:
            rows = [row for row in cells.records(agentic, partition)
                    if row.surface.startswith("invisible_boundary_")]
            self.assertEqual({row.layer for row in rows}, {"A", "R", "D"})
            self.assertEqual({span.label for row in rows for span in row.gold},
                             {"CREDITCARDNUMBER", "IBAN", "ETHADDRESS", "EMAIL", "TELEPHONENUM"})
            for shape in ("before", "after", "inside", "mixed"):
                selected = [row for row in rows if row.surface.endswith("_" + shape)]
                self.assertTrue(selected)
                self.assertTrue(all(row.gold for row in selected))
            self.assertEqual({row.family for row in rows if row.layer == "D"},
                             {"formatting_" + name for name in ("emoji_zwj", "emoji_vs16", "emoji_tag_flag",
                                                                "persian", "arabic", "indic", "rtl")})
            for row in rows:
                for span in row.gold:
                    self.assertEqual(row.text.encode()[span.start:span.end].decode(), span.value)
                self.assertEqual(len(row.gold), {"A": 1, "D": 0, "R": 2}[row.layer])
                if row.layer == "D":
                    continue
                shape = row.surface.rsplit("_", 1)[1]
                first = row.text.split("\n", 1)[0]
                gold = row.gold[0]
                before = first.encode()[:gold.start].decode()
                after = first.encode()[gold.end:].decode()
                if shape in {"before", "mixed"}:
                    word = "item" if partition == "dev" else "entry"
                    self.assertTrue(before.startswith(word))
                    self.assertEqual(len(before.removeprefix(word)), 1)
                else:
                    self.assertEqual(before, "Value: ")
                if shape in {"after", "mixed"}:
                    word = "paid" if partition == "dev" else "ready"
                    self.assertTrue(after.endswith(word + "; end."))
                    self.assertEqual(len(after.removesuffix(word + "; end.")), 1)
                else:
                    self.assertEqual(after, "; end.")
                # Formatting inside the value is gold, not merely present nearby.
                controls = [c for c in gold.value if not c.isascii()]
                self.assertEqual(len(controls), int(shape in {"inside", "mixed"}), row.uid)


ROOT = Path(__file__).resolve().parents[2]
# An explicit path must exist; only the default build location may be absent.
CLI = Path(os.environ.get("GAZE_CLI_BINARY", ROOT / "target/debug/gaze"))


def model_free_setup_policy():
    """The `gaze setup` policy without its model sections: every embedded
    rulepack, and the locale chain setup derives from them in pack order."""
    source = (ROOT / "crates/gaze-recognizers/src/lib.rs").read_text()
    packs = re.findall(r'\("([a-z-]+)", include_str!\("\.\./embedded/\1\.toml"\)\)', source)
    locales = ["en-US"]
    for pack in packs:
        rulepack = tomllib.loads((ROOT / f"crates/gaze-recognizers/embedded/{pack}.toml").read_text())
        for locale in (*rulepack.get("default_locales", ()),
                       *(tag for recognizer in rulepack.get("recognizers", ()) for tag in recognizer.get("locales", ()))):
            if locale != "global" and locale not in locales:
                locales.append(locale)
    return (f'schema_version = "0.1.0"\n\n[session]\nscope = "conversation"\n\n'
            f"[locale]\nactive = {json.dumps(locales)}\n\n[address_blocks]\nenabled = true\n\n"
            f"[policy.rulepacks]\nbundled = {json.dumps(packs)}\n\n"
            '[[rule]]\nkind = "default"\naction = "tokenize"\n')


@unittest.skipUnless(CLI.is_file() or "GAZE_CLI_BINARY" in os.environ, "build the gaze CLI to run this proof")
class CardTwinDetectorTests(unittest.TestCase):
    """The card twin guards checksum relaxation, so nothing else may claim it."""

    def detections(self, texts):
        with tempfile.TemporaryDirectory() as temporary:
            policy = Path(temporary) / "policy.toml"
            policy.write_text(model_free_setup_policy())

            def clean(text):
                result = subprocess.run([str(CLI), "clean", "--policy", str(policy)], input=text.encode(),
                                        capture_output=True, check=True)
                return [entry["class"] for entry in json.loads(result.stdout)["entries"]]

            with ThreadPoolExecutor(4) as pool:
                return dict(zip(texts, pool.map(clean, texts)))

    @staticmethod
    def twin_texts(version):
        texts = set()
        for partition in agentic.PARTITIONS:
            for row in agentic.records_as_of(version, agentic.generate(partition)):
                if row.family == "invisible_card_twin":
                    separator = cells.SEPARATORS[row.surface.split("_invisible_card_twin")[0].removeprefix("invisible_")]
                    # The ASCII-space form is what a pipeline that reads
                    # invisible separators as spaces sees.
                    texts |= {row.text, row.text.replace(separator, " ")}
        return sorted(texts)

    def test_no_rule_protects_any_part_of_a_card_twin(self):
        self.assertEqual(len(self.twin_texts(agentic.GENERATOR_VERSION)), 2 * agentic.DOCS_PER_FAMILY * 6)
        found = {text: classes for text, classes in self.detections(self.twin_texts(agentic.GENERATOR_VERSION)).items()
                 if classes}
        self.assertEqual(found, {})

    def test_the_v29_twins_carried_a_phone_fragment(self):
        # The detector sees the hazard this version removed: `0771 4620`.
        found = self.detections(self.twin_texts(cells.PHONE_FREE_CARD_TWIN_SINCE - 1))
        self.assertIn("Custom:phone", found["Public format fixture = 5234 7762 0771 4620. Not assigned."])
