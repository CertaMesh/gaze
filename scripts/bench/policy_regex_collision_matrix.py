#!/usr/bin/env python3
"""Both-direction policy matrix for policy regex collision families (todo #3757).

Runs a synthetic document set plus the pinned Dataiku en/de holdout through two
`gaze daemon` binaries (base, head) under a matrix of policies whose custom
recognizers are REGEX rules, and compares, per document and per expected span,
the class covering the span, the bytes the manifest protects, and whether the
raw value survives in the clean text (the axis-1 oracle: output bytes, not
manifest arithmetic).

Why: a policy regex custom recognizer used to register under a constant
recognizer id, so the registry could not find it by the id its
`[policy.custom_recognizers.collision]` membership was filed under. Precedence,
ties and mandatory anchors still decided (the candidate carried the policy
name), but the family token they emit derived its action from no member at
all and fell to the policy default: under `default = preserve` the tie token
and the no-anchor token shipped raw. Head registers the rule under its policy
name; the family token now takes the strictest member action (#624).

Invariants, every arm: `lost_bytes` (bytes base protected that head does not)
is 0, `lost_values` (expected values that survive raw in head but not in base)
is 0, and every changed document carries one of these reasons:

    family-derived          base shipped the family token raw, head protects it
                            (the fix; only with collision metadata on and a
                            `preserve` default)
    family-strictest-member both arms protect the family token; head writes the
                            strictest member action (`[REDACTED]`) where base
                            took the default token (collision on, `redact`
                            members, `tokenize` default)

Arms whose policy has no collision metadata, or whose members preserve, must
be byte-identical between the binaries (token hex aside), which pins that the
score and locale-basis choices in the fix moved no conflict winner.

Recognizers (all arms; classes below are the member rules):
    tenant.alpha / tenant.beta   `\\b[0-9]{5}\\b`   custom:alpha_doc / custom:beta_doc
                                 family tenant-document, precedence 10 / 10 (tie)
    tenant.order_id / order_ref  `ORD-[0-9]+`       custom:order_id / custom:order_ref
                                 family tenant-orders, precedence 50 / 60
    tenant.konto                 `K-[0-9]{6}`       custom:konto
                                 family tenant-account, mandatory_anchor = "iban"
    tenant.mail                  email shape        email (same class as the
                                 bundled `email.global`; no collision metadata)

Arms: collision {on, off} x member action {tokenize, redact, preserve}
    x default {tokenize, preserve} x bundles {none (global), core+locale-de
    (de-DE), core+locale-en (en-US)}.

Usage:
    uv run --project scripts/bench python scripts/bench/policy_regex_collision_matrix.py \\
        BASE_BIN HEAD_BIN OUT.json [--holdout target/bench-data/dataiku-en-de/test.parquet]

Exit status is 1 when any arm loses a byte or a value, when a must-be-identical
arm changes any document, when a changed document is not explained, when the two
binaries are the same build, or when a daemon response is missing.

IMPORTANT: run against immutable copies of the binaries; the SHA-256 of each is
recorded once at start-up.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tempfile
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import iban_trailing_word_enumeration as base_enum  # noqa: E402

PATTERNS = {
    "tie": r"\b[0-9]{5}\b",
    "precedence": r"ORD-[0-9]+",
    "anchor": r"K-[0-9]{6}",
    # Real TLDs only: the policy loader refuses a pattern that would also match
    # Gaze's own placeholder shape (`...@gaze-fake.invalid`).
    "mail": r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.(com|de|org|net)\b",
}
MEMBER_CLASSES = (
    "custom:alpha_doc",
    "custom:beta_doc",
    "custom:order_id",
    "custom:order_ref",
    "custom:konto",
    "email",
)
FAMILY_CLASSES = {
    "tie": "family:tenant-document",
    "precedence": None,
    "anchor": "family:tenant-account",
    "mail": None,
}

POLICY_HEAD = """schema_version = "0.1.0"

