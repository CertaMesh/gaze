"""Structured-input slice: person-linked IDs inside the payloads agents send.

Agents mostly send JSON (tool calls and results for Anthropic, OpenAI and
Gemini, API request bodies), plus CSV and NDJSON. The person-linked value
sits under a labelled field next to values that must stay readable:
unlabelled numbers, amounts, timestamps and public IDs. Each of those is a
recorded decoy, so a reader can count false positives per value as well as
per byte, and check that the payload still parses (`structured_slice.py`).

Layer A carries the labelled positives. Layer D carries the same shapes with
a public key in place of the person label, so every inserted value is a
decoy. Gold is the raw document bytes of the value, including inside
JSON-escaped strings. Every value is synthetic; emails use reserved domains.
"""
from types import ModuleType

import record_id_cells as record_ids

SURFACE_PREFIX = "structured_"
SHAPES = (
    "anthropic_tool_use",
    "anthropic_tool_result",
    "openai_tool_call",
    "gemini_function_call",
    "api_body",
    "nested_array",
    "csv",
    "ndjson",
    "numeric_label",
)
VALUE_SHAPES = ("numeric", "uuid", "grouped")
BENIGN_FAMILY = "structured_benign"
DECOY_KINDS = ("unlabelled_number", "amount", "timestamp", "benign_id")
KEYS = {
    "dev": {
        "customer_id": ("customer_id", "customerNumber"),
        "employee_id": ("employee_id", "staffNumber"),
        "record_id": ("person_record_id", "contactId"),
    },
    "test": {
        "customer_id": ("customerId", "customer_number"),
        "employee_id": ("employeeId", "personnel_number"),
        "record_id": ("contact_record_id", "memberId"),
    },
}
PUBLIC_KEYS = {
    "dev": ("order_id", "request_id", "invoice_id"),
    "test": ("orderId", "requestId", "invoiceNo"),
}
TOOLS = {
    "dev": ("crm_lookup", "fetch_profile", "account_summary"),
    "test": ("lookup_record", "get_account", "billing_history"),
}
EMAIL_DOMAINS = {"dev": ("example.org", "mail.test"), "test": ("example.com", "example.net")}

# Positive payloads; `{P}` is the person block, which layer D replaces.
TEMPLATES = {
    "anthropic_tool_use":
        '{"type":"tool_use","id":"{B}","name":"{F}","input":{"{K}":"{V}","limit":{N1},"page":{N2}}}',
    "anthropic_tool_result":
        '{"type":"tool_result","tool_use_id":"{B}","content":"{\\"{K}\\":\\"{V}\\",'
        '\\"balance\\":{A},\\"updated_at\\":\\"{T}\\"}"}',
    "openai_tool_call":
        '{"tool_calls":[{"id":"{B}","type":"function","function":{"name":"{F}",'
        '"arguments":"{\\"{K}\\":\\"{V}\\",\\"days\\":{N1}}"}}]}',
    "gemini_function_call":
        '{"functionCall":{"name":"{F}","args":{"{K}":"{V}","maxResults":{N1}}}}',
    "api_body":
        '{"{H}":{"{K}":"{V}",{P}},"items":[{"sku":"{B}","qty":{N1},"price":{A}}],"created_at":"{T}"}',
    "nested_array":
        '{"records":[{"{K}":"{V}","score":{N1}},{"{K}":"{V2}","score":{N2}}],"total":{N3}}',
    "csv":
        "{K},amount,created_at\n{V},{A},{T}\n{V2},{A2},{T2}\n",
    "ndjson":
        '{"event":"login","{K}":"{V}","duration_ms":{N1}}\n'
        '{"event":"search","{K}":"{V}","duration_ms":{N2}}\n'
        '{"event":"logout","{K}":"{V}","duration_ms":{N3}}\n',
    "numeric_label":
        '{"{K}":{V},"limit":{N1},"offset":{N2}}',
}
PERSON_BLOCK = '"name":{"given":"{G}","family":"{S}"},"email":"{E}"'
BENIGN_BLOCK = '"region":"europe-west","tier":"standard"'


