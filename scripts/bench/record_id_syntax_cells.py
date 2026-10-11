"""Authored CC0 record-ID scalar syntax, independent of runtime rules.

Numeric JSON scalars can be database identifiers. JSON null and booleans do
not supply an identifier value. Quoting a schema label does not turn the
following prose placeholder into an application key.
"""
from types import ModuleType

LABELS = {"customer_id": "CUSTOMER_ID", "employee_id": "EMPLOYEE_ID", "record_id": "RECORD_ID"}
FIELDS = {
    "dev": {
        "customer_id": ("customer_id", "customer ID", "Kundennummer"),
        "employee_id": ("employee_id", "employee ID", "Personalnummer"),
        "record_id": ("person_record_id", "person record ID", "Personendatensatz-ID"),
    },
    "test": {
        "customer_id": ("customerId", "customer number", "Kunden-ID"),
        "employee_id": ("employeeId", "employee number", "Mitarbeiter-ID"),
        "record_id": ("contact_record_id", "contact record ID", "Kontaktdatensatz-ID"),
    },
}


def records(api: ModuleType, partition: str) -> list:
    result = []

    def emit(layer, family, surface, index, template, fields, language="en"):
        text, gold, decoys = api._fill_with_decoys(template, fields)
        result.append(api.Record(
            uid=f"agentic-{partition}-{layer}-{family}-{index:03d}-{surface}",
            partition=partition, layer=layer, family=family, surface=surface,
            validity=api.BENIGN if layer == "D" else api.UNCHECKED,
            group=f"{partition}-recordids-syntax-{family}-{index:03d}",
            template=f"recordids-syntax/{family}/{surface}/{partition}",
            language=language, region="DE" if language == "de" else "US",
            text=text, gold=gold, decoys=decoys,
        ))

    for family, label in LABELS.items():
        rng = api.Rng(api.PARTITION_SEEDS[partition], "recordids-syntax/" + family)
        key, en, de = FIELDS[partition][family]
        for index in range(api.DOCS_PER_FAMILY):
            identifier = ("6" if partition == "dev" else "8") + rng.digits(5)
            fields = {"K": (key, None), "V": (identifier, label), "T": (str(index + 1), None)}
            emit("A", family, "recordids_syntax_json_numeric", index,
                 '{"tool":"lookup","arguments":{"{K}":{V}},"turn":{T}}', fields)
            emit("R", family, "recordids_syntax_json_numeric_repeat", index,
                 '{"{K}":{V}}\nUse {V} next; confirmed {V}.', fields)
            for keyword in ("null", "true", "false"):
                emit("D", family, "recordids_syntax_json_" + keyword, index,
                     '{"tool":"lookup","arguments":{"{K}":{V}},"turn":{T}}',
                     {"K": (key, None), "V": (keyword, "decoy:json_keyword"), "T": (str(index + 1), None)})
        for index, word in enumerate(("token", "value", "placeholder", "example")):
            for language, cue in (("en", en), ("de", de)):
                emit("D", family, "recordids_syntax_quoted_label_" + language, index,
                     '[schema "{C}" {V}]', {"C": (cue, None), "V": (word, "decoy:placeholder")}, language)
    emit("D", "customer_id", "recordids_syntax_bridge_quoted_label", 0,
         '[session A "customer ID" {V}]', {"V": ("token", "decoy:placeholder")})
    return result
