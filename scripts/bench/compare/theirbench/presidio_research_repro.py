#!/usr/bin/env python3
"""Reproduce Presidio Research's published scores with its own evaluator.

The two configurations transcribe microsoft/presidio-research notebooks 4
(vanilla AnalyzerEngine) and 5 (custom analyzer: OpenMed NER recognizer,
extra pattern recognizers, context enhancement) at the pinned commit. The
scorer is theirs: CanonicalMapper plus SpanEvaluator(iou_threshold=0.75),
F2, binary PII-vs-O level. Only aggregate numbers are written.

Runs in its own environment (requirements-presidio-research.lock), because
presidio-evaluator 0.3.2 needs spaCy >= 3.8 and transformers >= 5.3 while the
comparison harness pins spaCy 3.7.5.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.metadata
import json
import platform
import sys
import time
from pathlib import Path

REPO = "microsoft/presidio-research"
COMMIT = "6db3769a3388b4075b93ab2229c5e0b9c30137f7"
DATASET = "data/synth_dataset_v2.json"
OPENMED_MODEL = "OpenMed/OpenMed-PII-SuperClinical-Large-434M-v1"
OPENMED_REVISION = "df7af994d39d358e52f929ff1b3a40d894adf022"
PUBLISHED = {
    # Notebook outputs at COMMIT, binary level (PII vs O), beta = 2.
    "vanilla": {"precision": 0.733, "recall": 0.646, "f2": 0.661,
                "source": "notebooks/4_Evaluate_Presidio_Analyzer.ipynb"},
    "custom": {"precision": 0.921, "recall": 0.907, "f2": 0.91,
               "source": "notebooks/5_Evaluate_Custom_Presidio_Analyzer.ipynb"},
}

# Notebook 5, verbatim: OpenMed labels to Presidio entities.
OPENMED_MAPPING = {
    "first_name": "FIRST_NAME", "last_name": "LAST_NAME", "age": "AGE", "gender": "GENDER",
    "date_of_birth": "DATE_OF_BIRTH", "blood_type": "BLOOD_TYPE", "occupation": "OCCUPATION",
    "education_level": "EDUCATION_LEVEL", "employment_status": "EMPLOYMENT_STATUS",
    "language": "LANGUAGE", "race_ethnicity": "ETHNICITY", "sexuality": "SEXUALITY",
    "political_view": "POLITICAL_VIEW", "religious_belief": "RELIGIOUS_BELIEF",
    "biometric_identifier": "BIOMETRIC_IDENTIFIER", "pin": "ID",
    "account_number": "ACCOUNT_NUMBER", "customer_id": "ID", "employee_id": "ID",
    "unique_id": "ID", "bank_routing_number": "US_BANK_NUMBER", "swift_bic": "SWIFT_CODE",
    "certificate_license_number": "PROFESSIONAL_LICENSE",
    "medical_record_number": "MEDICAL_LICENSE", "health_plan_beneficiary_number": "ID",
    "credit_debit_card": "CREDIT_CARD", "cvv": "CVV", "ssn": "US_SSN", "tax_id": "ID",
    "license_plate": "LICENSE_PLATE", "vehicle_identifier": "ID", "mac_address": "ID",
    "device_identifier": "ID", "password": "PASSWORD", "user_name": "USER_NAME",
    "email": "EMAIL_ADDRESS", "phone_number": "PHONE_NUMBER", "fax_number": "PHONE_NUMBER",
    "url": "URL", "city": "CITY", "country": "COUNTRY", "county": "COUNTY", "state": "STATE",
    "street_address": "STREET_ADDRESS", "coordinate": "COORDINATE", "postcode": "ZIP_CODE",
    "ipv4": "IP_ADDRESS", "ipv6": "IP_ADDRESS", "date": "DATE_TIME", "date_time": "DATE_TIME",
    "time": "DATE_TIME", "company_name": "COMPANY",
}
NOTEBOOK5_REMOVED = (
    "NhsRecognizer", "UkNinoRecognizer", "SgFinRecognizer", "AuAbnRecognizer",
    "AuAcnRecognizer", "AuTfnRecognizer", "AuMedicareRecognizer", "InPanRecognizer",
    "InAadhaarRecognizer", "InVehicleRegistrationRecognizer", "InPassportRecognizer",
    "InVoterRecognizer", "CryptoRecognizer", "SpacyRecognizer",
)


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def vanilla_analyzer():
    from presidio_analyzer import AnalyzerEngine

    engine = AnalyzerEngine(default_score_threshold=0.4)
    return engine, None


def custom_analyzer(model_path: str):
    from presidio_analyzer import AnalyzerEngine, Pattern, PatternRecognizer, RecognizerRegistry
    from presidio_analyzer.context_aware_enhancers import LemmaContextAwareEnhancer
    from presidio_analyzer.nlp_engine import SlimSpacyNlpEngine
    from presidio_analyzer.predefined_recognizers.ner import HuggingFaceNerRecognizer

    nlp_engine = SlimSpacyNlpEngine()
    nlp_engine.load()
    ner = HuggingFaceNerRecognizer(
        model_name=model_path, label_mapping=OPENMED_MAPPING, aggregation_strategy="first",
    )
    titles = PatternRecognizer(
        deny_list=["Mr.", "Mrs.", "Ms.", "Miss", "Dr.", "Prof."],
        supported_entity="TITLE", name="TitlesRecognizer",
    )
    years = PatternRecognizer(
        patterns=[Pattern("YEAR", r"\b(19|20)\d{2}\b", score=0.1)],
        supported_entity="DATE_TIME", name="YearsRecognizer",
        context=["year", "at", "date", "in", "on"],
    )
    age = PatternRecognizer(
        supported_entity="AGE",
        patterns=[Pattern(name="age (very weak)", regex=r"\b(110|[1-9]?[0-9])\b", score=0.01)],
        name="AgeRecognizer", context=["month", "old", "turn", "age", "y/o"],
    )
    registry = RecognizerRegistry()
    registry.load_predefined_recognizers(nlp_engine=nlp_engine)
    for recognizer in (ner, titles, years, age):
        registry.add_recognizer(recognizer)
    for name in NOTEBOOK5_REMOVED:
        registry.remove_recognizer(name)
    engine = AnalyzerEngine(
        nlp_engine=nlp_engine,
        context_aware_enhancer=LemmaContextAwareEnhancer(context_prefix_count=10, context_suffix_count=10),
        registry=registry, default_score_threshold=0.3,
    )
    return engine, engine.default_score_threshold


def precomputed_model(dataset, predictions: Path, labels: dict[str, list[str]]):
    """A presidio-evaluator model that replays another system's spans.

    Tags come from PresidioAnalyzerWrapper's own span-to-tag conversion, so
    the only difference from a Presidio run is where the spans came from.
    Each tool label becomes the first Presidio gold label it covers; a label
    that covers none becomes the generic ID node, so its redactions still
    count at the binary PII-vs-O level.
    """
    from presidio_analyzer import RecognizerResult
    from presidio_evaluator.models import BaseModel, PresidioAnalyzerWrapper

    rows = [json.loads(line) for line in predictions.read_text(encoding="utf-8").splitlines()]
    if [row["index"] for row in rows] != list(range(len(dataset))):
        raise SystemExit("predictions must cover every document in dataset order")
    to_tags = PresidioAnalyzerWrapper._PresidioAnalyzerWrapper__recognizer_results_to_tags

    class Precomputed(BaseModel):
        def predict(self, sample, **kwargs):
            raise NotImplementedError

        def batch_predict(self, samples, **kwargs):
            tags = []
            for sample, row in zip(samples, rows, strict=True):
                results = [RecognizerResult((labels[label] or ["ID"])[0], start, end, 1.0)
                           for start, end, label in row["spans"]]
                tags.append(to_tags(results, sample))
            return tags

    return Precomputed()


def evaluate(config: str, dataset_path: Path, model_path: str | None, limit: int | None,
             predictions: Path | None = None, labels: dict[str, list[str]] | None = None) -> dict:
    from presidio_evaluator import InputSample
    from presidio_evaluator.entity_mapping import CanonicalMapper
    from presidio_evaluator.evaluation import SpanEvaluator
    from presidio_evaluator.models import PresidioAnalyzerWrapper

    dataset = InputSample.read_dataset_json(dataset_path)
    if limit is not None:
        dataset = dataset[:limit]
    if predictions is not None:
        wrapped = precomputed_model(dataset, predictions, labels)
    elif config == "vanilla":
        engine, _ = vanilla_analyzer()
        wrapped = PresidioAnalyzerWrapper(analyzer_engine=engine)
    else:
        engine, threshold = custom_analyzer(model_path)
        wrapped = PresidioAnalyzerWrapper(analyzer_engine=engine, score_threshold=threshold, language="en")
    started = time.perf_counter()
    results = wrapped.predict_dataset(dataset)
    seconds = time.perf_counter() - started
    mapper = CanonicalMapper()
    if config != "custom":
        mapper.analyze(results)
    else:
        # Notebook 5: suppress prediction-only labels, then apply the
        # explicit resolutions it derives from the remaining issues.
        mapper.analyze(results, min_collision_count=5)
        mapper.suppress_prediction_only()
        resolutions = {}
        for issue in mapper.get_issues():
            if issue.type.value in ("unresolved", "prediction_only"):
                resolutions.update(dict.fromkeys(issue.labels))
        if resolutions:
            mapper.map(resolutions)
    scores = SpanEvaluator(iou_threshold=0.75).calculate_hierarchical_scores(
        mapper.get_mapped_results_dataframe(), beta=2,
    )
    binary = scores["binary"]
    return {
        "documents": len(dataset),
        "precision": round(binary.pii_precision, 3),
        "recall": round(binary.pii_recall, 3),
        "f2": round(binary.pii_f, 3),
        "predict_seconds": round(seconds, 1),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--checkout", type=Path, required=True,
                        help=f"{REPO} checkout at {COMMIT}")
    parser.add_argument("--openmed-model", help=f"local {OPENMED_MODEL} snapshot at {OPENMED_REVISION}")
    parser.add_argument("--config", choices=["vanilla", "custom", "all"], default="all")
    parser.add_argument("--limit", type=int, help="smoke only: first N documents; never published")
    parser.add_argument("--predictions", type=Path,
                        help="theirbench.py predictions for one system; scored like notebook 4")
    parser.add_argument("--labels", type=Path, help="theirbench.py composed label map for that system")
    parser.add_argument("--system", help="name recorded for --predictions")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    import subprocess

    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=args.checkout, text=True).strip()
    if head != COMMIT:
        raise SystemExit(f"presidio-research checkout is at {head}, expected {COMMIT}")
    dataset_path = args.checkout / DATASET
    configs = ("vanilla", "custom") if args.config == "all" else (args.config,)
    if args.predictions is None and "custom" in configs and not args.openmed_model:
        raise SystemExit("the custom configuration needs --openmed-model")
    report = {
        "benchmark": {"repository": REPO, "commit": COMMIT, "dataset": DATASET,
                      "dataset_sha256": sha256(dataset_path),
                      "licence": "MIT code; Fake Name Generator identities CC-BY-SA-3.0-US"},
        "scorer": "presidio_evaluator CanonicalMapper + SpanEvaluator(iou_threshold=0.75), binary PII vs O, F2",
        "versions": {name: importlib.metadata.version(name) for name in (
            "presidio-evaluator", "presidio-analyzer", "presidio-anonymizer", "spacy",
            "en-core-web-lg", "transformers", "torch")},
        "openmed": {"model": OPENMED_MODEL, "revision": OPENMED_REVISION},
        "hardware": platform.platform(), "python": sys.version.split()[0],
        "smoke_limit": args.limit,
        "published": {name: PUBLISHED[name] for name in configs},
        "reproduced": {},
    }
    if args.predictions is not None:
        labels = json.loads(args.labels.read_text(encoding="utf-8"))
        report["published"] = {}
        report["system"] = args.system
        report["scored"] = evaluate("precomputed", dataset_path, None, args.limit, args.predictions, labels)
        print(f"{args.system}: {report['scored']}", file=sys.stderr, flush=True)
    for config in configs if args.predictions is None else ():
        report["reproduced"][config] = evaluate(config, dataset_path, args.openmed_model, args.limit)
        print(f"{config}: {report['reproduced'][config]}", file=sys.stderr, flush=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
