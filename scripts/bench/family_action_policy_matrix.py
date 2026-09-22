#!/usr/bin/env python3
"""Policy-matrix enumeration for collision-family token actions (todo #3746).

Runs the #3708 IBAN document set (`iban_trailing_word_enumeration.documents()`:
89 registry countries x 2 BBAN alphabets x 4 IBANs x spaced/compact x 3 prefixes
x 13 trailers) through two `gaze daemon` binaries (base, head) under a matrix of
policies that differ only in how they treat the payment family, and compares,
per document, the class covering the IBAN span and the IBAN bytes protected.

Why: a family-level token (`custom:family:payment-card-or-iban`, emitted when no
IBAN cue is in range or when a Luhn-valid card run collides with the IBAN) used
to resolve its policy action by its own class. A policy that names only the
member classes with a `preserve` default matched no rule, so the whole span
shipped raw. Head derives the token's action from the member classes' rules
and the family's own default, strictest wins, unless an explicit family rule
exists.

The derivation is monotone (never laxer than the default the family would have
taken), so the invariant is: `lost_bytes` (IBAN bytes base protected that head
does not) is exactly 0 in EVERY arm, and only the member-only arms may change
any document at all. Every changed document is listed with the reason class:

    family-derived      base left the family token raw, head tokenizes it
                        (the fix; only under a member-only policy)

Any other transition is UNEXPLAINED and fails the run.

Arms (policy x locale):
    member-only-tokenize        tokenize iban, credit_card, phone, postal_code; default preserve
                                (the #3708 rule set; the footgun shape)
    all-preserve                preserve the same four; default preserve
                                (no over-protection: head must equal base)
    all-preserve-default-tokenize
                                preserve the four; default tokenize
                                (monotone: head must equal base, tokenized)
    explicit-family-rule        family tokenize + the four tokenize; default preserve
                                (override path: head must equal base)
    default-tokenize            the four tokenize; default tokenize
                                (protective default: head must equal base)
each under de-DE (core + locale-de) and en-US (core + locale-en).

Usage:
    python3 scripts/bench/family_action_policy_matrix.py BASE_BIN HEAD_BIN OUT.json

Exit status is 1 when any arm loses a byte, when an arm that must be identical
changes any document, when a changed document is not `family-derived`, when the
two binaries are the same build, or when a daemon response is missing.

IMPORTANT: run against immutable copies of the binaries; the SHA-256 of each is
recorded once at start-up.
"""

from __future__ import annotations

import json
import sys
import tempfile
from collections import Counter, defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import iban_trailing_word_enumeration as base_enum  # noqa: E402

MEMBER_CLASSES = ("custom:iban", "custom:credit_card", "custom:phone", "custom:postal_code")
FAMILY_CLASS = "custom:family:payment-card-or-iban"
FAMILY_TOKEN_CLASS = "family:payment-card-or-iban"

RULE = """
[[rule]]
kind = "class"
class = "{cls}"
action = "{action}"
"""
DEFAULT = """
[[rule]]
kind = "default"
action = "{action}"
"""

# name -> (rules [(class, action)], default action, may_change)
POLICIES = {
    "member-only-tokenize": ([(c, "tokenize") for c in MEMBER_CLASSES], "preserve", True),
    "all-preserve": ([(c, "preserve") for c in MEMBER_CLASSES], "preserve", False),
    "all-preserve-default-tokenize": ([(c, "preserve") for c in MEMBER_CLASSES], "tokenize", False),
    "explicit-family-rule": (
        [(FAMILY_CLASS, "tokenize")] + [(c, "tokenize") for c in MEMBER_CLASSES],
        "preserve",
        False,
    ),
    "default-tokenize": ([(c, "tokenize") for c in MEMBER_CLASSES], "tokenize", False),
}
LOCALES = {
    "de-de": ('"core", "locale-de"', "de-DE"),
    "en-us": ('"core", "locale-en"', "en-US"),
}


def policy_text(bundled: str, locale: str, rules, default: str) -> str:
    text = base_enum.POLICY_HEAD.format(bundled=bundled, locale=locale)
    for cls, action in rules:
        text += RULE.format(cls=cls, action=action)
    return text + DEFAULT.format(action=default)


