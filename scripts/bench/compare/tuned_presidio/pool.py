"""Presidio recognizers and engines built from `space.py`.

One builder serves both the recording passes (`produce.py`: every unit of the
space, scores below every threshold kept) and the chosen configuration run live
(`live_analyzers`). A unit is one switchable recognizer family: a predefined
recognizer class, a custom recognizer, the NLP-engine NER, or an extra NER
recognizer. Its key is what candidate configurations and recorded results name.
"""

from __future__ import annotations

import copy
import inspect
from functools import lru_cache
from dataclasses import dataclass
from pathlib import Path
from typing import Mapping

import space

SPACY_UNIT = "SpacyRecognizer"
DSLIM_UNIT = "SpacyRecognizer#dslim"
EXTRA_UNITS = {"davlan": "DavlanNerRecognizer", "openmed": "OpenMedNerRecognizer",
               "gliner": "GLiNERRecognizer"}


@dataclass(frozen=True)
class ModelPaths:
    """Local snapshots of every pinned model (`spacy` maps a language to its package directory)."""

    spacy: Mapping[str, Path]
    dslim: Path | None = None
    davlan: Path | None = None
    openmed: Path | None = None
    gliner: Path | None = None


@lru_cache(maxsize=None)
def predefined_units() -> dict[str, tuple[str, ...]]:
    """Every predefined pattern recognizer class and its native languages within LANGUAGES.

    Native languages are where Presidio's own default registry loads the class, or
    else the class's default language (which may lie outside LANGUAGES, leaving the
    "native" scope empty for this corpus).
    """
    from presidio_analyzer import RecognizerRegistry
    from presidio_analyzer import predefined_recognizers as predefined

    default = RecognizerRegistry(supported_languages=list(space.LANGUAGES))
    default.load_predefined_recognizers(languages=list(space.LANGUAGES))
    native: dict[str, set[str]] = {}
    for recognizer in default.recognizers:
        native.setdefault(type(recognizer).__name__, set()).add(recognizer.supported_language)
    units = {}
    for name in sorted(dir(predefined)):
        cls = getattr(predefined, name)
        if not (name.endswith("Recognizer") and inspect.isclass(cls)) or name in space.PREDEFINED_EXCLUDED:
            continue
        languages = native.get(name) or {cls().supported_language}
        units[name] = tuple(sorted(languages & set(space.LANGUAGES)))
    return units


def _fresh_id(recognizer: object, suffix: str) -> object:
    recognizer._id = f"{recognizer.name}_{suffix}"
    return recognizer


def custom_recognizer(spec: Mapping[str, object], language: str) -> object:
    from presidio_analyzer import Pattern, PatternRecognizer

    flags = space.CASE_SENSITIVE if spec["case_sensitive"] else space.CASE_SENSITIVE | 2
    return PatternRecognizer(
        supported_entity=spec["entity"], name=spec["name"], supported_language=language,
        patterns=[Pattern(name, regex, score) for name, regex, score in spec["patterns"]] or None,
        deny_list=spec.get("deny_list") or None, deny_list_score=space.DENY_LIST_SCORE,
        context=list(spec["context"]), global_regex_flags=flags,
    )


def phone_wide(language: str) -> object:
    from presidio_analyzer.predefined_recognizers import PhoneRecognizer

    return PhoneRecognizer(
        supported_language=language, supported_regions=space.PHONE_WIDE["regions"],
        leniency=space.PHONE_WIDE["leniency"], name=space.PHONE_WIDE["name"],
    )


def openmed_mapping() -> dict[str, str]:
    import sys

    sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "theirbench"))
    import presidio_research_repro as repro

    return dict(repro.OPENMED_MAPPING)


class Extras:
    """The extra NER recognizers, each model loaded once and shared across languages."""

    def __init__(self, paths: ModelPaths, entities: set[str] | None = None) -> None:
        self.paths = paths
        self.entities = entities
        self._loaded: dict[str, object] = {}

    def gliner_mapping(self) -> dict[str, str]:
        mapping = dict(space.GLINER_LABELS)
        covered = set(mapping.values())
        for entity in sorted((self.entities or set()) - covered):
            mapping[entity] = entity
        return mapping

    def _first(self, key: str) -> object:
        if key not in self._loaded:
            spec = space.EXTRA_NER[key]
            path = getattr(self.paths, key)
            if path is None:
                raise ValueError(f"model path for {key} is not configured")
            if spec["recognizer"] == "GLiNERRecognizer":
                from presidio_analyzer.predefined_recognizers import GLiNERRecognizer

                recognizer = GLiNERRecognizer(
                    supported_language="en", name=EXTRA_UNITS[key], model_name=str(path),
                    entity_mapping=self.gliner_mapping(), flat_ner=spec["flat_ner"],
                    threshold=spec["floor"], map_location="cpu",
                )
            else:
                from presidio_analyzer.predefined_recognizers import HuggingFaceNerRecognizer

                mapping = openmed_mapping() if spec["label_mapping"] == "notebook5" else spec["label_mapping"]
                recognizer = HuggingFaceNerRecognizer(
                    supported_language="en", name=EXTRA_UNITS[key], model_name=str(path),
                    label_mapping=mapping, threshold=spec["floor"],
                    aggregation_strategy=spec["aggregation_strategy"], device="cpu",
                )
            recognizer.load()
            recognizer.is_loaded = True
            self._loaded[key] = recognizer
        return self._loaded[key]

    def recognizer(self, key: str, language: str) -> object:
        instance = copy.copy(self._first(key))
        instance.supported_language = language
        return _fresh_id(instance, language)


