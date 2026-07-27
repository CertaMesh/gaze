"""Single-document end-to-end probe for the solo #2484 safety-net redaction panic.

Drives ONE corpus document through `clean_for_bench --config full-stack-kiji-resolve`, the
shipped-default policy path (`SafetyNetPolicy::default()` = Resolve + Redact fallback) that
panicked with `range end index 260 out of range for slice of length 235` and exit 101.

Throwaway diagnostic, not a benchmark: it runs one document and asserts on the exit status.

Feature-gate trap this probe guards against: a `clean_for_bench` built WITHOUT `safety-net-kiji`
fails closed with a typed `SafetyNetRegistration` error and exit 1, not 101 — the cell never runs
and the panic looks fixed. This probe detects that case and reports INCONCLUSIVE (exit 2) rather
than letting it read as a pass.

Usage:
    uv run --project scripts/bench --with pyarrow python scripts/bench/panic_probe_2484.py \
        --document dataiku-test-1432
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import dataiku_en_de_gaze_bench as dataiku  # noqa: E402


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--document", default="dataiku-test-1432")
    parser.add_argument("--config", default="full-stack-kiji-resolve")
    parser.add_argument(
        "--dataset",
        type=Path,
        default=Path("target/bench-data/dataiku-en-de/test.parquet"),
    )
    parser.add_argument(
        "--kiji-model-dir",
        type=Path,
        default=Path(
            os.environ.get(
                "GAZE_KIJI_DISTILBERT_MODEL_DIR",
                "~/.local/share/gaze/models/kiji-distilbert",
            )
        ).expanduser(),
    )
    parser.add_argument(
        "--model-dir",
        type=Path,
        default=Path(
            os.environ.get(
                "GAZE_NER_MODEL_DIR",
                "~/.local/share/gaze/models/davlan-mbert-ner-hrl",
            )
        ).expanduser(),
    )
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parents[2]
    dataset = args.dataset if args.dataset.is_absolute() else repo_root / args.dataset
    dataiku.fetch_dataset(dataset)
    documents, _ = dataiku.load_documents(dataset)

    matching = [document for document in documents if document.uid == args.document]
    if not matching:
        raise SystemExit(f"document not found in corpus: {args.document}")
    document = matching[0]

    binary = repo_root / "target/debug/examples/clean_for_bench"
    if not binary.is_file():
        raise SystemExit(
            f"missing {binary}; build it with\n"
            "  cargo build -q -p gaze-recognizers --example clean_for_bench "
            "--features safety-net-kiji"
        )

    environment = os.environ.copy()
    environment["GAZE_NER_MODEL_DIR"] = str(args.model_dir)
    environment["GAZE_KIJI_DISTILBERT_MODEL_DIR"] = str(args.kiji_model_dir)
    environment["GAZE_NER_THRESHOLD"] = "0.3"

    request = {
        "fixture_id": document.uid,
        "locale_chain": document.locale_chain,
        "text": document.text,
    }
    process = subprocess.run(
        [str(binary), "--config", args.config],
        cwd=repo_root,
        env=environment,
        input=json.dumps(request, ensure_ascii=False) + "\n",
        capture_output=True,
        text=True,
        encoding="utf-8",
    )

    print(f"config={args.config} document={document.uid}")
    print(f"exit_code={process.returncode}")
    print(f"stdout_bytes={len(process.stdout.encode('utf-8'))}")
    stderr = process.stderr.strip()
    if stderr:
        print("--- stderr ---")
        print(stderr)

    if "SafetyNetRegistration" in stderr:
        print(
            "\nVERDICT: INCONCLUSIVE — the safety net never ran. Rebuild with "
            "--features safety-net-kiji."
        )
        return 2
    if process.returncode == 101:
        print("\nVERDICT: PANIC REPRODUCED (exit 101).")
        return 1
    if process.returncode != 0:
        print(f"\nVERDICT: non-zero exit {process.returncode} (not a panic).")
        return 1

    # A delivered document proves the cell ran to completion rather than failing closed early.
    record = json.loads(process.stdout.splitlines()[-1])
    print(f"response_keys={sorted(record)}")
    print("\nVERDICT: NO PANIC — the document was processed to completion.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
