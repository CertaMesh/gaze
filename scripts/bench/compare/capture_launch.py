"""Validate source-owned import slots before importing private capture runners."""
import _imp
import importlib.util
import marshal
import os
from pathlib import Path
import struct
import sys
import sysconfig
import types

_initialized = globals().get("_initialized", False)


class HarnessImportError(ValueError):
    """A value-free refusal before harness bytecode can execute."""


def _same_code(actual, expected):
    """Compare typed constants and metadata; importlib fixes cached filenames."""
    if type(actual) is not type(expected):
        return False
    if type(actual) is types.CodeType:
        fields = ("co_argcount", "co_posonlyargcount", "co_kwonlyargcount", "co_nlocals",
                  "co_stacksize", "co_flags", "co_code", "co_names", "co_varnames",
                  "co_freevars", "co_cellvars", "co_firstlineno", "co_linetable",
                  "co_exceptiontable", "co_name", "co_qualname")
        return (all(getattr(actual, key) == getattr(expected, key) for key in fields)
                and _same_code(actual.co_consts, expected.co_consts))
    if type(actual) is tuple:
        return len(actual) == len(expected) and all(_same_code(a, b) for a, b in zip(actual, expected))
    if type(actual) is frozenset:
        if len(actual) != len(expected):
            return False
        unmatched = list(expected)
        for item in actual:
            for index, other in enumerate(unmatched):
                if _same_code(item, other):
                    unmatched.pop(index)
                    break
            else:
                return False
        return True
    # Python equality loses signed zero and numeric types within containers.
    if type(actual) is float:
        return struct.pack("!d", actual) == struct.pack("!d", expected)
    if type(actual) is complex:
        return (struct.pack("!dd", actual.real, actual.imag)
                == struct.pack("!dd", expected.real, expected.imag))
    if type(actual) in (type(None), type(Ellipsis), bool, int, str, bytes):
        return actual == expected
    return False


def _check_bytecode(path):
    """Accept an effective CPython cache only when it executes the source.

    Cache absence and source-equivalent cache creation have the same identity.
    No import, execution, write, deletion or cache repair occurs here. Invalid
    or stale caches conservatively refuse even if CPython could recompile them.
    """
    try:
        cached = Path(importlib.util.cache_from_source(str(path)))
        if not cached.exists():
            return
        data = cached.read_bytes()
        if len(data) < 16 or data[:4] != importlib.util.MAGIC_NUMBER:
            raise ValueError("invalid header")
        flags, = struct.unpack("<I", data[4:8])
        if flags not in (0, 1, 3):
            raise ValueError("unsupported cache flags")
        source = path.read_bytes()
        if flags & 1:
            if data[8:16] != importlib.util.source_hash(source):
                raise ValueError("stale source hash")
        else:
            stamp, size = struct.unpack("<II", data[8:16])
            if stamp != int(path.stat().st_mtime) & 0xffffffff or size != len(source) & 0xffffffff:
                raise ValueError("stale source timestamp/size")
        actual = marshal.loads(data[16:])
        expected = compile(source, str(path), "exec", dont_inherit=True)
        if not _same_code(actual, expected):
            raise ValueError("effective bytecode differs from source")
    except (OSError, ValueError, EOFError, TypeError, SyntaxError):
        raise HarnessImportError("unverified import bytecode") from None


def check_harness():
    """Cover every importable module beneath the admitted benchmark roots."""
    bench = Path(__file__).resolve().parent.parent
    def unreadable(error):
        raise HarnessImportError("unreadable harness import directory") from None

    for directory, children, files in os.walk(bench, onerror=unreadable):
        children[:] = [name for name in children if not name.startswith(".") and name != "__pycache__"]
        for name in children:
            if (Path(directory) / name).is_symlink():
                raise HarnessImportError("linked harness import directory is unsupported")
        for name in files:
            path = Path(directory) / name
            if path.suffix == ".py":
                _check_bytecode(path)
            elif path.suffix == ".pyc":
                raise HarnessImportError("sourceless harness bytecode is unsupported")


def check_stdlib():
    """Validate lazy stdlib imports at their effective CPython cache locations."""
    def unreadable(error):
        raise HarnessImportError("unreadable stdlib import directory") from None

    roots = {Path(sysconfig.get_path(name)).absolute() for name in ("stdlib", "platstdlib")}
    approved = {path.resolve() for root in roots for path in (root, root / "lib-dynload")}
    # Prefix caches include the lexical import path, even for aliases of one tree.
    roots |= {Path(entry or ".").absolute() for entry in sys.path
              if Path(entry or ".").resolve() in approved}
    for root in roots:
        for directory, children, files in os.walk(root, onerror=unreadable):
            children[:] = [name for name in children
                           if name not in {"site-packages", "dist-packages"}]
            for name in children:
                if (Path(directory) / name).is_symlink():
                    raise HarnessImportError("linked stdlib import directory is unsupported")
            for name in files:
                path = Path(directory) / name
                if path.suffix == ".py":
                    parts = path.relative_to(root).with_suffix("").parts
                    module = ".".join(parts[:-1] if parts[-1] == "__init__" else parts)
                    if not _imp.is_frozen(module) and module not in sys.builtin_module_names:
                        _check_bytecode(path)
                elif path.suffix == ".pyc" and not (
                        path.parent.name == "__pycache__" and "." in path.stem):
                    # Tagged cache names contain dots and cannot be selected as
                    # direct module files. Their source owners are checked above.
                    raise HarnessImportError("sourceless stdlib bytecode is unsupported")


def initialize(*, entrypoint_spec=None):
    global _initialized
    if entrypoint_spec is not None:
        raise HarnessImportError("script-path launch only")
    check_stdlib()
    check_harness()
    if _initialized:
        return
    here = Path(__file__).resolve().parent
    # Preserve every pre-existing entry, including duplicate external entries.
    sys.path[:] = [str(here.parent), str(here), str(here / "tuned_presidio"),
                   str(here / "theirbench"), *sys.path]
    _initialized = True
