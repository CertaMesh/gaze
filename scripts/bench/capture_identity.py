"""Content identities for local native captures; unresolved inputs disable reuse."""

from __future__ import annotations

import hashlib
import importlib.metadata
import json
import platform
import subprocess
import sys
import sysconfig
from pathlib import Path


SCHEMA_VERSION = 3


def digest(value: object) -> str:
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":"),
                                     allow_nan=False).encode()).hexdigest()


def file_digest(path: Path) -> str:
    result = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(chunk)
    return result.hexdigest()


def tree(path: Path) -> dict[str, str]:
    if path.is_symlink() or not path.is_dir():
        raise ValueError("model tree must be a local directory without symlinks")
    result = {}
    for child in sorted(path.rglob("*")):
        if child.is_symlink():
            raise ValueError("model tree contains a symlink")
        if child.is_file():
            result[child.relative_to(path).as_posix()] = file_digest(child)
    if not result:
        raise ValueError("model tree is empty")
    return result


def _command(*args: str) -> str:
    return subprocess.check_output(args, text=True, stderr=subprocess.PIPE).strip()


def native_runtime(binaries: dict[str, Path]) -> dict:
    """Bind the actual loader closure, including OS-shipped shared-cache libraries."""
    system = platform.system()
    libraries = {}
    if system == "Darwin":
        os_build = _command("/usr/bin/sw_vers")
        pending = list(binaries.values())
        seen = set()
        while pending:
            binary = pending.pop()
            if binary in seen:
                continue
            seen.add(binary)
            for line in _command("/usr/bin/otool", "-L", str(binary)).splitlines()[1:]:
                name = line.strip().split(" (", 1)[0]
                path = Path(name)
                if not path.is_absolute():
                    raise ValueError("unresolved dynamic library")
                if name.startswith(("/usr/lib/", "/System/Library/")):
                    libraries[name] = {"apple_os_build": digest(os_build)}
                elif path.is_file():
                    libraries[name] = {"sha256": file_digest(path)}
                    pending.append(path)
                else:
                    raise ValueError("missing dynamic library")
        hardware = _command("/usr/sbin/sysctl", "-n", "machdep.cpu.brand_string", "hw.machine", "hw.ncpu")
    elif system == "Linux":
        os_build = platform.platform()
        hardware = _command("lscpu")
        for binary in binaries.values():
            for line in _command("ldd", str(binary)).splitlines():
                line = line.strip()
                if line.startswith("linux-vdso"):
                    continue
                name = (line.split("=>", 1)[-1] if "=>" in line else line).strip().split(" (", 1)[0]
                path = Path(name)
                if not path.is_absolute() or not path.is_file():
                    raise ValueError("unresolved dynamic library")
                libraries[name] = {"sha256": file_digest(path)}
    else:
        raise ValueError("native cache runtime is unsupported")
    packages = {}
    for distribution in importlib.metadata.distributions():
        if distribution.files is None:
            raise ValueError("installed package has no file inventory")
        packages[distribution.metadata["Name"]] = {
            str(path): file_digest(Path(distribution.locate_file(path)))
            for path in distribution.files if Path(distribution.locate_file(path)).is_file()
        }
    standard_library = Path(sysconfig.get_path("stdlib"))
    python_files = {
        path.relative_to(standard_library).as_posix(): file_digest(path)
        for path in standard_library.rglob("*")
        if path.is_file() and not {"site-packages", "dist-packages"}.intersection(path.relative_to(standard_library).parts)
    }
    return {"system": system, "os_build_sha256": digest(os_build),
            "hardware_sha256": digest(hardware), "libraries": libraries,
            "python_sha256": file_digest(Path(sys.executable)),
            "python_version": sys.version, "stdlib_sha256": digest(python_files),
            "packages_sha256": digest(packages)}


def snapshot(*, files: dict[str, Path], models: dict[str, Path], environment: dict[str, str],
             runtime: dict) -> dict:
    if "GAZE_BENCH_RANDOM_SESSION" in environment:
        raise ValueError("random sessions cannot be replayed")
    if any(name.startswith(("LD_", "DYLD_", "ORT_")) for name in environment):
        raise ValueError("external runtime overrides are not supported by capture reuse")
    if any(environment.get(name) for name in ("PYTHONPATH", "PYTHONHOME")):
        raise ValueError("external Python import roots are not supported by capture reuse")
    return {"schema_version": SCHEMA_VERSION,
            "files": {role: file_digest(path) for role, path in sorted(files.items())},
            "models": {role: tree(path) for role, path in sorted(models.items())},
            # Hash the complete inherited environment, including locale and absent/present flags.
            "environment_sha256": digest(environment),
            "runtime": runtime}
