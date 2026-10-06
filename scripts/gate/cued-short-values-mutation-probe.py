#!/usr/bin/env python3
"""Mutation probe for `age.phrase`, `birth_date.answer`, `card.cued_short` and `postal.cued_short`.

Applies each weakening below alone to `crates/gaze-recognizers/embedded/core.toml` (every edit
site must occur exactly once in the file), runs `cargo test -p gaze-recognizers --all-features
--test cued_short_values`, and expects the run to fail. A run that executes zero tests counts as
a failure of the probe, not a kill. The rulepack is restored afterwards. Commit before running:
the probe rewrites core.toml in place. Exit code 1 if any mutant survives.

Usage: python3 scripts/gate/cued-short-values-mutation-probe.py
"""
import os, re, subprocess, sys
root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
core = os.path.join(root, "crates/gaze-recognizers/embedded/core.toml")
env = dict(os.environ)
original = open(core, encoding="utf-8").read()

def section(rid):
    start = original.index(f'id = "{rid}"')
    end = original.find("\n[[recognizers]]", start)
    return start, (end if end != -1 else len(original))

MUTANTS = [
  # (name, recognizer id, old, new)
  ("age: drop the animal refusal", "age.phrase",
   "(?:cat|kitten|dog|puppy|horse", "(?:zzzcat|kitten|dog|puppy|zzzhorse"),
  ("age: turned accepts any follower", "age.phrase",
   "(?:[\\x20\\t\\x{A0}\\x{202F}]*(?:[,.;:!?)}\\]\"']|\\r?\\n|\\z)|[\\x20\\t\\x{A0}\\x{202F}]+(?:and|but|so|or|last|this|next|in|on|at|yesterday|today|recently|already|when|while|because|since|before|after|years?|yrs?)\\b)", "\\b"),
  ("age: turned needs no person", "age.phrase",
   "|girl|woman|man|lady|guy)\\b\n  [^\\n.!?]{0,32}?\\b(?:turned", "|girl|woman|man|lady|guy|the|die|das|our)\\b\n  [^\\n.!?]{0,32}?\\b(?:turned"),
  ("age: at-the-age-of needs no person", "age.phrase",
   "|girl|woman|man|lady|guy)\\b\n  [^\\n.!?]{0,64}?\\bat", "|girl|woman|man|lady|guy|the)\\b\n  [^\\n.!?]{0,64}?\\bat"),
  ("age: geworden needs no person", "age.phrase",
   "enkelin|enkel|freundin|freund)\\b\n  [^\\n.!?]{0,40}?", "enkelin|enkel|freundin|freund|die|das)\\b\n  [^\\n.!?]{0,40}?"),
  ("age: im-alter-von needs no person", "age.phrase",
   "enkelin|enkel|freundin|freund)\\b\n  [^\\n.!?]{0,64}?\\bim", "enkelin|enkel|freundin|freund|die)\\b\n  [^\\n.!?]{0,64}?\\bim"),
  ("age: y/o copula branch removed", "age.phrase",
   "((?:[1-9][0-9]?|1[01][0-9]|12[0-2]))[\\x20\\t\\x{A0}\\x{202F}]?y/o\\b\n", "(zzzz)[\\x20\\t\\x{A0}\\x{202F}]?y/o\\b\n"),
  ("age: y/o person-noun list admits objects", "age.phrase",
   "y/o[\\x20\\t\\x{A0}\\x{202F}]+(?:(?:fe)?male|man", "y/o[\\x20\\t\\x{A0}\\x{202F}]+(?:laptop|(?:fe)?male|man"),
  ("dob: copula optional", "birth_date.answer",
   "(?:it[’']?s|it[\\x20\\t]+is|that[’']?s|that[\\x20\\t]+is|es[\\x20\\t]+ist|das[\\x20\\t]+ist)\n", "(?:[^\\n0-9]{0,24}?)\n"),
  ("card: loose cue window", "card.cued_short",
   "| [\"'\\x20]*[:=]?[\"'\\x20]*(?i:(?:is|ist|lautet)\\x20+)?\n", "| [\"'\\x20]*[:=]?[^\\d\\n.;!?:,=]{0,32}?\n"),
  ("card: any issuer prefix", "card.cued_short",
   "| (?:5[06-9]|6[0-9])[0-9]{10,13}", "| [0-9][0-9]{11,14}"),
  ("card: no trailing boundary", "card.cued_short",
   ")\n(?:[^\\d\\x20\\x{00A0}\\x{202F}-]|[\\x20\\x{00A0}\\x{202F}-][^\\d]|[\\x20\\x{00A0}\\x{202F}-]?\\z)\n'''", ")\n'''"),
  ("postal: loose cue window", "postal.cued_short",
   "  [\\x20\\t\\x{A0}\\x{202F}:=\"'-]*(?:(?:is|ist|lautet)[\\x20\\t\\x{A0}\\x{202F}]+)?\n  (\n", "  [^\\d\\n]{0,40}?\n  (\n"),
  ("postal: three digits after PLZ", "postal.cued_short",
   "post[\\x20\\t_-]*code|p[oó]stn[uú]mer)\\b\n  [\\x20\\t\\x{A0}\\x{202F}:=\"'-]*(?:is", "post[\\x20\\t_-]*code|plz|p[oó]stn[uú]mer)\\b\n  [\\x20\\t\\x{A0}\\x{202F}:=\"'-]*(?:is"),
  ("postal: no trailing boundary", "postal.cued_short",
   ")\n(?:[^\\d\\x20\\x{A0}\\x{202F}-]|[\\x20\\x{A0}\\x{202F}-][^\\d]|[\\x20\\x{A0}\\x{202F}-]?\\z)'''", ")'''"),
]
def run_tests():
    run = subprocess.run(["nice", "-n", "19", "cargo", "test", "-q", "-p", "gaze-recognizers", "--all-features",
                          "--test", "cued_short_values"], cwd=root, env=env, capture_output=True, text=True)
    return run, run.stdout + run.stderr

baseline, out = run_tests()
if baseline.returncode != 0 or not re.search(r"running [1-9]\d* tests", out):
    sys.exit(f"baseline must pass on the unmodified rulepack (exit {baseline.returncode})")
print("baseline -> passes", flush=True)
results = []
try:
    for name, rid, old, new in MUTANTS:
        start, end = section(rid)
        body = original[start:end]
        if body.count(old) != 1 or original.count(old) != 1:
            results.append((name, f"EDIT SITE NOT UNIQUE ({body.count(old)} in rule, {original.count(old)} in file)")); continue
        open(core, "w", encoding="utf-8").write(original[:start] + body.replace(old, new) + original[end:])
        run, out = run_tests()
        ran = re.search(r"running (\d+) tests", out)
        failed = re.findall(r"^    (\w+)$", out, flags=re.M)
        if ran is None or ran.group(1) == "0":
            verdict = f"NO TESTS RAN (exit {run.returncode})"
        elif run.returncode == 0:
            verdict = "SURVIVED"
        else:
            verdict = "killed by " + ", ".join(sorted(set(failed))) if failed else f"killed (exit {run.returncode})"
        results.append((name, verdict)); print(name, "->", verdict, flush=True)
finally:
    open(core, "w", encoding="utf-8").write(original)
print("\nSUMMARY")
for name, verdict in results: print(f"- {name}: {verdict}")
sys.exit(0 if all(verdict.startswith("killed") for _, verdict in results) else 1)
