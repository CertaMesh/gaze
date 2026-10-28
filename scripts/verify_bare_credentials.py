#!/usr/bin/env python3
"""Mutate issuer boundaries and private-key framing; require behavioral failures.

Run from a committed checkout with RUSTC/RUSTDOC and CARGO_BUILD_JOBS set.
The original rulepack bytes are restored even when Cargo or an assertion fails.
"""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]
RULEPACK = ROOT / "crates/gaze-recognizers/embedded/secrets.toml"
TEST = "crates/gaze-recognizers/tests/bare_credentials.rs"
BOUNDARY_TEST = "issuer_boundaries_protect_framing_and_preserve_identifiers"
PEM_TEST = "truncated_mismatched_and_framed_private_keys_are_whole_and_restorable"
PREFIX_TEST = "tool_diff_and_gutter_prefixed_private_keys_stay_whole"
HEADER_TEST = "literal_armour_headers_accept_free_text_and_extension_names"
PROBES = [
    ("bare-disabled", "security_token.bare", "enabled = true", "enabled = false", "bare_vendor_prefixes_are_whole_restorable_credentials"),
    ("opaque-ghs-dot", "security_token.bare", r"|\.(?:\z|[^A-Za-z0-9_\\-])", "", "bare_vendor_prefixes_are_whole_restorable_credentials"),
    ("pem-disabled", "security_token.pem", "enabled = true", "enabled = false", "private_pem_blocks_include_headers_body_and_footer"),
    ("start-boundary", "security_token.bare", r"\A|", "", BOUNDARY_TEST),
    ("end-boundary", "security_token.bare", r"\z|", "", BOUNDARY_TEST),
    ("ascii-boundary", "security_token.bare", r"[^a-z0-9\\]|", "", BOUNDARY_TEST),
    ("leading-boundary-fp", "security_token.bare", r'(?:\A|[^a-z0-9\\]|\\(?:[ntrbf"\\/]|u[0-9A-Fa-f]{4})|%[0-9A-Fa-f]{2})', "", BOUNDARY_TEST),
    ("google-near-prefix-fp", "security_token.bare", "AIzaSy", "AIza", "prose_and_public_material_remain_untouched"),
    ("json-boundary", "security_token.bare", r'\\(?:[ntrbf"\\/]|u[0-9A-Fa-f]{4})', r'\\q', BOUNDARY_TEST),
    ("json-slash-boundary", "security_token.bare", r'[ntrbf"\\/]', r'[ntrbf"\\]', BOUNDARY_TEST),
    ("unicode-boundary", "security_token.bare", r"|u[0-9A-Fa-f]{4}", "", BOUNDARY_TEST),
    ("url-boundary", "security_token.bare", r"|%[0-9A-Fa-f]{2}", "", BOUNDARY_TEST),
    ("pem-literal-newline", "security_token.pem", r"(?:\r{0,2}\n|", "(?:", PEM_TEST),
    ("pem-json-newline", "security_token.pem", r"\\(?:r\\)?n|", "", PEM_TEST),
    ("pem-nested-json-newline", "security_token.pem", r"\\\\(?:r\\\\)?n|", "", PEM_TEST),
    ("pem-unicode-newline", "security_token.pem", r"(?:\\u000[dD])?\\u000[aAdD]|", "", PEM_TEST),
    ("pem-html-newline", "security_token.pem", r"(?i:<br\x20*/?>)|", "", PEM_TEST),
    ("pem-shell-newline", "security_token.pem", r"|\\\r?\n", "", PEM_TEST),
    ("pem-string-join", "security_token.pem", r"""(?:["'][\x20\t]*[+,]?[\x20\t]*(?:\r?\n[\x20\t]*)?["'])?""", "", PEM_TEST),
    ("pem-comma", "security_token.pem", "[+,]?", r"\+?", PEM_TEST),
    ("pem-url-label", "security_token.pem", r"(?:\x20|%20)", r"\x20", PEM_TEST),
    ("pem-url-newline", "security_token.pem", "|%0[aAdD]", "", PEM_TEST),
    ("pem-space-joined", "security_token.pem", r"|[\x20\t]+", "", PEM_TEST),
    ("pem-comment", "security_token.pem", r"\#|", "", PEM_TEST),
    ("pem-slash-comment", "security_token.pem", "//|", "", PEM_TEST),
    ("pem-quote", "security_token.pem", ">|", "", PEM_TEST),
    ("pem-star", "security_token.pem", r"|\*", "", PEM_TEST),
    ("suffix-evidence-disabled", "security_token.bare", r"(?x)\A(?:\A|", r"(?x)\b\B(?:\A|", "word_tails_require_digit_and_contiguous_suffix_evidence"),
    ("suffix-digit-disabled", "security_token.bare", r"[A-Za-z_.-]+", r"\b\B", "word_tails_require_digit_and_contiguous_suffix_evidence"),
    ("suffix-run-disabled", "security_token.bare", r"(?:[A-Za-z0-9]{0,15}[_-])*[A-Za-z0-9]{0,15}", r"\b\B", "word_tails_require_digit_and_contiguous_suffix_evidence"),
    ("generic-sk-alphabet-widened", "security_token.bare", r"(sk-[A-Za-z0-9]{32,}[A-Za-z0-9_-]*)", r"(sk-[A-Za-z0-9_-]{32,}[A-Za-z0-9_-]*)", "issuer_alphabets_keep_separator_dense_bodies_whole"),
    ("issuer-alphabet-narrowed", "security_token.bare", r"(?:[A-Za-z0-9_-]{0,15}[.])*[A-Za-z0-9_-]{0,15}", r"(?:[A-Za-z0-9]{0,15}[_.-])*[A-Za-z0-9]{0,15}", "issuer_alphabets_keep_separator_dense_bodies_whole"),
    ("pem-gutter-disabled", "security_token.pem", r"|[0-9]+(?:[\x20\t]*[:|][\x20\t]*|[\x20\t]+)", "", "numbered_bulleted_and_entity_framed_private_keys_stay_whole"),
    ("pem-entity-disabled", "security_token.pem", r"|(?:(?i:&\#(?:0*13|x0*d);))?(?:(?i:&\#(?:0*10|x0*a);)|&NewLine;)", "", "numbered_bulleted_and_entity_framed_private_keys_stay_whole"),
    ("pem-minimum", "security_token.pem", "{16}", "{1}", PEM_TEST),
    ("pem-prose-fp", "security_token.pem", r"(?:[A-Za-z0-9+/=]|\\/|%2[BbFf]|%3[Dd]){16}", r"(?:[A-Za-z0-9+/=][\x20\t\r\n]*){16}", "prose_and_public_material_remain_untouched"),
    ("pem-truncation", "security_token.pem", "-----|", r"-----|\b\B", PEM_TEST),
    ("pem-header-allowlist", "security_token.pem", r"[A-Za-z][A-Za-z0-9-]*:[^\r\n]*", r"(?:Proc-Type:[\x20\t]*4,ENCRYPTED|DEK-Info:[\x20\t]*[A-Za-z0-9-]+,[0-9A-Fa-f]+|(?:Version|Comment|Charset):[^\r\n\\<>\"\']*)", "literal_armour_headers_accept_free_text_and_extension_names"),
    ("pem-serialized-header-boundary", "security_token.pem", r'''[^\r\n\\<"\']|''', r"[^\r\n]|", "literal_armour_headers_accept_free_text_and_extension_names"),
    ("pem-header-indentation", "security_token.pem", r"[\x20\t]*[A-Za-z][A-Za-z0-9-]*:", "[A-Za-z][A-Za-z0-9-]*:", "literal_armour_headers_accept_free_text_and_extension_names"),
    ("pem-header-json-escapes", "security_token.pem", r'''|\\(?:["\\/bft]|u(?:[1-9A-Fa-f][0-9A-Fa-f]{3}|0[1-9A-Fa-f][0-9A-Fa-f]{2}|00[1-9A-Fa-f][0-9A-Fa-f]|000[0-9BCEFbcef]))''', "", "literal_armour_headers_accept_free_text_and_extension_names"),
    ("pem-two-unit-unicode-newline", "security_token.pem", r"(?:\\u000[dD])?", "", "literal_armour_headers_accept_free_text_and_extension_names"),
    ("pem-double-cr", "security_token.pem", r"\r{0,2}\n", r"\r?\n", "literal_armour_headers_accept_free_text_and_extension_names"),
    ("pem-generic-prefix", "security_token.pem", "{0,80}", r"{0,80}\b\B", PREFIX_TEST),
    ("pem-prefix-bound", "security_token.pem", "{0,80}", "{0,20}", PREFIX_TEST),
    ("pem-prefix-dash", "security_token.pem", r"→!%\-]", "→!%]", PREFIX_TEST),
    ("pem-prefix-colon", "security_token.pem", r"[\x20\t:;|│┃→!%\-]", r"[\x20\t;|│┃→!%\-]", PREFIX_TEST),
    ("pem-prefix-arrow", "security_token.pem", r"┃→!%\-]", r"┃!%\-]", PREFIX_TEST),
    ("pem-prefix-space", "security_token.pem", r"[\x20\t:;|│┃→!%\-]", r"[:;|│┃→!%\-]", PREFIX_TEST),
    ("pem-prefix-percent", "security_token.pem", r"→!%\-]", r"→!\-]", PREFIX_TEST),
    ("pem-prefix-dash-run-guard", "security_token.pem", "-{1,4}", "-+", PREFIX_TEST),
    ("pem-prefix-quote-guard", "security_token.pem", r"[\x20\t:;|│┃→!%\-]", r"""[\x20\t:;|│┃→!%"\-]""", "prose_and_public_material_remain_untouched"),
    ("pem-short-last-line", "security_token.pem", "{1,31}", r"{1,31}\b\B", PREFIX_TEST),
    ("pem-ended-alternative", "security_token.pem", "-----END", r"\b\B-----END", PREFIX_TEST),
    ("pem-line-suffix", "security_token.pem", r"[\x20\t]*(?:[│┃|\]\}\-]{1,4}[\x20\t]*)?", "", PREFIX_TEST),
    ("pem-word-diff-suffix", "security_token.pem", r"|\]\}", r"|\}", PREFIX_TEST),
    ("pem-entity-tab", "security_token.pem", r"|(?i:&\#(?:0*9|x0*9);|&Tab;)", "", PREFIX_TEST),
    ("pem-paragraph", "security_token.pem", r"(?i:</p>[\x20\t\r\n]*<p>)|", "", PREFIX_TEST),
    ("pem-serialized-markup", "security_token.pem", r'''|<(?:[^bB\r\n\\<"\']|[bB](?:[^rR\r\n\\<"\']|[rR][^\x20/>\r\n\\<"\']))''', "", HEADER_TEST),
    ("pem-xml-entities", "security_token.pem", r"(?:(?i:&\#(?:0*13|x0*d);))?(?:(?i:&\#(?:0*10|x0*a);)|&NewLine;)", r"(?i:&\#(?:10|x0a);)", "numbered_bulleted_and_entity_framed_private_keys_stay_whole"),
]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe", choices=[probe[0] for probe in PROBES])
    args = parser.parse_args()
    selected_probes = [probe for probe in PROBES if args.probe is None or probe[0] == args.probe]
    if os.environ.get("CARGO_BUILD_JOBS") != "2":
        raise SystemExit("Set CARGO_BUILD_JOBS=2 before running mutation probes")
    subprocess.run(["git", "diff", "--exit-code", "HEAD", "--", str(RULEPACK), TEST], cwd=ROOT, check=True)
    commit = subprocess.check_output(["git", "cat-file", "-p", "HEAD"], cwd=ROOT)
    if b"\ngpgsig " not in commit:
        raise SystemExit("Commit the implementation with a signature before probing")
    original = RULEPACK.read_bytes()
    logs = Path(tempfile.mkdtemp(prefix="gaze-bare-credential-mutations-"))
    try:
        for name, rule, before, after, test in selected_probes:
            sections = original.decode().split("[[recognizers]]")
            selected = [i for i, section in enumerate(sections) if f'id = "{rule}"\n' in section]
            if len(selected) != 1:
                raise AssertionError(f"expected one rule {rule}")
            index = selected[0]
            if before not in sections[index]:
                raise AssertionError(f"{name}: mutation no longer identifies its target")
            sections[index] = sections[index].replace(before, after)
            RULEPACK.write_text("[[recognizers]]".join(sections))
            log = logs / (name + ".log")
            with log.open("w") as output:
                result = subprocess.run(["nice", "-n", "10", "cargo", "test", "-p", "gaze-recognizers", "--test", "bare_credentials", test, "--", "--exact"], cwd=ROOT, stdout=output, stderr=subprocess.STDOUT)
            text = log.read_text()
            if result.returncode != 101 or "whole credential ownership" not in text or "1 failed" not in text:
                raise AssertionError(f"{name}: expected behavioral failure, inspect {log}")
            print(f"{name}: mutation makes {test} fail; {log}", flush=True)
            RULEPACK.write_bytes(original)
    finally:
        RULEPACK.write_bytes(original)
    print(f"MUTATION DONE: {len(selected_probes)} mutations caught; source restored")


if __name__ == "__main__":
    main()
