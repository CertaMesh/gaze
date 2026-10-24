#!/usr/bin/env python3
"""Measure native proxy ingress ordering with an own-policy CLI binary.

The text-only C/A/D/R harness does not execute proxy traversal. This separate
case set measures actual forwarded UTF-8 bytes, owner-side token restoration,
and refusals before forwarding. It makes no latency claim. Run both revisions
under the bench-machine lease with each revision's gaze setup policy. Inputs
are synthetic and pinned by their committed fixture hash.

Capture each side with this committed driver:
  python3 scripts/bench/build_native_session_proxy.py --runtime-root <checkout> --output <checkout>/target/session-host
  python3 scripts/bench/proxy_request_order.py --runtime-root <checkout> \
    --binary <checkout>/target/release/gaze --policy <checkout>/target/gate/policy.toml \
    --session-binary <checkout>/target/release/gaze-native-session-proxy \
    --output target/proxy-<side>.json
Compare the captures with --compare target/proxy-base.json target/proxy-candidate.json
and --output target/proxy-comparison.json. C/A/D/R must also show no regression.
"""
from __future__ import annotations

import argparse
from collections import Counter
from contextlib import contextmanager
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import queue
import re
import socket
import subprocess
import sys
import threading
import tomllib
import time
import urllib.error
import urllib.request
import uuid

FIXTURE = Path(__file__).with_name("fixtures") / "proxy_request_order.json"
TOKEN = re.compile(r"<[0-9a-f]{8}:[^<>]+>")
PROVIDERS = ("openai", "gemini", "anthropic")
V2_CONTRACT = Path(__file__).resolve().parents[2] / "docs/reference/benchmarks/scored-labels-v2.json"
NEUTRAL_V2 = {row["class"] for row in json.loads(V2_CONTRACT.read_text())["neutral_prediction_classes"]}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load_cases(fixture=FIXTURE):
    """Expand authored large-carrier shapes; no detector output defines their gold."""
    data = json.loads(fixture.read_text())
    cases = data["cases"]
    for shape in data.get("large_owned_carriers", []):
        name = "X" * shape["name_bytes"]
        seed_text = f"From: {name} <m@example.invalid>"
        seed = {"text": seed_text, "gold": [
            {"start": 6, "end": 6 + len(name), "label": "FIRSTNAME"},
            {"start": 8 + len(name), "end": 25 + len(name), "label": "EMAIL"}]}
        wire = ("{seed}" + shape.get("separator", " ")) * shape["copies"] + shape.get("suffix", "")
        if shape["private_byte"]:
            middle = shape["copies"] // 2
            wire = "{seed} " * middle + "S " + "{seed} " * (shape["copies"] - middle)
            source = f"From: {name} S {name} <m@example.invalid>"
            source_gold = [{"start": 6, "end": 6 + len(name), "label": "FIRSTNAME"},
                           {"start": 7 + len(name), "end": 8 + len(name), "label": "FIRSTNAME"},
                           {"start": 9 + len(name), "end": 9 + 2 * len(name), "label": "FIRSTNAME"},
                           {"start": 11 + 2 * len(name), "end": 28 + 2 * len(name), "label": "EMAIL"}]
        else:
            source, source_gold = "ordinary source", []
        text = wire.replace("{seed}", name)
        gold = [{"start": found.start(), "end": found.end(), "label": "FIRSTNAME"}
                for found in re.finditer(r"X+|S", text)]
        cases.append({"id": shape["id"], "order": "source_first", "seed": seed,
                      "providers": shape["providers"], "provider_exclusion_reason": shape["provider_exclusion_reason"],
                      "leaves": [{"text": source, "gold": source_gold},
                                 {"text": text, "wire_text": wire, "gold": gold, "carrier": "unsupported"}]})
    return cases


def leaves(provider, body):
    if provider == "gemini":
        texts = [item["parts"][0]["text"] for item in body["contents"]]
    else:
        texts = [item["content"] for item in body["messages"]]
    if "unsupported_carrier" in body:
        texts.append(body["unsupported_carrier"]["nested"][0])
    return texts


def observation_leaves(case):
    """Wire traversal order: mutable leaves first, then the unsupported carrier."""
    return sorted(case["leaves"], key=lambda leaf: leaf.get("carrier") == "unsupported")


def replies(provider, texts):
    if provider == "openai":
        return {"choices": [{"index": index, "message": {"role": "assistant", "content": text}}
                            for index, text in enumerate(texts)]}
    if provider == "gemini":
        return {"candidates": [{"index": index, "content": {"role": "model", "parts": [{"text": text}]}}
                               for index, text in enumerate(texts)]}
    return {"id": "msg_synthetic", "type": "message", "role": "assistant", "model": "gpt-test",
            "content": [{"type": "text", "text": text} for text in texts],
            "stop_reason": "end_turn", "stop_sequence": None,
            "usage": {"input_tokens": 1, "output_tokens": 1}}


