- The Nym safety net now runs with four ONNX Runtime intra-op threads by
  default instead of one, which cuts its share of a warm daemon turn by about
  a third. Piece scores are bit-identical across thread counts; the proof is in
  `docs/reference/benchmarks/gates/nym-intra-threads/`. Lower the count with
  `--nym-intra-threads` on `gaze clean`, `daemon` and `eval`, or with
  `GAZE_NYM_INTRA_THREADS` for `NymConfig::from_env` in the library.
