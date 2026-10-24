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
REQUEST_ASSEMBLY = frozenset(("assembly_ns", "recognizer_construction_ns", "rule_construction_ns"))
CLOCK_PHASES = {**dict.fromkeys(STARTUP_CLOCKS, ("startup",)),
                **dict.fromkeys((*PROCESSING_CLOCKS, "request_ns"), ("request",))}
CLOCK_PHASES.update(dict.fromkeys(REQUEST_ASSEMBLY, ("startup", "request")))
RECOGNIZERS = "recognizers[*]"


def clock_values(record: dict, key: str) -> tuple[int, ...]:
    """Expand recognizer ordinals without treating their total as an emitted metric."""
    if key == RECOGNIZERS:
        return tuple(metric["ns"] for metric in record["recognizers"].values())
    return (record["metrics"].get(key, {}).get("ns", 0),)


def violates_check(record: dict, check) -> bool:
    children = [ns for key in check.children for ns in clock_values(record, key)]
    capacity = sum(ns for key in check.parents for ns in clock_values(record, key))
    return (sum(children) > capacity if check.kind == "serial"
            else any(ns > capacity for ns in children))


RECORD_CHECKS = ("metric-candidates", "ner-encodes", "ner-windows", "nym-windows", "nym-views", "sweep-counter-owner", "parent-invocations")


def violates_record(record: dict, check: str) -> bool:
    metrics, counters = record["metrics"], record["counters"]
    if check == "sweep-counter-owner":
        return counters["sweep_candidates"] > 0 and metrics["sweep_ns"]["invocations"] == 0
    if check == "parent-invocations":
        def invoked(key):
            return (any(entry["invocations"] for entry in record["recognizers"].values())
                    if key == RECOGNIZERS else metrics[key]["invocations"] > 0)
        return any(any(invoked(key) for key in edge.children)
                   and not any(invoked(key) for key in edge.parents)
                   for edge in CLOCK_CHECKS if edge.phase == record["phase"]
                   and edge.kind == "parent" and not edge.daemon_only)
    if check == "metric-candidates":
        return any(metric["candidates"] for key, metric in metrics.items() if key != "rules_ns")
    if check == "ner-encodes":
        return counters["ner_encodes"] != metrics["ner_tokenize_ns"]["invocations"]
    if check == "ner-windows":
        return counters["ner_windows"] != metrics["ner_ort_ns"]["invocations"]
    if check == "nym-windows":
        return counters["nym_windows"] != metrics["nym_ort_ns"]["invocations"]
    if check == "nym-views":
        return counters["nym_windows"] != counters["nym_windows_stable"] + counters["nym_windows_neutral"]
    raise ValueError("unknown record check")


def active_models(config: dict) -> dict[str, bool]:
    """Mirror RawNerPolicy/parse_dob_judge and CLI safety-net selection."""
    return {"ner": config.get("ner", {}).get("model_dir") is not None,
            "dob": config.get("dob_judge", {}).get("enabled", False) is True,
            "nym": config.get("safety_net", {}).get("backend", "none") == "nym"}


OWNERSHIP_CHECKS = ("recognizer-invocations", "rule-owner-ns", "rule-owner-invocations", "rule-owner-candidates",
                    "ner-owner-capacity", "inactive-nym", "recognizer-role", "stable-recognizer-role")