def restored_values(provider, body):
    if provider == "openai":
        return [choice["message"]["content"] for choice in body["choices"]]
    if provider == "gemini":
        return [item["content"]["parts"][0]["text"] for item in body["candidates"]]
    return [item["text"] for item in body["content"]]


def protected_bytes(raw, clean, mappings):
    """Reconstruct exact original bytes and mark token-substituted extents."""
    parts, mask, cursor = [], [], 0
    for match in TOKEN.finditer(clean):
        gap = clean[cursor:match.start()]
        value = mappings[match.group()]
        parts.extend((gap, value))
        mask.extend([None] * len(gap.encode("utf-8")))
        mask.extend([match.group().split(":", 1)[1].rsplit("_", 1)[0].lower()] * len(value.encode("utf-8")))
        cursor = match.end()
    tail = clean[cursor:]
    parts.append(tail)
    mask.extend([None] * len(tail.encode("utf-8")))
    if "".join(parts) != raw:
        raise ValueError("proxy output did not restore exactly to the case input")
    return mask


class AmbiguousAlignment(ValueError):
    """The observed literals and known owner mappings do not determine one alignment."""


def infer_mappings(raw, clean, known=None):
    """Require one complete literal-gap alignment with consistent reused tokens."""
    known = known or {}
    matches = list(TOKEN.finditer(clean))
    if not matches:
        if raw != clean:
            raise ValueError("token-free leaf changed")
        return {}
    prefix = clean[:matches[0].start()]
    if not raw.startswith(prefix):
        raise ValueError("protected leaf changed its literal prefix")
    parts = []
    for index, match in enumerate(matches):
        next_start = matches[index + 1].start() if index + 1 < len(matches) else len(clean)
        gap = clean[match.end():next_start]
        if (not gap and index + 1 < len(matches)
                and match.group() not in known and matches[index + 1].group() not in known):
            raise AmbiguousAlignment("adjacent tokens have no independent byte boundary")
        parts.append((match.group(), gap))
    solutions, mappings = [], {match.group(): known[match.group()] for match in matches if match.group() in known}
    explored = 0

    def align(index, cursor):
        nonlocal explored
        explored += 1
        if explored > 20000:
            raise ValueError("literal-gap alignment search limit exceeded")
        if len(solutions) > 1:
            return
        if index == len(parts):
            if cursor == len(raw):
                solutions.append(dict(mappings))
            return
        token, gap = parts[index]
        if token in mappings:
            value = mappings[token]
            end = cursor + len(value)
            if raw.startswith(value, cursor) and raw.startswith(gap, end):
                align(index + 1, end + len(gap))
            return
        end = raw.find(gap, cursor + 1) if gap or index + 1 < len(parts) else len(raw)
        while end > cursor:
            mappings[token] = raw[cursor:end]
            align(index + 1, end + len(gap))
            del mappings[token]
            if (not gap and index + 1 == len(parts)) or len(solutions) > 1:
                break
            end = raw.find(gap, end + 1)

    align(0, len(prefix))
    if not solutions:
        raise ValueError("literal-gap alignment has inconsistent or empty token mappings")
    if len(solutions) != 1:
        raise AmbiguousAlignment("literal gap does not uniquely identify token extent")
    protected_bytes(raw, clean, solutions[0])
    return solutions[0]


def document_mappings(raw_leaves, clean_leaves, known=None, owner_facts=None):
    facts = dict(known or {})
    for token, value in (owner_facts or {}).items():
        if token in facts and facts[token] != value:
            raise ValueError("owner echo contradicts the earlier seed mapping")
        facts[token] = value
    pending = list(zip(raw_leaves, clean_leaves, strict=True))
    while pending:
        remaining, progress = [], False
        for leaf, clean in pending:
            try:
                found = infer_mappings(leaf["text"], clean, facts)
            except AmbiguousAlignment:
                remaining.append((leaf, clean))
                continue
            for token, value in found.items():
                if token in facts and facts[token] != value:
                    raise ValueError("one token has inconsistent source values")
                facts[token] = value
            progress = True
        if remaining and not progress:
            raise AmbiguousAlignment("document has no unique owner-constrained alignment")
        pending = remaining
    tokens = {token for clean in clean_leaves for token in TOKEN.findall(clean)}
    return {token: facts[token] for token in tokens}


