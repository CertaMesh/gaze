#!/usr/bin/env python3
"""One library-only dictionary cache sequence extension, using the canonical scorer.

Capture each layer/control as an ordinary observation record. Its storage layer
is C (the recorder's standalone corpus slot), not a claim about the Kiji corpus.
Attach replays as K/B extension layers alongside a full C/A/D/R scorecard.
"""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
import random
import subprocess
from dataclasses import dataclass
from pathlib import Path
from unittest.mock import patch

import agentic_layers as agentic
import gaze_bench_score as score
import scorecard_record as records
from dictionary_sequence_generators import (
    DEFAULT_GENERATOR, GeneratorProvenanceError, resolve as resolve_generator,
)

LAYERS = ("K", "B")
SEQUENCES = ("switch", "fresh", "literal")
VERSION = 1
SEED = 20260710
ROOT = Path(__file__).resolve().parents[2]
RUST_ADAPTER = Path("crates/gaze-recognizers/examples/support/dictionary_sequence.rs")


def digest(value):
    return hashlib.sha256(json.dumps(value, ensure_ascii=False, sort_keys=True,
                                    separators=(",", ":")).encode()).hexdigest()


@dataclass(frozen=True)
class Case:
    document: score.Document
    terms: tuple[str, ...]
    literal_spans: tuple[score.Span, ...]
    layer: str

    def context(self):
        return json.dumps({"dictionaries": {"sequence": {
            "terms": list(self.terms), "case_sensitive": True,
        }}}, ensure_ascii=False)


def generate(seed=SEED, partition="published"):
    if partition not in ("published", "calibration"):
        raise ValueError("unknown partition")
    rng = random.Random(f"dictionary-sequences-v{VERSION}/{seed}/{partition}")
    marker = "Zav" if partition == "published" else "Qev"
    cases = []
    # Artificial two-word names: never sourced from people or runtime rulepacks.
    syllables = ("vo", "zi", "qa", "nu", "xe", "pa", "ri", "ko")
    accents = ("É", "Ü", "Ö", "Å", "Straß", "ASCII")
    surfaces = (
        ("prose", "Résumé: ", ".\n"),
        ("tool", 'Tool returned «', '».\n'),
        ("json", '{"résultat":"', '"}\n'),
    )
    for number in range(36):
        terms = []
        for index in range(1 + number % 4):
            nonce = "".join(rng.choices(syllables, k=5))
            terms.append(f"{marker}{nonce} {accents[number % len(accents)]}{index}lune")
        rng.shuffle(terms)
        terms = tuple(terms)
        for surface, prefix, suffix in surfaces:
            for variant in ("original", "lower", "upper"):
                for layer in LAYERS:
                    parts, gold, literal, offset = [prefix], [], [], len(prefix.encode())
                    for term in terms:
                        value = term if variant == "original" else getattr(term, variant)()
                        # A first-character substitution cannot be a case variant.
                        near = " ".join("W" + word[1:] for word in value.split())
                        assert all(near.casefold() != t.casefold() for t in terms)
                        tokens = [(value, True), (near, False), (value, True)] if layer == "K" else [
                            (near, False), ("X" + value + "Y", False),
                        ]
                        for token, positive in tokens:
                            start, end = offset, offset + len(token.encode())
                            parts.extend([token, "; "])
                            offset = end + 2
                            if positive:
                                gold.append(score.Span(start, end, "FIRSTNAME"))
                                if token in terms:
                                    literal.append(gold[-1])
                    parts.append(suffix)
                    uid = f"dictseq/{partition}/{number}/{surface}/{variant}/{layer}"
                    doc = score.Document(uid, "".join(parts), "en", "US",
                                         "gaze-dictionary-sequences", tuple(gold),
                                         negative_category="dictionary-near-miss" if layer == "B" else None)
                    cases.append(Case(doc, terms, tuple(literal), layer))
    return cases


