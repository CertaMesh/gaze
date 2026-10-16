"""Local research models. These are NOT Gaze production APIs.

Every model consumes trusted owner records, never gold annotations. Occurrence
tokens retain exact source bytes independently of optional semantic roots.
"""
from __future__ import annotations

import hashlib
import json
import re
import unicodedata

ARMS = ("exact", "normalized", "structured", "evidence", "recent_first",
        "evidence_no_context", "fuzzy_unscoped", "canonical_restore", "scoped_parts")


def norm(s):
    s = unicodedata.normalize("NFKC", s).casefold().replace("’", "'")
    s = re.sub(r"straße\b|str\.\b|str\.(?=\s|$)", "strasse", s)
    s = re.sub(r"\bst\.(?=\s|$)|\bstreet\b", "street", s)
    return " ".join(s.split())


def aliases(e):
    out = {e["full"]}
    if e["kind"] == "Name":
        out.update([e["given"], e["family"], e["given"][0] + ". " + e["family"]])
        out.update(t + e["family"] for t in ("Ms. ", "Mr. ", "Frau ", "Herr "))
        out.add(e["full"].replace("’", "'").replace("ß", "ss"))
    else:
        out.update([e["city"], e["road"], e["road"] + " " + e["house"]])
        road = e["road"].replace("straße", "str.").replace(" Street", " St.")
        out.update([road + " " + e["house"], e["full"].replace(e["road"], road)])
    return out


def pattern(value, exact=False):
    pieces = re.split(r"\s+", value)
    return re.compile(r"(?<!\w)" + ("\\s+".join(re.escape(p) for p in pieces) if not exact else re.escape(value)) + r"(?!\w)",
                      0 if exact else re.I)


def edit_one(a, b):
    """True Levenshtein distance <= 1; no transitive alias expansion."""
    a, b = norm(a), norm(b)
    if abs(len(a) - len(b)) > 1:
        return False
    prev = list(range(len(b) + 1))
    for i, ca in enumerate(a, 1):
        row = [i]
        for j, cb in enumerate(b, 1):
            row.append(min(row[-1] + 1, prev[j] + 1, prev[j - 1] + (ca != cb)))
        prev = row
    return prev[-1] <= 1


def candidates(text, entities, arm):
    found, scans, comparisons = {}, 0, 0
    for e in entities:
        values = {e["full"]} if arm in {"exact", "normalized"} else aliases(e)
        if arm == "normalized" and e["kind"] == "Location":
            values.add(e["full"].replace("straße", "str.").replace(" Street", " St."))
        for value in values:
            scans += 1
            for m in pattern(value, arm == "exact").finditer(text):
                a, b = len(text[:m.start()].encode()), len(text[:m.end()].encode())
                found.setdefault((a, b), set()).add(e["id"])
        if arm in {"evidence", "scoped_parts", "fuzzy_unscoped"} and e["kind"] == "Name":
            words = list(re.finditer(r"[^\W\d_]+(?:[’'\-][^\W\d_]+)*", text))
            for a, b in zip(words, words[1:]):
                raw = text[a.start():b.end()]
                comparisons += 1
                if edit_one(raw, e["full"]):
                    found.setdefault((len(text[:a.start()].encode()), len(text[:b.end()].encode())), set()).add(e["id"])
    # Same-position full matches dominate components; no partial overlapping replacements.
    taken, result = set(), []
    for (start, end), ids in sorted(found.items(), key=lambda x: (-(x[0][1] - x[0][0]), x[0][0])):
        if taken.isdisjoint(range(start, end)):
            result.append((start, end, sorted(ids)))
            taken.update(range(start, end))
    return sorted(result), scans, comparisons


