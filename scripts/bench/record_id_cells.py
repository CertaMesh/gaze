"""Authored CC0 person-linked CRM/HR identifiers, independent of runtime rules.

There is no universal identifier format: numeric database keys, UUIDs and opaque
application keys all occur. Field ownership determines gold; public resource IDs
and schema/prose placeholders are benign even when their shape is identical.
"""
from types import ModuleType

LABELS = {
    "customer_id": "CUSTOMER_ID",
    "employee_id": "EMPLOYEE_ID",
    "record_id": "RECORD_ID",
}
SHAPES = ("numeric", "uuid", "grouped", "long")
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
PUBLIC_FIELDS = {
    "dev": ("order_id", "invoice_id", "ticket_id", "issue_id", "build_id", "commit_id", "tracking_id", "id", "record_id"),
    "test": ("orderId", "invoiceId", "ticketId", "issueId", "buildId", "commitId", "trackingId", "id", "unique_id"),
}


def value(rng, partition: str, shape: str) -> str:
    # Partition-specific leading digit prevents value overlap between dev/test.
    lead = "6" if partition == "dev" else "8"
    if shape == "numeric":
        return lead + rng.digits(5)
    if shape == "uuid":
        def hexadecimal(n):
            return "".join(rng.choice("0123456789abcdef") for _ in range(n))
        return lead + hexadecimal(7) + "-" + hexadecimal(4) + "-4" + hexadecimal(3) + "-a" + hexadecimal(3) + "-" + hexadecimal(12)
    if shape == "grouped":
        return "SYN-" + lead + rng.digits(4) + "/" + rng.digits(3)
    if shape == "long":
        return "SYN_" + lead + rng.digits(47)
    raise ValueError(shape)


def records(api: ModuleType, partition: str) -> list:
    result = []

    def emit(layer, family, surface, index, template, fields, language="en"):
        text, gold, decoys = api._fill_with_decoys(template, fields)
        result.append(api.Record(
            uid=f"agentic-{partition}-{layer}-{family}-{index:03d}-{surface}",
            partition=partition, layer=layer, family=family, surface=surface,
            validity=api.BENIGN if layer == "D" else api.UNCHECKED,
            group=f"{partition}-recordids-{family}-{index:03d}",
            template=f"recordids/{family}/{surface}/{partition}",
            language=language, region="DE" if language == "de" else "US",
            text=text, gold=gold, decoys=decoys,
        ))

    for family, label in LABELS.items():
        rng = api.Rng(api.PARTITION_SEEDS[partition], "recordids/" + family)
        key, en, de = FIELDS[partition][family]
        for shape in SHAPES:
            for index in range(api.DOCS_PER_FAMILY):
                identifier = value(rng, partition, shape)
                gold = {"V": (identifier, label)}
                emit("A", family, "recordids_json_" + shape, index,
                     '{"tool":"lookup","arguments":{"{K}":"{V}"}}',
                     {**gold, "K": (key, None)})
                emit("A", family, "recordids_en_" + shape, index,
                     "The CRM/HR response lists {C}: {V}.",
                     {**gold, "C": (en, None)})
                emit("A", family, "recordids_de_" + shape, index,
                     "Im CRM/HR-Auszug steht {C}: {V}.",
                     {**gold, "C": (de, None)}, "de")
                # Every occurrence remains gold, including an unlabelled follow-up.
                emit("R", family, "recordids_repeat_" + shape, index,
                     "{C}: {V}\nUse {V} for the next lookup; confirmed {V}.",
                     {**gold, "C": (en, None)})
                public = PUBLIC_FIELDS[partition][index % len(PUBLIC_FIELDS[partition])]
                benign = {"V": (identifier, "decoy:" + family), "K": (public, None)}
                emit("D", family, "recordids_public_json_" + shape, index,
                     '{"tool":"public_resource","arguments":{"{K}":"{V}"}}', benign)
                emit("D", family, "recordids_public_prose_" + shape, index,
                     "Public resource {K}: {V}. Customer ID belongs in a separate field.", benign)
    for index, word in enumerate(("token", "value", "placeholder", "example")):
        for family in LABELS:
            _, en, de = FIELDS[partition][family]
            emit("D", family, "recordids_placeholder_en", index,
                 "[session A {C} {V}]", {"C": (en, None), "V": (word, "decoy:placeholder")})
            emit("D", family, "recordids_placeholder_de", index,
                 "[{C}: {V}]", {"C": (de, None), "V": (word, "decoy:placeholder")}, "de")
    # Pin the actual schema-writing use case as well as the partitioned variants.
    emit("D", "customer_id", "recordids_bridge_placeholder", 0,
         "[session A customer ID {V}]", {"V": ("token", "decoy:placeholder")})
    return result