def documents(cases, sequence):
    return [case.document if sequence != "literal" else
            score.Document(**{**case.document.__dict__, "spans": case.literal_spans})
            for case in cases]


def identity(cases, seed, partition, adapter_root=ROOT):
    return {
        "version": VERSION, "seed": seed, "partition": partition,
        "layers": list(LAYERS), "sequences": list(SEQUENCES),
        "adapter_sha256": score.sha256_file(adapter_root / RUST_ADAPTER),
        "generator_sha256": DEFAULT_GENERATOR,
        "corpus_sha256": digest([
            [c.document.uid, c.document.text, c.terms,
             [(s.start, s.end, s.label) for s in c.document.spans],
             [(s.start, s.end, s.label) for s in c.literal_spans]] for c in cases
        ]),
    }


def capture(repo, binary, probe, output, seed=SEED, partition="published"):
    # Bind the observations to a fresh build of this checkout, rather than trusting
    # a caller-supplied executable that may predate the adapter or runtime changes.
    profile = binary.parent.parent.name
    if (binary.name != "clean_for_bench" or binary.parent.name != "examples"
            or profile not in ("debug", "release") or not binary.is_relative_to(repo)):
        raise ValueError("producer must be a checkout-owned debug/release example")
    command = ["rustup", "run", "1.96.0", "cargo", "build", "--locked",
               "-p", "gaze-recognizers", "--example", "clean_for_bench", "--all-features",
               "--target-dir", str(binary.parents[2])]
    if profile == "release":
        command.append("--release")
    subprocess.run(command, cwd=repo, check=True)
    generator = resolve_generator(DEFAULT_GENERATOR)
    cases = generator.generate(seed, partition)
    manifest = {"identity": identity(cases, seed, partition),
                "runtime": score.git_metadata(repo),
                "binary_sha256": score.sha256_file(binary), "records": {}}
    manifest["producer_binding"] = {
        "toolchain": subprocess.check_output(
            ["rustup", "run", "1.96.0", "rustc", "--version"], text=True).strip(),
        "profile": profile, "fresh_locked_build": True,
        "example_source_sha256": score.sha256_file(repo /
            "crates/gaze-recognizers/examples/clean_for_bench.rs"),
        "cargo_lock_sha256": score.sha256_file(repo / "Cargo.lock"),
    }
    # A historical overlay must be byte-identical to the committed adapter.
    if score.sha256_file(repo / RUST_ADAPTER) != manifest["identity"]["adapter_sha256"]:
        raise ValueError("runtime adapter differs from the committed harness")
    output.mkdir(parents=True, exist_ok=True)
    base_transport = score.BenchSubprocess
    contexts = {case.document.uid: case.context() for case in cases}

    class Transport(base_transport):
        def exchange(self, request):
            return super().exchange({**request, "context_json": contexts[request["fixture_id"]]})

    for sequence in SEQUENCES:
        for layer in LAYERS:
            selected = [c for c in cases if c.layer == layer]
            docs = generator.documents(selected, sequence)
            corpus = digest([[d.uid, d.text, [(s.start, s.end, s.label) for s in d.spans]]
                             for d in docs])
            measurements = score.collect_validator_measurements(probe, docs, (d.uid for d in docs))
            writer = records.RecordWriter(docs, measurements, corpus_sha256=corpus, binary=binary)
            environment = {**os.environ, "GAZE_BENCH_DICTIONARY_SEQUENCE": sequence}
            with patch.object(score, "BenchSubprocess", Transport):
                run = score.run_config(
                    repo, binary, "rule-floor-core", docs, Path("."), None, None, None,
                    0.5, output / "logs", base_environment=environment,
                    validator_measurements=measurements, warmup_count=1,
                    record_document=lambda config, doc, response, measured:
                        writer.add("C", config, doc, response, measured),
                )
            population = score.population_summary(docs)
            card = score.assemble_scorecard(
                repo_root=repo, dataset_metadata={"name": "dictionary API sequence"},
                dataset_report={"integrity": {"sha256": corpus}},
                sampling_report={"available_population": population, "evaluated_population": population},
                parameters={"ner_threshold": 0.5, "dictionary_sequence": sequence},
                runs=[run], scored_label_contract=score.scored_label_contract_report(
                    score.SCORED_LABEL_CONTRACT_V1, docs),
            )
            path = output / f"{sequence}-{layer}.jsonl.gz"
            evidence = writer.write(path, card, add_reference=False)
            manifest["records"][f"{sequence}/{layer}"] = evidence
    (output / "capture.json").write_text(json.dumps(manifest, indent=2) + "\n")
    return manifest


