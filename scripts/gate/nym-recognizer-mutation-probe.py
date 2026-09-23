#!/usr/bin/env python3
"""Mutation probe for the Nym recognizer (single-pass Stage A, solo todo 3738).

A test that cannot fail guards nothing. Each case below breaks one property the Stage A
contract relies on, runs the tests that pin it, and requires exactly the named tests to go
red; then the source is restored byte for byte. A final pass reruns every command on the
restored tree and requires green.

Every edit site must occur exactly once in its file (the probe refuses otherwise), and the
probe refuses to run over uncommitted changes in any file it edits, because it restores
from its own copy and a dirty file would hide a mutation's effect.

Run from the repository root under the pinned toolchain (the repo wrapper exports it):

    python3 scripts/gate/nym-recognizer-mutation-probe.py            # every case
    python3 scripts/gate/nym-recognizer-mutation-probe.py memo-bypass # one case
"""

from __future__ import annotations

import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FEATURES = "safety-net-nym,test-support"
LIB = ["cargo", "test", "-p", "gaze-recognizers", "--features", FEATURES, "--lib", "nym_recognizer"]
PIPELINE = ["cargo", "test", "-p", "gaze-recognizers", "--features", FEATURES, "--test", "nym_recognizer"]
BENCH = [
    "cargo", "test", "-p", "gaze-recognizers", "--features", "safety-net-nym",
    "--example", "clean_for_bench", "single_pass_policy",
]
RECOGNIZER = "crates/gaze-recognizers/src/nym_recognizer.rs"
RESOLVER = "crates/gaze/src/resolver.rs"
PIPELINE_RS = "crates/gaze/src/pipeline.rs"
PIPELINE_TESTS = "crates/gaze-recognizers/tests/nym_recognizer.rs"
BENCH_RS = "crates/gaze-recognizers/examples/clean_for_bench.rs"


@dataclass(frozen=True)
class Case:
    name: str
    why: str
    file: str
    old: str
    new: str
    commands: tuple[tuple[str, ...], ...]
    red: frozenset[str]


CASES = (
    Case(
        "memo-bypass",
        "drop the request-scoped inference result: every adapter runs the model itself",
        RECOGNIZER,
        "        ctx.memo()\n            .get_or_try_insert_with(MEMO_OWNER, &self.memo_key(input), || self.checked(input))\n",
        "        let _ = (ctx.memo(), MEMO_OWNER);\n        self.checked(input).map(Rc::new)\n",
        (tuple(LIB), tuple(PIPELINE)),
        frozenset({
            "nym_recognizer::tests::one_inference_per_request_across_every_adapter",
            "one_inference_per_request_and_none_stale_across_requests",
            "concurrent_requests_never_share_an_inference",
        }),
    ),
    Case(
        "memo-key-ignores-input",
        "key the shared result without the input digest: a reused context answers for new text",
        RECOGNIZER,
        "            input.as_bytes(),\n            self.model_revision.as_bytes(),\n",
        "            self.model_revision.as_bytes(),\n",
        (tuple(LIB),),
        frozenset({"nym_recognizer::tests::a_new_request_never_sees_the_previous_result"}),
    ),
    Case(
        "span-check-off",
        "accept any model span: impossible output no longer fails closed",
        RECOGNIZER,
        "    let invalid = |message: &str| SafetyNetError::InvalidOutput {\n        message: message.to_string(),\n    };\n    if span.start >= span.end\n",
        "    let invalid = |message: &str| SafetyNetError::InvalidOutput {\n        message: message.to_string(),\n    };\n    if true {\n        return Ok(());\n    }\n    if span.start >= span.end\n",
        (tuple(LIB),),
        frozenset({"nym_recognizer::tests::impossible_model_output_fails_closed"}),
    ),
    Case(
        "priority-highest",
        "give Nym candidates the highest rule priority: a rule no longer wins the ladder",
        RECOGNIZER,
        "pub const NYM_RECOGNIZER_PRIORITY: i32 = i32::MIN;",
        "pub const NYM_RECOGNIZER_PRIORITY: i32 = i32::MAX;",
        (tuple(PIPELINE),),
        frozenset({
            "a_rule_wins_the_same_span_against_a_nym_candidate",
            "a_rule_wins_a_partial_overlap_and_the_nym_remainder_stays_protected",
        }),
    ),
    Case(
        "nym-not-learned",
        "drop Nym from the learned evidence tier: a Nym span swallows a plain-regex candidate",
        RESOLVER,
        "        || candidate\n            .source\n            .split('+')\n            .all(|part| part.starts_with(NYM_RECOGNIZER_SOURCE_PREFIX))\n",
        "        || (candidate.source.is_empty() && NYM_RECOGNIZER_SOURCE_PREFIX.is_empty())\n",
        (tuple(PIPELINE),),
        frozenset({"a_nym_span_never_swallows_a_contained_rule_candidate"}),
    ),
    Case(
        "structured-rung-unguarded",
        "let the structured-containment rung hand a learned custom span a validated email",
        RESOLVER,
        "    if is_learned(container) && !is_learned(enclosed) {\n        return None;\n    }\n",
        "",
        (tuple(PIPELINE),),
        frozenset({"a_nym_span_never_swallows_a_contained_validated_email"}),
    ),
    Case(
        "feed-raw",
        "hand recognizers the raw text instead of the normalized text",
        PIPELINE_RS,
        "            .detect_candidate_pool(&normalized.text, &ctx)?;",
        "            .detect_candidate_pool(text, &ctx)?;",
        (tuple(PIPELINE),),
        frozenset({"the_model_reads_normalized_text_and_spans_map_back_to_raw_bytes"}),
    ),
    Case(
        "learned-class-preserve",
        "the pipeline tests' policy gives the learned classes `preserve`",
        PIPELINE_TESTS,
        "            overrides.push((class, Action::Tokenize));",
        "            overrides.push((class, Action::Preserve));",
        (tuple(PIPELINE),),
        frozenset({
            "a_lone_nym_candidate_is_one_token_of_its_class_with_nym_provenance",
            "one_inference_per_request_and_none_stale_across_requests",
        }),
    ),
    Case(
        "bench-learned-class-default",
        "the single-pass bench policy stops naming the learned classes",
        BENCH_RS,
        "    for class in learned_classes {\n        if seen.insert(class.clone()) {\n            rules.push(RuleSpec::Class {\n                class: class.clone(),\n                action: Action::Tokenize,\n",
        "    for class in learned_classes {\n        if seen.insert(class.clone()) {\n            rules.push(RuleSpec::Class {\n                class: class.clone(),\n                action: Action::Preserve,\n",
        (tuple(BENCH),),
        frozenset({"tests::single_pass_policy_declares_an_explicit_tokenize_rule_per_learned_class"}),
    ),
)