def simulate(record, arm):
    entities = record["entities"]
    by_id = {e["id"]: e for e in entities}
    turns, intern, state, stats = [], {}, {}, {"regex_scans": 0, "candidate_comparisons": 0}
    for ti, turn in enumerate(record["turns"]):
        spans = []
        text = turn["text"].encode()
        detected, scans, comparisons = candidates(turn["text"], entities, arm)
        for start, end, ids in detected:
            value = text[start:end].decode()
            context = turn.get("context")
            # Host context is not read from agent_claim, natural language, or gold.
            trusted = context and context.get("trusted") is True and context.get("version") == 1
            # Exact evidence for another record conflicts with fuzzy evidence.
            # Do not let a supplied hint turn a one-edit match into identity.
            exact = [eid for eid in ids if norm(value) == norm(by_id[eid]["full"])]
            conflicting_fuzzy = exact and set(exact) != set(ids)
            fuzzy_only = all(norm(value) not in {norm(x) for x in aliases(by_id[eid])} for eid in ids)
            linked = None
            if arm in {"exact", "normalized", "recent_first", "canonical_restore", "fuzzy_unscoped"}:
                linked = ids[-1] if arm == "recent_first" else ids[0]
            elif arm in {"evidence", "scoped_parts"} and context is not None:
                if trusted and context["entity"] in ids:
                    linked = context["entity"]
            elif len(ids) == 1:
                linked = ids[0]
            if arm in {"evidence", "scoped_parts"} and fuzzy_only and not (trusted and linked == context["entity"]):
                # No unscoped fuzzy detection: such a candidate is ignored, not merely unlinked.
                continue
            if arm in {"evidence", "scoped_parts"} and conflicting_fuzzy:
                linked = exact[0] if trusted and context["entity"] in exact and len(exact) == 1 else None
            if arm == "scoped_parts" and not turn.get("seed") and not trusted and all(norm(value) != norm(by_id[eid]["full"]) for eid in ids):
                continue
            key = (by_id[linked]["kind"] if linked else "Unknown", value)
            if key not in intern:
                token = f"<occ_{len(intern) + 1}>"
                intern[key] = token
                state[token] = by_id[linked]["full"] if arm == "canonical_restore" and linked else value
            token = intern[key]
            root = linked or "abstain"
            spans.append({"start": start, "end": end, "token": token, "root": root})
        stats["regex_scans"] += scans
        stats["candidate_comparisons"] += comparisons
        cursor, clean = 0, b""
        for span in spans:
            clean += text[cursor:span["start"]] + span["token"].encode()
            cursor = span["end"]
        clean += text[cursor:]
        restored = re.sub(r"<occ_\d+>", lambda m: state[m[0]], clean.decode())
        turns.append({"spans": spans, "restore_ok": restored.encode() == text})
    structured = arm not in {"exact", "normalized"}
    stored = entities if structured else [{k: e[k] for k in ("id", "kind", "full")} for e in entities]
    aliases_count = sum(len(aliases(e)) if structured else 1 for e in entities)
    stats.update({"state_bytes": len(json.dumps({"records": stored, "occurrences": state}, ensure_ascii=False).encode()),
                  "aliases": aliases_count})
    return {"id": record["id"], "turns": turns, "resource": stats}


def authorize(request, grant, records, now=10):
    """Typed-field exact policy gate; all grant fields are host-authored.

    Invocation is local: the result must only reach grant.destination. A valid
    request cannot set/extend a grant, expiry, trust, session or principal.
    """
    if not isinstance(request, dict) or not isinstance(grant, dict):
        return None
    required = {"tenant", "principal", "session", "epoch", "root", "component", "action", "destination"}
    if set(request) != required or any(not isinstance(request[k], str) for k in required):
        return None
    if any(type(grant.get(k)) is not int for k in ("issued_at", "expires_at")):
        return None
    if not (grant["issued_at"] <= now < grant["expires_at"]) or grant.get("revoked") is not False:
        return None
    if any(request[k] != grant.get(k) for k in required):
        return None
    record = records.get(request["root"])
    if not isinstance(record, dict) or request["component"] not in {"full", "given", "family", "city", "road", "house"}:
        return None
    value = record.get(request["component"])
    return value if isinstance(value, str) else None


def utility(record, arm):
    """Capabilities of fixed field requests, not observed LLM task success."""
    entities = {e["id"]: e for e in record["entities"]}
    tasks = [("reply_drafting", "p1", "given", "owner-draft"),
             ("scheduling", "p2", "full", "owner-calendar"),
             ("crm_update", "p1", "family", "owner-crm"),
             ("shipping_label", "a1", "full", "owner-print"),
             ("summarization", "a1", "city", "owner-draft")]
    out = {}
    for name, entity, component, sink in tasks:
        request = {"tenant": "example-tenant", "principal": "owner", "session": record["id"], "epoch": "1", "root": entity,
                   "component": component, "action": name, "destination": sink}
        grant = {**request, "issued_at": 0, "expires_at": 20, "revoked": False}
        structured = arm not in {"exact", "normalized", "native", "native_learned"}
        selected = component if structured else "full"
        actual = authorize({**request, "component": selected}, grant, entities)
        # A whole-value-only path cannot satisfy a component-scoped grant.
        out[name] = int(actual == entities[entity][component])
    return out


def fingerprint(value):
    return hashlib.sha256(value.encode()).hexdigest()
