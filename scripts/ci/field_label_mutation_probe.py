#!/usr/bin/env python3
"""Disable each field-labelled record-ID protection and require a behavioural test failure.

Every mutant edits committed source sites, runs one named test, and must make that test
fail (exit 101 with `test result: FAILED`). The two producer mutants run with the typed
refusal fixture blessed, so the fixture drift check cannot kill them: the first-path-only
emitter must fail the live Rust checker itself, and with that checker call also removed
the Rust test passes, writes the incomplete fixture, and the Python guard must then fail
on it with an assertion failure. Sources must match HEAD first, so a probe never runs over
uncommitted fixes; every file is restored afterwards.
"""
import argparse
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
RECOGNIZER = ROOT / "crates/gaze-recognizers/src/field_label.rs"
OUTCOMES = ROOT / "crates/gaze/src/pipeline/outcomes.rs"
PROTECTION = ROOT / "crates/gaze/src/pipeline/protection.rs"
PIPELINE = ROOT / "crates/gaze/src/pipeline.rs"
FIELD_CONTEXT = ROOT / "crates/gaze/src/field_context.rs"
CODEC = ROOT / "crates/gaze-proxy/src/codecs/anthropic.rs"
SERVER = ROOT / "crates/gaze-proxy/src/server.rs"
RULEPACK = ROOT / "crates/gaze/src/rulepack.rs"
NORMALIZE = ROOT / "crates/gaze-types/src/normalize.rs"
DISPATCH = ROOT / "crates/gaze-mcp-core/src/dispatch.rs"
BENCH = ROOT / "crates/gaze-recognizers/examples/clean_for_bench.rs"
FIXTURE = ROOT / "crates/gaze-recognizers/tests/fixtures/typed_refusal_producer.json"
BENCH_TEST = ("gaze-recognizers", ["--example", "clean_for_bench"])
LIVE_GUARD = "tests::typed_refusal_producer_guard_requires_every_refused_span"
FIRST_PATH_ONLY = (
    "std::iter::once((*reason, field_path)).chain(others.iter().map(|(r, p)| (*r, p)));",
    "std::iter::once((*reason, field_path)).chain(others.iter().take(0).map(|(r, p)| (*r, p)));")
LIVE_CHECK = (
    """            assert_eq!(
                check_typed_refusal_response(&response, &expected),
                Ok(true),
                "{id}: every case must refuse with complete pinned evidence"
            );
""", "")
PYTHON_GUARD = ("test_typed_refusal_producer.TypedRefusalProducerGuardTests."
                "test_identity_rows_name_every_refused_range_and_round_trip")
BLESS = {"GAZE_BLESS_TYPED_REFUSAL_FIXTURE": "1"}
ASSEMBLY_TEST = ("gaze-assembly", ["--test", "field_labelled_record_ids"])
PROXY_TEST = ("gaze-proxy", ["--test", "structured_field_record_ids"])
PROXY_LIB = ("gaze-proxy", ["--lib"])
MCP_TEST = ("gaze-mcp-core", ["--test", "structured_field_record_ids"])
RECOGNIZER_LIB = ("gaze-recognizers", ["--lib"])

