"""Versioned, value-free refusal evidence shared by live scoring and replay."""
from collections.abc import Mapping

VERSION = 2
UNTRANSFORMABLE_REASONS = (
    "json_number_no_text_only_path", "encoded_carrier", "no_safe_rendering",
    "caller_value_too_short",
)
UNTRANSFORMABLE_CODES = frozenset("untransformable_" + r for r in UNTRANSFORMABLE_REASONS)
POSSIBLE_REASONS = frozenset({
    "label_value_shape_unclear", "label_binding_unproven", "other_field_repeat",
})
PROTECTION_CODES = frozenset("protection_" + r for r in (
    "empty_primary", "unsupported_coverage", "primary", "safety_net", "residual", "provenance",
))
# Increment 1's independent ownership-labelled gold, never detector class names.
CONFIRMED_ID_LABELS = frozenset({"CUSTOMER_ID", "EMPLOYEE_ID", "RECORD_ID"})
# Producers whose `evidence_complete` is honoured. A subject flag alone proves
# nothing: an identity joins this pin, and benchmark-protocol.json, only in the
# change that adds its end-to-end completeness test. Empty: no live refusal is blocked.
COMPLETE_EVIDENCE_PRODUCERS = frozenset()


def validate_outcome(value):
    if value == "protected":
        return
    if (isinstance(value, dict) and set(value) == {"possibly_sensitive"}
            and isinstance(value["possibly_sensitive"], str)
            and value["possibly_sensitive"] in POSSIBLE_REASONS):
        return
    raise ValueError("invalid protection outcome")


def validate_evidence(response: Mapping, byte_length: int, boundaries=None):
    code = response.get("pipeline_error_code")
    evidence = response.get("refusal_evidence")
    if "evidence_complete" in response and type(response["evidence_complete"]) is not bool:
        raise ValueError("invalid refusal completeness")
    if "evidence_producer" in response and (
            not isinstance(response["evidence_producer"], str) or not response["evidence_producer"]):
        raise ValueError("invalid refusal evidence producer")
    if evidence is None:
        if response.get("evidence_complete") is True:
            raise ValueError("complete refusal without evidence")
        return  # Old records and failures without located evidence remain ordinary refusals.
    if code not in UNTRANSFORMABLE_CODES or not isinstance(evidence, list) or not evidence:
        raise ValueError("invalid typed refusal evidence")
    previous = 0
    for span in evidence:
        if (not isinstance(span, dict)
                or set(span) != {"raw_start", "raw_end", "outcome", "reason"}
                or span["outcome"] != "confirmed_untransformable"
                or span["reason"] != code.removeprefix("untransformable_")):
            raise ValueError("invalid typed refusal evidence")
        start, end = span["raw_start"], span["raw_end"]
        if (type(start) is not int or type(end) is not int
                or not previous <= start < end <= byte_length
                or boundaries is not None and (start not in boundaries or end not in boundaries)):
            raise ValueError("invalid refusal span")
        previous = end


def eligible(document, response):
    """Every refused value must exactly match ownership-labelled confirmed-ID gold.

    Presence of unrelated gold never licenses refusal of a benign sibling.
    """
    validate_evidence(response, len(document.text.encode("utf-8")))
    evidence = response.get("refusal_evidence")
    if (response.get("pipeline_error_code") not in UNTRANSFORMABLE_CODES or not evidence
            or response.get("evidence_complete") is not True
            or response.get("evidence_producer") not in COMPLETE_EVIDENCE_PRODUCERS
            or response["pipeline_error_code"] == "untransformable_encoded_carrier"):
        return False
    gold = {(s.start, s.end) for s in document.spans if s.label in CONFIRMED_ID_LABELS}
    return all((s["raw_start"], s["raw_end"]) in gold for s in evidence)


def rates(document_count, blocked_count, benign_count, false_refusals):
    def rate(numerator, denominator):
        return {"numerator": numerator, "denominator": denominator,
                "rate": numerator / denominator if denominator else None}
    return {"blocked_operation_rate": rate(blocked_count, document_count),
            "false_refusal_rate": rate(false_refusals, benign_count)}
