"""CC0 CRM/HR field ownership cells, authored from application semantics.

A labelled field owns its complete scalar, not its prefix or the next field.
No detector patterns, source IDs or recognizer decisions define these spans.
"""
import json
from types import ModuleType


LABELS = {"customer_id": "CUSTOMER_ID", "employee_id": "EMPLOYEE_ID", "record_id": "RECORD_ID"}
FIELDS = {
    "dev": {
        "customer_id": ("customer_id", "customer ID", "Kundennummer"),
        "employee_id": ("employee_id", "personnel number", "Personalnummer"),
        "record_id": ("person_record_id", "person record ID", "Personendatensatz-ID"),
    },
    "test": {
        "customer_id": ("customerId", "customer number", "Kunden-ID"),
        "employee_id": ("employeeId", "employee ID", "Mitarbeiter-ID"),
        "record_id": ("member_id", "member ID", "Mitgliedsnummer"),
    },
}
WORDS = {"dev": ("SABLE", "COBALT", "UMBER"), "test": ("OCHRE", "TOPAZ", "MAROON")}
PUBLIC = {
    "dev": (("order_id", "PACK-"), ("version", "3.2."), ("device_serial", "BOX-")),
    "test": (("invoiceId", "LOT-"), ("VERSION", "4.8."), ("equipmentCode", "RACK-")),
}


