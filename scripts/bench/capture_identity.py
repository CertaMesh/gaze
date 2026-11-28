"""Content identities for local native captures; unresolved inputs disable reuse."""

from __future__ import annotations

import hashlib
import importlib.machinery
import importlib.metadata
import json
import os
import platform
import subprocess
import sys
import sysconfig
from pathlib import Path


SCHEMA_VERSION = 5
ENVIRONMENT_KEYS = frozenset({
    "CARGO_ENCODED_RUSTFLAGS", "CARGO_HOME", "CC", "CFLAGS", "CPPFLAGS", "CXX", "CXXFLAGS",
    "HOME", "LDFLAGS", "MACOSX_DEPLOYMENT_TARGET", "PATH", "RUSTC", "RUSTC_WRAPPER",
    "RUSTFLAGS", "RUSTUP_HOME", "RUSTUP_TOOLCHAIN", "SDKROOT", "LANG", "LANGUAGE", "TZ", "TZDIR",
    "PYTHONHASHSEED", "PYTHONDONTWRITEBYTECODE", "PYTHONUTF8", "PYTHONIOENCODING",
})
ENVIRONMENT_PREFIXES = (
    "CARGO_BUILD_", "CARGO_PROFILE_", "CARGO_TARGET_", "DYLD_", "GAZE_", "LC_", "LD_",
    "JEMALLOC_", "MALLOC_", "MIMALLOC_", "MKL_", "OMP_", "ORT_", "RAYON_", "TOKENIZERS_",
    "OPENBLAS_", "GOTO_", "BLIS_", "KMP_", "VECLIB_", "NUMEXPR_", "ARROW_", "PYARROW_", "NUMPY_",
)


def observation_environment(environment: dict[str, str]) -> dict[str, str]:
    """Declared build/runtime inputs, excluding orchestration IDs and output destinations."""
    return {name: value for name, value in sorted(environment.items())
            if name in ENVIRONMENT_KEYS or name.startswith(ENVIRONMENT_PREFIXES)}


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
    return subprocess.check_output(args, text=True, stderr=subprocess.PIPE,
                                   env={**os.environ, "LC_ALL": "C"}).strip()


def _machine() -> tuple[str, str, str]:
    system = platform.system()
    if system == "Darwin":
        return (system, _command("/usr/bin/sw_vers"),
                _command("/usr/sbin/sysctl", "-n", "machdep.cpu.brand_string", "hw.machine", "hw.ncpu"))
    if system == "Linux":
        return (system, platform.platform(),
                "\n".join(line for line in _command("lscpu").splitlines()
                          if not line.startswith(("CPU MHz:", "CPU(s) scaling MHz:"))))
    raise ValueError("native cache runtime is unsupported")


def machine_identity() -> dict:
    """The OS build and CPU a capture ran on; a gain gate compares only equal machines."""
    system, os_build, hardware = _machine()
    return {"system": system, "os_build_sha256": digest(os_build), "hardware_sha256": digest(hardware)}


def native_runtime(binaries: dict[str, Path]) -> dict:
    """Bind the actual loader closure, including OS-shipped shared-cache libraries."""
    native_roots = {Path(sys.executable), *binaries.values()}

    def include_native(path: Path) -> None:
        if (any(str(path).endswith(suffix) for suffix in importlib.machinery.EXTENSION_SUFFIXES)
                or ".so" in path.suffixes or path.suffix == ".dylib"):
            if not path.is_file():
                raise ValueError("missing Python native module")
            native_roots.add(path)

    # Include lazy imports too: a module first loaded during scoring must be pinned.
    for module in tuple(sys.modules.values()):
        filename = getattr(module, "__file__", None)
        if isinstance(filename, str):
            include_native(Path(filename))
    packages = {}
    for distribution in importlib.metadata.distributions():
        if distribution.files is None:
            raise ValueError("installed package has no file inventory")
        for path in distribution.files:
            include_native(Path(distribution.locate_file(path)))
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
    loaded_roots = set(native_roots)
    for path in standard_library.rglob("*"):
        if path.is_file():
            include_native(path)
    # Stdlib extensions nothing has imported yet. One may still be imported
    # lazily, so its closure is bound, but a dependency the loader cannot find
    # means it can never load (uv's _tkinter without Tcl/Tk): record that
    # instead of refusing the whole inventory.
    unimported = native_roots - loaded_roots
    system, os_build, hardware = _machine()
    libraries = {}
    if system == "Darwin":
        pending = list(native_roots)
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
    else:
        # Sorted, so the walk order (and which side reaches a shared library first) is fixed.
        pending = [(path, path in unimported) for path in sorted(native_roots)]
        seen = set()
        while pending:
            binary, optional = pending.pop()
            if (binary, optional) in seen or (binary, False) in seen:
                continue
            seen.add((binary, optional))
            output = _command("ldd", str(binary))
            if not output:
                raise ValueError("unresolved dynamic library closure")
            # glibc's dynamic loader (and any static object) has no
            # dependencies; its own bytes are bound by the object loading it.
            if output == "statically linked":
                continue
            for line in output.splitlines():
                line = line.strip()
                if line.startswith("linux-vdso"):
                    continue
                if optional and line.endswith("=> not found"):
                    libraries[f"{binary}: {line.split('=>', 1)[0].strip()}"] = {"unresolved": True}
                    continue
                name = (line.split("=>", 1)[-1] if "=>" in line else line).strip().split(" (", 1)[0]
                path = Path(name)
                if not path.is_absolute() or not path.is_file():
                    raise ValueError("unresolved dynamic library")
                libraries[name] = {"sha256": file_digest(path)}
                pending.append((path, optional))
    return {"system": system, "os_build_sha256": digest(os_build),
            "hardware_sha256": digest(hardware), "libraries": libraries,
            "native_roots": {str(path): file_digest(path) for path in sorted(native_roots)},
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
            "environment_sha256": digest(observation_environment(environment)),
            "runtime": runtime}