def checked_document_mappings(raw_leaves, clean_leaves, known, owner_facts):
    try:
        return document_mappings(raw_leaves, clean_leaves, known, owner_facts), True
    except ValueError:
        if not owner_facts:
            raise
        # A broken owner echo must not erase the bytes already forwarded.
        # Score only if the authored literals and prior seed still prove one mapping.
        return document_mappings(raw_leaves, clean_leaves, known), False


def token_positions(text, mappings):
    positions, cursor, expanded = [], 0, 0
    for match in TOKEN.finditer(text):
        expanded += len(text[cursor:match.start()].encode())
        value = mappings[match.group()]
        end = expanded + len(value.encode())
        positions.append((match.group(), expanded, end))
        expanded, cursor = end, match.end()
    return positions


def owned_identities_preserved(bound_leaves, clean_leaves, known, mappings):
    for leaf, clean in zip(bound_leaves, clean_leaves, strict=True):
        expected = token_positions(leaf.get("wire_text", leaf["text"]), known)
        actual = set(token_positions(clean, mappings))
        if any(position not in actual for position in expected):
            return False
    return True


def echoed_tokens(tokens):
    # This layer's gold classes are names and emails. Other classes still count
    # toward request FP bytes, but are not reflected as model-authored answer text.
    return [token for token in tokens if re.fullmatch(r"<[0-9a-f]{8}:(?:Name|Email)_\d+>", token)]


def gold_bytes(leaf):
    result = [False] * len(leaf["text"].encode("utf-8"))
    for span in leaf["gold"]:
        start, end = span["start"], span["end"]
        if span["label"] not in {"FIRSTNAME", "SURNAME", "EMAIL"} or not 0 <= start < end <= len(result):
            raise ValueError("invalid synthetic gold span")
        result[start:end] = [True] * (end - start)
    return result


def score_leaf(leaf, clean, mappings):
    protected = protected_bytes(leaf["text"], clean, mappings)
    gold = gold_bytes(leaf)
    return {"gold_bytes": sum(gold),
            "leaked_bytes": sum(expected and covered is None for expected, covered in zip(gold, protected, strict=True)),
            "false_positive_bytes_v1": sum(covered is not None and not expected for expected, covered in zip(gold, protected, strict=True)),
            "false_positive_bytes_v2": sum(covered is not None and covered not in NEUTRAL_V2 and not expected for expected, covered in zip(gold, protected, strict=True))}


def request_json(url, body=None, timeout=180, session_id=None):
    headers = {"content-type": "application/json", "x-api-key": "synthetic-key",
               "anthropic-version": "2023-06-01"}
    if session_id is not None:
        headers["x-gaze-session-id"] = session_id
    request = urllib.request.Request(url, None if body is None else json.dumps(body).encode(), headers)
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    try:
        response = opener.open(request, timeout=timeout)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        return response.status, json.loads(response.read())


def native_request(provider, case):
    texts = [leaf.get("wire_text", leaf["text"]) for leaf in case["leaves"]
             if leaf.get("carrier") != "unsupported"]
    carriers = [leaf.get("wire_text", leaf["text"]) for leaf in case["leaves"]
                if leaf.get("carrier") == "unsupported"]
    if len(carriers) > 1:
        raise ValueError("one unsupported carrier per request")
    if provider == "gemini":
        path, body = "/v1beta/models/gemini-test:generateContent", {
            "contents": [{"role": "user", "parts": [{"text": text}]} for text in texts]}
    else:
        path = "/v1/messages" if provider == "anthropic" else "/v1/chat/completions"
        body = {"model": "gpt-test", "max_tokens": 256,
                "messages": [{"role": "user", "content": text} for text in texts]}
    if carriers:
        body["unsupported_carrier"] = {"nested": [carriers[0]]}
    return path, body


@contextmanager
def upstream():
    captured = queue.Queue()

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers["content-length"])))
            provider = ("anthropic" if self.path == "/v1/messages" else
                        "gemini" if self.path.startswith("/v1beta/") else "openai")
            tokens = list(dict.fromkeys(TOKEN.findall("\n".join(leaves(provider, body)))))
            captured.put((provider, body, tokens))
            encoded = json.dumps(replies(provider, [token + "\n" for token in echoed_tokens(tokens)] or ["ok"])).encode()
            self.send_response(200)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(encoded)))
            self.end_headers()
            self.wfile.write(encoded)

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    worker = threading.Thread(target=server.serve_forever, daemon=True)
    worker.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}", captured
    finally:
        server.shutdown()
        server.server_close()
        worker.join()


