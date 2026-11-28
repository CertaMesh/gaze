- Benchmark capture reuse works on glibc Linux hosts such as orbs. The input
  inventory refused `ldd`'s answer for the dynamic loader and the unresolvable
  Tcl/Tk dependencies of uv's never-imported `_tkinter`, which disabled the
  observation cache on every Linux capture. A disabled cache now prints a
  warning with its cause instead of falling back silently.
