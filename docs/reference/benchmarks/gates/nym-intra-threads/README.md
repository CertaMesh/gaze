# Nym intra-op thread proof

Nym's ONNX Runtime session runs with deterministic compute, one inter-op thread and
sequential execution. The default intra-op thread count rose from one to four for
latency. That is safe only if piece scores do not depend on the thread count. If they
did, the count would change detection output.

## What the proof compares

`verify_nym_threads` loads one session with one intra-op thread and one session per
count in `GAZE_NYM_VERIFY_THREADS` (default `2,4,8`). It scores every input text with
each session, without inference reuse. It then compares the tokenizer char offsets and,
for every piece, the label, the begin flag and every bit of the score mass. Any
difference exits with an error naming the document and thread count.

The inputs are the F8 envelope workload: for generator seeds 20261126 to 20261225, every
history block and the new message from `scripts/perf/f8_envelope.py` `conversation()`.
That is 1,300 synthetic EN/DE texts. The new messages always contain PII sentences.

## Run it

```sh
cargo build --release -p gaze-recognizers --features safety-net-nym,test-support \
  --example verify_nym_threads
mkdir -p target/nym-threads
python3 - <<'EOF'
import importlib.util, json
spec = importlib.util.spec_from_file_location("f8", "scripts/perf/f8_envelope.py")
f8 = importlib.util.module_from_spec(spec); spec.loader.exec_module(f8)
with open("target/nym-threads/inputs.jsonl", "w") as out:
    for seed in range(100):
        history, new = f8.conversation(20261126 + seed, 1)
        for text in history + new:
            out.write(json.dumps({"text": text}, ensure_ascii=False) + "\n")
EOF
GAZE_NYM_MODEL_DIR="$HOME/.local/share/gaze/models/nym-small-int8" \
  target/release/examples/verify_nym_threads < target/nym-threads/inputs.jsonl
```

## Result

2026-11-27, MacBook Pro M5 Max (arm64, macOS), ONNX Runtime from the pinned `ort`
crate, bundle `nym-small-int8`, inputs sha256
`948c9f3a02750b86394df951c2c825fb9207da8a121465d60e80d7d950769994`:

```json
{"documents":1300,"pieces":765150,"threads":[2,4,8]}
```

Every piece matched the one-thread session bit for bit at 2, 4 and 8 threads.

2026-11-27, Amp orb a1.medium (x86-64 Linux 6.1, 4 vCPU), `ort` 2.0.0-rc.12, same
bundle and the same inputs sha256:

```json
{"documents":1300,"pieces":765150,"threads":[2,4,8]}
```

Every piece matched bit for bit here too.

End to end on the same orb, the F8 server workload (`measure --envelope server
--samples 8 --turns 3 --cold-runs 1 --allow-noisy`, 24 warm turns per side) gave the
same output digest for main 9ff7ad9 (one thread) and for four threads,
`5ae433300216b067916e21f5196b67cb0ffd893a5d1a542f4d65998027a9c49e`, and `compare`
reported byte-identical outputs.

## Limits

The proof covers the CPU execution provider and the machines it ran on. A new ONNX
Runtime version, execution provider or CPU architecture needs a fresh run. Detection
output is also checked end to end. F8 `compare` runs main and the candidate daemon on
the same workload and requires byte-identical output digests.