@contextmanager
def proxy(binary, policy, upstream_url, log_path, runtime_root, session_binary=None):
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        port = reservation.getsockname()[1]
    command = [str(binary), "proxy", "serve", "--bind", f"127.0.0.1:{port}", "--policy", str(policy)]
    for provider in PROVIDERS:
        command.extend(["--upstream-" + provider, upstream_url])
    if session_binary is not None:
        command = [str(session_binary), f"127.0.0.1:{port}", str(policy), upstream_url]
    with log_path.open("wb") as log:
        process = subprocess.Popen(command, stdout=log, stderr=log, cwd=runtime_root)
        try:
            deadline = time.monotonic() + 180
            while True:
                if process.poll() is not None:
                    raise RuntimeError(f"proxy startup failed; inspect {log_path}")
                try:
                    status, _ = request_json(f"http://127.0.0.1:{port}/_gaze_proxy/healthz", timeout=1)
                    if status == 200:
                        break
                except (OSError, ValueError):
                    pass
                if time.monotonic() >= deadline:
                    raise TimeoutError("proxy startup deadline exceeded")
                time.sleep(0.1)
            yield f"http://127.0.0.1:{port}", command
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=20)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()


def seed_session(base_url, provider, case, captured, session_id):
    """Bind a placeholder only to a token observed in a successful earlier request."""
    seed = case["seed"]
    path, body = native_request(provider, {"leaves": [seed]})
    status, reply = request_json(base_url + path, body, session_id=session_id)
    if captured.empty():
        raise ValueError(f"{provider}/{case['id']}: seed request refused ({status}); two-turn capture is unavailable")
    actual_provider, actual, tokens = captured.get_nowait()
    if actual_provider != provider or not captured.empty() or not 200 <= status < 300:
        raise ValueError("seed request lacked a successful accounted response")
    clean = leaves(provider, actual)[0]
    known = infer_mappings(seed["text"], clean)
    selected = echoed_tokens(tokens)
    if restored_values(provider, reply) != [known[token] + "\n" for token in selected]:
        raise ValueError("seed did not restore exactly")
    # The fixture's seed is a header name followed by a fictional email address.
    name = seed["text"].split("From: ", 1)[1].split(" <", 1)[0]
    owners = [token for token, value in known.items() if value == name and ":Name_" in token]
    if len(owners) != 1:
        raise ValueError(f"{provider}/{case['id']}: seed did not yield one manifest-owned full name token")
    for leaf in case["leaves"]:
        if leaf.get("wire_text", leaf["text"]).replace("{seed}", name) != leaf["text"]:
            raise ValueError("seeded wire text differs from the authored owner text")
    bound = dict(case, leaves=[dict(leaf, wire_text=leaf.get("wire_text", leaf["text"]).replace("{seed}", owners[0]))
                              for leaf in case["leaves"]])
    return bound, known, score_leaf(seed, clean, known), owners[0]


def prove_owner_mappings(base_url, provider, tokens, captured, session_id, known):
    """Get authoritative constraints without changing the measured target response.

    Each auxiliary request contains one already-owned token. Require its exact
    identity upstream and a successful single-token owner restore. These proofs
    supply no target byte or availability credit.
    """
    facts, proofs = {}, []
    for token in echoed_tokens(tokens):
        if token in known:
            continue
        if session_id is None:
            raise AmbiguousAlignment("single-token owner proof requires session continuity")
        path, body = native_request(provider, {"leaves": [{"text": token}]})
        status, reply = request_json(base_url + path, body, session_id=session_id)
        if captured.empty():
            raise ValueError("single-token owner proof did not forward")
        actual_provider, actual, actual_tokens = captured.get_nowait()
        if (actual_provider != provider or not captured.empty()
                or leaves(provider, actual) != [token] or actual_tokens != [token]):
            raise ValueError("single-token owner proof changed its owned token or forwarded raw bytes")
        restored = restored_values(provider, reply) if 200 <= status < 300 else None
        if (restored is None or len(restored) != 1 or not restored[0].endswith("\n")
                or not restored[0][:-1]):
            raise ValueError("single-token owner proof did not restore successfully")
        facts[token] = restored[0][:-1]
        proofs.append({"token": token, "value": facts[token], "status": status,
                       "forwarded_token_identical": True, "raw_source_bytes_forwarded": 0})
    return facts, proofs


def native_session_id(provider, case_id):
    return str(uuid.UUID(bytes=uuid.uuid5(uuid.NAMESPACE_URL, provider + ":" + case_id).bytes, version=4))


