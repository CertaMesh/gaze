"""Retain the independently observed GLiNER reload used by tuned inference."""
from contextlib import nullcontext
from pathlib import Path
import pool
import space
import subject_identity as identity
from pool import *

def openmed_mapping():
    from theirbench import presidio_research_repro as repro
    return dict(repro.OPENMED_MAPPING)

class Extras(pool.Extras):
    def __init__(self, paths, entities, *, gliner_plan=None):
        super().__init__(paths, entities)
        self.gliner_plan = gliner_plan
        self.gliner_receipt = None

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
            # The constructor already loads once. The original explicit load
            # replaces that model; only its return is retained across languages.
            with identity.gliner_identity.observe(self.gliner_plan) if key == "gliner" else nullcontext() as capture:
                recognizer.load()
            if key == "gliner":
                self.gliner_receipt = capture.receipt(recognizer.gliner) if capture is not None else None
            recognizer.is_loaded = True
            self._loaded[key] = recognizer
        return self._loaded[key]


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


def live_analyzers(config: Mapping[str, object], paths: ModelPaths, *, gliner_plan=None) -> dict[str, object]:
    """One AnalyzerEngine per language holding exactly the configuration's recognizers,
    thresholds (Presidio's `score_thresholds`) and context enhancer."""
    from presidio_analyzer import AnalyzerEngine, RecognizerRegistry

    engines = nlp_engines(paths, dslim=config["artifact_ner"] == "dslim")
    extras = Extras(paths, pool_entities(), gliner_plan=gliner_plan)
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
