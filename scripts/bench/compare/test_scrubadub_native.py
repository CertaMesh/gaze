"""Native locale checks run in the separately locked scrubadub environment."""

import importlib.metadata
import importlib.util
import unittest

import compare


HAS_SCRUBADUB = importlib.util.find_spec("scrubadub") is not None


@unittest.skipUnless(HAS_SCRUBADUB, "requires the separately pinned scrubadub environment")
class NativeScrubadubLocaleTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        for name, expected in (("scrubadub", "2.0.0"), ("scrubadub-spacy", "2.0.0"),
                               ("spacy", "3.7.5"), ("en-core-web-lg", "3.7.1")):
            if importlib.metadata.version(name) != expected:
                raise RuntimeError("native locale checks require the committed package and model pins")
        cls.backends = {
            "scrubadub-base": compare.Scrubadub(),
            "scrubadub-spacy": compare.Scrubadub("en_core_web_lg"),
        }

    @staticmethod
    def documents():
        documents = []
        for language, region, positive, benign in (
            ("es", "ES", "Escribe a alice@example.invalid.", "El informe queda listo."),
            ("en", "US", "Write to alice@example.invalid.", "The report is ready."),
        ):
            value = "alice@example.invalid"
            start = positive.index(value)
            documents.append(compare.score.Document(
                f"synthetic-{language}-positive", positive, language, region, "synthetic",
                (compare.score.Span(start, start + len(value), "EMAIL"),)))
            documents.append(compare.score.Document(
                f"synthetic-{language}-benign", benign, language, region, "synthetic", ()))
        return documents

    def test_native_spanish_and_english_positive_and_benign_inputs(self):
        for name, backend in self.backends.items():
            for document in self.documents():
                with self.subTest(tool=name, language=document.language, positive=bool(document.spans)):
                    predictions = backend.predict(document)
                    if document.spans:
                        gold, = document.spans
                        self.assertTrue(any(span.start <= gold.start and span.end >= gold.end
                                            for span in predictions))
                    else:
                        self.assertEqual(predictions, [])

    def test_both_configurations_score_the_full_population_without_spanish_skips(self):
        for name, backend in self.backends.items():
            with self.subTest(tool=name):
                result = compare.measure(name, backend.predict, {"C": self.documents()},
                                         compare.load_mapping()["scrubadub"], tuple(backend.scrubbers))
                for contract, layers in result["contracts"].items():
                    with self.subTest(contract=contract):
                        row = layers["C"]
                        self.assertEqual(row["documents"], 4)
                        self.assertEqual(row["processed_documents"], 4)
                        self.assertEqual(row["skipped_documents"], 0)
                        self.assertEqual(row["skipped_languages"], {})
                        self.assertEqual(row["leaked_bytes"], 0)
                        self.assertEqual(row["false_positive_bytes"], 0)


if __name__ == "__main__":
    if not HAS_SCRUBADUB:
        raise SystemExit("native checks require the committed scrubadub environment")
    unittest.main()
