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


def encoded_content(value, depth, ascii_unicode=False):
    """Content bytes of a string, reserialized depth additional times."""
    for step in range(depth + 1):
        value = json.dumps(value, ensure_ascii=ascii_unicode and step == 0)[1:-1]
    return value


def serialized_template(key, depth):
    value_marker = "GAZE_BOUNDARY_VALUE_MARKER"
    public_marker = "GAZE_BOUNDARY_PUBLIC_MARKER"
    text = json.dumps({"arguments": {key: value_marker}, "public_version": public_marker},
                      ensure_ascii=False, separators=(",", ":"))
    for _ in range(depth):
        text = json.dumps({"payload": text}, ensure_ascii=False, separators=(",", ":"))
    return text.replace(value_marker, "{V}").replace(public_marker, "{W}")


def records(api, partition, *, version=27):
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
                                       ("unicode_escape", first + " É E\u0301" + other),
                                       ("quote", first + '"\\' + other),
                                       ("tab", first + "\t" + other)):
                    raw = encoded_content(decoded, depth, shape == "unicode_escape")
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
                    value = (newline if closure == "closed" or version < 27 else "") + first + " É" + other
                    surface = f"opening_lf_{newline_name}_{closure}"
                    template = '{C}: "{V}' + quote + ("\n" if version >= 27 and not quote else "; ") + "public_version={W}"
                    fields = {"C": (cue, None), "V": (value, LABELS[family]),
                              "W": ("5.4." + other, "decoy:public_column")}
                    emit("A", family, surface, index, template, fields, language)
                    # The copy precedes the malformed scalar so it cannot be
                    # hidden inside conservative tail coverage.
                    emit("R", family, surface, index, "Lookup {V} again.\n" + template, fields, language)
                    emit("D", family, surface, index, template,
                         {**fields, "C": (public_key, None), "V": (value, "decoy:public_reference")}, language)

            for delimiter_name, delimiter in (("semicolon", ";"), ("comma", ","), ("newline", "\n")):
                value = first + ("\\n" if version >= 27 and delimiter == "\n" else delimiter) + "É" + other
                for boundary_name, boundary in (("eof", ""), ("public", "; public_version={W}"),
                                                 ("quoted_public", ', "public_version": {W}')):
                    fields = {"C": (cue, None), "V": (value, LABELS[family]),
                              "W": ("5.8." + other, "decoy:public_column")}
                    surface = f"unterminated_{delimiter_name}_{boundary_name}"
                    template = '{C}: "{V}' + ('"' if version >= 27 and boundary else "") + boundary
                    emit("A", family, surface, index, template, fields, language)
                    emit("R", family, surface, index, "Lookup {V} again.\n" + template, fields, language)
                    emit("D", family, surface, index, template,
                         {**fields, "C": (public_key, None), "V": (value, "decoy:public_reference")}, language)

            if version >= 27:
                # A key-looking fragment is application-owned private content.
                # Public counterweights are separate fields after a record end
                # or outside the enclosing serialized string, never inferred
                # from a key-looking fragment inside an unfinished value.
                for suffix_name, suffix in (
                    ("private_assignment", ";private_code=É" + other),
                    ("short_id", ";ID: " + other[:3]),
                    ("name_assignment", ", name=É" + other),
                    ("nbsp_assignment", ";\u00a0private_code=É" + other),
                    ("public_looking", "; public_version=É" + other),
                    ("escaped_record", "\r\nprivate_code=É" + other),
                    ("quoted_key", '; "private_code": É' + other),
                ):
                    for shape, depth in (("record", None), ("note", 0), ("payload", 2), ("deep_payload", 9)):
                        if shape == "deep_payload" and index != 0:
                            continue
                        if shape == "record" and suffix_name == "escaped_record":
                            continue
                        value = first + suffix
                        public = "5.9." + other
                        surface = f"private_keys_{suffix_name}_{shape}"
                        if depth is None:
                            prefix = "format_hint='quote character: \"'; " if index % 2 else ""
                            template = prefix + '{C}: "{V}\npublic_version={W}'
                        else:
                            marker, public_marker = "GAZE_PRIVATE_VALUE", "GAZE_PUBLIC_VALUE"
                            content = '{C}: "' + marker if shape == "note" else '{"{C}":"' + marker
                            payload = json.dumps({"note": content, "public_version": public_marker}, ensure_ascii=False)
                            for _ in range(depth):
                                payload = json.dumps({"payload": payload}, ensure_ascii=False)
                            template = payload.replace(marker, "{V}").replace(public_marker, "{W}")
                            value = encoded_content(value, depth)
                            public = encoded_content(public, depth)
                        fields = {"C": (key if shape in ("payload", "deep_payload") else cue, None),
                                  "V": (value, LABELS[family]), "W": (public, "decoy:public_column")}
                        emit("A", family, surface, index, template, fields, language)
                        emit("R", family, surface, index, "Lookup {V} again.\n" + template, fields, language)
                        emit("D", family, surface, index, template,
                             {**fields, "C": (public_key, None), "V": (value, "decoy:public_reference")}, language)

            if version >= 27:
                # Producers declare the whole raw scalar independently of the
                # scanner. Public siblings and public reference fields retain
                # their real quote syntax, including later apostrophe values.
                dialects = [
                    ("yaml_single", first + "''É" + other, "{C}: '{V}'", "yaml"),
                    ("sql_single", first + "''É" + other, "{C}='{V}'", "sql"),
                    ("csv_double", first + '""É' + other, '{C}: "{V}"', "csv"),
                    ("opaque_prefix", first + ";private_code=É" + other,
                     'format_hint: "marker; {C}: "{V}', "unfinished"),
                ]
                for quote, name in (("'", "literal"), ('"', "basic")):
                    delimiter = quote * 3
                    for shape, value in (
                        ("multiline", first + "\nÉ" + other),
                        ("initial_lf", "\n" + first + " É" + other),
                        ("quote_tail", first + quote + "É" + other + quote * 2),
                    ):
                        dialects.append((f"toml_{name}_{shape}", value,
                                         "{C} = " + delimiter + "{V}" + delimiter, "toml"))
                private_key = first + '; "private_code": É' + other
                for quote, name in (("'", "single"), ('"', "double")):
                    dialects.append(("ambiguous_" + name, quote + private_key,
                                     "{C}: " + quote + "{V}", "unfinished"))
                    dialects.append(("unfinished_" + name, first + ";É" + other,
                                     "{C}: " + quote + "{V}", "unfinished"))
                    delimiter = quote * 3
                    dialects.append(("prefix_triple_" + name, private_key,
                                     "format_hint = " + delimiter + "quote '\" marker" + delimiter
                                     + "\n{C} = '{V}'", "toml"))
                for dialect, decoded, declaration, kind in dialects:
                    depths = [None, *range(9)] if index == 0 else [None]
                    for depth in depths:
                        value, public = decoded, "6.8." + other
                        if depth is None:
                            if kind == "yaml":
                                template = declaration + "\npublic_version: '{W}'"
                            elif kind == "sql":
                                template = "UPDATE synthetic_records SET " + declaration + ", public_version='{W}';"
                            elif kind == "csv":
                                template = declaration + ', "{W}"'
                            else:
                                template = declaration + '\npublic_version = "{W}"'
                            shape = "record"
                        else:
                            marker, public_marker = "GAZE_DIALECT_VALUE", "GAZE_DIALECT_PUBLIC"
                            note = declaration.replace("{V}", marker)
                            payload = json.dumps({"note": note, "public_version": public_marker,
                                                  "tags": ["'", "FRAME"]}, ensure_ascii=False)
                            for _ in range(depth):
                                payload = json.dumps({"payload": payload, "public_version": public_marker},
                                                     ensure_ascii=False)
                            template = payload.replace(marker, "{V}").replace(public_marker, "{W}")
                            value, public = encoded_content(value, depth), encoded_content("'", depth)
                            shape = f"json_depth{depth + 1}"
                        fields = {"C": (key, None), "V": (value, LABELS[family]),
                                  "W": (public, "decoy:public_column")}
                        surface = f"quote_dialects_{dialect}_{shape}"
                        emit("A", family, surface, index, template, fields)
                        emit("R", family, surface, index, "Lookup {V} again.\n" + template, fields)
                        emit("D", family, surface, index, template,
                             {**fields, "C": (public_key, None), "V": (value, "decoy:public_reference")})

                for depth in (range(9) if index == 0 else (0,)):
                    marker = "GAZE_EMPTY_NOTE_PUBLIC"
                    payload = json.dumps({"note": "{C}: ", "public_version": marker,
                                          "tags": ["'", "FRAME"]}, ensure_ascii=False)
                    for _ in range(depth):
                        payload = json.dumps({"payload": payload, "public_version": marker}, ensure_ascii=False)
                    emit("D", family, f"quote_dialects_absent_value_json_depth{depth + 1}", index,
                         payload.replace(marker, "{W}"),
                         {"C": (key, None), "W": ("'", "decoy:public_column")})

            for shape, value in (("unicode", first + "\tÉ" + other),
                                 ("combining", first + "\tE\u0301" + other)):
                single(family, "tab_group_" + shape, index, value,
                       "{C}: {V}; public_version={W}", cue, language, "6.1." + other)
            for name in ("equals", "is", "space"):
                template = "{C}: {V}\t" + public_cue + CONNECTORS[name] + "{W}"
                single(family, "tab_group_public_" + name, index, first,
                       template, cue, language, "6.2." + other)
            # A producer knows this last TSV column is public, but the raw
            # scalar alone is ambiguous. Conservative grouping pays its cost.
            single(family, "tab_group_ambiguous_column", index, first + "\tÉ" + other,
                   "{C}: {V}\t{W}", cue, language, word)
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
