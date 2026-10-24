#!/usr/bin/env python3
"""Declared hook graph, admission checks, and generated guide tables."""
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
class ClockGroup:
    id: str
    phase: str
    parents: tuple[str, ...]
    children: tuple[str, ...]
    relationship: str
    hooks: str
    exception: str = ""
    daemon_only: bool = False


# Aggregate clocks need pooled parents: the protocol does not retain which
# constructor/view contributed each leaf. Alternative groups express partial
# overlaps without treating inclusive clocks as disjoint intervals.
CLOCK_GRAPH = (
    ClockGroup("ready-build", "startup", ("process_to_ready_ns",),
               ("pipeline_build_ns", "nym_attach_ns"), "serial",
               "pipeline/run.rs: prepare_clean_pipeline; cli/perf.rs: Document::from_env"),
    ClockGroup("ready-policy", "startup", ("process_to_ready_ns",),
               ("process_to_policy_ns", "assembly_ns", "nym_attach_ns"), "serial",
               "pipeline/build.rs: policy_loaded before assembly; pipeline/run.rs: Nym after build",
               "pipeline_build overlaps process_to_policy; use two alternative serial groups"),
    ClockGroup("ready-daemon", "startup", ("process_to_ready_ns",),
               ("daemon_startup_ns",), "overlapping",
               "commands/daemon.rs: run; cli/perf.rs: ready",
               "daemon_startup overlaps the policy prefix and contains pipeline construction"),
    ClockGroup("policy-load", "startup", ("process_to_policy_ns",),
               ("policy_load_ns",), "overlapping", "pipeline/build.rs: resolve_pipeline_builder"),
    ClockGroup("pipeline", "startup", ("pipeline_build_ns",),
               ("policy_load_ns", "assembly_ns"), "serial",
               "pipeline/build.rs: resolve_pipeline and resolve_pipeline_builder"),
    ClockGroup("daemon-build", "startup", ("daemon_startup_ns",),
               ("pipeline_build_ns", "nym_attach_ns"), "serial",
               "commands/daemon.rs: run and Daemon::new", daemon_only=True),
    ClockGroup("assembly", "startup", ("assembly_ns",),
               ("recognizer_construction_ns", "model_constructor_ns", "rule_construction_ns"),
               "serial", "gaze-assembly/lib.rs: build_pipeline_builder_inner"),
    ClockGroup("nym-attach", "startup", ("nym_attach_ns",),
               ("nym_outer_verify_ns", "nym_model_constructor_ns"), "serial",
               "gaze-assembly/lib.rs: attach_nym_safety_net"),
    ClockGroup("constructor-loads", "startup", ("model_constructor_ns", "nym_model_constructor_ns"),
               ("tokenizer_load_ns", "model_load_ns"), "serial",
               "ner/detector.rs; ner/backend/ort.rs; dob_judge.rs; safety_net/nym/ort.rs",
               "leaf totals pool NER, DOB and Nym constructors; no per-constructor leaf split exists"),
    ClockGroup("constructor-leaves", "startup",
               ("model_constructor_ns", "nym_model_constructor_ns", "nym_outer_verify_ns"),
               ("bundle_verify_ns", "tokenizer_load_ns", "model_load_ns"), "serial",
               "ner/loader.rs; bundle.rs; gaze-assembly/lib.rs; constructor hooks above",
               "bundle_verify also includes Nym outer verification; load-only group excludes that capacity"),
    ClockGroup("request-sweep", "request", ("request_ns",),
               ("registry_ns", "resolve1_ns", "sweep_ns", "safety_view_build_ns", "safety_model_ns",
                "manifest_ns", "audit_ns"), "serial",
               "pipeline.rs: redact_text_with_manifest_uncached, sweep_repeat_values, run_safety_nets_in_context, log_*; "
               "pipeline/occurrence.rs: Ledger; safety_net_scan.rs: new",
               "resolve2 partly runs inside sweep and partly afterwards; never sum sweep + resolve2"),
    ClockGroup("request-resolve", "request", ("request_ns",),
               ("registry_ns", "resolve1_ns", "resolve2_ns", "safety_view_build_ns", "safety_model_ns",
                "manifest_ns", "audit_ns"), "serial",
               "pipeline.rs: recovery::plan at sweep, house-number and address-piece sites",
               "alternative serial group accounts for resolve2 without double counting sweep"),
    ClockGroup("request-overlap", "request", ("request_ns",),
               ("validators_ns",), "overlapping", "gaze-types/lib.rs: ValidatorKind",
               "validators run inside recognizers, registry vetoes, recovery and other request work"),
    ClockGroup("registry", "request", ("registry_ns",),
               ("rules_ns", "ner_tokenize_ns", "ner_mutex_wait_ns", "ner_ort_ns"), "serial",
               "registry.rs: detect_candidate_pool/detect_declared; perf.rs: RecognizerSpan; "
               "ner/backend/preparation.rs and ort.rs",
               "rules sums rule-evidence recognizers; NER is learned evidence; validators may overlap both"),
    ClockGroup("nym-views", "request", ("safety_model_ns",),
               ("nym_stable_ns", "nym_neutral_ns"), "serial",
               "pipeline.rs: run_safety_nets_in_context; safety_net/nym/mod.rs: check_with_neutral_and_telemetry"),
    ClockGroup("nym-inference", "request", ("nym_stable_ns", "nym_neutral_ns"),
               ("nym_tokenize_ns", "nym_mutex_wait_ns", "nym_ort_ns"), "serial",
               "safety_net/nym/ort.rs: score_pieces and run_window",
               "leaf totals pool both serial views; the protocol does not split leaves by view"),
)


