"""Small native capture proof and adversarial exact-input cache checks."""

import copy
import gzip
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest import mock
from types import SimpleNamespace

import benchmark_cache as cache
import capture_identity as identity
import gaze_bench_score as score
import scorecard_record as records
import stage_timing as stages
import run_no_opf_benchmark as runner
import render_benchmark_doc as renderer

ROOT = Path(__file__).resolve().parents[2]
CORPUS = Path(__file__).parent / "fixtures/capture-reuse.jsonl"


def documents():
    return [(row["layer"], score.Document(
        row["id"], row["text"], "en", "US", "synthetic-cache-fixture",
        tuple(score.Span(*span) for span in row["gold"])))
        for row in map(json.loads, CORPUS.read_text().splitlines())]


def write_capture(binary, directory, key):
    population = documents()
    main = [doc for layer, doc in population if layer == "C"]
    extra = [doc for layer, doc in population if layer != "C"]
    measurements = {"validator_kinds_by_class": {}, "documents": {
        doc.uid: {"gold_validation": [
            {"start": span.start, "end": span.end, "label": span.label,
             "applicable": False, "validator_passed": None} for span in doc.spans],
            "predictions": None} for _, doc in population}}
    writer = records.RecordWriter(main, records.filter_measurements(measurements, main),
                                  corpus_sha256=identity.file_digest(CORPUS), binary=binary, extra_documents=extra,
                                  layer_contract=score.SCORED_LABEL_CONTRACT_V1)
    runs = {}
    with stages.session(directory / "run-evidence.json",
                        scorecard=directory / "scorecard-v4.json",
                        record=directory / "observations-v1.jsonl.gz") as evidence:
        evidence.cache = {"status": "miss", "key_sha256": cache.key_digest(key)}
        for layer, doc in population:
            with stages.labels(layer=layer), stages.scoring():
                runs[layer] = score.run_config(
                    ROOT, binary, "rule-floor-core", [doc], directory, None, None, None,
                    0.3, directory, base_environment={"PATH": os.environ.get("PATH", "")},
                    validator_measurements=records.filter_measurements(measurements, [doc]),
                    record_document=lambda config, document, response, measured, layer=layer:
                        writer.add(layer, "rule-floor-core", document, response, measured))
        generator = {"corpus_sha256": identity.file_digest(CORPUS), "generator_version": "fixture-v1",
                     "documents": 3, "documents_by_layer": {"A": 1, "D": 1, "R": 1}}
        card = {
            "schema_version": 4, "generated_at": "synthetic", "gaze": {"revision": "synthetic", "dirty": False},
            "dataset": {"integrity": {"sha256": identity.file_digest(CORPUS)},
                        "validator_gold_census": score.validator_gold_census(main, measurements)},
            "scoring": {"scored_label_contract": score.scored_label_contract_report(score.SCORED_LABEL_CONTRACT_V1, main)},
            "parameters": {"ner_threshold": 0.3, "configs": ["rule-floor-core"]},
            "cache_replay": False, "capture_cache": cache.provenance(key), "runs": [runs["C"]],
            "layers": {"generator": generator, "scored_label_contract": score.scored_label_contract_report(
                score.SCORED_LABEL_CONTRACT_V1, extra),
                **{layer: {"population": score.population_summary([doc for l, doc in population if l == layer]),
                           "runs": [runs[layer]]} for layer in ("A", "D", "R")}},
        }
        path = directory / "observations-v1.jsonl.gz"
        card["observation_record"] = writer.write(path, card, add_reference=True)
        cache.store_layers(directory / "cache", key, path)
        (directory / "scorecard-v4.json").write_text(json.dumps(card, indent=2) + "\n")
    return path, card