class ReplayBindingError(ValueError):
    """A capture cannot attest the regenerated sequence inputs (no values exposed)."""

    def __init__(self):
        super().__init__("dictionary record does not match regenerated inputs")


def validate_binding(path, evidence, docs, sequence):
    header, observations = records._read(path)
    corpus = digest([[d.uid, d.text, [(s.start, s.end, s.label) for s in d.spans]]
                     for d in docs])
    card = header["scorecard"]
    inventory = [{**records._document_row(d), "layer": "C"}
                 for d in sorted(docs, key=lambda d: d.uid)]
    recorded = [{k: v for k, v in row.items() if k != "validator"}
                for row in header["documents"]]
    population = score.population_summary(docs)
    expected_observations = sorted(("C", "rule-floor-core", d.uid) for d in docs)
    actual_observations = sorted((row["layer"], row["config"], row["document_id"])
                                 for row in observations)
    if (card["parameters"].get("dictionary_sequence") != sequence
            or header["corpus_sha256"] != corpus
            or evidence.get("corpus_sha256") != corpus
            or recorded != inventory
            or card["dataset"].get("available_population") != population
            or card["dataset"].get("evaluated_population") != population
            or evidence.get("observations") != len(docs)
            or actual_observations != expected_observations
            or len(card["runs"]) != 1
            or card["runs"][0]["config"] != "rule-floor-core"):
        raise ReplayBindingError()


def replay(directory, contract):
    manifest = json.loads((directory / "capture.json").read_text())
    expected = {f"{sequence}/{layer}" for sequence in SEQUENCES for layer in LAYERS}
    if set(manifest["records"]) != expected:
        raise ValueError("incomplete dictionary sequence capture")
    declared = manifest["identity"]
    generator = resolve_generator(declared.get("generator_sha256"))
    try:
        cases = generator.generate(declared["seed"], declared["partition"])
        regenerated = identity(cases, declared["seed"], declared["partition"])
        regenerated["version"] = generator.VERSION
        regenerated["layers"] = list(generator.LAYERS)
        regenerated["sequences"] = list(generator.SEQUENCES)
        # Source provenance has been verified before regenerating historical inputs.
        inputs = ("version", "seed", "partition", "layers", "sequences", "corpus_sha256")
        if any(declared.get(field) != regenerated[field] for field in inputs):
            raise ReplayBindingError()
    except (KeyError, TypeError, ValueError):
        raise ReplayBindingError() from None
    bound_identity = {**declared, **{field: regenerated[field] for field in inputs}}
    result = {"identity": bound_identity,
              "contract": score.scored_label_contract_report(contract, [])["file_sha256"],
              "runtime": manifest["runtime"], "binary_sha256": manifest["binary_sha256"],
              "runs": {}, "evidence": manifest["records"]}
    result["producer_binding"] = manifest["producer_binding"]
    for key, evidence in manifest["records"].items():
        path = directory / evidence["file"]
        if score.sha256_file(path) != evidence["sha256"]:
            raise ValueError("dictionary observation digest mismatch")
        sequence, layer = key.split("/")
        docs = generator.documents([case for case in cases if case.layer == layer], sequence)
        try:
            validate_binding(path, evidence, docs, sequence)
        except (KeyError, TypeError, ValueError):
            raise ReplayBindingError() from None
        card = records.rescore(path, contract, max_workers=1)
        if card["gaze"] != manifest["runtime"]:
            raise ValueError("dictionary record runtime mismatch")
        result["runs"][key] = card["runs"][0]
    return result


