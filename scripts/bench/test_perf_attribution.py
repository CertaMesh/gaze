"""Model-free contract tests for attribution, not latency measurements."""
import json
import os
from pathlib import Path
import tempfile
import subprocess
import unittest
from unittest.mock import patch

import perf_attribution as perf


def trace(phase="request", ordinal=1, *, daemon=False, models=0, nym=False, dob=False):
    keys = (perf.LIFECYCLE - {"daemon_startup_ns", "nym_attach_ns", "model_constructor_ns",
                              "tokenizer_load_ns", "model_load_ns", "bundle_verify_ns",
                              "nym_model_constructor_ns", "nym_outer_verify_ns"}) if phase == "startup" else {"request_ns"}
    metrics = {key: {"ns": 0, "invocations": 1, "candidates": 0} for key in keys}
    if phase == "startup":
        for key, n in {"daemon_startup_ns": int(daemon), "nym_attach_ns": int(nym),
                       "model_constructor_ns": models - int(nym), "nym_model_constructor_ns": int(nym),
                       "nym_outer_verify_ns": int(nym), "tokenizer_load_ns": models,
                       "model_load_ns": models, "bundle_verify_ns": models + int(nym)}.items():
            metrics[key] = {"ns": 0, "invocations": n, "candidates": 0}
    metrics = {**{key: {"ns": 0, "invocations": 0, "candidates": 0} for key in perf.METRICS}, **metrics}
    return {"schema": 3, "phase": phase, "ordinal": ordinal, "metrics": metrics, "counters": dict.fromkeys(perf.COUNTERS, 0), "recognizers": {}}


def fake_script(*, timers=True):
    return ('#!/usr/bin/env python3\nimport sys,json,tomllib\n'
            'sys.path.insert(0, ' + repr(str(Path(__file__).parent.resolve())) + ')\n'
            'from test_perf_attribution import trace\n'
            'daemon = sys.argv[1] == "daemon"\n'
            'policy=tomllib.loads(open(sys.argv[sys.argv.index("--policy")+1]).read())\n'
            'nym=policy.get("safety_net",{}).get("backend")=="nym"\n'
            'dob=bool(policy.get("dob_judge"))\n'
            'models=int(policy.get("ner",{}).get("model_dir") is not None)+int(policy.get("dob_judge",{}).get("enabled",False))+int(nym)\n'
            + ('print("GAZE_PERF "+json.dumps(trace("startup",0,daemon=daemon,models=models,nym=nym,dob=dob)),file=sys.stderr,flush=True)\n' if timers else '')
            + 'def respond(i):\n'
            + (' print("GAZE_PERF "+json.dumps(trace(ordinal=i)),file=sys.stderr,flush=True)\n' if timers else '')
            + ' print(json.dumps({"clean_text":"safe"}),flush=True)\n'
            'if daemon:\n'
            ' for i,line in enumerate(sys.stdin,1): respond(i)\n'
            'else:\n'
            ' sys.stdin.read()\n'
            ' respond(1)\n')


