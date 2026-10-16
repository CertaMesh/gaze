"""Scorer sensitivity checks: a partial hit must not count as full removal."""
import copy
import unittest
from experiment import summary, validate, normalized_view, source_response


def observation(text, start, end):
    raw = text.encode()
    token = "<R2_0>"
    return dict(session_id="one", clean_text=(raw[:start] + token.encode() + raw[end:]).decode(),
                exact_restore=True, tokens=[dict(token=token, original=raw[start:end].decode())],
                manifest=[dict(raw_span=dict(start=start, end=end),
                               clean_span=dict(start=start, end=start + len(token)))])


class ScorerTests(unittest.TestCase):
    def test_expanding_and_deleted_normalization_offsets_restore_source(self):
        row = dict(text="Ａ\u200bﬁ@example.invalid", session="one")
        view, mapping = normalized_view(row["text"])
        self.assertEqual(view, "Afi@example.invalid")
        response = observation(view, 0, len(view.encode()))
        projected = source_response(row, dict(row, text=view), response, mapping)
        self.assertEqual(validate(row, projected), [(0, len(row["text"].encode()))])

    def test_partial_mutant_leaves_bytes_and_fails_complete_coverage(self):
        row = dict(text="Nelia", gold=[[0, 5]], category="name", session="one")
        response = observation(row["text"], 0, 3)
        m = summary([row], [response], "baseline")["metrics"]["ALL"]
        self.assertEqual((m["leaked"], m["partial"], m["complete"]), (2, 1, 0))
        self.assertEqual(m["residual_after_hit"], 2)

    def test_unicode_offsets_and_alignment_mutant(self):
        row = dict(text="é Nelia\u2028", gold=[[3, 8]], category="name", session="one")
        response = observation(row["text"], 3, 8)
        self.assertEqual(validate(row, response), [(3, 8)])
        mutant = copy.deepcopy(response)
        mutant["manifest"][0]["raw_span"]["end"] = 7
        with self.assertRaises(AssertionError):
            validate(row, mutant)

    def test_false_positive_and_merge_counter(self):
        row = dict(text="Nelia and Tavin", gold=[[0, 5], [10, 15]], category="name", session="one")
        response = observation(row["text"], 0, 15)
        m = summary([row], [response], "baseline")["metrics"]["ALL"]
        self.assertEqual((m["fp"], m["incorrect_merges"], m["leaked"]), (5, 1, 0))

    def test_refusal_never_earns_a_restore_or_recall(self):
        row = dict(text="Nelia", gold=[[0, 5]], category="name", session="one")
        m = summary([row], [dict(error="backend")], "baseline")["metrics"]["ALL"]
        self.assertEqual(m["refusals"], 1)
        self.assertEqual(m["exact_restores"], 0)
        self.assertEqual(m["recall"], None)


if __name__ == "__main__":
    unittest.main()