@dataclass(frozen=True)
class ClockCheck:
    id: str
    phase: str
    children: tuple[str, ...]
    parents: tuple[str, ...]
    kind: str
    daemon_only: bool


def clock_checks():
    edges = set()
    for group in CLOCK_GRAPH:
        for child in group.children:
            edge = (group.phase, child, group.parents, group.daemon_only)
            if edge not in edges:
                edges.add(edge)
                yield ClockCheck(f"{group.id}-parent-{child}", group.phase, (child,),
                                 group.parents, "parent", group.daemon_only)
        if group.relationship == "serial":
            yield ClockCheck(f"{group.id}-serial", group.phase, group.children,
                             group.parents, "serial", group.daemon_only)
        elif group.relationship != "overlapping":
            raise ValueError(f"unknown clock relationship: {group.relationship}")


CLOCK_CHECKS = tuple(clock_checks())


def documentation() -> str:
    rows = ["<!-- BEGIN GENERATED CLOCK CONTRACT -->",
            "| Metric | Phase | Boundary |", "| --- | --- | --- |"]
    for key, description in STARTUP_CLOCKS.items():
        rows.append(f"| `{key}` | startup | {description} |")
    rows.append("| `request_ns` | request | One-shot processing after startup, or one daemon handle_line call |")
    rows.extend(["", "The hook graph below declares serial children (sum <= parent capacity) or",
                 "overlapping children (each <= parent capacity, with no sum). Admission",
                 "checks every parent edge and every serial sum. A parent capacity containing",
                 "multiple clocks pools their durations; it is not a new measured clock.",
                 "Only daemon-build is conditional on daemon mode.", "",
                 "| Group | Relationship | Children | Parent capacity | Hook sites and exceptions |",
                 "| --- | --- | --- | --- | --- |"])
    for group in CLOCK_GRAPH:
        children = " + ".join(f"`{key}`" for key in group.children)
        parents = " + ".join(f"`{key}`" for key in group.parents)
        note = group.hooks + (". " + group.exception if group.exception else "")
        rows.append(f"| {group.id} | {group.relationship} | {children} | {parents} | {note} |")
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