class AttributionTests(unittest.TestCase):
    def test_nearest_rank_tails(self):
        self.assertEqual(perf.percentiles(list(range(1, 101))), {"p50": 50, "p95": 95, "p99": 99})

    def test_load_gate_and_dry_run_fail_closed(self):
        with patch.object(perf, "quiet_threshold", return_value=2.0):
            quiet = [{"load": [1.9, 0, 0]}]
            self.assertTrue(perf.publishable(quiet, dry=False, passes=2, warmups=1))
            for probes, dry, passes, warmups in [
                (quiet, True, 2, 1), ([{"load": [2.0, 0, 0]}], False, 2, 1),
                (quiet, False, 1, 1), (quiet, False, 2, 0),
            ]:
                self.assertFalse(perf.publishable(probes, dry=dry, passes=passes, warmups=warmups))

    def test_model_inventory_detects_changed_bytes_without_paths(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "model.onnx").write_bytes(b"old")
            before = perf.model_inventory([root])
            (root / "model.onnx").write_bytes(b"new")
            self.assertNotEqual(before, perf.model_inventory([root]))
            self.assertNotIn(tmp, json.dumps(before))

    def test_vernio_policy_is_core_only_and_no_safety_net(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = perf.vernio_policy(Path(tmp) / "policy.toml", Path(tmp))
            config = perf.tomllib.loads(path.read_text())
            self.assertEqual(config["policy"]["rulepacks"]["bundled"], ["core"])
            self.assertEqual(config["locale"]["active"], ["de-DE", "en-US", "global"])
            self.assertEqual(config["safety_net"]["backend"], "none")
            self.assertTrue(config["address_blocks"]["enabled"])

    def test_daemon_protocol_old_and_instrumented_binaries(self):
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / "fake-gaze"
            for timers in [False, True]:
                binary.write_text(fake_script(timers=timers))
                (Path(tmp) / "policy").write_text("")
                binary.chmod(0o700)
                docs = [perf.Document(str(i), "private test input", "digest") for i in range(5)]
                rows = perf.run_pass(binary, Path(tmp) / "policy", docs, warmups=2,
                                     dry=True, timeout=5, per_file=False)
                self.assertEqual(len(rows), 5)
                self.assertTrue(all(row["e2e_ns"] is None for row in rows))
                self.assertEqual(rows[0]["trace"] is not None, timers)
                self.assertNotIn("private test input", json.dumps(rows))

    def test_identical_binaries_rejected_before_models_or_setup(self):
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / "gaze"
            binary.write_bytes(b"same binary")
            with patch.object(perf, "commit", return_value="a" * 40):
                with self.assertRaisesRegex(ValueError, "identical binaries"):
                    perf.main(["--binary-a", str(binary), "--binary-b", str(binary),
                               "--common-policy", "unused", "--ner-model-dir", "unused", "--dry-run",
                               "--out", str(Path(tmp) / "out.json")])


class SummaryTests(unittest.TestCase):
    def test_stages_include_zero_work_documents_and_unknown_old_stages(self):
        def trace(metrics):
            return {"metrics": {**metrics, "request_ns": {"ns": 0}}, "recognizers": {}}
        rows = [{"id": "one", "e2e_ns": 100, "trace": trace({"rules_ns": {"ns": 40}})},
                {"id": "two", "e2e_ns": 200, "trace": trace({})}]
        result = perf.summary(rows)
        self.assertEqual(result["stages_ns"]["rules_ns"]["p50"], 0)
        self.assertEqual(result["per_document"]["two"]["stages_ns"]["rules_ns"]["p99"], 0)
        result = perf.summary([{**rows[0], "trace": None}])
        self.assertIsNone(result["stages_ns"])

    def test_each_one_shot_file_gets_its_own_record(self):
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / "fake-gaze"
            binary.write_text(fake_script())
            (Path(tmp) / "policy").write_text("")
            binary.chmod(0o700)
            rows = perf.run_pass(binary, Path(tmp) / "policy", [perf.Document("one", "text", "digest")],
                                 warmups=1, dry=True, timeout=5, per_file=True)
            self.assertEqual(len(rows), 1)
            self.assertEqual(rows[0]["trace"]["schema"], 3)
            self.assertIsNone(rows[0]["e2e_ns"])


class PrivacyTests(unittest.TestCase):
    def test_unknown_fields_and_raw_ids_are_rejected(self):
        base = trace()
        for bad_trace in [{**base, "text": "private"}, {**base, "recognizers": {"private": {"ns": 1}}},
                      {**base, "recognizers": {"a" * 64: {"ns": 1, "invocations": 1, "candidates": 0}}},
                      {**base, "metrics": {"private": {"ns": 1}}},
                      {**base, "counters": {"input_bytes": "private"}}]:
            with self.assertRaisesRegex(ValueError, "contents suppressed"):
                perf.validate_trace(bad_trace)
        self.assertEqual(perf.validate_trace(base), base)



class ReceiptTests(unittest.TestCase):
    def test_complete_receipt_alternates_passes_and_preserves_pairing(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            a, b = root / "a", root / "b"
            a.write_bytes(b"binary a")
            b.write_bytes(b"binary b")
            policy = root / "policy.toml"
            policy.write_text("# fixed policy")
            docs = [perf.Document(str(i), "never copied", "digest") for i in range(400)]
            corpus = [perf.Document(str(i), "different corpus bytes", "other") for i in range(150)]
            def pinned_documents(revision, subdir, seed, *, tree=False):
                return "a" * 40, docs if tree else corpus
            def run(binary, policy, documents, **kwargs):
                return [{"id": d.id, "input_bytes": len(d.text), "e2e_ns": 100 if binary == a.resolve() else 120,
                         "trace": None} for d in documents]
            with patch.object(perf, "commit", return_value="a" * 40), \
                 patch.object(perf, "documents", side_effect=pinned_documents), \
                 patch.object(perf, "model_inventory", return_value=[]), \
                 patch.object(perf, "policy_models", return_value=[]), \
                 patch.object(perf, "host", return_value={"load": [0, 0, 0]}), \
                 patch.object(perf, "run_pass", side_effect=run):
                out = root / "receipt.json"
                perf.main(["--binary-a", str(a), "--binary-b", str(b),
                           "--common-policy", str(policy), "--shipped-policy-a", str(policy),
                           "--shipped-policy-b", str(policy), "--ner-model-dir", str(root),
                           "--measure", "--passes", "3", "--out", str(out)])
            receipt = json.loads(out.read_text())
            self.assertTrue(receipt["publishable"])
            self.assertEqual(len(receipt["arms"]), 5)
            one, daemon = (receipt["arms"][name] for name in ("source-tree", "source-tree-daemon"))
            self.assertEqual(one["input_sha256"], daemon["input_sha256"])
            self.assertEqual(one["input_bytes"], daemon["input_bytes"])
            self.assertEqual(len(one["runs"][0]["sides"]["a"]), 400)
            self.assertEqual(len(daemon["runs"][0]["sides"]["a"]), 400)
            self.assertEqual(len(receipt["arms"]["common"]["runs"][0]["sides"]["a"]), 150)
            self.assertNotEqual(one["input_sha256"], receipt["arms"]["common"]["input_sha256"])
            self.assertEqual(receipt["policies"]["source-tree"], receipt["policies"]["source-tree-daemon"])
            self.assertNotIn("never copied", out.read_text())
            for arm in receipt["arms"].values():
                orders = [run["order"] for run in arm["runs"]]
                self.assertEqual(orders[0], orders[2])
                self.assertEqual(orders[0][::-1], orders[1])
                self.assertEqual(arm["paired_delta_ns_b_minus_a"]["0"]["p50"], 20)
                self.assertIsNone(arm["summary"]["a"]["stages_ns"])



class HookIdentityTests(unittest.TestCase):
    def test_semantic_projection_normalizes_only_session_prefixes(self):
        from perf_instrumentation_proof import canonical
        a = {"clean_text": "<abcdef01:Email_1>", "tokens": [{"token": "<abcdef01:Email_1>", "class": "Email"}], "manifest": [{"clean_span": {"start": 0, "end": 18}}]}
        b = json.loads(json.dumps(a).replace("abcdef01", "12345678"))
        self.assertEqual(canonical(a, daemon=True), canonical(b, daemon=True))
        b["manifest"][0]["clean_span"]["end"] += 1
        self.assertNotEqual(canonical(a, daemon=True), canonical(b, daemon=True))
        b = json.loads(json.dumps(a).replace("Email_1", "Email_2"))
        self.assertNotEqual(canonical(a, daemon=True), canonical(b, daemon=True))

    def test_only_exact_timer_hooks_are_removed(self):
        from perf_hook_identity import without_hooks
        original = 'fn load() {\n    detect();\n}\n'
        hooked = original.replace('    detect();', '    let _verify = gaze_types::perf_span!("bundle_verify_ns");\n    detect();')
        self.assertEqual(without_hooks(hooked), original)
        self.assertNotEqual(without_hooks(hooked.replace('detect()', 'skip()')), original)


class LifecycleTests(unittest.TestCase):
    def test_reviewer_constructor_counterexample(self):
        with tempfile.TemporaryDirectory() as tmp:
            policy = Path(tmp) / "policy"
            policy.write_text('[ner]\nmodel_dir="/synthetic/ner"\n')
            records = [trace("startup", 0, models=1), trace()]
            for key, value in {"process_to_ready_ns": 1000, "pipeline_build_ns": 900,
                               "assembly_ns": 800, "model_constructor_ns": 100,
                               "bundle_verify_ns": 700, "tokenizer_load_ns": 40,
                               "model_load_ns": 40}.items():
                records[0]["metrics"][key]["ns"] = value
            with self.assertRaisesRegex(ValueError, "constructor-leaves"):
                perf.validate_lifecycle(records, 1, daemon=False, policy=policy, required=True)

    def test_reviewer_request_counterexample(self):
        with tempfile.TemporaryDirectory() as tmp:
            policy = Path(tmp) / "policy"
            policy.write_text("")
            records = [trace("startup", 0), trace()]
            records[1]["metrics"]["request_ns"]["ns"] = 1
            records[1]["metrics"]["registry_ns"] = {"ns": 1000000, "invocations": 1, "candidates": 0}
            with self.assertRaisesRegex(ValueError, "registry"):
                perf.validate_lifecycle(records, 1, daemon=False, policy=policy, required=True)

    def test_reviewer_startup_processing_counterexample(self):
        with tempfile.TemporaryDirectory() as tmp:
            policy = Path(tmp) / "policy"
            policy.write_text("")
            records = [trace("startup", 0), trace()]
            records[0]["metrics"]["registry_ns"] = {"ns": 123, "invocations": 1, "candidates": 0}
            with self.assertRaisesRegex(ValueError, "clock phase"):
                perf.validate_lifecycle(records, 1, daemon=False, policy=policy, required=True)

    def test_runner_identity_binds_clock_contract(self):
        original = Path.read_bytes
        def changed(path):
            data = original(path)
            return data + b"\n# changed clock contract\n" if path.name == "perf_lifecycle_contract.py" else data
        with patch.object(Path, "read_bytes", changed):
            self.assertNotEqual(perf.runner_sha256(), perf.RUNNER_SHA256)

    def test_clock_guide_is_generated_from_validation_table(self):
        from perf_lifecycle_contract import update_guide
        update_guide(check=True)

    def test_missing_misattributed_and_old_lifecycle_refused(self):
        import copy
        with tempfile.TemporaryDirectory() as tmp:
            policy = Path(tmp) / "policy"
            policy.write_text("")
            good = [trace("startup", 0, daemon=True), trace()]
            self.assertEqual(perf.validate_lifecycle(good, 1, daemon=True, policy=policy)[0], good[0])
            cases = [[], good[1:], [good[1], good[0]], [good[0], {**good[1], "ordinal": 2}],
                     [{**good[0], "schema": 1}, good[1]]]
            for key in ("process_to_ready_ns", "process_to_policy_ns", "policy_load_ns",
                        "pipeline_build_ns", "assembly_ns", "rule_construction_ns", "recognizer_construction_ns", "daemon_startup_ns"):
                missing = copy.deepcopy(good)
                missing[0]["metrics"][key]["invocations"] = 0
                cases.append(missing)
            wrong = copy.deepcopy(good)
            wrong[1]["metrics"]["policy_load_ns"] = {"ns": 1, "invocations": 1, "candidates": 0}
            cases.append(wrong)
            for key in ("policy_load_ns", "daemon_startup_ns", "nym_attach_ns", "model_constructor_ns", "bundle_verify_ns"):
                wrong = copy.deepcopy(good)
                wrong[0]["metrics"][key]["ns"] = 100
                cases.append(wrong)
            for records in cases:
                with self.subTest(records=records), self.assertRaises(ValueError):
                    perf.validate_lifecycle(records, 1, daemon=True, policy=policy, required=True)

    def test_policy_model_counts_and_bundle_verifications_are_checked(self):
        with tempfile.TemporaryDirectory() as tmp:
            policy = Path(tmp) / "policy"
            policy.write_text('[ner]\nmodel_dir="/synthetic"\n[dob_judge]\nenabled=true\nmodel_dir="/synthetic"\n[safety_net]\nbackend="nym"\n')
            good = [trace("startup", 0, models=3, nym=True, dob=True), trace()]
            perf.validate_lifecycle(good, 1, daemon=False, policy=policy)
            for key in ("model_constructor_ns", "nym_model_constructor_ns", "nym_outer_verify_ns", "model_load_ns", "tokenizer_load_ns", "bundle_verify_ns", "nym_attach_ns"):
                count = good[0]["metrics"][key]["invocations"]
                good[0]["metrics"][key]["invocations"] = count - 1
                with self.assertRaises(ValueError):
                    perf.validate_lifecycle(good, 1, daemon=False, policy=policy)
                good[0]["metrics"][key]["invocations"] = count

    def test_input_identity_binds_order_bytes_and_duplicate_occurrences(self):
        docs = [perf.Document("a", "one", "old"), perf.Document("b", "two", "old")]
        for other in (docs[::-1], docs + docs[:1], [perf.Document("a", "changed", "old"), docs[1]]):
            self.assertNotEqual(perf.input_identity(docs), perf.input_identity(other))

    def test_cold_startup_and_warm_processing_are_separate(self):
        with tempfile.TemporaryDirectory() as tmp:
            binary, policy = Path(tmp) / "gaze", Path(tmp) / "policy"
            policy.write_text("")
            binary.write_text(fake_script())
            binary.chmod(0o700)
            docs = [perf.Document("one", "synthetic", "digest")]
            rows = perf.run_pass(binary, policy, docs, warmups=1, dry=True, timeout=5,
                                 per_file=False, require_timers=True)
            self.assertEqual(rows[0]["startup_trace"]["phase"], "startup")
            self.assertEqual(rows[0]["trace"]["ordinal"], 2)
            self.assertIsNone(rows[0]["startup_e2e_ns"])
            self.assertIsNone(rows[0]["e2e_ns"])

    def test_model_activation_follows_rust_policy_fields(self):
        from perf_lifecycle_contract import active_models, violates_ownership
        for section in ('[ner]\nthreshold=0.3\n', '[ner]\nlocale="de-DE"\n',
                        '[dob_judge]\nenabled=false\n', '[dob_judge]\n',
                        '[safety_net.nym]\nmodel_dir="relative-unused"\n'):
            with self.subTest(section=section), tempfile.TemporaryDirectory() as tmp:
                policy = Path(tmp) / "policy"
                policy.write_text(section)
                config = perf.tomllib.loads(section)
                self.assertFalse(any(active_models(config).values()))
                self.assertEqual(perf.policy_models(policy), [])
                perf.validate_lifecycle([trace("startup", 0), trace()], 1,
                                        daemon=False, policy=policy, required=True)
                for role in ("ner", "dob"):
                    entry = {"ns": 0, "invocations": 1, "candidates": 0,
                             "evidence": "learned", "model": role}
                    self.assertTrue(violates_ownership({"recognizers": {"r1": entry}},
                                    "recognizer-role", config=config, roles={}))

    @unittest.skipUnless(os.environ.get("GAZE_ATTRIBUTION_TEST_BINARY"), "requires a locally built timer CLI")
    def test_real_cli_model_free_policy_matrix_admits(self):
        binary = Path(os.environ["GAZE_ATTRIBUTION_TEST_BINARY"]).resolve()
        proof = json.loads(Path(__file__).with_name("perf-lifecycle-synthetic-proof.json").read_text())
        self.assertEqual(perf.file_sha(binary), proof["timer_binary_sha256"])
        base = ('schema_version="0.1.0"\n[session]\nscope="conversation"\n'
                '[policy.rulepacks]\nbundled=["core"]\n'
                '[[rule]]\nkind="default"\naction="tokenize"\n')
        policies = {
            'rule': '',
            'custom-regex': ('[[policy.custom_recognizers]]\nkind="regex"\nname="synthetic-order"\n'
                             'pattern="SYNTH-[0-9]+"\nclass="custom:order_id"\n'),
            'ner-threshold': '[ner]\nthreshold=0.3\n',
            'ner-locale': '[ner]\nlocale="de-DE"\n',
            'dob-disabled': '[dob_judge]\nenabled=false\n',
            'nym-inactive': '[safety_net.nym]\nmodel_dir="/synthetic/unused"\n',
        }
        for name, settings in policies.items():
            with self.subTest(policy=name), tempfile.TemporaryDirectory() as tmp:
                policy = Path(tmp) / "policy.toml"
                policy.write_text(base + settings)
                self.assertEqual(perf.policy_models(policy), [])
                text = "SYNTH-123 alice@example.invalid"
                docs = [perf.Document("one", text, perf.sha(text.encode()))]
                for one_shot in (True, False):
                    rows = perf.run_pass(binary, policy, docs, warmups=0, dry=True, timeout=30,
                                         per_file=one_shot, require_timers=True)
                    self.assertEqual(len(rows), 1)
                    self.assertEqual(rows[0]["startup_trace"]["metrics"]["model_constructor_ns"]["invocations"], 0)
                    self.assertTrue(all(e["model"] == "none" for e in rows[0]["trace"]["recognizers"].values()))
                requests = [{"session_id": "synthetic", "text": text},
                            {"session_id": "synthetic", "text": text, "context": {}}]
                run = subprocess.run([str(binary), "daemon", "--policy", str(policy)],
                                     input="".join(json.dumps(r) + "\n" for r in requests),
                                     capture_output=True, text=True, timeout=30, env=perf.child_environment())
                self.assertEqual(run.returncode, 0, "daemon failed; diagnostics suppressed")
                responses = [json.loads(line) for line in run.stdout.splitlines()]
                self.assertEqual(len(responses), 2)
                self.assertTrue(all("clean_text" in r and not r.get("error") for r in responses))
                traces = [json.loads(line[len(perf.PREFIX):]) for line in run.stderr.splitlines()
                          if line.startswith(perf.PREFIX)]
                _, admitted = perf.validate_lifecycle(traces, 2, daemon=True, policy=policy, required=True)
                for key in perf.REQUEST_ASSEMBLY:
                    self.assertEqual(admitted[0]["metrics"][key]["invocations"], 0)
                    self.assertEqual(admitted[1]["metrics"][key]["invocations"], 1)

    @unittest.skipUnless(os.environ.get("GAZE_ATTRIBUTION_TEST_BINARY"), "requires a locally built timer CLI")
    def test_real_cli_lifecycle_hooks_are_validated_by_runner(self):
        with tempfile.TemporaryDirectory() as tmp:
            policy = Path(tmp) / "policy.toml"
            policy.write_text('schema_version="0.1.0"\n[session]\nscope="conversation"\n'
                              '[policy.rulepacks]\nbundled=["core"]\n[safety_net]\nbackend="none"\n'
                              '[[rule]]\nkind="default"\naction="tokenize"\n')
            binary = Path(os.environ["GAZE_ATTRIBUTION_TEST_BINARY"]).resolve()
            docs = [perf.Document(str(i), text, perf.sha(text.encode())) for i, text in enumerate(
                ["alice@example.invalid", "No contact details.", "Unicode Grüße: alice@example.invalid"])]
            for one_shot in (True, False):
                try:
                    rows = perf.run_pass(binary, policy, docs, warmups=1, dry=True, timeout=30,
                                         per_file=one_shot, require_timers=True)
                except ValueError as error:
                    self.fail(f"real CLI lifecycle admission failed: {error}")
                self.assertEqual(len(rows), 3)
                self.assertTrue(all(r["trace"]["metrics"]["request_ns"]["invocations"] == 1 for r in rows))

    @unittest.skipUnless(os.environ.get("GAZE_ATTRIBUTION_TEST_BINARY"), "requires a locally built timer CLI")
    def test_real_daemon_context_assembly_is_admitted(self):
        binary = Path(os.environ["GAZE_ATTRIBUTION_TEST_BINARY"]).resolve()
        proof = json.loads(Path(__file__).with_name("perf-lifecycle-synthetic-proof.json").read_text())
        self.assertEqual(perf.file_sha(binary), proof["timer_binary_sha256"])
        with tempfile.TemporaryDirectory() as tmp:
            policy = Path(tmp) / "policy.toml"
            policy.write_text('schema_version="0.1.0"\n[session]\nscope="conversation"\n'
                              '[policy.rulepacks]\nbundled=["core"]\n[safety_net]\nbackend="none"\n'
                              '[[rule]]\nkind="default"\naction="tokenize"\n')
            requests = [{"session_id": "review", "text": "safe text"},
                        {"session_id": "review", "text": "safe text", "context": {}}]
            run = subprocess.run([str(binary), "daemon", "--policy", str(policy)],
                                 input="".join(json.dumps(r) + "\n" for r in requests),
                                 capture_output=True, text=True, timeout=30, env=perf.child_environment())
            self.assertEqual(run.returncode, 0, "daemon failed; diagnostics suppressed")
            responses = [json.loads(line) for line in run.stdout.splitlines()]
            self.assertEqual(len(responses), 2)
            self.assertTrue(all("clean_text" in r and not r.get("error") for r in responses))
            traces = [json.loads(line[len(perf.PREFIX):]) for line in run.stderr.splitlines()
                      if line.startswith(perf.PREFIX)]
            _, admitted = perf.validate_lifecycle(traces, 2, daemon=True, policy=policy, required=True)
            for key in perf.REQUEST_ASSEMBLY:
                self.assertEqual(admitted[0]["metrics"].get(key, {}).get("invocations", 0), 0)
                self.assertEqual(admitted[1]["metrics"][key]["invocations"], 1)
                self.assertGreater(admitted[1]["metrics"][key]["ns"], 0)



class FrozenPolicyTests(unittest.TestCase):
    def test_external_unpinned_inputs_and_implicit_models_are_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "policy.toml"
            for content in ['[policy.rulepacks]\npaths=["outside.toml"]\n',
                            '[[detector]]\nterms_file="outside.txt"\n',
                            '[dob_judge]\nenabled=true\n',
                            '[safety_net]\nbackend="nym"\n',
                            '[ner]\nmodel_dir="~/models"\n',
                            '[ner]\nmodel_dir="${GAZE_NER_MODEL_DIR}"\n']:
                path.write_text(content)
                with self.assertRaises(ValueError):
                    perf.policy_models(path)


class ChildEnvironmentTests(unittest.TestCase):
    def test_setup_daemon_and_one_shot_use_only_inventoried_models(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            pinned, inherited = root / "pinned", root / "inherited"
            pinned.mkdir()
            inherited.mkdir()
            (pinned / "model.onnx").write_bytes(b"pinned")
            (inherited / "model.onnx").write_bytes(b"unrecorded model bytes")
            journal = root / "child-environments.jsonl"
            policy = root / "policy.toml"
            policy_text = ('[safety_net]\nbackend="nym"\n[safety_net.nym]\n'
                           f'model_dir={json.dumps(str(pinned))}\n')
            policy.write_text(policy_text)
            script = ('#!/usr/bin/env python3\nimport json,os,sys\nfrom pathlib import Path\n'
                      f'with open({str(journal)!r}, "a") as log: log.write(json.dumps(dict(os.environ))+"\\n")\n'
                      'if sys.argv[1] == "setup":\n'
                      f' Path(sys.argv[sys.argv.index("--policy-out")+1]).write_text({policy_text!r})\n'
                      ' sys.exit(0)\n'
                      + fake_script().split("\n", 1)[1])
            binaries = [root / side for side in ("a", "b")]
            for index, binary in enumerate(binaries):
                binary.write_text(script + f"# side {index}\n")
                binary.chmod(0o700)
            docs = [perf.Document(str(i), "synthetic input", "digest") for i in range(400)]
            overrides = {"GAZE_NYM_MODEL_DIR": str(inherited), "GAZE_NER_MODEL_DIR": str(inherited),
                         "GAZE_FUTURE_POLICY_DIR": str(inherited), "GAZE_NYM_INTRA_THREADS": "9",
                         "OPF_CHECKPOINT": str(inherited), "GAZE_OPENAI_FILTER_OPF": "unrecorded",
                         "ORT_DYLIB_PATH": "unrecorded-runtime", "GAZE_PERF": "0"}
            with patch.dict(os.environ, overrides), \
                 patch.object(perf, "commit", return_value="a" * 40), \
                 patch.object(perf, "documents", return_value=("a" * 40, docs)), \
                 patch.object(perf, "host", return_value={"load": [0, 0, 0]}):
                out = root / "receipt.json"
                perf.main(["--binary-a", str(binaries[0]), "--binary-b", str(binaries[1]),
                           "--common-policy", str(policy), "--ner-model-dir", str(pinned),
                           "--dry-run", "--out", str(out)])
            receipt = json.loads(out.read_text())
            self.assertFalse(receipt["publishable"])
            self.assertRegex(receipt["child_environment_sha256"], r"^[0-9a-f]{64}$")
            self.assertEqual(receipt["model_inventory"], perf.model_inventory([pinned]))
            for arm in receipt["arms"].values():
                for rows in arm["runs"][0]["sides"].values():
                    self.assertTrue(all(row["trace"]["schema"] == 3 for row in rows))
            environments = [json.loads(line) for line in journal.read_text().splitlines()]
            self.assertGreater(len(environments), 2)
            for environment in environments:
                self.assertEqual({k: v for k, v in environment.items() if k.startswith("GAZE_")},
                                 {"GAZE_PERF": "1"})
                self.assertNotIn("OPF_CHECKPOINT", environment)
                self.assertNotIn("ORT_DYLIB_PATH", environment)

    def test_mocked_workload_cannot_publish_an_uninventoried_override(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            pinned, inherited = root / "pinned", root / "inherited"
            pinned.mkdir()
            inherited.mkdir()
            (pinned / "model.onnx").write_bytes(b"pinned")
            (inherited / "model.onnx").write_bytes(b"unrecorded")
            binaries = [root / side for side in ("a", "b")]
            for index, binary in enumerate(binaries):
                binary.write_bytes(str(index).encode())
            policy = root / "policy.toml"
            policy.write_text('[safety_net]\nbackend="nym"\n[safety_net.nym]\n'
                              f'model_dir={json.dumps(str(pinned))}\n')
            docs = [perf.Document(str(i), "synthetic input", "digest") for i in range(400)]
            effective_models = set()
            def run(binary, policy, documents, **kwargs):
                environment = kwargs.get("child_env", os.environ)
                effective_models.add(Path(environment.get("GAZE_NYM_MODEL_DIR", pinned)))
                return [{"id": d.id, "input_bytes": len(d.text), "e2e_ns": 100, "trace": None}
                        for d in documents]
            with patch.dict(os.environ, {"GAZE_NYM_MODEL_DIR": str(inherited)}), \
                 patch.object(perf, "commit", return_value="a" * 40), \
                 patch.object(perf, "documents", return_value=("a" * 40, docs)), \
                 patch.object(perf, "host", return_value={"load": [0, 0, 0]}), \
                 patch.object(perf, "run_pass", side_effect=run):
                out = root / "receipt.json"
                perf.main(["--binary-a", str(binaries[0]), "--binary-b", str(binaries[1]),
                           "--common-policy", str(policy), "--shipped-policy-a", str(policy),
                           "--shipped-policy-b", str(policy), "--ner-model-dir", str(pinned),
                           "--measure", "--passes", "2", "--out", str(out)])
            receipt = json.loads(out.read_text())
            self.assertTrue(receipt["publishable"])
            self.assertEqual(receipt["model_inventory"], perf.model_inventory(sorted(effective_models)))
            self.assertNotIn(str(inherited), out.read_text())


if __name__ == "__main__":
    unittest.main()