RESULT = re.compile(r"^test (\S+) \.\.\. (ok|FAILED|ignored)$")


def run(command: tuple[str, ...]) -> tuple[int, dict[str, str]]:
    completed = subprocess.run(
        [*command, "--", "--test-threads", "4"] if "--" not in command else list(command),
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    results = {}
    for line in completed.stdout.splitlines():
        match = RESULT.match(line.strip())
        if match:
            results[match.group(1)] = match.group(2)
    return completed.returncode, results


def require_clean(files: set[str]) -> None:
    dirty = subprocess.run(
        ["git", "status", "--porcelain", "--", *sorted(files)],
        cwd=ROOT, capture_output=True, text=True, check=True,
    ).stdout.strip()
    if dirty:
        sys.exit(f"FATAL: uncommitted changes in files the probe edits:\n{dirty}")


def main(argv: list[str]) -> int:
    cases = [case for case in CASES if not argv or case.name in argv]
    if argv and len(cases) != len(argv):
        sys.exit(f"unknown case in {argv}")
    require_clean({case.file for case in cases})
    failures = 0
    for case in cases:
        path = ROOT / case.file
        original = path.read_text(encoding="utf-8")
        if original.count(case.old) != 1:
            sys.exit(f"FATAL {case.name}: edit site occurs {original.count(case.old)} times (the probe is stale)")
        try:
            path.write_text(original.replace(case.old, case.new, 1), encoding="utf-8")
            failed: set[str] = set()
            ran: set[str] = set()
            for command in case.commands:
                _, results = run(command)
                ran |= set(results)
                failed |= {name for name, outcome in results.items() if outcome == "FAILED"}
        finally:
            path.write_text(original, encoding="utf-8")
        missing = case.red - failed
        not_run = case.red - ran
        verdict = "RED as required" if not missing else "SURVIVED"
        print(f"{case.name}: {verdict} — {case.why}")
        print(f"  required red: {sorted(case.red)}")
        print(f"  went red:     {sorted(failed)}")
        if not_run:
            print(f"  not run (build failure?): {sorted(not_run)}")
        failures += bool(missing)
    print("restored tree: rerunning every command")
    for command in sorted({command for case in cases for command in case.commands}):
        code, results = run(command)
        bad = sorted(name for name, outcome in results.items() if outcome == "FAILED")
        print(f"  {' '.join(command[3:])}: exit {code}, {len(results)} tests, failed {bad}")
        failures += code != 0 or not results
    print("mutation probe:", "PASS" if failures == 0 else f"FAIL ({failures})")
    return 0 if failures == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