def measure(binary, policy, fixture, output, runtime_root, session_binary=None):
    # Exact known-token alignment still uses one frame per wire token.
    sys.setrecursionlimit(max(sys.getrecursionlimit(), 10000))
    dirty = subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=no", "--",
                                     "crates", "Cargo.toml", "Cargo.lock"], cwd=runtime_root, text=True)
    if dirty:
        raise ValueError("capture requires committed runtime source")
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=runtime_root, text=True).strip()
    # Policies resolve relative model/dictionary paths from the runtime checkout.
    policy_data = tomllib.loads(policy.read_text())
    if policy_data.get("safety_net", {}).get("backend") not in {"nym", "kiji"}:
        raise ValueError("capture requires the full own-policy safety-net arm")
    cases = load_cases(fixture)
    rows = []
    with upstream() as (upstream_url, captured), proxy(binary, policy, upstream_url, output.with_suffix(".log"), runtime_root, **({"session_binary": session_binary} if session_binary else {})) as (base_url, command):
        for provider in PROVIDERS:
            for case in cases:
                if provider not in case.get("providers", PROVIDERS):
                    continue
                session_id = native_session_id(provider, case["id"]) if session_binary is not None else None
                bound, known, seed_scores, owner_token = case, {}, {}, None
                if "seed" in case:
                    if session_binary is None:
                        raise ValueError("two-turn fixtures require --session-binary")
                    bound, known, seed_scores, owner_token = seed_session(base_url, provider, case, captured, session_id)
                path, body = native_request(provider, bound)
                status, reply = request_json(base_url + path, body, **({"session_id": session_id} if session_id else {}))
                scored_leaves = observation_leaves(case)
                bound_leaves = observation_leaves(bound)
                row = {"id": case["id"], "provider": provider, "order": case["order"],
                       "status": status, "error_code": reply.get("error") if isinstance(reply, dict) else None,
                       "resource_limit_refusals": 0, "attempted": 1, "seed_forwarded": int("seed" in case), "seed_restore_exact": int("seed" in case), "forwarded": 0, "failed_closed": 0,
                       "selected_restore_exact": 0, "alignment_valid": 0, "owned_identities_preserved": 0,
                       "seed_leaked_bytes": seed_scores.get("leaked_bytes", 0), "target_leaked_bytes": 0,
                       "seed_false_positive_bytes_v1": seed_scores.get("false_positive_bytes_v1", 0),
                       "seed_false_positive_bytes_v2": seed_scores.get("false_positive_bytes_v2", 0),
                       "gold_bytes": sum(sum(gold_bytes(leaf)) for leaf in case["leaves"]) + seed_scores.get("gold_bytes", 0),
                       "leaked_bytes": 0, "false_positive_bytes_v1": 0, "false_positive_bytes_v2": 0}
                for metric in ("leaked_bytes", "false_positive_bytes_v1", "false_positive_bytes_v2"):
                    row[metric] = seed_scores.get(metric, 0)
                if captured.empty():
                    if 200 <= status < 300:
                        raise ValueError("successful request never reached upstream")
                    row["failed_closed"] = 1
                    row["resource_limit_refusals"] = int(status == 413 and row["error_code"] == "ResidualExpansionLimitExceeded")
                    rows.append(row)
                    continue
                actual_provider, actual, tokens = captured.get_nowait()
                if actual_provider != provider or not captured.empty():
                    raise ValueError("unexpected upstream request accounting")
                row["forwarded"] = 1
                clean_leaves = leaves(provider, actual)
                selected = echoed_tokens(tokens)
                restored = restored_values(provider, reply) if 200 <= status < 300 else None
                owner_facts = {}
                if (selected and restored is not None and len(restored) == len(selected)
                        and all(value.endswith("\n") for value in restored)):
                    owner_facts = {token: value[:-1] for token, value in zip(selected, restored, strict=True)}
                row["owner_mapping_proofs"] = []
                try:
                    try:
                        mappings, echo_consistent = checked_document_mappings(scored_leaves, clean_leaves, known, owner_facts)
                    except AmbiguousAlignment:
                        proof_facts, row["owner_mapping_proofs"] = prove_owner_mappings(
                            base_url, provider, tokens, captured, session_id, {**known, **owner_facts})
                        mappings, echo_consistent = checked_document_mappings(
                            scored_leaves, clean_leaves, {**known, **proof_facts}, owner_facts)
                except ValueError as error:
                    # Synthetic fixture inputs only. This is an incomplete diagnostic,
                    # never a scorecard: keep the actual bytes and constraints that failed.
                    failure = {"schema": "gaze-proxy-alignment-failure-v1", "complete": False,
                               "runtime_revision": revision, "fixture_sha256": digest(fixture),
                               "harness_sha256": digest(Path(__file__)), "provider": provider,
                               "case": case, "status": status, "clean_leaves": clean_leaves,
                               "known_seed_mappings": known, "owner_facts": owner_facts,
                               "owner_reply": reply, "owner_mapping_proofs": row["owner_mapping_proofs"],
                               "completed_case_rows": rows,
                               "error": str(error)}
                    output.with_suffix(".failure.json").write_text(json.dumps(failure, indent=2) + "\n")
                    raise type(error)(f"{provider}/{case['id']}: {error}") from error
                if set(mappings) != set(tokens):
                    raise ValueError("unaccounted upstream token")
                # Authored synthetic inputs only: retain the exact observation and proven
                # constraints so a new leak-to-refusal transition can be independently rescored.
                row["observed_leaves"] = clean_leaves
                row["proved_mappings"] = mappings
                row["owned_identities_preserved"] = int(owned_identities_preserved(bound_leaves, clean_leaves, known, mappings))
                # Echo facts must also reconstruct every authored leaf exactly.
                # Failed owner responses still leave all forwarded bytes scored.
                expected = [mappings[token] + "\n" for token in selected] or ["ok"]
                exact = echo_consistent and restored == expected
                totals = Counter(seed_scores)
                for leaf, clean in zip(scored_leaves, clean_leaves, strict=True):
                    totals.update(score_leaf(leaf, clean, mappings))
                row.update(totals)
                row["target_leaked_bytes"] = row["leaked_bytes"] - row["seed_leaked_bytes"]
                row["selected_restore_exact"] = int(exact)
                row["alignment_valid"] = 1
                rows.append(row)
    counts = ("attempted", "resource_limit_refusals", "seed_forwarded", "seed_restore_exact", "forwarded", "failed_closed", "selected_restore_exact", "alignment_valid", "owned_identities_preserved",
              "gold_bytes", "leaked_bytes", "false_positive_bytes_v1", "false_positive_bytes_v2")
    result = {"schema": "gaze-proxy-request-order-v3", "layer": "proxy-request-order", "contract_labels": ["FIRSTNAME", "SURNAME", "EMAIL"],
              "runtime_revision": revision, "scored_contracts": ["v2", "v1"], "v2_contract_sha256": digest(V2_CONTRACT), "fixture_sha256": digest(fixture),
              "harness_sha256": digest(Path(__file__)), "binary_sha256": digest(binary), "policy_sha256": digest(policy), "command": command,
              "session_host_sha256": digest(Path(__file__).with_name("native-session-proxy") / "main.rs"),
              "session_builder_sha256": digest(Path(__file__).with_name("build_native_session_proxy.py")),
              "session_binary_sha256": digest(session_binary) if session_binary else None,
              "scope": "native nonstream proxy requests; seed requests forwarded, restored and byte-scored; owner-constrained source-byte alignment, owned-token positions and Name/Email owner echo; other classes priced as request FP bytes; no exported manifest validation",
              "owner_mapping_proof_requests": sum(len(row.get("owner_mapping_proofs", [])) for row in rows),
              "owner_mapping_proof_scope": "auxiliary same-session requests forward exactly one existing owned token and restore it alone; no raw source bytes or target byte/restore/availability gain credit",
              "totals": {key: sum(row[key] for row in rows) for key in counts}, "cases": rows}
    output.write_text(json.dumps(result, indent=2) + "\n")
    return result