# name, file, before, after, (package, target), test
# Producer mutants: name, file, edits, (package, target), test, extra environment,
# and the Python guard test that must fail after a passing Rust run (None: Rust must fail).
PRODUCER_MUTANTS = (
    ("producer_lists_first_field_only", BENCH, (FIRST_PATH_ONLY,), BENCH_TEST, LIVE_GUARD,
     BLESS, None),
    ("producer_live_check_removed", BENCH, (FIRST_PATH_ONLY, LIVE_CHECK), BENCH_TEST, LIVE_GUARD,
     BLESS, PYTHON_GUARD),
)
MUTANTS = (
    ("binder_disabled", RECOGNIZER,
     "if !self.binds(&field.path.segments()) {",
     "if true || !self.binds(&field.path.segments()) {",
     ASSEMBLY_TEST, "labelled_strings_protect_with_shape_and_restore"),
    ("hint_promoted", RECOGNIZER,
     "FieldEvidence::ValidatedParse | FieldEvidence::CallerDeclared\n        );",
     "FieldEvidence::ValidatedParse | FieldEvidence::CallerDeclared | FieldEvidence::Hint(_)\n        );",
     RECOGNIZER_LIB,
     "field_label::tests::hints_never_protect_and_fragments_never_prove_a_complete_id"),
    ("number_rendered_as_string", RECOGNIZER,
     "        JsonScalarType::Number => Carrier::Number,\n        JsonScalarType::Bool | JsonScalarType::Null => Carrier::Literal,\n        _ => Carrier::Text(parsed),",
     "        JsonScalarType::Bool | JsonScalarType::Null => Carrier::Literal,\n        _ => Carrier::Text(parsed),",
     ASSEMBLY_TEST, "labelled_numbers_refuse_and_name_every_refused_field"),
    ("refusal_drops_field_path", OUTCOMES,
     ".map(|field| field.path)",
     ".map(|_| gaze_types::FieldPath::default())",
     ASSEMBLY_TEST, "labelled_numbers_refuse_and_name_every_refused_field"),
    ("refusal_drops_further_fields", OUTCOMES,
     "            others: fields.collect(),",
     "            others: fields.take(0).collect(),",
     ASSEMBLY_TEST, "labelled_numbers_refuse_and_name_every_refused_field"),
    ("possibly_sensitive_ignores_policy", OUTCOMES,
     "PossiblySensitiveDisposition::PassAudited => passed.push(candidate),",
     "PossiblySensitiveDisposition::PassAudited => kept.push(candidate),",
     ASSEMBLY_TEST, "possibly_sensitive_values_follow_the_outcomes_disposition"),
    ("winner_audit_outcome_lost", PIPELINE,
     "        if let Some(outcome) = detection.outcome {\n            entry = entry.with_protection_outcome(outcome);\n        }",
     "",
     ASSEMBLY_TEST, "audit_rows_carry_outcomes_and_field_evidence"),
    ("repeat_into_number", OUTCOMES,
     "if !hits.iter().any(|hit| bound.contains(&hit.class)) {",
     "if true || !hits.iter().any(|hit| bound.contains(&hit.class)) {",
     ASSEMBLY_TEST, "repeats_into_numbers_and_keys_refuse_typed"),
    ("repeat_into_key", FIELD_CONTEXT,
     "carriers.keys.push((key, member.value.path.clone()));",
     "let _ = key;",
     ASSEMBLY_TEST, "repeats_into_numbers_and_keys_refuse_typed"),
    ("boundary_refusal_mapping_lost", PROTECTION,
     "                Error::Protection(error @ ProtectionError::Untransformable { .. }) => error,\n",
     "",
     ASSEMBLY_TEST, "leaf_protection_keeps_typed_refusals_at_the_boundary"),
    ("anthropic_number_unscanned", CODEC,
     "if ctx.protect_field(spelling, &field)? == spelling {",
     "if { let _ = &field; true } {",
     PROXY_TEST, "anthropic_structured_calls_protect_strings_and_refuse_labelled_numbers"),
    ("legacy_number_unscanned", SERVER,
     "                    self.admit_labelled_number(field, &spelling)?;\n",
     "",
     PROXY_TEST, "gemini_structured_calls_and_responses_protect_strings_and_refuse_labelled_numbers"),
    # Field keys compare through the shared detection normalization (fullwidth, U+200B).
    ("label_normalization_bypassed", NORMALIZE,
     "    normalize(label)\n        .text\n        .chars()",
     "    label\n        .chars()",
     ASSEMBLY_TEST, "normalized_key_spellings_bind_and_benign_ones_do_not"),
    ("generic_label_validation_unnormalized", RULEPACK,
     "let folded = gaze_types::normalize::fold_field_label(label);",
     "let folded = label.to_lowercase();",
     ("gaze-pii", ["--lib"]),
     "rulepack::tests::field_labelled_rejects_generic_labels_in_every_normalized_spelling"),
    # Array indices are transparent between a bound parent and its value key.
    ("array_index_binds_as_key", RECOGNIZER,
     ".filter(|segment| !is_index(segment))",
     ".filter(|segment| !segment.is_empty())",
     ASSEMBLY_TEST, "arrays_between_a_label_and_its_value_keep_the_binding"),
    # An owned token splitting a labelled leaf never exposes the rest of the identifier,
    # and a fragment is never promoted to a proven complete identifier.
    ("fragment_ignored", RECOGNIZER,
     "Some(FieldContext::Fragment { field, .. }) => {",
     "Some(FieldContext::Fragment { field, .. }) if false => {",
     ASSEMBLY_TEST, "owned_tokens_in_a_labelled_leaf_never_expose_the_rest"),
    ("fragment_promoted", RECOGNIZER,
     "let shaped = complete\n",
     "let shaped = (complete || true)\n",
     RECOGNIZER_LIB,
     "field_label::tests::fragments_of_a_bound_leaf_cover_their_sensitive_bytes_only"),
    # Every pre-output refusal, direct and repeat (D6), is named in one error.
    ("direct_refusal_before_sweep", OUTCOMES,
     "        Ok((crate::resolver::CandidatePool::new(kept), Refusals(refused)))",
     "        if !refused.is_empty() {\n"
     "            return Err(self.refuse(target, refused, field_name, document_kind));\n"
     "        }\n"
     "        Ok((crate::resolver::CandidatePool::new(kept), Refusals(refused)))",
     ASSEMBLY_TEST, "direct_and_repeat_refusals_are_named_together"),
    ("possible_refusal_audited_as_confirmed", OUTCOMES,
     "if candidate.outcome.is_none() {",
     "if true {",
     ASSEMBLY_TEST, "refused_possible_values_keep_their_outcome_in_the_audit"),
    # JSON-text carriers (OpenAI `arguments`, MCP string arguments) bind their own fields.
    ("proxy_json_text_unbound", SERVER,
     "let field = if embedded.is_empty() { field } else { None };",
     "let field = if embedded.is_empty() { field } else { None };\n"
     "        let embedded: Vec<(std::ops::Range<usize>, gaze_types::FieldProvenance)> = Vec::new();",
     PROXY_TEST, "openai_serialized_arguments_and_tool_results_are_protected"),
    ("proxy_json_text_fragment_dropped", SERVER,
     "gaze::FieldPiece::Fragment { field, offset } => {\n"
     "                    Some(gaze_types::FieldContext::Fragment {\n"
     "                        field,\n"
     "                        offset: *offset,\n"
     "                    })",
     "gaze::FieldPiece::Fragment { field, offset } => {\n"
     "                    let _ = (field, offset);\n"
     "                    None",
     PROXY_LIB,
     "server::json_carrier_tests::owned_token_in_a_labelled_argument_never_exposes_the_rest"),
    ("mcp_json_text_unbound", DISPATCH,
     "pipeline.json_text_field_spans(text, path)",
     "{ let _ = path; Vec::new() }",
     MCP_TEST, "mcp_owned_tokens_and_json_text_arguments_bind_with_stock_core"),
    # A JSON-text carrier whose own key is a label stays one labelled value.
    ("labelled_carrier_unbound", PROTECTION,
     "if self.registry.binds_field_path(&carrier.segments()) {",
     "if false && self.registry.binds_field_path(&carrier.segments()) {",
     MCP_TEST, "mcp_owned_tokens_and_json_text_arguments_bind_with_stock_core"),
    # Unicode dash punctuation folds like `-` in a label.
    ("label_unicode_dash_kept", NORMALIZE,
     "'_' | '.' | '-' | '\\u{2010}'..='\\u{2015}' | '\\u{2212}' | '\\u{FE58}' | '\\u{FE63}'",
     "'_' | '.' | '-'",
     ASSEMBLY_TEST, "normalized_key_spellings_bind_and_benign_ones_do_not"),
    # A fragment never starts inside a JSON escape of its carrier.
    ("fragment_splits_escape", RECOGNIZER,
     "if backslashes % 2 == 1 {",
     "if false && backslashes % 2 == 1 {",
     PROXY_LIB,
     "server::json_carrier_tests::owned_token_in_a_labelled_argument_never_exposes_the_rest"),
    # A bound repeat overlapping a JSON number or key refuses; it is never dropped raw.
    ("partial_overlap_dropped", OUTCOMES,
     "                Some((_, path, reason)) => {",
     "                Some((range, _, _)) if range != raw => {}\n"
     "                Some((_, path, reason)) => {",
     ASSEMBLY_TEST, "repeats_into_numbers_and_keys_refuse_typed"),
)
ALL = tuple((name, path, ((before, after),), target, test, {}, None)
            for name, path, before, after, target, test in MUTANTS) + PRODUCER_MUTANTS