def attach(full, extension):
    resolve_generator(extension.get("identity", {}).get("generator_sha256"))
    result = copy.deepcopy(full)
    if result["gaze"] != extension["runtime"]:
        raise ValueError("extension runtime differs from ordinary scorecard")
    expected = agentic.dictionary_sequence_primary_contract(result)
    if "contract" not in extension or expected != extension["contract"]:
        raise ValueError("extension contract differs from primary scoring")
    result["dictionary_sequence"] = extension
    return result


def install_adapter(repo):
    """Overlay only the committed example adapter on a clean release checkout."""
    if subprocess.check_output(["git", "status", "--porcelain"], cwd=repo, text=True).strip():
        raise ValueError("historical adapter requires a clean checkout")
    source = repo / "crates/gaze-recognizers/src/dictionary.rs"
    if "pub fn with_record_matching(" not in source.read_text():
        raise ValueError("release has no public record-matching API; no backport allowed")
    path = repo / "crates/gaze-recognizers/examples/clean_for_bench.rs"
    text = path.read_text()
    before = """        match handle_request_with_policy(
            config,
            full,
            request,
            policy_run.as_ref(),
            std::env::var_os("GAZE_BENCH_KNOWN_RECORD_ARM").is_some(),
        )? {"""
    after = """        let outcome = if let Ok(sequence) = std::env::var("GAZE_BENCH_DICTIONARY_SEQUENCE") {
            dictionary_sequence::handle(&sequence, request)?
        } else {
            handle_request_with_policy(
                config,
                full,
                request,
                policy_run.as_ref(),
                std::env::var_os("GAZE_BENCH_KNOWN_RECORD_ARM").is_some(),
            )?
        };
        match outcome {"""
    if text.count(before) != 1 or text.count("use sha2::{Digest, Sha256};") != 1:
        raise ValueError("release example needs a reviewed harness-only compatibility adapter")
    module = '\n#[path = "support/dictionary_sequence.rs"]\nmod dictionary_sequence;\n'
    path.write_text(text.replace(before, after).replace(
        "use sha2::{Digest, Sha256};", "use sha2::{Digest, Sha256};\n" + module))
    destination = repo / RUST_ADAPTER
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_bytes((ROOT / RUST_ADAPTER).read_bytes())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    measure = sub.add_parser("capture")
    measure.add_argument("--repo", type=Path, required=True)
    measure.add_argument("--binary", type=Path, required=True)
    measure.add_argument("--probe", type=Path, required=True)
    measure.add_argument("--output", type=Path, required=True)
    measure.add_argument("--seed", type=int, default=SEED)
    measure.add_argument("--partition", choices=("published", "calibration"), default="published")
    play = sub.add_parser("replay")
    play.add_argument("--capture", type=Path, required=True)
    play.add_argument("--scored-labels", type=Path)
    play.add_argument("--full-scorecard", type=Path)
    play.add_argument("--output", type=Path, required=True)
    install = sub.add_parser("install-adapter")
    install.add_argument("--repo", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "capture":
        capture(args.repo.resolve(), args.binary.resolve(), args.probe.resolve(),
                args.output, args.seed, args.partition)
    elif args.command == "install-adapter":
        install_adapter(args.repo.resolve())
    else:
        contract = score.load_scored_label_contract(args.scored_labels) if args.scored_labels else score.SCORED_LABEL_CONTRACT_V1
        result = replay(args.capture, contract)
        if args.full_scorecard:
            result = attach(json.loads(args.full_scorecard.read_text()), result)
        args.output.write_text(json.dumps(result, indent=2) + "\n")


if __name__ == "__main__":
    main()