def classify(base_cls: str, base_bytes: int, head_cls: str, head_bytes: int, span: int) -> str:
    """Reason class for a document whose IBAN view differs between the arms."""
    if base_cls == "raw" and head_cls == FAMILY_TOKEN_CLASS and head_bytes == span:
        return "family-derived"
    if base_bytes > head_bytes:
        return "LOST"
    return "UNEXPLAINED"


def main() -> int:
    base_bin, head_bin, out_path = sys.argv[1:4]
    base_id = base_enum.binary_identity(base_bin)
    head_id = base_enum.binary_identity(head_bin)
    if base_id["sha256"] == head_id["sha256"]:
        print("base and head binaries are identical; nothing to compare", file=sys.stderr)
        return 1
    docs = base_enum.documents()
    report = {
        "documents": len(docs),
        "seed": base_enum.SEED,
        "binaries": {"base": base_id, "head": head_id},
        "arms": {},
    }
    failed = False
    with tempfile.TemporaryDirectory() as tmp:
        for policy_name, (rules, default, may_change) in POLICIES.items():
            for locale_name, (bundled, locale) in LOCALES.items():
                arm = f"{policy_name}/{locale_name}"
                policy = Path(tmp) / f"{policy_name}-{locale_name}.toml"
                policy.write_text(policy_text(bundled, locale, rules, default))
                base = base_enum.run(base_bin, policy, docs)
                head = base_enum.run(head_bin, policy, docs)

                stats = Counter()
                reasons = Counter()
                changed = []
                by_country = defaultdict(int)
                for doc, b, h in zip(docs, base, head):
                    if b is None or h is None:
                        stats["missing_response"] += 1
                        continue
                    span = doc["iban_span"][1] - doc["iban_span"][0]
                    b_cls, b_bytes = base_enum.iban_view(b, doc["iban_span"])
                    h_cls, h_bytes = base_enum.iban_view(h, doc["iban_span"])
                    stats["lost_bytes"] += max(0, b_bytes - h_bytes)
                    stats["gained_bytes"] += max(0, h_bytes - b_bytes)
                    b_res = base_enum.raw_residue(b, doc)
                    h_res = base_enum.raw_residue(h, doc)
                    stats["base_residue_docs"] += b_res is not None
                    stats["head_residue_docs"] += h_res is not None
                    if b.get("clean_text") == h.get("clean_text"):
                        continue
                    # Token hex differs per session even for identical decisions,
                    # so "changed" means the IBAN view (class or bytes) changed.
                    if (b_cls, b_bytes) == (h_cls, h_bytes):
                        continue
                    reason = classify(b_cls, b_bytes, h_cls, h_bytes, span)
                    reasons[reason] += 1
                    by_country[doc["country"]] += 1
                    changed.append(
                        {
                            "text": doc["text"],
                            "prefix": doc["prefix"],
                            "trailer": doc["trailer"],
                            "base": [b_cls, b_bytes],
                            "head": [h_cls, h_bytes],
                            "span": span,
                            "reason": reason,
                        }
                    )
                stats["changed_docs"] = len(changed)
                arm_failed = (
                    stats["lost_bytes"] > 0
                    or stats["missing_response"] > 0
                    or (not may_change and changed)
                    or any(reason != "family-derived" for reason in reasons)
                )
                failed |= arm_failed
                report["arms"][arm] = {
                    "policy": {"rules": rules, "default": default, "may_change": may_change},
                    "stats": dict(stats),
                    "reasons": dict(reasons),
                    "changed_by_country": dict(sorted(by_country.items())),
                    "changed_docs": changed,
                    "failed": arm_failed,
                }
                print(
                    f"{arm:45s} lost={stats['lost_bytes']:6d} gained={stats['gained_bytes']:6d} "
                    f"changed={len(changed):5d} reasons={dict(reasons)} "
                    f"residue base={stats['base_residue_docs']} head={stats['head_residue_docs']}"
                    + ("  FAILED" if arm_failed else "")
                )
    report["failed"] = failed
    Path(out_path).write_text(json.dumps(report, indent=1, ensure_ascii=False))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
