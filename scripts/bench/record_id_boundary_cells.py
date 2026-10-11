"""CC0 application-owned identifier boundaries; no detector-defined gold.

Serialized scalar content retains its exact source spelling. Public suffixes
remain non-gold even when a conservative malformed/deep-tail scanner owns them.
"""
import json


LABELS = {"customer_id": "CUSTOMER_ID", "employee_id": "EMPLOYEE_ID", "record_id": "RECORD_ID"}
FIELDS = {
    "dev": {
        "customer_id": ("customer_record_id", "customer number", "Kundennummer"),
        "employee_id": ("personnel_id", "personnel number", "Personalnummer"),
        "record_id": ("contact_id", "contact ID", "Kontakt-ID"),
    },
    "test": {
        "customer_id": ("customerId", "customer ID", "Kunden-ID"),
        "employee_id": ("employeeId", "employee ID", "Mitarbeiter-ID"),
        "record_id": ("member_id", "member ID", "Mitgliedsnummer"),
    },
}
CONNECTORS = {"colon": ": ", "equals": "=", "is": " is ", "ist": " ist ",
              "lautet": " lautet ", "space": " "}
PUBLIC = {"dev": ("artifact_reference", "build version", "CEDAR"),
          "test": ("inventory_reference", "package version", "ONYX")}


def encoded_content(value, depth):
    """Content bytes of a string, reserialized depth additional times."""
    for _ in range(depth + 1):
        value = json.dumps(value, ensure_ascii=False)[1:-1]
    return value


def serialized_template(key, depth):
    value_marker = "GAZE_BOUNDARY_VALUE_MARKER"
    public_marker = "GAZE_BOUNDARY_PUBLIC_MARKER"
    text = json.dumps({"arguments": {key: value_marker}, "public_version": public_marker},
                      ensure_ascii=False, separators=(",", ":"))
    for _ in range(depth):
        text = json.dumps({"payload": text}, ensure_ascii=False, separators=(",", ":"))
    return text.replace(value_marker, "{V}").replace(public_marker, "{W}")


