"""Generated nonfunctional credential cases for directed runtime probes."""

# Class-generated directed cases. The immutable benchmark corpus is separate.
# Deliberately generate words independently of the recognizer's regex.
ISSUER_PREFIXES = (
    "ghp_", "gho_", "ghu_", "ghs_", "ghr_", "github_pat_", "sk-",
    "sk-proj-", "sk-svcacct-", "sk-admin-", "sk-None-", "sk-ant-api03-",
    "xoxa-", "xoxb-", "xoxp-", "xoxr-", "xoxs-", "xapp-", "xoxe-",
    "xoxe.xoxp-", "sk_live_", "rk_live_", "sk_test_", "rk_test_", "whsec_", "AIzaSy",
)
IDENTIFIER_BOUNDARIES = ("_", ".", "$", "-", "/", "#", "::", " ", "(", '\"', "'")


def identifier_cases(partition):
    words = (("signing", "secret", "config", "endpoint", "rotation", "policy")
             if partition == "dev" else
             ("verify", "header", "setting", "service", "refresh", "option"))
    for prefix in ISSUER_PREFIXES:
        for boundary in IDENTIFIER_BOUNDARIES:
            for length in (6, 12, 24):
                parts = [words[i % len(words)] for i in range(length)]
                for style, tail in (
                    ("snake", "_".join(parts)), ("kebab", "-".join(parts)),
                    ("camel", parts[0] + "".join(p.title() for p in parts[1:])),
                    ("screaming", "_".join(p.upper() for p in parts)),
                    ("snake_digit", "1_".join(parts)),
                ):
                    yield prefix, boundary, style, length, boundary + prefix + tail


def framing_cases():
    # Serializations from all reviewer rounds, with their syntax left in gold.
    breaks = {
        "lf": "\n", "crlf": "\r\n", "json": r"\n", "json_crlf": r"\r\n",
        "nested_json": r"\\n", "nested_json_crlf": r"\\r\\n",
        "unicode": r"\u000a", "unicode_upper": r"\u000A", "unicode_cr": r"\u000d",
        "html": "<br>", "html_closed": "<br/>", "shell": "\\\n", "shell_crlf": "\\\r\n",
        "concat": "\\n' +\n'", "concat_double": '\\n" + "',
        "space": " ", "tab": "\t", "comma": '\\n", "',
        "comma_lines": '\\n",\n    "', "url": "%0A", "url_crlf": "%0d%0a",
        "entity_decimal": "&#10;", "entity_hex": "&#x0a;",
    }
    for name, newline in breaks.items():
        for style, prefix in {"plain": "", "comment": "# ", "slash": "// ",
                              "quote": "> ", "star": "* ", "bullet": "- "}.items():
            yield name + "_" + style, newline + prefix
    for name, prefix in {
        "line_space": "1 ", "line_indent": "  12  ", "cat_n": "     2\t",
        "grep_n": "3:", "editor_pipe": "2 | ", "compact_pipe": "2|",
    }.items():
        yield name, "\n" + prefix


def issuer_value(prefix, chars):
    """Mint format witnesses independently of the rule, never authentic keys.

    Random bodies remain unconditioned: missing digit evidence must remain
    measurable gold. Letter-only camel words are benign counterweights;
    digit-bearing words are an accepted false-positive residual.
    """
    body = chars
    if prefix == "github_pat_":
        return prefix + body(22) + "_" + body(59)
    if prefix == "xoxp-":
        return prefix + "-".join(["0" * 12] * 3 + [body(32)])
    if prefix.startswith("xox") and prefix not in ("xoxe-", "xoxe.xoxp-"):
        return prefix + "0" * 12 + "-" + "0" * 12 + "-" + body(32)
    if prefix == "xapp-":
        return prefix + "1-A" + "0" * 10 + "-" + "0" * 13 + "-" + body(64)
    if prefix.startswith("xoxe"):
        return prefix + "1-" + body(140)
    if prefix == "sk-ant-api03-":
        return prefix + body(93) + "AA"
    if prefix.startswith("sk-"):
        return prefix + body(58) + "T3BlbkFJ" + body(58)
    if prefix.startswith("gh"):
        return prefix + body(36)
    if prefix == "AIzaSy":
        return prefix + body(33)
    return prefix + body(32)


