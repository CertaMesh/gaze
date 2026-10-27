#!/usr/bin/env python3
"""Build the committed benchmark host against an exact runtime checkout's libraries."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tomllib


def build(root, output):
    root, output = root.resolve(), output.resolve()
    source = Path(__file__).with_name("native-mcp-operation") / "main.rs"
    (output / "src").mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, output / "src/main.rs")
    manifest = '[package]\nname="gaze-native-mcp-operation"\nversion="0.0.0"\nedition="2021"\n[workspace]\n[dependencies]\n'
    for name, folder, extra in (("gaze", "gaze", ', package="gaze-pii"'),
                                ("gaze-mcp-core", "gaze-mcp-core", ""),
                                ("gaze-assembly", "gaze-assembly", ', features=["safety-net-nym"]')):
        manifest += f'{name}={{path={json.dumps(str(root / "crates" / folder))}{extra}}}\n'
    manifest += 'tokio={version="1",features=["macros","rt-multi-thread"]}\nasync-trait="0.1"\nserde={version="1",features=["derive"]}\nserde_json="1"\n'
    (output / "Cargo.toml").write_text(manifest)
    shutil.copyfile(root / "Cargo.lock", output / "Cargo.lock")
    command = ["cargo", "build", "--release", "--offline", "--manifest-path", str(output / "Cargo.toml"),
               "--target-dir", str(root / "target")]
    subprocess.run(command, check=True, cwd=root)
    original = tomllib.loads((root / "Cargo.lock").read_text())
    generated = tomllib.loads((output / "Cargo.lock").read_text())
    def registry_packages(lock):
        return {(row["name"], row["version"], row.get("checksum")) for row in lock["package"] if "source" in row}
    if not registry_packages(generated) <= registry_packages(original):
        raise ValueError("benchmark host resolved dependencies outside the runtime lockfile")
    def digest(path):
        return hashlib.sha256(path.read_bytes()).hexdigest()
    binary = root / "target/release/gaze-native-mcp-operation"
    result = {"runtime_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
              "source_sha256": digest(source), "builder_sha256": digest(Path(__file__)), "binary_sha256": digest(binary),
              "runtime_lock_sha256": digest(root / "Cargo.lock"),
              "host_lock_sha256": digest(output / "Cargo.lock"), "command": command,
              "registry_dependencies_pinned_to_runtime": True}
    (output / "build-evidence.json").write_text(json.dumps(result, indent=2) + "\n")
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime-root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(build(args.runtime_root, args.output), indent=2))
