#!/usr/bin/env python3
"""Prove credential boundary tests reject weakened embedded patterns.

Run under the project's cargo-build lock. Restores the rulepack after every
probe, including failed commands. Logs contain synthetic fixtures only.
"""
import argparse
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
PACK = ROOT / "crates/gaze-recognizers/embedded/secrets.toml"
MUTATIONS = [
    ("hash-left", "password.hash", r"(?:\A|[^A-Za-z0-9_./+$-])", "", "hash_shapes"),
    ("hash-right", "password.hash", r"(?:\z|[^A-Za-z0-9_./+$=-])", "", "hash_shapes"),
    ("bcrypt-width", "password.hash", "{53}", "{1,53}", "hash_shapes"),
    ("scrypt-width", "password.hash", r"{0,1366}\$[A-Za-z0-9+/]{43}", r"{0,1366}\$[A-Za-z0-9+/]{42,43}", "every_supported_hash_format"),
    ("md5-salt", "password.hash", "{0,8}", "{0,16}", "every_supported_hash_format"),
    ("phpass-width", "password.hash", "{31}", "{30,31}", "every_supported_hash_format"),
]
for source in ("password.code", "security_token.code"):
    MUTATIONS.extend([
        (source + "-left", source, r"(?:\A|[\s{\[(,;])", "", "code_literal"),
        (source + "-tail", source, r"(?:[,;})\]]|\r?\n|\z)", "", "code_literal"),
        (source + "-delimiter", source, "(?:=>|:|=)", r"(?:=>|:|=|\?\?)", "code_literal"),
        (source + "-closing-quote", source, '+)\"', '+)\"?', "code_literal"),
    ])

MUTATIONS.append(("password.code-bare-tail", "password.code", r"[\x20\t]*[,;})\]]", r"[\x20\t]*", "code_literal"))


for source in ("password.code", "security_token.code"):
    MUTATIONS.extend([
        (source + "-array-left", source, r"(?:\A|[\s{\[(,;])", r"(?:\A|[\s{\[,;])", "php_array_credentials"),
        (source + "-array-right", source, r"[,;})\]]", r"[,;}\]]", "php_array_credentials"),
    ])
MUTATIONS.extend([
    ("crypt-salt-alphabet", "password.hash", r"[^$:\n\x00]", "[./A-Za-z0-9]", "producer_hashes"),
    ("django-salt-alphabet", "password.hash", r"[^$]+", "[A-Za-z0-9./_-]+", "producer_hashes"),
    ("generic-token", "security_token.code", "|api_key", "|api_key|token", "generic_token_metadata"),
])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, default=ROOT / "target/gh824/mutations")
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    original = PACK.read_text()
    for name, source, before, after, filter_ in MUTATIONS:
        blocks = original.split("\n[[recognizers]]")
        block_index = next(i for i, block in enumerate(blocks) if f'id = "{source}"' in block)
        if before not in blocks[block_index]:
            raise RuntimeError(f"{name}: mutation no longer identifies its boundary")
        blocks[block_index] = blocks[block_index].replace(before, after)
        try:
            PACK.write_text("\n[[recognizers]]".join(blocks))
            with (args.output_dir / (name + ".log")).open("w") as log:
                result = subprocess.run(["cargo", "test", "-p", "gaze-recognizers", "--test",
                                         "code_credentials", filter_], cwd=ROOT, stdout=log, stderr=log)
            output = (args.output_dir / (name + ".log")).read_text()
            if result.returncode != 101 or "test result: FAILED" not in output:
                raise RuntimeError(f"{name}: expected a test assertion failure, exit={result.returncode}")
            print(name + ": caught", flush=True)
        finally:
            PACK.write_text(original)
    print(f"Caught {len(MUTATIONS)} credential boundary mutations.")


if __name__ == "__main__":
    main()