def records(api: ModuleType, partition: str) -> list:
    result = []

    def emit(layer, family, surface, index, template, fields, language="en"):
        text, gold, decoys = api._fill_with_decoys(template, fields)
        result.append(api.Record(
            uid=f"agentic-{partition}-{layer}-{family}-{index:03d}-{surface}",
            partition=partition, layer=layer, family=family, surface="recordids_ownership_" + surface,
            validity=api.BENIGN if layer == "D" else api.UNCHECKED,
            group=f"{partition}-recordids-ownership-{family}-{index:03d}",
            template=f"recordids-ownership/{family}/{surface}/{partition}",
            language=language, region="DE" if language == "de" else "US",
            text=text, gold=gold, decoys=decoys,
        ))

    families = tuple(LABELS)
    lead = "6" if partition == "dev" else "8"
    for position, family in enumerate(families):
        label = LABELS[family]
        key, en, de = FIELDS[partition][family]
        rng = api.Rng(api.PARTITION_SEEDS[partition], "recordids-ownership/" + family)
        for index in range(api.DOCS_PER_FAMILY):
            word = rng.choice(WORDS[partition])
            serial = lead + rng.digits(5)
            other = lead + rng.digits(5)
            while other == serial:
                other = lead + rng.digits(5)
            for separator_name, separator in (("space", " "), ("nbsp", "\u00a0")):
                value = word + separator + serial + "Z"
                for language, cue in (("en", en), ("de", de)):
                    fields = {"C": (cue, None), "V": (value, label)}
                    surface = f"grouped_{separator_name}_{language}"
                    emit("A", family, surface, index, "{C}: {V}; status=active", fields, language)
                    emit("R", family, surface + "_repeat", index,
                         "{C}: {V}; status=active\nLookup {V} again.", fields, language)
                    emit("D", family, surface + "_public", index,
                         "inventory_reference: {V}; status=active",
                         {"V": (value, "decoy:public_reference")}, language)

            # Values reflect opaque application scalars; separators/escaping have
            # no semantic power to move bytes outside the labelled identifier.
            decoded_values = {
                "words": word.lower() + " mineral " + serial,
                "unicode": word + " é" + serial,
                "at": word + serial + "@" + word,
                "tab": word + "\t" + serial,
                "quote": word + '"' + serial,
                "backslash": word + "\\" + serial,
                "alphabetic": word.lower() + " mineral",
            }
            for shape, decoded in decoded_values.items():
                raw = json.dumps(decoded, ensure_ascii=False)[1:-1]
                fields = {"K": (key, None), "V": (raw, label)}
                surface = "quoted_" + shape
                template = '{"tool":"lookup","arguments":{"{K}":"{V}"}}'
                emit("A", family, surface, index, template, fields)
                emit("D", family, surface + "_public", index, template,
                     {"K": ("artifact_key", None), "V": (raw, "decoy:public_scalar")})
                if shape != "alphabetic":
                    emit("R", family, surface + "_repeat", index,
                         template + '\nLookup {V} again.', fields)
                # The outer agent payload is another JSON string; gold keeps
                # the exact doubly escaped byte spelling in that surface.
                if shape in ("tab", "quote", "backslash", "unicode"):
                    embedded_raw = json.dumps(raw, ensure_ascii=False)[1:-1]
                    embedded = '{"payload":"{\\"arguments\\":{\\"{K}\\":\\"{V}\\"}}"}'
                    embedded_fields = {"K": (key, None), "V": (embedded_raw, label)}
                    emit("A", family, surface + "_embedded", index, embedded, embedded_fields)
                    emit("R", family, surface + "_embedded_repeat", index,
                         embedded + '\nLookup {V} again.', embedded_fields)
                    emit("D", family, surface + "_embedded_public", index, embedded,
                         {"K": ("artifact_key", None), "V": (embedded_raw, "decoy:public_scalar")})

            for language, cue, quote in (("en", en, '"'), ("de", de, "'")):
                value = word.lower() + " é" + serial
                template = "{C}: " + quote + "{V}" + quote + "; status=active"
                fields = {"C": (cue, None), "V": (value, label)}
                surface = "quoted_log_" + language
                emit("A", family, surface, index, template, fields, language)
                emit("R", family, surface + "_repeat", index,
                     template + "\nLookup {V} again.", fields, language)
                emit("D", family, surface + "_public", index,
                     "artifact_key: " + quote + "{V}" + quote + "; status=active",
                     {"V": (value, "decoy:public_scalar")}, language)

            first = word + serial
            second = rng.choice(WORDS[partition]) + other
            for relation, second_family in (("same", family), ("different", families[(position + 1) % len(families)])):
                second_key = FIELDS[partition][second_family][0]
                second_key = second_key.upper() if index % 2 else second_key
                for boundary, separator in (("space", " "), ("semicolon", "; ")):
                    fields = {"C": (en, None), "V": (first, label), "K": (second_key, None),
                              "W": (second, LABELS[second_family])}
                    template = "{C}: {V}" + separator + "{K}={W}"
                    surface = f"adjacent_personal_{relation}_{boundary}"
                    emit("A", family, surface, index, template, fields)
                    emit("R", family, surface + "_repeat", index, template + "; next lookup {W}", fields)
                    emit("D", family, surface + "_public", index,
                         "artifact_key: {V}" + separator + "inventory_key={W}",
                         {"V": (first, "decoy:public_reference"), "W": (second, "decoy:public_reference")})
            for public_key, prefix in PUBLIC[partition]:
                public_value = prefix + other
                surface = "adjacent_public_" + public_key
                emit("A", family, surface, index, "{C}: {V} {K}={W}",
                     {"C": (en, None), "V": (first, label), "K": (public_key, "decoy:field_label"),
                      "W": (public_value, "decoy:public_reference")})
                emit("D", family, surface + "_placeholder", index, '[schema {C} {P} {K}={W}]',
                     {"C": (en, None), "P": ("placeholder", "decoy:placeholder"),
                      "K": (public_key, "decoy:field_label"), "W": (public_value, "decoy:public_reference")})
            for language, cue in (("en", en), ("de", de)):
                fields = {"C": (cue, None), "P": ("token", "decoy:placeholder"),
                          "K": (key.upper() if index % 2 else key, None), "V": (first, label)}
                surface = "resumption_" + language
                emit("A", family, surface, index, "{C} {P} {K}={V}", fields, language)
                emit("R", family, surface + "_repeat", index, "{C} {P} {K}={V}; lookup {V}", fields, language)
                emit("D", family, surface + "_placeholder", index, '[schema "{C}" {P}]',
                     {"C": (cue, None), "P": ("token", "decoy:placeholder")}, language)
    return result