def records(api, partition):
    result = []
    families = tuple(LABELS)
    public_key, public_cue, word = PUBLIC[partition]
    lead = "5" if partition == "dev" else "7"

    def emit(layer, family, surface, index, template, fields, language="en"):
        text, gold, decoys = api._fill_with_decoys(template, fields)
        result.append(api.Record(
            uid=f"agentic-{partition}-{layer}-boundary-{family}-{index:03d}-{surface}",
            partition=partition, layer=layer, family=family,
            surface="recordids_boundary_" + surface,
            validity=api.BENIGN if layer == "D" else api.UNCHECKED,
            group=f"{partition}-recordids-boundary-{family}-{index:03d}",
            template=f"recordids-boundary/{family}/{surface}/{partition}",
            language=language, region="DE" if language == "de" else "US",
            text=text, gold=gold, decoys=decoys,
        ))

    def single(family, surface, index, value, template, cue, language="en", public=None):
        fields = {"C": (cue, None), "V": (value, LABELS[family])}
        if public is not None:
            fields["W"] = (public, "decoy:public_column")
        emit("A", family, surface, index, template, fields, language)
        emit("R", family, surface, index, template + "\nLookup {V} again.", fields, language)
        benign = {**fields, "C": (public_key, None), "V": (value, "decoy:public_reference")}
        emit("D", family, surface, index, template, benign, language)

    for position, family in enumerate(families):
        key, en, de = FIELDS[partition][family]
        rng = api.Rng(api.PARTITION_SEEDS[partition], "recordids-boundary/" + family)
        for index in range(api.DOCS_PER_FAMILY):
            serial = lead + rng.digits(5)
            other = lead + rng.digits(5)
            while other == serial:
                other = lead + rng.digits(5)
            first, second = "NX" + serial, "RZ" + other
            cue = en if index % 2 == 0 else de
            language = "en" if index % 2 == 0 else "de"
            for shape, value, punctuation, quote in (
                ("period", first, ".", ""),
                ("colon", first, ":", ""),
                ("dotted", "NX." + serial[:3] + "." + serial[3:], ".", ""),
                ("internal_colon", "NX:" + serial[:3] + ":" + serial[3:], ".", ""),
                ("quoted_period", first + ".", ".", '"'),
                ("quoted_colon", first + ":", ".", "'"),
            ):
                template = "{C}: " + quote + "{V}" + quote + punctuation + " Status remains unchanged."
                single(family, "punctuation_" + shape, index, value, template, cue, language)
            if index == 0:
                value = "CD.56.78" if partition == "dev" else "AB.12.34"
                single(family, "punctuation_short_dotted", index, value,
                       "{C}: {V}. Status remains unchanged.", cue, language)

            for name, connector in CONNECTORS.items():
                for relation, following in (("same", family), ("different", families[(position + 1) % len(families)]),
                                            ("public", None)):
                    next_cue = FIELDS[partition][following][2 if name in ("ist", "lautet") else 1] if following else public_cue
                    if index % 2:
                        next_cue = next_cue.upper()
                    fields = {"C": (cue, None), "V": (first, LABELS[family]), "K": (next_cue, None),
                              "W": (second if following else "3.2." + other,
                                    LABELS[following] if following else "decoy:public_column")}
                    template = "{C}: {V} {K}" + connector + "{W}"
                    surface = f"connectors_{name}_{relation}"
                    emit("A", family, surface, index, template, fields, language)
                    emit("R", family, surface, index,
                         template + ("; Lookup {W} again." if following else "; Lookup {V} again."), fields, language)
                    benign = {**fields, "C": (public_key, None), "V": (first, "decoy:public_reference"),
                              "K": (public_cue, None),
                              "W": (fields["W"][0], "decoy:public_column")}
                    emit("D", family, surface, index, template, benign, language)

            for depth in (0, 1, 2, 3, 8, 9):
                if depth >= 8 and index != 0:
                    continue
                for shape, decoded in (("unicode", first + " É" + other),
                                       ("quote", first + '"\\' + other),
                                       ("tab", first + "\t" + other)):
                    raw = encoded_content(decoded, depth)
                    public = encoded_content("4.7." + other, depth)
                    surface = f"serialized_depth{depth}_{shape}"
                    fields = {"V": (raw, LABELS[family]), "W": (public, "decoy:public_column")}
                    template = serialized_template(key, depth)
                    emit("A", family, surface, index, template, fields)
                    emit("R", family, surface, index, template + "\nLookup {V} again.", fields)
                    emit("D", family, surface, index, serialized_template(public_key, depth),
                         {"V": (raw, "decoy:public_reference"), "W": (public, "decoy:public_column")})

            for newline_name, newline in (("lf", "\n"), ("crlf", "\r\n")):
                for closure, quote in (("closed", '"'), ("malformed", "")):
                    value = newline + first + " É" + other
                    surface = f"opening_lf_{newline_name}_{closure}"
                    template = '{C}: "{V}' + quote + "; public_version={W}"
                    fields = {"C": (cue, None), "V": (value, LABELS[family]),
                              "W": ("5.4." + other, "decoy:public_column")}
                    emit("A", family, surface, index, template, fields, language)
                    # The copy precedes the malformed scalar so it cannot be
                    # hidden inside conservative tail coverage.
                    emit("R", family, surface, index, "Lookup {V} again.\n" + template, fields, language)
                    emit("D", family, surface, index, template,
                         {**fields, "C": (public_key, None), "V": (value, "decoy:public_reference")}, language)

            for shape, value in (("unicode", first + "\tÉ" + other),
                                 ("combining", first + "\tE\u0301" + other)):
                single(family, "tab_group_" + shape, index, value,
                       "{C}: {V}; public_version={W}", cue, language, "6.1." + other)
            for name in ("equals", "is", "space"):
                template = "{C}: {V}\t" + public_cue + CONNECTORS[name] + "{W}"
                single(family, "tab_group_public_" + name, index, first,
                       template, cue, language, "6.2." + other)
            for shape, template, fields in (
                ("header", key + "\tpublic_version\n{V}\t{W}",
                 {"V": ("placeholder", "decoy:placeholder"), "W": ("2.0." + other, "decoy:public_column")}),
                ("placeholder", "{C}: placeholder\tmineral\tpublic_version={W}",
                 {"C": (cue, None), "W": ("2.1." + other, "decoy:public_column")}),
                ("untyped_public", "{C}\t{V}\t{W}",
                 {"C": (public_key, None), "V": (first + "\tÉ" + other, "decoy:public_reference"),
                  "W": (word, "decoy:public_column")}),
            ):
                emit("D", family, "tab_group_" + shape, index, template, fields, language)
    return result
