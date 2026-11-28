- The gain gate and the observation cache compare per-layer benchmark
  sub-specifications instead of full specification IDs, so harness and
  documentation changes outside the C/A/D/R capture closure no longer force a
  recapture. The cache key binds executable bytes instead of Git revisions; a
  hit from another commit is rebound to the executing specification and build.
- Captures record the machine they ran on, and the gate refuses base and
  candidate captured on different machines. Any single quiet machine, such as
  one orb for both sides, can run a gain gate.
