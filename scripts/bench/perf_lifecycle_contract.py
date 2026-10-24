#!/usr/bin/env python3
"""Declared clock phases and inclusive bounds; also generates the guide's tables."""
from dataclasses import dataclass
from pathlib import Path
import argparse


STARTUP_CLOCKS = {
    "process_to_ready_ns": "First operation in main through completion of startup",
    "process_to_policy_ns": "First operation in main through policy load and overrides",
    "policy_load_ns": "CLI policy load and overrides, including the policy-less default",
    "pipeline_build_ns": "CLI policy resolution through PipelineBuilder::build",
    "assembly_ns": "build_pipeline_builder_inner, including NER and DOB constructors",
    "recognizer_construction_ns": "Recognizer registration before model registration",
    "rule_construction_ns": "Policy action rule construction after model registration",
    "nym_attach_ns": "Nym outer verification, constructor and checked attachment",
    "model_constructor_ns": "NER and DOB constructors, including their children",
    "nym_model_constructor_ns": "Nym constructor, including its children",
    "nym_outer_verify_ns": "Assembly's extra Nym bundle verification call",
    "bundle_verify_ns": "All complete pinned-bundle verifications, including Nym's extra call",
    "tokenizer_load_ns": "All constructor tokenizer loads and configuration",
    "model_load_ns": "All constructor ORT session builds and model loads",
    "daemon_startup_ns": "Daemon entry through setup, before the request loop",
}
PROCESSING_CLOCKS = (
    "rules_ns", "registry_ns", "validators_ns", "ner_tokenize_ns", "ner_ort_ns",
    "ner_mutex_wait_ns", "resolve1_ns", "sweep_ns", "resolve2_ns", "safety_view_build_ns",
    "safety_model_ns", "nym_tokenize_ns", "nym_ort_ns", "nym_mutex_wait_ns", "nym_stable_ns",
    "nym_neutral_ns", "manifest_ns", "audit_ns",
)
CLOCK_PHASES = {**dict.fromkeys(STARTUP_CLOCKS, "startup"),
                **dict.fromkeys((*PROCESSING_CLOCKS, "request_ns"), "request")}


@dataclass(frozen=True)
class Bound:
    id: str
    phase: str
    children: tuple[str, ...]
    parents: tuple[str, ...]
    daemon_only: bool = False


BOUNDS = (
    Bound("policy-ready", "startup", ("process_to_policy_ns",), ("process_to_ready_ns",)),
    Bound("policy-load", "startup", ("policy_load_ns",), ("process_to_policy_ns",)),
    Bound("pipeline", "startup", ("policy_load_ns", "assembly_ns"), ("pipeline_build_ns",)),
    Bound("ready", "startup", ("pipeline_build_ns", "nym_attach_ns"), ("process_to_ready_ns",)),
    Bound("daemon-ready", "startup", ("daemon_startup_ns",), ("process_to_ready_ns",)),
    Bound("daemon-build", "startup", ("pipeline_build_ns", "nym_attach_ns"), ("daemon_startup_ns",), True),
    Bound("assembly", "startup", ("recognizer_construction_ns", "rule_construction_ns", "model_constructor_ns"), ("assembly_ns",)),
    Bound("nym-attach", "startup", ("nym_outer_verify_ns", "nym_model_constructor_ns"), ("nym_attach_ns",)),
    Bound("constructor-loads", "startup", ("tokenizer_load_ns", "model_load_ns"), ("model_constructor_ns", "nym_model_constructor_ns")),
    Bound("constructor-leaves", "startup", ("bundle_verify_ns", "tokenizer_load_ns", "model_load_ns"),
          ("model_constructor_ns", "nym_model_constructor_ns", "nym_outer_verify_ns")),
    *(Bound(key.removesuffix("_ns"), "request", (key,), ("request_ns",)) for key in PROCESSING_CLOCKS),
)


def documentation() -> str:
    rows = ["<!-- BEGIN GENERATED CLOCK CONTRACT -->",
            "| Metric | Phase | Boundary |", "| --- | --- | --- |"]
    for key, description in STARTUP_CLOCKS.items():
        rows.append(f"| `{key}` | startup | {description} |")
    rows.append("| `request_ns` | request | One-shot processing after startup, or one daemon handle_line call |")
    rows.extend(["", "Each row below requires the sum of the children to fit within the sum of",
                 "the parents. Only the daemon build bound is conditional on daemon mode.", "",
                 "| Bound | Serial children | Inclusive parents |", "| --- | --- | --- |"])
    for bound in BOUNDS:
        children = " + ".join(f"`{key}`" for key in bound.children)
        parents = " + ".join(f"`{key}`" for key in bound.parents)
        rows.append(f"| {bound.id} | {children} | {parents} |")
    rows.extend(["", "All metrics belong exclusively to their declared phase. A startup record",
                 "must have zero processing durations, invocation/candidate counts, work",
                 "counters and recognizer entries. Request records must have zero startup",
                 "durations and counts. Each recognizer duration must fit within request_ns.",
                 "<!-- END GENERATED CLOCK CONTRACT -->"])
    return "\n".join(rows)


def update_guide(*, check: bool) -> None:
    path = Path(__file__).with_name("perf-attribution.md")
    text = path.read_text()
    start = text.index("<!-- BEGIN GENERATED CLOCK CONTRACT -->")
    end = text.index("<!-- END GENERATED CLOCK CONTRACT -->") + len("<!-- END GENERATED CLOCK CONTRACT -->")
    updated = text[:start] + documentation() + text[end:]
    if check:
        if updated != text:
            raise ValueError("lifecycle guide differs from declared clock contract")
    else:
        path.write_text(updated)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    update_guide(check=args.check)
