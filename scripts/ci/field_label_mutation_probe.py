#!/usr/bin/env python3
"""Disable each field-labelled record-ID protection and require a behavioural test failure.

Every mutant edits exactly one committed source site, runs one named test, and must make
that test fail (exit 101 with `test result: FAILED`). Sources must match HEAD first, so a
probe never runs over uncommitted fixes; every file is restored afterwards.
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
BENCH = ROOT / "crates/gaze-recognizers/examples/clean_for_bench.rs"
ASSEMBLY_TEST = ("gaze-assembly", ["--test", "field_labelled_record_ids"])
PROXY_TEST = ("gaze-proxy", ["--test", "structured_field_record_ids"])

# name, file, before, after, (package, target), test
MUTANTS = (
    ("binder_disabled", RECOGNIZER,
     "if !self.binds(&field.path.segments()) {",
     "if true || !self.binds(&field.path.segments()) {",
     ASSEMBLY_TEST, "labelled_strings_protect_with_shape_and_restore"),
    ("hint_promoted", RECOGNIZER,
     "FieldEvidence::ValidatedParse | FieldEvidence::CallerDeclared\n        );",
     "FieldEvidence::ValidatedParse | FieldEvidence::CallerDeclared | FieldEvidence::Hint(_)\n        );",
     ("gaze-recognizers", ["--lib"]),
     "field_label::tests::hints_never_protect_and_fragments_never_bind"),
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
    ("producer_lists_first_field_only", BENCH,
     "std::iter::once((*reason, field_path)).chain(others.iter().map(|(r, p)| (*r, p)));",
     "std::iter::once((*reason, field_path)).chain(others.iter().take(0).map(|(r, p)| (*r, p)));",
     ("gaze-recognizers", ["--example", "clean_for_bench"]),
     "tests::typed_refusal_producer_names_every_refused_span_end_to_end"),
)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe", action="append", default=[],
                        help="Run only this named mutation (repeatable)")
    args = parser.parse_args()
    unknown = set(args.probe) - {mutant[0] for mutant in MUTANTS}
    if unknown:
        raise SystemExit(f"unknown probes: {sorted(unknown)}")
    selected = [m for m in MUTANTS if not args.probe or m[0] in args.probe]
    originals = {m[1]: m[1].read_bytes() for m in MUTANTS}
    for path, original in originals.items():
        relative = path.relative_to(ROOT).as_posix()
        if original != subprocess.check_output(["git", "show", f"HEAD:{relative}"], cwd=ROOT):
            raise SystemExit(f"Commit {relative} before probing; source must match HEAD")
    for name, path, before, _, _, _ in MUTANTS:
        if originals[path].decode().count(before) != 1:
            raise SystemExit(f"{name}: expected exactly one mutation site")
    environment = dict(os.environ, RUSTUP_TOOLCHAIN="1.96.0", GAZE_DISABLE_OS_KEYSTORE="1")
    environment.setdefault("CARGO_BUILD_JOBS", "2")
    output = ROOT / "target/field-label-checks/mutations"
    output.mkdir(parents=True, exist_ok=True)
    for index, (name, path, before, after, (package, target), test) in enumerate(selected, 1):
        print(f"MUTATION {index}/{len(selected)}: {name}", flush=True)
        source = originals[path].decode()
        try:
            path.write_text(source.replace(before, after, 1))
            result = subprocess.run(
                ["nice", "-n", "10", "cargo", "test", "-p", package, *target, test,
                 "--", "--exact"],
                cwd=ROOT, env=environment, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            (output / f"{name}.log").write_bytes(result.stdout)
            if result.returncode != 101 or b"test result: FAILED" not in result.stdout:
                raise SystemExit(f"{name}: mutation did not fail its regression; inspect {output}")
            print(f"MUTATION {index}/{len(selected)}: {name} rejected", flush=True)
        finally:
            path.write_bytes(originals[path])
    print(f"FIELD_LABEL_MUTATIONS {len(selected)}/{len(selected)} rejected")


if __name__ == "__main__":
    main()