def _hex(rng, count: int) -> str:
    return "".join(rng.choice("0123456789abcdef") for _ in range(count))


def _timestamp(rng) -> str:
    return (f"2026-{rng.between(1, 12):02d}-{rng.between(1, 28):02d}"
            f"T{rng.between(0, 23):02d}:{rng.between(0, 59):02d}:{rng.between(0, 59):02d}Z")


def _amount(rng) -> str:
    return f"{rng.between(1, 4999)}.{rng.digits(2)}"


def _benign_id(rng, shape: str) -> str:
    if shape.startswith("anthropic"):
        return "toolu_" + _hex(rng, 20)
    if shape == "openai_tool_call":
        return "call_" + _hex(rng, 20)
    return "SKU-" + rng.digits(6)


def _decoys(api: ModuleType, rng, shape: str) -> dict:
    decoy = api.DECOY_PREFIX
    return {
        "B": (_benign_id(rng, shape), decoy + "benign_id"),
        # Six digits, the numeric ID's own shape: a bare number must pass (D3).
        "N1": (str(rng.between(1, 999999)), decoy + "unlabelled_number"),
        "N2": (str(rng.between(1, 9999)), decoy + "unlabelled_number"),
        "N3": (str(rng.between(2, 99)), decoy + "unlabelled_number"),
        "A": (_amount(rng), decoy + "amount"),
        "A2": (_amount(rng), decoy + "amount"),
        "T": (_timestamp(rng), decoy + "timestamp"),
        "T2": (_timestamp(rng), decoy + "timestamp"),
    }


def records(api: ModuleType, partition: str) -> list:
    result = []

    def emit(layer, family, shape, index, fields, block):
        template = TEMPLATES[shape].replace("{P}", block)
        text, gold, decoys = api._fill_with_decoys(template, fields)
        surface = SURFACE_PREFIX + shape
        result.append(api.Record(
            uid=f"agentic-{partition}-{layer}-{family}-{index:03d}-{surface}",
            partition=partition, layer=layer, family=family, surface=surface,
            validity=api.BENIGN if layer == "D" else api.UNCHECKED,
            group=f"{partition}-structured-{family}-{index:03d}",
            template=f"structured/{shape}/{partition}",
            language="en", region="US", text=text, gold=gold, decoys=decoys,
        ))

    def identifier(rng, shape, index):
        value_shape = "numeric" if shape == "numeric_label" else VALUE_SHAPES[index % len(VALUE_SHAPES)]
        return record_ids.value(rng, partition, value_shape)

    for family, label in record_ids.LABELS.items():
        for shape in SHAPES:
            rng = api.Rng(api.PARTITION_SEEDS[partition], f"structured/{family}/{shape}")
            for index in range(api.DOCS_PER_FAMILY):
                given, surname = api._person(rng, partition)
                local = api._ascii_fold(f"{given}.{surname}").lower()
                fields = {
                    **_decoys(api, rng, shape),
                    "K": (rng.choice(KEYS[partition][family]), None),
                    "F": (rng.choice(TOOLS[partition]), None),
                    "H": ("customer", None),
                    "V": (identifier(rng, shape, index), label),
                    "V2": (identifier(rng, shape, index), label),
                    "G": (given, "GIVENNAME"),
                    "S": (surname, "SURNAME"),
                    "E": (f"{local}@{rng.choice(EMAIL_DOMAINS[partition])}", "EMAIL"),
                }
                emit("A", family, shape, index, fields, PERSON_BLOCK)

    for shape in SHAPES:
        rng = api.Rng(api.PARTITION_SEEDS[partition], f"structured/benign/{shape}")
        for index in range(api.DOCS_PER_FAMILY):
            benign = api.DECOY_PREFIX + "benign_id"
            fields = {
                **_decoys(api, rng, shape),
                "K": (rng.choice(PUBLIC_KEYS[partition]), None),
                "F": (rng.choice(TOOLS[partition]), None),
                "H": ("merchant", None),
                "V": (identifier(rng, shape, index), benign),
                "V2": (identifier(rng, shape, index), benign),
            }
            emit("D", BENIGN_FAMILY, shape, index, fields, BENIGN_BLOCK)
    return result
