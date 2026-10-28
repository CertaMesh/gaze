# Normalization compatibility

The shared normalizer preserves both relocation parents' behavior, with two intended changes from main: removing every Unicode default-ignorable scalar and folding U+2011 to ASCII hyphen.

The relocation parents are `a0316b08782361d187a250f4496147e543d3835d` for the branch and `cd54f1a8d388a02c2fc0ed0b0f92dd084d9cbd6f` for main. The latter's normalizer is unchanged in integrated main `3fb02d70e756102c871c51471cfd76d7ce6de4bc`.

Each implementation visits the input with `char_indices`. It maps one scalar, applies NFC to that scalar alone, appends its output, and records the scalar's complete raw-byte range for every output byte. It does not compose neighboring scalars or carry normalization state between them.

For any input without default-ignorables or U+2011, equal output for every scalar implies equal concatenated text. The raw starting offset of each scalar depends only on the preceding input bytes. Equal per-scalar provenance therefore also implies equal provenance for the entire input. Both implementations of `raw_range` select the first source start and last source end, so equal provenance implies equal restored source ranges.

`scripts/ci/verify_normalize_merge.py` extracts both parents directly from Git. Its generated Rust test compares output text and provenance for every Unicode scalar against the branch parent. It also compares every scalar outside the two intended changes against main. Composition and range samples check the concatenation argument independently. The generated test is removed on exit.

The existing `normalization_runs_before_detection` contract and `unicode_group_separators` tests are present in both parents. The affected-crate suite runs them alongside the separator, strict-restore, differential, and recognizer tests. Release CLI equality checks the committed synthetic inputs under no policy and each binary's own setup policy.

This argument proves normalization compatibility. It does not prove detector compatibility. The fresh-main card/code matrix, all-validator neighbor matrix, mutation checks and release equality provide separate detection evidence. U+2011 is the sole approved exception to exact visible-control identity and still permits no coverage loss.
