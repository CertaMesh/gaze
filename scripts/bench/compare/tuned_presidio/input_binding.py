"""Own tuned selection, replay and live inputs; publish only their commitment."""
from __future__ import annotations

import copy
import dataclasses
import json
from pathlib import Path

import document_capture
import subject_identity
from capture_identity import digest, file_digest


def canonical(value):
    if dataclasses.is_dataclass(value):
        return canonical(dataclasses.asdict(value))
    if isinstance(value, dict):
        return {key: canonical(item) for key, item in value.items()}
    if isinstance(value, (list, tuple)):
        return [canonical(item) for item in value]
    if isinstance(value, (set, frozenset)):
        return sorted(canonical(item) for item in value)
    return value


class Inputs:
    """The same objects supply scoring, live verification and private admission."""
    def __init__(self, *, selection, replayer, paths, files, sample, live_verify, rows, gliner_plan=None, runtime_reader=None):
        self.selection, self.replayer, self.paths = selection, replayer, paths
        self.files, self.sample, self._live_verify, self.rows = files, sample, live_verify, rows
        self.gliner_plan = gliner_plan
        self.runtime_reader = runtime_reader
        self.original = copy.deepcopy(self.binding())
        self.observed = {}

    def config(self, objective):
        choice = self.selection["choices"][objective]
        return choice["finals"][choice["start"]]["config"]

    def binding(self):
        if self.gliner_plan is not None:
            self.gliner_plan.recheck()
            request = subject_identity.gliner_identity.Request("tuned", str(self.paths.gliner), {
                "map_location": "cpu", "load_onnx_model": False, "onnx_model_file": "model.onnx"})
            if self.gliner_plan.descriptor["request"] != request.descriptor():
                raise ValueError("live model selector differs from independent production plan")
        models = {**self.paths.spacy, **{name: getattr(self.paths, name) for name in ("dslim", "davlan", "openmed", "gliner")}}
        return {"schema_version": 1,
                "selection": copy.deepcopy(self.selection),
                "configs": {objective: copy.deepcopy(self.config(objective)) for objective in self.rows},
                "models": {name: {"selector": str(path), "sha256": subject_identity.model_digest(path)}
                           for name, path in models.items() if path is not None},
                "replay": canonical(self.replayer.docs),
                "mapping": canonical(self.replayer.mapping),
                "typed_mapping": canonical(self.replayer.typed_mapping),
                "gliner": self.gliner_plan.descriptor if self.gliner_plan is not None else None,
                "runtime": self.runtime_reader() if self.runtime_reader is not None else None,
                "sample": [doc.uid for doc in self.replayer.docs if self.sample(doc.uid)],
                "files": {str(path): file_digest(path) for path in self.files}}

    def recheck(self):
        if digest(self.binding()) != digest(self.original):
            raise ValueError("selected live inputs changed during measurement")

    def verify_live(self, objective):
        if objective not in self.rows or objective in self.observed:
            raise ValueError("unknown or repeated selected live objective")
        self.recheck()
        result = self._live_verify(self.config(objective), self.paths, self.replayer)
        self.recheck()
        self.observed[objective] = copy.deepcopy(result)
        return result

    def public(self):
        return {"schema_version": 1, "binding_sha256": digest(self.original),
                "sample_documents": len(self.original["sample"])}

    def seal(self, report, path):
        self.recheck()
        for objective, name in self.rows.items():
            provenance = report["provenance"][name]
            if (objective not in self.observed or digest(provenance["config"]) != digest(self.config(objective))
                    or digest(provenance["live_verification"]) != digest(self.observed[objective])
                    or provenance["live_inputs"] != self.public()):
                raise ValueError("tuned output differs from original live computation")
        private = {"binding": self.original, "live": self.observed, "report": report}
        private["sha256"] = digest(private)
        document_capture.atomic_json(path, private)

    def admit(self, report, path, reproduce):
        """An output digest alone never proves its selected replay computation."""
        self.recheck()
        private = json.loads(path.read_text())
        if (set(private) != {"binding", "live", "report", "sha256"}
                or private["sha256"] != digest({k: v for k, v in private.items() if k != "sha256"})
                or digest(private["binding"]) != digest(self.original)
                or digest(private["report"]) != digest(report)):
            raise ValueError("private tuned report/input binding mismatch")
        for objective, name in self.rows.items():
            provenance = report["provenance"][name]
            if (provenance["live_inputs"] != self.public()
                    or digest(provenance["live_verification"]) != digest(private["live"][objective])
                    or digest(provenance["config"]) != digest(self.config(objective))
                    or digest(reproduce(objective)) != digest(report["rows"][name])):
                raise ValueError("private tuned replay differs from submitted computation")
        self.recheck()