def nlp_engines(paths: ModelPaths, dslim: bool) -> dict[str, object]:
    """spaCy large for every language; with `dslim`, English runs Presidio's transformers engine."""
    import warnings

    from presidio_analyzer.nlp_engine import NlpEngineProvider

    spacy_engine = NlpEngineProvider(nlp_configuration={
        "nlp_engine_name": "spacy",
        "models": [{"lang_code": language, "model_name": str(path)} for language, path in paths.spacy.items()],
    }).create_engine()
    engines = {language: spacy_engine for language in space.LANGUAGES}
    if dslim:
        # Upstream's warning interpolates the document text; detection is unchanged.
        warnings.filterwarnings(
            "ignore", message="Skipping annotation, .*overlapping or can't be aligned",
            module="spacy_huggingface_pipelines.token_classification",
        )
        engines["en"] = NlpEngineProvider(nlp_configuration={
            "nlp_engine_name": "transformers",
            "models": [{"lang_code": "en", "model_name": {
                "spacy": str(paths.spacy["en"]), "transformers": str(paths.dslim),
            }}],
            "ner_model_configuration": {
                "model_to_presidio_entity_mapping": space.DSLIM["mapping"],
                "low_confidence_score_multiplier": space.DSLIM["low_confidence_score_multiplier"],
                "low_score_entity_names": space.DSLIM["low_score_entity_names"],
            },
        }).create_engine()
    return engines


def enhancer(mode: str) -> object:
    from presidio_analyzer.context_aware_enhancers import LemmaContextAwareEnhancer

    factor, minimum, prefix, suffix = space.CONTEXT_MODES[mode]
    return LemmaContextAwareEnhancer(
        context_similarity_factor=factor, min_score_with_context_similarity=minimum,
        context_prefix_count=prefix, context_suffix_count=suffix,
    )


def base_recognizers(language: str) -> list[tuple[str, object]]:
    """(unit, recognizer) for the pattern pool: every predefined class in every
    language, every custom recognizer, the wide phone recognizer and spaCy NER."""
    from presidio_analyzer import predefined_recognizers as predefined
    from presidio_analyzer.predefined_recognizers import SpacyRecognizer

    result = []
    for name in predefined_units():
        result.append((name, _fresh_id(getattr(predefined, name)(supported_language=language), language)))
    for spec in space.CUSTOM_RECOGNIZERS:
        result.append((spec["name"], _fresh_id(custom_recognizer(spec, language), language)))
    result.append((space.PHONE_WIDE["name"], _fresh_id(phone_wide(language), language)))
    result.append((SPACY_UNIT, _fresh_id(SpacyRecognizer(supported_language=language), language)))
    return result


def unit_active(config: Mapping[str, object], unit: str, language: str) -> bool:
    if unit == SPACY_UNIT:
        return config["artifact_ner"] == "spacy" or (config["artifact_ner"] == "dslim" and language != "en")
    if unit == DSLIM_UNIT:
        return config["artifact_ner"] == "dslim" and language == "en"
    for key, extra_unit in EXTRA_UNITS.items():
        if unit == extra_unit:
            scope = config["extra"][key]
            return scope == "all" or (scope == "en" and language == "en")
    scope = config["scope"][unit]
    if scope == "native":
        return language in predefined_units()[unit]
    return scope == "all"


def threshold_key(unit: str, entity: str) -> str:
    return f"{unit}|{entity}"


def pool_entities() -> set[str]:
    """Every entity any non-GLiNER unit can emit (GLiNER's identity entries)."""
    entities = set()
    for language in space.LANGUAGES:
        for _unit, recognizer in base_recognizers(language):
            entities.update(recognizer.supported_entities)
    entities.update(space.DSLIM["mapping"].values())
    entities.update(space.EXTRA_NER["davlan"]["label_mapping"].values())
    entities.update(openmed_mapping().values())
    return entities


def live_analyzers(config: Mapping[str, object], paths: ModelPaths) -> dict[str, object]:
    """One AnalyzerEngine per language holding exactly the configuration's recognizers,
    thresholds (Presidio's `score_thresholds`) and context enhancer."""
    from presidio_analyzer import AnalyzerEngine, RecognizerRegistry

    engines = nlp_engines(paths, dslim=config["artifact_ner"] == "dslim")
    extras = Extras(paths, pool_entities())
    analyzers = {}
    for language in space.LANGUAGES:
        members = []
        for unit, recognizer in base_recognizers(language):
            if unit == SPACY_UNIT and config["artifact_ner"] == "dslim" and language == "en":
                unit = DSLIM_UNIT  # the same recognizer, reading the transformers engine's entities
            if unit_active(config, unit, language):
                members.append((unit, recognizer))
        for key, unit in EXTRA_UNITS.items():
            if unit_active(config, unit, language):
                members.append((unit, extras.recognizer(key, language)))
        registry = RecognizerRegistry(supported_languages=[language])
        for unit, recognizer in members:
            prefix = unit + "|"
            recognizer.score_thresholds = {"default": 0.0, **{
                key[len(prefix):]: value for key, value in config["thresholds"].items() if key.startswith(prefix)}}
            registry.add_recognizer(recognizer)
        analyzers[language] = AnalyzerEngine(
            nlp_engine=engines[language], registry=registry, supported_languages=[language],
            context_aware_enhancer=enhancer(config["context"]), default_score_threshold=0.0,
        )
    return analyzers


def live_analyze(analyzers: Mapping[str, object], config: Mapping[str, object], text: str, language: str) -> list:
    return analyzers[language].analyze(
        text=text, language=language, score_threshold=None,
        allow_list=list(config["allow_list"]) or None, allow_list_match="exact",
    )