class CaptureIdentityTests(unittest.TestCase):
    def test_python_and_extension_native_library_mutations_invalidate_identity(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            stdlib = root / "stdlib"
            stdlib.mkdir()
            python, detector, libpython, extension, dependency, libc = (
                root / name for name in ("python", "detector", "libpython.so", "extension.so", "dependency.so", "libc.so"))
            for path in (python, detector, libpython, extension, dependency, libc):
                path.write_bytes(b"synthetic original")
            calls = []
            def command(*args):
                calls.append(args)
                if args[0] == "lscpu":
                    return "Architecture: synthetic"
                library = (libpython if args[1] == str(python) else
                           dependency if args[1] == str(extension) else libc)
                return f"{library} (0x000)"
            with mock.patch.object(identity.platform, "system", return_value="Linux"), \
                    mock.patch.object(identity, "_command", side_effect=command), \
                    mock.patch.object(identity.importlib.metadata, "distributions", return_value=[]), \
                    mock.patch.object(identity.sysconfig, "get_path", return_value=str(stdlib)), \
                    mock.patch.object(identity.sys, "executable", str(python)), \
                    mock.patch.dict(identity.sys.modules, {"synthetic_extension": SimpleNamespace(__file__=str(extension))}):
                original = identity.native_runtime({"detector": detector})
                for path in (libpython, dependency):
                    with self.subTest(library=path.name):
                        path.write_bytes(b"synthetic changed")
                        self.assertNotEqual(identity.native_runtime({"detector": detector}), original)
                        path.write_bytes(b"synthetic original")

    def test_unresolved_python_loader_disables_runtime_inventory(self):
        def command(*args):
            if args[0] == "lscpu":
                return "Architecture: synthetic"
            return "libpython.so => not found"
        with mock.patch.object(identity.platform, "system", return_value="Linux"), \
                mock.patch.object(identity, "_command", side_effect=command), \
                mock.patch.object(identity.importlib.metadata, "distributions", return_value=[]), \
                mock.patch.object(identity.sysconfig, "get_path", return_value="/nonexistent"):
            with self.assertRaisesRegex(ValueError, "unresolved dynamic library"):
                identity.native_runtime({})

    def test_linux_hardware_identity_ignores_live_clock_speed_but_binds_cpu_features(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            library = root / "synthetic-library"
            library.write_bytes(b"synthetic")
            hardware = "Architecture: x86_64\nFlags: synthetic\nCPU(s) scaling MHz: 70%\nCPU MHz: 2400"
            def command(*args):
                return hardware if args[0] == "lscpu" else f"{library} (0x000)"
            with mock.patch.object(identity.platform, "system", return_value="Linux"), \
                    mock.patch.object(identity, "_command", side_effect=command), \
                    mock.patch.object(identity.importlib.metadata, "distributions", return_value=[]), \
                    mock.patch.object(identity.sysconfig, "get_path", return_value=str(root)):
                original = identity.native_runtime({"detector": library})
                hardware = hardware.replace("70%", "95%").replace("2400", "3200")
                self.assertEqual(identity.native_runtime({"detector": library}), original)
                hardware = hardware.replace("Flags: synthetic", "Flags: changed")
                self.assertNotEqual(identity.native_runtime({"detector": library}), original)

    def test_unverified_base_capture_cannot_bypass_release_admission(self):
        with self.assertRaisesRegex(renderer.RenderError, "run evidence"):
            renderer.history_entry_from_scorecard(
                {"capture_cache": {}, "cache_replay": False}, version="v0.1.0", machine="synthetic",
                scorecard_filename="synthetic.json", scorecard_sha256="0" * 64)

    def test_runner_key_binds_native_artifacts_and_capture_parameters(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "docs/reference/benchmarks").mkdir(parents=True)
            paths = {name: root / name for name in ("detector", "validator", "dataset", "negative", "Cargo.lock")}
            paths["spec"] = root / "docs/reference/benchmarks/README.md"
            paths["protocol"] = root / runner.benchmark_spec.PROTOCOL
            paths["protocol"].parent.mkdir(parents=True)
            for path in paths.values():
                path.write_bytes(b"synthetic")
            model = root / "model"
            model.mkdir()
            (model / "weights").write_bytes(b"synthetic")
            args = runner.parse_args(["quick", "--dataset", str(paths["dataset"]),
                                      "--negative-corpus", str(paths["negative"]), "--no-agentic-layers"])
            source = {"revision": "synthetic", "crates_tree_hash": "synthetic", "harness_tree_hash": "synthetic"}
            inputs = dict(repo_root=root, args=args, policy_path=None, policy_sha256=None,
                          corpus_sha256="synthetic", contract=score.SCORED_LABEL_CONTRACT_V1,
                          model_provenance=[], policy_dependencies=None, agentic_prepared=None,
                          document_ids=["synthetic"], threshold=0.3, source_environment={"LANG": "C"},
                          binary=paths["detector"], validator_probe=paths["validator"], davlan_model=model)
            with mock.patch.object(runner, "source_tree_identity", return_value=(source, None)), \
                    mock.patch.object(identity, "native_runtime", return_value={"os": "synthetic"}):
                read = lambda: runner.observation_cache_key(**inputs)[0]
                original = read()
                self.assertIsNotNone(original)
                self.assertEqual(original.get("build_features"), [])
                self.assertEqual(original.get("response_validation"), runner.benchmark_spec.CURRENT_VALIDATION)
                args.manifest_actions = "tokenize"
                self.assertNotEqual(read(), original)
                del args.manifest_actions
                args.cross_check = True
                self.assertNotEqual(read(), original)
                args.cross_check = False
                self.assertEqual(original["binary_sha256"], identity.file_digest(paths["detector"]))
                self.assertEqual(original["binary_sha256"], original["effective_inputs"]["files"]["detector_binary"])
                for name, path in paths.items():
                    with self.subTest(input=name):
                        path.write_bytes(b"changed")
                        self.assertNotEqual(read(), original)
                        path.write_bytes(b"synthetic")
                for field in ("seed", "warmups", "measured_repetitions", "quick_documents"):
                    previous = getattr(args, field)
                    setattr(args, field, previous + 1)
                    self.assertNotEqual(read(), original)
                    setattr(args, field, previous)
                for field in ("corpus_sha256", "document_ids", "threshold", "source_environment"):
                    changed = {**inputs, field: {"corpus_sha256": "changed", "document_ids": ["changed"],
                                               "threshold": 0.4, "source_environment": {"LC_ALL": "changed"}}[field]}
                    self.assertNotEqual(runner.observation_cache_key(**changed)[0], original)
                with mock.patch.object(records, "SCHEMA_VERSION", records.SCHEMA_VERSION + 1):
                    self.assertNotEqual(read(), original)
                prepared = SimpleNamespace(contract=score.SCORED_LABEL_CONTRACT_V1,
                                           manifest={"corpus_sha256": "synthetic", "generator_version": "fixture-v1"})
                inputs["agentic_prepared"] = prepared
                with_layers = read()
                prepared.manifest["generator_version"] = "fixture-v2"
                self.assertNotEqual(read(), with_layers)

    def test_every_effective_file_model_and_environment_change_changes_key(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            files = {name: root / name for name in
                     ("detector_binary", "validator_binary", "policy", "corpus", "generator", "layer_spec", "harness")}
            for path in files.values():
                path.write_bytes(b"synthetic-original")
            model = root / "model"
            model.mkdir()
            (model / "weights").write_bytes(b"synthetic-weights")
            (model / ".configuration").write_bytes(b"synthetic-hidden-input")
            read = lambda env={"LANG": "C", "LC_ALL": "C", "GAZE_NER_LOCALE": "en"}, runtime={"os": "synthetic"}: identity.snapshot(
                files=files, models={"nym": model}, environment=env, runtime=runtime)
            original = read()
            for name, path in files.items():
                with self.subTest(input=name):
                    path.write_bytes(b"synthetic-change")
                    self.assertNotEqual(cache.key_digest(read()), cache.key_digest(original))
                    path.write_bytes(b"synthetic-original")
            for path in model.iterdir():
                with self.subTest(input=path.name):
                    old = path.read_bytes()
                    path.write_bytes(b"synthetic-change")
                    self.assertNotEqual(read(), original)
                    path.write_bytes(old)
            for name in ("LANG", "LC_ALL", "GAZE_NER_LOCALE", "TZ", "PYTHONHASHSEED"):
                self.assertNotEqual(read(env={"LANG": "C", "LC_ALL": "C", "GAZE_NER_LOCALE": "en", name: "synthetic-changed"}), original)
            self.assertEqual(read(env={"LANG": "C", "LC_ALL": "C", "GAZE_NER_LOCALE": "en",
                                       "SOLO_PROCESS_ID": "synthetic", "PWD": "/synthetic/output"}), original)
            self.assertNotEqual(read(runtime={"os": "changed"}), original)
            for value in ("", "0", "1"):
                with self.assertRaisesRegex(ValueError, "random sessions"):
                    read(env={"GAZE_BENCH_RANDOM_SESSION": value})
            (model / "link").symlink_to(model / "weights")
            with self.assertRaisesRegex(ValueError, "symlink"):
                read()


BINARY = Path(os.environ.get("GAZE_CAPTURE_FIXTURE_BINARY", ROOT / "target/debug/examples/clean_for_bench")).resolve()


@unittest.skipUnless(BINARY.is_file(), "build the small native fixture producer to run this proof")
class NativeCaptureReuseTests(unittest.TestCase):
    def test_fresh_capture_hit_is_byte_identical_and_tampering_is_refused(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            fresh = root / "fresh"
            try:
                runtime = identity.native_runtime({"detector": BINARY})
            except (OSError, ValueError) as error:
                self.skipTest(f"native reuse is disabled: {error}")
            key = {"schema_version": identity.SCHEMA_VERSION,
                   "binary_sha256": identity.file_digest(BINARY), "corpus_sha256": identity.file_digest(CORPUS),
                   "policy_sha256": "synthetic", "model_bundle": {}, "seed": 20260710,
                   "generator_version": "fixture-v1", "layer_spec": ["C", "A", "D", "R"],
                   "locale_environment": "synthetic", "harness_schema": records.SCHEMA_VERSION,
                   "runtime": runtime}
            original, card = write_capture(BINARY, fresh, key)
            self.assertEqual(card["observation_record"]["binary_sha256"], key["binary_sha256"])
            destination = root / "hit" / original.name
            with stages.session(destination.parent / "run-evidence.json",
                                scorecard=destination.parent / "scorecard-v4.json",
                                record=destination) as evidence:
                status = cache.restore_layers(fresh / "cache", key, destination)
                evidence.cache = {"status": status, "key_sha256": cache.key_digest(key)}
                self.assertEqual(status, "hit")
                replay = records.rescore(destination, score.SCORED_LABEL_CONTRACT_V1, max_workers=1)
                (destination.parent / "scorecard-v4.json").write_text(json.dumps(replay, indent=2) + "\n")
            self.assertEqual(original.read_bytes(), destination.read_bytes())
            self.assertEqual(json.dumps(card, indent=2), json.dumps(replay, indent=2))
            self.assertEqual(json.loads((fresh / "run-evidence.json").read_text())["cache"]["status"], "miss")
            self.assertEqual(json.loads((destination.parent / "run-evidence.json").read_text())["cache"]["status"], "hit")
            timings = json.loads((fresh / "run-evidence.json").read_text())["stages"]
            self.assertEqual({row["layer"] for row in timings if row["stage"] == "detection"}, {"C", "A", "D", "R"})
            self.assertTrue(all(row["cpu_seconds"] >= 0 for row in timings if row["stage"] in stages.NATIVE_STAGES))
            for name in key:
                mutated = copy.deepcopy(key)
                mutated[name] = {"synthetic_changed": True}
                with self.subTest(input=name):
                    self.assertEqual(cache.restore_layers(fresh / "cache", mutated, root / "miss.gz"), "miss_absent")
            entry = fresh / "cache" / cache.key_digest(key)
            self.assertEqual({p.name for p in entry.glob("[CADR].jsonl.gz")}, {f"{layer}.jsonl.gz" for layer in "CADR"})
            for part in ("header", "C", "A", "D", "R"):
                path = entry / f"{part}.jsonl.gz"
                old = path.read_bytes()
                path.write_bytes(old + b"tampered")
                self.assertEqual(cache.restore_layers(fresh / "cache", key, root / "tampered.gz"), "miss_record_hash")
                self.assertFalse((root / "tampered.gz").exists())
                path.write_bytes(old)
            # Even a legacy entry whose hashes and key binding have all been
            # refreshed must miss when its observation header has no binary pin.
            metadata = entry / "layers.json"
            header_path = entry / "header.jsonl.gz"
            original_header = header_path.read_bytes()
            original_metadata = metadata.read_bytes()
            with gzip.open(header_path, "rt") as stream:
                header = json.loads(next(stream))
            for bound in (None, "f" * 64):
                with self.subTest(record_binary=bound):
                    altered = copy.deepcopy(header)
                    altered.pop("binary_sha256")
                    if bound is not None:
                        altered["binary_sha256"] = bound
                    unbound_record = root / "unbound.jsonl.gz"
                    _, observations = records._read(original)
                    records._write_rows(unbound_record, [altered, *observations])
                    with self.assertRaisesRegex(ValueError, "capture-time binary"):
                        cache.store_layers(root / "rejected-cache", key, unbound_record)
                    records._write_rows(header_path, [altered])
                    forged = json.loads(original_metadata)
                    forged["parts"]["header"] = identity.file_digest(header_path)
                    forged["record_sha256"] = identity.file_digest(unbound_record)
                    metadata.write_text(json.dumps(forged))
                    refused = root / "unbound-hit.gz"
                    self.assertEqual(cache.restore_layers(fresh / "cache", key, refused), "miss_binary_binding")
                    self.assertFalse(refused.exists())
                    self.assertFalse(refused.with_suffix(".gz.cache-tmp").exists())
            header_path.write_bytes(original_header)
            metadata.write_bytes(original_metadata)
            changed = json.loads(metadata.read_text())
            changed["key"]["seed"] += 1
            metadata.write_text(json.dumps(changed))
            self.assertEqual(cache.restore_layers(fresh / "cache", key, root / "wrong-key.gz"), "miss_key")


if __name__ == "__main__":
    unittest.main()