[session]
scope = "persistent"
ttl_secs = 86400
"""
BUNDLES = """
[policy.rulepacks]
bundled = [{bundled}]

[locale]
active = ["{locale}"]
"""
RECOGNIZER = """
[[policy.custom_recognizers]]
kind = "regex"
name = "{name}"
pattern = '{pattern}'
class = "{cls}"
"""
COLLISION = """
[policy.custom_recognizers.collision]
family = "{family}"
variant = "{variant}"
precedence = {precedence}
"""
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

RECOGNIZERS = [
    # name, pattern key, class, family, variant, precedence, mandatory_anchor
    ("tenant.alpha", "tie", "custom:alpha_doc", "tenant-document", "alpha", 10, None),
    ("tenant.beta", "tie", "custom:beta_doc", "tenant-document", "beta", 10, None),
    ("tenant.order_id", "precedence", "custom:order_id", "tenant-orders", "order-id", 50, None),
    ("tenant.order_ref", "precedence", "custom:order_ref", "tenant-orders", "order-ref", 60, None),
    ("tenant.konto", "anchor", "custom:konto", "tenant-account", "konto", 10, "iban"),
    ("tenant.mail", "mail", "email", None, None, None, None),
]

BUNDLE_ARMS = {
    "global": None,
    "core-de-de": ('"core", "locale-de"', "de-DE"),
    "core-en-us": ('"core", "locale-en"', "en-US"),
}
MEMBER_ACTIONS = ("tokenize", "redact", "preserve")
DEFAULTS = ("tokenize", "preserve")

SYNTHETIC = [
    # tie family
    "ticket CASE-0001 open",
    "PLZ 10115 Berlin, Kundennummer 20457",
    "Zip 90210 and 30301 both listed",
    "reference 12345.",
    "code 123456 is six digits, 54321 is five",
    # precedence family
    "order ORD-1234 shipped",
    "orders ORD-1 and ORD-22 pending",
    # anchored family
    "Zahlung K-123456 heute",
    "IBAN K-123456 heute",
    "Konto K-654321 bitte prüfen",
    "Kontonummer: K-000001",
    "K-111111 " + ("x" * 70) + " IBAN",
    "IBAN " + ("y" * 70) + " K-222222",
    # email vs bundled email.global
    "mail alice@example.com now",
    "Contact bob.smith+tag@mail.example.org, thanks",
    "a@example.de,b@example.net",
    # mixed
    "ORD-77 for 10115 via K-123456 to c@example.com",
    "IBAN K-123456 ORD-9 10115 d@example.com",
    # negatives
    "nothing here in 2026",
    "call 555 0100 or 1234",
    "",
]


def policy_text(bundle: str, collision: bool, member: str, default: str) -> str:
    text = POLICY_HEAD
    if BUNDLE_ARMS[bundle] is not None:
        bundled, locale = BUNDLE_ARMS[bundle]
        text += BUNDLES.format(bundled=bundled, locale=locale)
    for name, key, cls, family, variant, precedence, anchor in RECOGNIZERS:
        text += RECOGNIZER.format(name=name, pattern=PATTERNS[key], cls=cls)
        if collision and family is not None:
            text += COLLISION.format(family=family, variant=variant, precedence=precedence)
            if anchor is not None:
                text += f'mandatory_anchor = "{anchor}"\n'
    for cls in MEMBER_CLASSES:
        text += RULE.format(cls=cls, action=member)
    return text + DEFAULT.format(action=default)


def expected_spans(text: str) -> list[dict]:
    spans = []
    for key, pattern in PATTERNS.items():
        for match in re.finditer(pattern, text):
            start = len(text[: match.start()].encode("utf-8"))
            end = start + len(match.group(0).encode("utf-8"))
            spans.append({"key": key, "span": (start, end), "value": match.group(0)})
    spans.sort(key=lambda s: s["span"])
    return spans


TOKEN = re.compile(r"<[0-9a-f]{8}:[^>]+>")


def shape(clean: str) -> str:
    """Clean text with session-specific token hex blanked."""
    return TOKEN.sub(lambda m: "<" + m.group(0).split(":", 1)[1], clean)


def survives_raw(response: dict, span: tuple[int, int], value: str) -> bool:
    """Whether the expected span's raw bytes leave the process at their own
    position in the clean text.

    A substring search would misfire whenever the value also occurs elsewhere
    (`12345` inside `CH-1234567890`), so the raw offset is mapped through the
    manifest's raw/clean spans and the clean text is read at the mapped
    position. A span the manifest covers only partly counts as surviving.
    """
    clean = response.get("clean_text")
    if clean is None:
        return True
    start, end = span
    clean_bytes = clean.encode("utf-8")
    delta = 0
    for entry in sorted(response.get("manifest", []), key=lambda e: e["raw_span"]["start"]):
        raw, mapped = entry["raw_span"], entry["clean_span"]
        if raw["end"] <= start:
            delta += (mapped["end"] - mapped["start"]) - (raw["end"] - raw["start"])
            continue
        if raw["start"] >= end:
            break
        # Overlap: fully covered means protected, anything less survives.
        return not (raw["start"] <= start and raw["end"] >= end)
    return clean_bytes[start + delta : end + delta] == value.encode("utf-8")


def span_view(response: dict, span: tuple[int, int], value: str) -> tuple[str, int, bool]:
    cls, protected = base_enum.iban_view(response, span)
    return cls, protected, survives_raw(response, span, value)


def classify(key: str, arm: dict, base: tuple, head: tuple, span_len: int) -> str:
    b_cls, b_bytes, b_survives = base
    h_cls, h_bytes, h_survives = head
    family = FAMILY_CLASSES[key]
    if b_bytes > h_bytes or (h_survives and not b_survives):
        return "LOST"
    if (
        arm["collision"]
        and arm["default"] == "preserve"
        and arm["member"] in ("tokenize", "redact")
        and family is not None
        and b_cls == "raw"
        and b_survives
        and h_cls == family
        and h_bytes == span_len
        and not h_survives
    ):
        return "family-derived"
    if (
        arm["collision"]
        and arm["default"] == "tokenize"
        and arm["member"] == "redact"
        and family is not None
        and b_cls == h_cls == family
        and b_bytes == h_bytes == span_len
        and not b_survives
        and not h_survives
    ):
        return "family-strictest-member"
    return "UNEXPLAINED"


def load_holdout(path: Path) -> list[str]:
    import dataiku_en_de_gaze_bench as dataiku

    documents, _ = dataiku.load_documents(path)
    return [document.text for document in documents]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("base_bin")
    parser.add_argument("head_bin")
    parser.add_argument("out")
    parser.add_argument("--holdout", type=Path, default=None)
    args = parser.parse_args()

    base_id = base_enum.binary_identity(args.base_bin)
    head_id = base_enum.binary_identity(args.head_bin)
    if base_id["sha256"] == head_id["sha256"]:
        print("base and head binaries are identical; nothing to compare", file=sys.stderr)
        return 1

    texts = [{"text": text, "source": "synthetic"} for text in SYNTHETIC]
    if args.holdout is not None:
        texts.extend({"text": text, "source": "holdout"} for text in load_holdout(args.holdout))
    docs = [{"text": t["text"], "source": t["source"], "spans": expected_spans(t["text"])} for t in texts]
    report = {
        "documents": len(docs),
        "documents_with_expected_spans": sum(1 for d in docs if d["spans"]),
        "expected_spans": Counter(s["key"] for d in docs for s in d["spans"]),
        "binaries": {"base": base_id, "head": head_id},
        "arms": {},
    }
    failed = False
    with tempfile.TemporaryDirectory() as tmp:
        for bundle in BUNDLE_ARMS:
            for collision in (True, False):
                for member in MEMBER_ACTIONS:
                    for default in DEFAULTS:
                        arm = {
                            "bundle": bundle,
                            "collision": collision,
                            "member": member,
                            "default": default,
                        }
                        name = (
                            f"{bundle}/collision-{'on' if collision else 'off'}/"
                            f"member-{member}/default-{default}"
                        )
                        may_change = collision and member != "preserve" and not (
                            member == "tokenize" and default == "tokenize"
                        )
                        policy = Path(tmp) / (name.replace("/", "-") + ".toml")
                        policy.write_text(policy_text(bundle, collision, member, default))
                        base = base_enum.run(args.base_bin, policy, docs)
                        head = base_enum.run(args.head_bin, policy, docs)

                        stats = Counter()
                        reasons = Counter()
                        changed = []
                        for doc, b, h in zip(docs, base, head):
                            if b is None or h is None:
                                stats["missing_response"] += 1
                                continue
                            doc_changed = shape(b.get("clean_text", "")) != shape(
                                h.get("clean_text", "")
                            )
                            span_reasons = []
                            for span in doc["spans"]:
                                bv = span_view(b, span["span"], span["value"])
                                hv = span_view(h, span["span"], span["value"])
                                stats["lost_bytes"] += max(0, bv[1] - hv[1])
                                stats["gained_bytes"] += max(0, hv[1] - bv[1])
                                stats["lost_values"] += int(hv[2] and not bv[2])
                                stats["base_raw_values"] += int(bv[2])
                                stats["head_raw_values"] += int(hv[2])
                                if bv != hv or doc_changed:
                                    reason = classify(
                                        span["key"],
                                        arm,
                                        bv,
                                        hv,
                                        span["span"][1] - span["span"][0],
                                    )
                                    if bv == hv and reason == "UNEXPLAINED":
                                        # The span itself is unchanged; the
                                        # document changed elsewhere.
                                        continue
                                    span_reasons.append(
                                        {
                                            "key": span["key"],
                                            "base": list(bv),
                                            "head": list(hv),
                                            "reason": reason,
                                        }
                                    )
                            if doc_changed and not span_reasons:
                                span_reasons.append(
                                    {"key": None, "base": None, "head": None, "reason": "UNEXPLAINED"}
                                )
                            if span_reasons:
                                for sr in span_reasons:
                                    reasons[sr["reason"]] += 1
                                changed.append(
                                    {
                                        "source": doc["source"],
                                        "text": doc["text"],
                                        "base_clean": shape(b.get("clean_text", "")),
                                        "head_clean": shape(h.get("clean_text", "")),
                                        "spans": span_reasons,
                                    }
                                )
                        stats["changed_docs"] = len(changed)
                        arm_failed = (
                            stats["lost_bytes"] > 0
                            or stats["lost_values"] > 0
                            or stats["missing_response"] > 0
                            or (not may_change and changed)
                            or any(
                                reason not in ("family-derived", "family-strictest-member")
                                for reason in reasons
                            )
                        )
                        failed |= arm_failed
                        report["arms"][name] = {
                            "policy": {**arm, "may_change": may_change},
                            "stats": dict(stats),
                            "reasons": dict(reasons),
                            "changed_docs": changed,
                            "failed": arm_failed,
                        }
                        print(
                            f"{name:60s} lost_b={stats['lost_bytes']:5d} lost_v={stats['lost_values']:4d} "
                            f"gained_b={stats['gained_bytes']:6d} changed={len(changed):5d} "
                            f"raw base={stats['base_raw_values']} head={stats['head_raw_values']} "
                            f"reasons={dict(reasons)}" + ("  FAILED" if arm_failed else ""),
                            flush=True,
                        )
    report["failed"] = failed
    Path(args.out).write_text(json.dumps(report, indent=1, ensure_ascii=False))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