def compare(base, candidate, *, gate_policy="strict"):
    """The transport layer's gain rule; C/A/D/R need their own admitted captures."""
    if gate_policy not in {"strict", "paired", "leak-to-refusal"}:
        raise ValueError("unknown proxy gate policy")
    identity = ("schema", "layer", "contract_labels", "scored_contracts", "fixture_sha256",
                "harness_sha256", "session_host_sha256", "session_builder_sha256", "v2_contract_sha256", "policy_sha256")
    if (base["fixture_sha256"] != digest(FIXTURE)
            or base["v2_contract_sha256"] != digest(V2_CONTRACT)):
        raise ValueError("proxy capture inputs differ from the committed case/scoring contract")
    if any(base[key] != candidate[key] for key in identity):
        raise ValueError("proxy captures have different input/scoring identities")
    counts = tuple(base["totals"])
    expected = {(provider, case["id"]) for provider in PROVIDERS
                for case in load_cases() if provider in case.get("providers", PROVIDERS)}
    for card in (base, candidate):
        keys = [(row["provider"], row["id"]) for row in card["cases"]]
        if len(keys) != len(expected) or set(keys) != expected:
            raise ValueError("proxy capture has a missing or duplicate case")
        for row in card["cases"]:
            if any(type(row[key]) is not int or row[key] < 0 for key in counts):
                raise ValueError("proxy counts must be nonnegative integers")
            if (row["attempted"] != 1 or row["forwarded"] + row["failed_closed"] != 1
                    or row["selected_restore_exact"] > row["forwarded"]
                    or row["alignment_valid"] != row["forwarded"]
                    or row["seed_forwarded"] != row["seed_restore_exact"]
                    or row["owned_identities_preserved"] > row["forwarded"]):
                raise ValueError("proxy capture lacks complete forward/restore accounting")
        totals = {key: sum(row[key] for row in card["cases"]) for key in counts}
        if totals != card["totals"]:
            raise ValueError("proxy totals differ from case evidence")
    rows = { (row["provider"], row["id"]): row for row in base["cases"] }
    regressions = []
    leak_refusals = []
    resource_refusals = []
    for row in candidate["cases"]:
        old = rows[(row["provider"], row["id"])]
        leak_refused = False
        resource_refused = False
        for observed in (old, row):
            resource_count = observed.get("resource_limit_refusals", 0)
            if (type(resource_count) is not int or resource_count not in (0, 1)
                    or resource_count > observed["failed_closed"]
                    or (resource_count and (observed.get("status") != 413
                        or observed.get("error_code") != "ResidualExpansionLimitExceeded"))):
                raise ValueError("invalid typed resource-limit refusal evidence")
        if gate_policy == "leak-to-refusal":
            for observed in (old, row):
                seed = observed.get("seed_leaked_bytes")
                target = observed.get("target_leaked_bytes")
                if (type(seed) is not int or type(target) is not int or min(seed, target) < 0
                        or seed + target != observed["leaked_bytes"]):
                    raise ValueError("leak-to-refusal requires separate seed and target byte evidence")
            resource_refused = bool(old["forwarded"] and row.get("resource_limit_refusals", 0))
            if resource_refused:
                resource_refusals.append({"provider": row["provider"], "id": row["id"],
                                          "base": old, "candidate": row})
            leak_refused = bool(not resource_refused and old["forwarded"] and row["failed_closed"]
                                and old["target_leaked_bytes"] > 0
                                and row["target_leaked_bytes"] == 0
                                and row["seed_leaked_bytes"] <= old["seed_leaked_bytes"])
            if leak_refused:
                leak_refusals.append({"provider": row["provider"], "id": row["id"],
                                      "transition": "leak -> refused",
                                      "base_target_leaked_bytes": old["target_leaked_bytes"],
                                      "base": old, "candidate": row})
        if (row["seed_forwarded"] != old["seed_forwarded"]
                or row["seed_restore_exact"] < old["seed_restore_exact"]
                or row["gold_bytes"] != old["gold_bytes"] or row["leaked_bytes"] > old["leaked_bytes"]
                or (not (leak_refused or resource_refused) and row["failed_closed"] > old["failed_closed"])
                or (not (leak_refused or resource_refused) and row["selected_restore_exact"] < old["selected_restore_exact"])
                or ((gate_policy == "strict" or (old["forwarded"] and not (leak_refused or resource_refused)))
                    and row["forwarded"] - row["selected_restore_exact"] > old["forwarded"] - old["selected_restore_exact"])
                or (not (leak_refused or resource_refused) and row["alignment_valid"] < old["alignment_valid"])
                or (not (leak_refused or resource_refused) and row["owned_identities_preserved"] < old["owned_identities_preserved"])):
            regressions.append([row["provider"], row["id"]])
    paired = [(rows[(row["provider"], row["id"])], row) for row in candidate["cases"]
              if rows[(row["provider"], row["id"])]["forwarded"] and row["forwarded"]]
    old = {key: sum(before[key] for before, _ in paired) for key in counts}
    new = {key: sum(after[key] for _, after in paired) for key in counts}
    transitions = []
    fixture = {case["id"]: case for case in load_cases()}
    for row in candidate["cases"]:
        previous = rows[(row["provider"], row["id"])]
        if previous["forwarded"] or not row["forwarded"]:
            continue
        # Same authored content, source first: an already-forwarded baseline
        # control proves the transition's FP is existing pipeline behaviour.
        twin_id = row["id"].replace("source_last", "source_first")
        control = rows.get((row["provider"], twin_id))
        same_content = (twin_id != row["id"] and twin_id in fixture and
                        sorted(json.dumps(leaf, sort_keys=True) for leaf in fixture[twin_id]["leaves"]) ==
                        sorted(json.dumps(leaf, sort_keys=True) for leaf in fixture[row["id"]]["leaves"]))
        control_kind = "source_first"
        if not (same_content and control and control["forwarded"]):
            expanded_id = fixture[row["id"]].get("admission_control")
            expanded_case = fixture.get(expanded_id)
            if expanded_case and expanded_case.get("expanded_copy_control"):
                def owner_leaves(case):
                    return sorted(json.dumps({"text": leaf["text"], "gold": leaf["gold"]}, sort_keys=True)
                                  for leaf in case["leaves"])
                same_content = (expanded_case.get("seed") == fixture[row["id"]].get("seed")
                                and owner_leaves(expanded_case) == owner_leaves(fixture[row["id"]]))
                control = rows.get((row["provider"], expanded_id))
                twin_id = expanded_id
                control_kind = "expanded_owner_content"
        proven = bool(same_content and control and control["forwarded"] and
                      all(control["false_positive_bytes_" + contract] == row["false_positive_bytes_" + contract]
                          for contract in ("v2", "v1")))
        transitions.append({"provider": row["provider"], "id": row["id"],
                            "candidate": row, "base_source_first_control": control, "control_id": twin_id, "control_kind": control_kind,
                            "preexisting_fp_proven": proven})
        if not proven and gate_policy == "strict":
            regressions.append([row["provider"], row["id"], "unproven admission FP"])
    refusal_byte_drop = sum(item["base_target_leaked_bytes"] for item in leak_refusals)
    leak_drop = old["leaked_bytes"] - new["leaked_bytes"] + refusal_byte_drop
    verdicts = {}
    for contract in ("v2", "v1"):
        seed_fp_cost = 0
        for transition in leak_refusals + resource_refusals:
            key = "seed_false_positive_bytes_" + contract
            before, after = transition["base"].get(key), transition["candidate"].get(key)
            if (type(before) is not int or type(after) is not int or min(before, after) < 0
                    or before > transition["base"]["false_positive_bytes_" + contract]
                    or after > transition["candidate"]["false_positive_bytes_" + contract]):
                raise ValueError("leak-to-refusal requires separate seed false-positive evidence")
            seed_fp_cost += max(0, after - before)
        fp_delta = new["false_positive_bytes_" + contract] - old["false_positive_bytes_" + contract] + seed_fp_cost
        improved = ((leak_drop > 0 and fp_delta < leak_drop) or
                    (leak_drop == 0 and fp_delta < 0) or
                    (leak_drop == 0 and fp_delta == 0 and new["selected_restore_exact"] > old["selected_restore_exact"]))
        verdicts[contract] = {"verdict": "pass" if improved and not regressions else "fail",
                              "leaked_bytes_removed": leak_drop, "false_positive_bytes_added": fp_delta,
                              "net_bytes": leak_drop - fp_delta, "refusal_seed_fp_cost": seed_fp_cost}
    stage_moves = [transition for transition in transitions
                   if not transition["candidate"]["selected_restore_exact"]]
    return {"schema": "gaze-proxy-request-order-comparison-v3", "contracts": verdicts,
            "gate_policy": gate_policy,
            "leak_to_refused": leak_refusals,
            "resource_limit_transitions": resource_refusals,
            "refusal_target_bytes_stopped": refusal_byte_drop,
            "refused_in_both_stage_moved": stage_moves,
            "unproven_admission_fp": [transition for transition in transitions
                                      if not transition["preexisting_fp_proven"]],
            "regressions": regressions,
            "scope": "both-forwarded byte gate; leak-to-refusal credits only proven stopped target leaks, allows separately typed resource-limit refusals without byte credit, and charges seed FP increases; admissions reported separately without gain credit; separate C/A/D/R certification required",
            "paired": {"cases": len(paired), "base": old, "candidate": new},
            "refused_to_forwarded": transitions,
            "base": base["totals"], "candidate": candidate["totals"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime-root", type=Path)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--policy", type=Path)
    parser.add_argument("--session-binary", type=Path)
    parser.add_argument("--fixture", type=Path, default=FIXTURE)
    parser.add_argument("--compare", type=Path, nargs=2, metavar=("BASE", "CANDIDATE"))
    parser.add_argument("--gate-policy", choices=("strict", "paired", "leak-to-refusal"), default="strict",
                        help="comparison only: paired excludes new admissions; leak-to-refusal also permits proven unsafe target refusals")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    if args.compare:
        result = compare(*(json.loads(path.read_text()) for path in args.compare), gate_policy=args.gate_policy)
        args.output.write_text(json.dumps(result, indent=2) + "\n")
        print(json.dumps(result["contracts"], sort_keys=True))
        return 0 if all(row["verdict"] == "pass" for row in result["contracts"].values()) else 1
    if not all((args.binary, args.policy, args.runtime_root)):
        parser.error("capture requires --binary, --policy and --runtime-root")
    result = measure(args.binary.resolve(), args.policy.resolve(), args.fixture, args.output, args.runtime_root.resolve(), args.session_binary)
    print(json.dumps(result["totals"], sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