def violates_ownership(record: dict, check: str, *, config: dict, roles: dict) -> bool:
    entries = record["recognizers"]
    if check == "recognizer-invocations":
        return any(entry["invocations"] == 0 for entry in entries.values())
    if check.startswith("rule-owner-"):
        field = check.removeprefix("rule-owner-")
        return record["metrics"].get("rules_ns", {}).get(field, 0) != sum(
            entry[field] for entry in entries.values() if entry["evidence"] == "rule")
    if check == "ner-owner-capacity":
        owners = [entry for entry in entries.values() if entry["model"] == "ner"]
        leaf_metrics = [record["metrics"].get(key, {}) for key in
                        ("ner_tokenize_ns", "ner_mutex_wait_ns", "ner_ort_ns")]
        if any(any(metric.values()) for metric in leaf_metrics) and len(owners) != 1:
            return True
        leaves = sum(metric.get("ns", 0) for metric in leaf_metrics)
        return leaves > sum(entry["ns"] for entry in owners)
    if check == "inactive-nym":
        return not active_models(config)["nym"] and any(
            any(metric.values()) for key, metric in record["metrics"].items()
            if key.startswith("nym_") or key in ("safety_model_ns", "safety_view_build_ns"))
    if check == "recognizer-role":
        combined = {**roles, **{key: (entry["evidence"], entry["model"])
                                for key, entry in entries.items()}}
        for model in ("ner", "dob"):
            count = sum(role[1] == model for role in combined.values())
            if count > int(active_models(config)[model]):
                return True
        return any((entry["evidence"], entry["model"]) not in
                   {("rule", "none"), ("learned", "ner"), ("learned", "dob")}
                   for entry in entries.values())
    if check == "stable-recognizer-role":
        return any(key in roles and roles[key] != (entry["evidence"], entry["model"])
                   for key, entry in entries.items())
    raise ValueError("unknown recognizer ownership check")


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
               ("assembly_ns", "registry_ns", "resolve1_ns", "sweep_ns", "safety_view_build_ns", "safety_model_ns",
                "manifest_ns", "audit_ns"), "serial",
               "pipeline.rs: redact_text_with_manifest_uncached, sweep_repeat_values, run_safety_nets_in_context, log_*; "
               "pipeline/occurrence.rs: Ledger; safety_net_scan.rs: new",
               "resolve2 partly runs inside sweep and partly afterwards; never sum sweep + resolve2"),
    ClockGroup("request-resolve", "request", ("request_ns",),
               ("assembly_ns", "registry_ns", "resolve1_ns", "resolve2_ns", "safety_view_build_ns", "safety_model_ns",
                "manifest_ns", "audit_ns"), "serial",
               "pipeline.rs: recovery::plan at sweep, house-number and address-piece sites",
               "alternative serial group accounts for resolve2 without double counting sweep"),
    ClockGroup("request-assembly", "request", ("assembly_ns",),
               ("recognizer_construction_ns", "rule_construction_ns"), "serial",
               "commands/daemon.rs: clean_request; gaze-assembly/lib.rs: pipeline_with_context",
               "context rebuilds recognizers and rules; resident NER, Nym and DOB handles are reused",
               daemon_only=True),
    ClockGroup("request-overlap", "request", ("request_ns",),
               ("validators_ns",), "overlapping", "gaze-types/lib.rs: ValidatorKind",
               "validators run inside recognizers, registry vetoes, recovery and other request work"),
    ClockGroup("registry-recognizers", "request", ("registry_ns",),
               (RECOGNIZERS,), "serial",
               "registry.rs: detect_candidate_pool/detect_declared; perf.rs: RecognizerSpan",
               "built-in clean runs recognizers serially inside registry"),
    ClockGroup("recognizer-work", "request", (RECOGNIZERS,),
               ("rules_ns", "ner_tokenize_ns", "ner_mutex_wait_ns", "ner_ort_ns"), "serial",
               "perf.rs: RecognizerSpan::drop; ner/backend/preparation.rs and ort.rs",
               "necessary aggregate bound; the ownership checks below additionally enforce exact Rule totals "
               "and containment within the single declared NER ordinal"),
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
        rows.append(f"| `{key}` | {' or '.join(CLOCK_PHASES[key])} | {description} |")
    rows.append("| `request_ns` | request | One-shot processing after startup, or one daemon handle_line call |")
    rows.extend(["", "The hook graph below declares serial children (sum <= parent capacity) or",
                 "overlapping children (each <= parent capacity, with no sum). Admission",
                 "checks every parent edge and every serial sum. A parent capacity containing",
                 "multiple clocks pools their durations; it is not a new measured clock.",
                 "Daemon-build and request-assembly apply only in daemon mode.",
                 "`recognizers[*]` expands to each emitted recognizer ordinal. Parent edges",
                 "check each ordinal; serial groups sum them. As a parent, it pools their durations.", "",
                 "| Group | Phase | Relationship | Children | Parent capacity | Hook sites and exceptions |",
                 "| --- | --- | --- | --- | --- | --- |"])
    for group in CLOCK_GRAPH:
        children = " + ".join(f"`{key}`" for key in group.children)
        parents = " + ".join(f"`{key}`" for key in group.parents)
        note = group.hooks + (". " + group.exception if group.exception else "")
        rows.append(f"| {group.id} | {group.phase} | {group.relationship} | {children} | {parents} | {note} |")
    rows.extend(["", "A startup record must have zero processing durations, invocation/candidate",
                 "counts, work counters and recognizer entries. Daemon context requests may",
                 "emit assembly, recognizer-construction and rule-construction clocks, once each.",
                 "One-shot requests cannot emit those clocks. All request records must have",
                 "zero model initialization and other startup-only durations and counts.", "",
                 "Schema 3 recognizer entries contain fixed `evidence` (`rule` or `learned`)",
                 "and `model` (`none`, `ner`, `dob`, `other`) labels, never declared IDs or input values.",
                 "Schema 3 requires every declared metric and counter and all three u64 metric fields.",
                 "Only `rules_ns` may have candidates. `ner_encodes`, `ner_windows` and",
                 "`nym_windows` equal their tokenize/ORT invocation counts; Nym window views",
                 "sum to the total. Every emitted recognizer has at least one invocation,",
                 "even with zero duration. Invoked child clocks require an invoked parent,",
                 "including zero-duration clocks; sweep candidates require a sweep invocation.",
                 "NER activates only with `ner.model_dir`; DOB only with `dob_judge.enabled=true`.",
                 "Nym activates only with `safety_net.backend=\"nym\"`. Only active model paths",
                 "enter the inventory; every active model must pin an absolute directory.",
                 "Admission requires `rules_ns` duration, invocations and candidates to equal the",
                 "corresponding sums of Rule entries. Learned entries never contribute to rules.",
                 "The three serial NER leaves must fit the single NER entry; they cannot borrow",
                 "DOB capacity. At most one NER and one DOB ordinal are allowed, only when configured.",
                 "Other learned recognizers, contradictory labels and ordinal role changes across",
                 "requests refuse admission. These checks are linear in the emitted entries.",
                 "",
                 "| Ownership check | Requirement |",
                 "| --- | --- |",
                 "| recognizer-invocations | Every emitted entry was invoked |",
                 "| inactive-nym | Safety-net processing requires the active Nym backend |",
                 "| rule-owner-ns | Exact Rule duration sum |",
                 "| rule-owner-invocations | Exact Rule invocation sum |",
                 "| rule-owner-candidates | Exact Rule candidate sum |",
                 "| ner-owner-capacity | Serial NER leaves fit the NER ordinal |",
                 "| recognizer-role | Configured single NER/DOB; other learned roles refused |",
                 "| stable-recognizer-role | One role per ordinal across the capture |",
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