def run_python_guard(test):
    environment = dict(os.environ, PYTHONDONTWRITEBYTECODE="1")
    return subprocess.run(
        ["uv", "run", "--project", ".", "--python", "3.13", "--locked",
         "python", "-m", "unittest", test],
        cwd=ROOT / "scripts/bench", env=environment,
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe", action="append", default=[],
                        help="Run only this named mutation (repeatable)")
    args = parser.parse_args()
    if len(ALL) != 28:
        raise SystemExit("mutant roster changed")
    unknown = set(args.probe) - {mutant[0] for mutant in ALL}
    if unknown:
        raise SystemExit(f"unknown probes: {sorted(unknown)}")
    selected = [m for m in ALL if not args.probe or m[0] in args.probe]
    originals = {path: path.read_bytes() for path in {m[1] for m in ALL} | {FIXTURE}}
    for path, original in originals.items():
        relative = path.relative_to(ROOT).as_posix()
        if original != subprocess.check_output(["git", "show", f"HEAD:{relative}"], cwd=ROOT):
            raise SystemExit(f"Commit {relative} before probing; source must match HEAD")
    for name, path, edits, *_ in ALL:
        for before, _ in edits:
            if originals[path].decode().count(before) != 1:
                raise SystemExit(f"{name}: expected exactly one mutation site")
    base = dict(os.environ, RUSTUP_TOOLCHAIN="1.96.0", GAZE_DISABLE_OS_KEYSTORE="1")
    base.setdefault("CARGO_BUILD_JOBS", "2")
    for variable in BLESS:
        base.pop(variable, None)
    output = ROOT / "target/field-label-checks/mutations"
    output.mkdir(parents=True, exist_ok=True)
    for index, (name, path, edits, (package, target), test, extra, python) in enumerate(
            selected, 1):
        print(f"MUTATION {index}/{len(selected)}: {name}", flush=True)
        source = originals[path].decode()
        for before, after in edits:
            source = source.replace(before, after, 1)
        try:
            path.write_text(source)
            result = subprocess.run(
                ["nice", "-n", "10", "cargo", "test", "-p", package, *target, test,
                 "--", "--exact"],
                cwd=ROOT, env=dict(base, **extra),
                stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            (output / f"{name}.log").write_bytes(result.stdout)
            if python is None:
                if result.returncode != 101 or b"test result: FAILED" not in result.stdout:
                    raise SystemExit(
                        f"{name}: mutation did not fail its regression; inspect {output}")
            else:
                # The Rust run must pass and rewrite the fixture, proving the Python guard
                # is the oracle that catches the incomplete producer.
                if (result.returncode != 0
                        or b"test result: ok. 1 passed" not in result.stdout
                        or FIXTURE.read_bytes() == originals[FIXTURE]):
                    raise SystemExit(
                        f"{name}: Rust run did not pass and rewrite the fixture; inspect {output}")
                guard = run_python_guard(python)
                (output / f"{name}.python.log").write_bytes(guard.stdout)
                if (guard.returncode != 1 or b"FAILED (failures=1)" not in guard.stdout
                        or b"AssertionError" not in guard.stdout):
                    raise SystemExit(
                        f"{name}: Python guard did not fail with an assertion; inspect {output}")
            print(f"MUTATION {index}/{len(selected)}: {name} rejected", flush=True)
        finally:
            path.write_bytes(originals[path])
            FIXTURE.write_bytes(originals[FIXTURE])
    print(f"FIELD_LABEL_MUTATIONS {len(selected)}/{len(selected)} rejected")


if __name__ == "__main__":
    main()
