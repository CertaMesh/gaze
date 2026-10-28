#!/usr/bin/env python3
"""Count value characters left raw when one ignorable splits a value at each inner position.

Usage: probe_split_exposure.py --candidate BIN --base BIN --locale de-DE|en-US
Each case runs in its own process, without policy, so the repeat sweep cannot hide a result.
The candidate's output must also restore byte-exactly.
"""
import argparse
import json
import os
import re
import subprocess
import tempfile
from concurrent.futures import ThreadPoolExecutor

VALUES = {
    "de-DE": [
        ("Tel. ", "+49 89 12345678"), ("Tel. ", "+49 30 1234567"), ("Tel. ", "+49 30 12345678"),
        ("Tel. ", "030 12345678"), ("Tel. ", "+49 151 23456789"), ("Tel. ", "0151 23456789"),
        ("IBAN ", "DE89 3704 0044 0532 0130 00"), ("Karte ", "4111 1111 1111 1111"),
        ("", "zorbix.quell@example.org"),
    ],
    "en-US": [
        ("Tel ", "+44 20 7946 0958"), ("Tel ", "+44 7700 900123"), ("Tel ", "(212) 555-0147"),
        ("Tel ", "+1 212 555 0147"), ("Tel ", "+33 1 23 45 67 89"), ("Tel ", "+39 06 1234 5678"),
        ("Tel ", "+49 89 12345678"), ("Card ", "4111111111111111"), ("Card ", "3782 822463 10005"),
        ("IBAN ", "GB82WEST12345698765432"), ("", "zorbix.quell@example.org"),
        ("IP ", "192.168.10.20"), ("SSN ", "536-90-4399"),
    ],
}
SEPS = ["​", "‌", "‍", "­", "⁠"]


def alnum(text):
    return re.sub(r"[^0-9A-Za-z]", "", text)


def run(binary, args, payload, cwd):
    out = subprocess.run([binary, *args], input=payload, capture_output=True, cwd=cwd,
                         env=dict(os.environ, GAZE_DISABLE_OS_KEYSTORE="1"))
    return json.loads(out.stdout) if out.returncode == 0 else None


def exposed(binary, locale, label, text, cwd, check_restore):
    out = run(binary, ["clean", "--locale", locale], text.encode(), cwd)
    if out is None:
        raise SystemExit(f"{binary} failed on {text!r}")
    clean = out["clean_text"]
    for entry in out.get("entries", []):
        if entry.get("token"):
            clean = clean.replace(entry["token"], "")
    if check_restore:
        payload = json.dumps({"session_blob": out["session_blob"], "text": out["clean_text"]}).encode()
        restored = run(binary, ["restore"], payload, cwd)
        if restored is None or restored["text"] != text:
            raise SystemExit(f"restore mismatch on {text!r}")
    return len(alnum(clean)) - len(alnum("Ref: " + label + " ok"))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--candidate", required=True)
    parser.add_argument("--base", required=True)
    parser.add_argument("--locale", required=True, choices=sorted(VALUES))
    args = parser.parse_args()
    cases = [(label, value, sep, at) for label, value in VALUES[args.locale] for sep in SEPS
             for at in range(1, len(value)) if value[at - 1].isalnum() or value[at].isalnum()]
    with tempfile.TemporaryDirectory() as cwd:
        def job(case):
            label, value, sep, at = case
            text = f"Ref: {label}{value[:at]}{sep}{value[at:]} ok"
            return (text, exposed(args.candidate, args.locale, label, text, cwd, True),
                    exposed(args.base, args.locale, label, text, cwd, False))
        with ThreadPoolExecutor(4) as pool:
            rows = list(pool.map(job, cases))
    for text, candidate, base in rows:
        if candidate > 0:
            print(f"EXPOSED candidate={candidate} base={base} {text!r}")
    print(json.dumps({"locale": args.locale, "cases": len(rows),
                      "candidate_exposed": sum(c > 0 for _, c, _ in rows),
                      "base_exposed": sum(b > 0 for _, _, b in rows)}))


if __name__ == "__main__":
    main()
