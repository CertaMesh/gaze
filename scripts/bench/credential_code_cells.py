"""CC0 synthetic password encodings and literal credential fields.

Shapes follow libxcrypt crypt(5), PHC Argon2, Passlib scrypt and Django
hashers. Gold is the producer's complete stored credential, never detector output.
"""


def hashes(rng):
    def chars(size, alphabet="./ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"):
        return "".join(alphabet[rng.below(len(alphabet))] for _ in range(size))

    b64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
    salt, digest = chars(16, b64), chars(43, b64)
    bcrypt = "$2b$12$" + chars(53)
    argon = "$argon2id$v=19$m=65536,t=3,p=2$" + salt + "$" + digest
    return {
        "bcrypt": bcrypt,
        "argon2id": argon,
        "argon2i": argon.replace("argon2id", "argon2i"),
        "argon2d": argon.replace("argon2id", "argon2d"),
        "scrypt_phc": "$scrypt$ln=16,r=8,p=1$" + salt + "$" + chars(43, b64),
        "scrypt_crypt": "$7$" + chars(33) + "$" + chars(43),
        "yescrypt": "$y$j9T$" + chars(22) + "$" + chars(43),
        "sha256crypt": "$5$" + chars(12) + "$" + chars(43),
        "sha512crypt": "$6$rounds=10000$" + chars(12) + "$" + chars(86),
        "md5crypt": "$1$" + chars(8) + "$" + chars(22),
        "phpass_p": "$P$" + chars(31),
        "phpass_h": "$H$" + chars(31),
        "django_pbkdf2_sha256": "pbkdf2_sha256$600000$salt" + chars(8, b64[:62]) + "$" + chars(43, b64) + "=",
        "django_pbkdf2_sha1": "pbkdf2_sha1$600000$salt" + chars(8, b64[:62]) + "$" + chars(27, b64) + "=",
        "django_argon2": "argon2" + argon,
        "django_bcrypt_sha256": "bcrypt_sha256$" + bcrypt,
        "django_scrypt": "scrypt$16384$salt" + chars(8, b64[:62]) + "$8$5$" + chars(86, b64) + "==",
    }


TEMPLATES = {
    "dev": {"bare": "Stored digest: {V}", "log": "stored_digest={V}", "php": "['digest' => '{V}'],",
            "json": '{"stored_digest":"{V}"}', "sql": "INSERT INTO accounts (password) VALUES ('{V}');"},
    "test": {"bare": "Account encoding = {V}", "log": "encoded_value={V}", "php": "array('encoded' => '{V}');",
             "json": '{"result":{"encoding":"{V}"}}', "sql": "INSERT INTO members (password, enabled) VALUES ('{V}', true);"},
}


def records(api, partition):
    result = []

    def emit(layer, family, surface, index, template, value, label):
        text, gold, decoys = api._fill_with_decoys(template, {"V": (value, None if layer == "D" else label)})
        result.append(api.Record(
            uid=f"agentic-{partition}-{layer}-credentials-{family}-{surface}-{index}",
            partition=partition, layer=layer, family=family, surface="credentials_" + surface,
            validity=api.BENIGN if layer == "D" else api.UNCHECKED,
            group=f"{partition}-credentials-{family}-{index}",
            template=f"credentials/{partition}/{surface}", language="en", region="US",
            text=text, gold=gold, decoys=decoys,
        ))

    rng = api.Rng(api.PARTITION_SEEDS[partition], "credential-code")
    for index in range(api.DOCS_PER_FAMILY):
        for family, value in hashes(rng).items():
            for surface, template in TEMPLATES[partition].items():
                emit("A", family, "hash_" + surface, index, template, value, "PASSWORD")
                emit("R", family, "hash_" + surface, index, template + "\nCompare {V} again.", value, "PASSWORD")
            # Variable-length PHC encodings are incomplete only when a component
            # is missing. Fixed-width encodings lose one checksum character.
            incomplete = value.rsplit("$", 1)[0] if "argon2" in value else value[:-1]
            emit("D", family, "hash_truncated", index, "sample='{V}'", incomplete, "decoy:incomplete_hash")
            emit("D", family, "hash_glued", index, "sample='x{V}_tail'", value, "decoy:hash_fragment")
        for key, label in (("password", "PASSWORD"), ("passphrase", "PASSWORD"),
                           ("api_token", "SECURITYTOKEN"), ("remember_token", "SECURITYTOKEN")):
            value = ("dev_" if partition == "dev" else "test_") + "synthetic!" + rng.digits(6)
            for surface, template in (("php", "['" + key + "' => '{V}'],"),
                                      ("json", '{"' + key + '":"{V}","enabled":true}'),
                                      ("field", key + ': "{V}",')):
                emit("A", key, "code_" + surface, index, template, value, label)
                emit("R", key, "code_" + surface, index, template + "\nCompare {V} again.", value, label)
                unrelated = template.replace(key, "build_reference")
                emit("D", key, "code_" + surface, index, unrelated, value, "decoy:public_scalar")
        for family, value in (("prices", "It costs $2 or $10 per month."),
                              ("short_bcrypt", "$2y$10$short"),
                              ("short_argon", "$argon2id$v=19$m=65536"),
                              ("expression", "'password' => Hash::make('secret'),")):
            emit("D", family, "code_negative", index, "{V}", value, "decoy:code_syntax")
    return result



# Assign wording, syntax and value pools before drawing any values. These are
# rendered contexts, not partition-labelled IDs over otherwise shared inputs.
REVIEW_TEMPLATES = {
    "dev": {
        "repeat": "\nCompare {V} again.",
        "hash_truncated": "Example of an incomplete encoding: {V}",
        "hash_invalid": "Malformed encoding example, not a stored credential: {V}",
        "array_compact": "$seed = array('{K}'=>'{V}');",
        "array_first": "$seed = array('{K}'=>'{V}','enabled'=>true);",
        "array_final": "$seed = array('enabled'=>true,'{K}'=>'{V}');",
        "array_nested": "$seed = array('outer'=>array('{K}'=>'{V}'));",
        "password": "Credential export for a synthetic account; the actual stored password is array('password'=>'{V}');",
        "generic_token": 'Synthetic account credential export: {"token":"{V}"}',
        "logprob": 'Model likelihood output, not authentication data: {"content":[{"token":"{V}","logprob":-0.1}]}',
        "tokenizer": 'Tokenizer vocabulary entry, not an access credential: {"token":"{V}","id":42}',
        "token_schema": 'OpenAPI example for tokenizer output, no credentials: {"type":"object","example":{"token":"{V}"}}',
        "password_schema": 'OpenAPI schema example, not an account password: {"type":"object","example":{"password":"{V}"}}',
        "validation": "Laravel validation rules, not stored account data: $request->validate(['password'=>'{V}']);",
        "language": "Laravel translation file, no account data: return ['password'=>'{V}'];",
        "masked": 'A public UI response with its password already masked: {"password":"{V}","masked":true}',
    },
    "test": {
        "repeat": "\nThe subsequent credential lookup returns {V}.",
        "hash_truncated": "The import manual illustrates a broken digest with `encoded={V}`.",
        "hash_invalid": "Parser rejection fixture; this malformed string is no account credential: [{V}]",
        "array_compact": 'return array("{K}"=>"{V}");',
        "array_first": 'return array("{K}" => "{V}", "role" => "reader");',
        "array_final": 'return array("role" => "reader", "{K}" => "{V}");',
        "array_nested": 'return array("credentials" => array("{K}" => "{V}"));',
        "password": 'Provisioning output for a fictional login; this is the real stored credential: {"credentials":{"password":"{V}"}}',
        "generic_token": 'A fictional service account stores its access credential in {"connection":{"token":"{V}"}}.',
        "logprob": 'Generation diagnostics report word probabilities: {"generation":{"logprobs":[{"logprob":-0.75,"token":"{V}"}]}}',
        "tokenizer": 'BPE configuration for a text processor, with no access secrets: {"vocabulary":{"entry":{"index":73,"token":"{V}"}}}',
        "token_schema": 'API documentation illustrates a lexical item, not a credential: {"examples":{"lexeme":{"value":{"token":"{V}"}}}}',
        "password_schema": 'The schema manual uses this public dummy request, not stored login data: {"examples":{"request":{"value":{"password":"{V}"}}}}',
        "validation": 'Validation configuration checks user input: $rules = Validator::make($input, array("password" => "{V}"));',
        "language": 'Authentication message catalogue for display to visitors: return array("messages" => array("password" => "{V}"));',
        "masked": 'The account screen exports only an obscured display value: {"profile":{"password":"{V}"},"display":{"obscured":true}}',
    },
}

REVIEW_VALUES = {
    "dev": {
        "array_prefix": ("seed_literal!", "local_fixture!"),
        "passphrase_prefix": ("cedar branch under snow ", "quiet field beside pines "),
        "pipe_prefix": ("seed|synthetic|secret|", "local|fixture|access|"),
        "token_prefix": ("seed_opaque!", "local_access!"),
        "schema": ("string", "text"),
        "stars": (8, 9, 10),
        "logprob": ("Hello", "nearby", "green"),
        "tokenizer": ("indivisible", "unshipped", "merging"),
        "validation": ("required|string|min:8|confirmed", "required|string|min:10|confirmed"),
        "language": ("The provided password is incorrect.", "This password does not match our records."),
    },
    "test": {
        "array_prefix": ("handoff_literal-", "provisioned_fixture-"),
        "passphrase_prefix": ("harbor lantern after sunset ", "paper boat across water "),
        "pipe_prefix": ("handoff|private|phrase|", "provision|account|value|"),
        "token_prefix": ("handoff_opaque-", "provision_access-"),
        "schema": ("example", "sample"),
        "stars": (12, 13, 14),
        "logprob": ("Goodbye", "beyond", "amber"),
        "tokenizer": ("counterfactual", "segmentable", "encoding"),
        "validation": ("required|min:12|string|confirmed", "required|string|max:72|confirmed"),
        "language": ("The supplied password cannot be accepted.", "That password did not pass authentication."),
    },
}


def review_records(api, partition):
    """Independent v22 contexts; leave all v21 documents and random draws intact."""
    result = []
    templates = REVIEW_TEMPLATES[partition]
    pools = REVIEW_VALUES[partition]

    def emit(layer, family, surface, index, template, value, label):
        text, gold, decoys = api._fill_with_decoys(template, {"V": (value, None if layer == "D" else label)})
        result.append(api.Record(
            uid=f"agentic-{partition}-{layer}-credentials-review-{family}-{surface}-{index}",
            partition=partition, layer=layer, family=family, surface="credentials_review_" + surface,
            validity=api.BENIGN if layer == "D" else api.UNCHECKED,
            group=f"{partition}-credentials-review-{family}-{index}",
            template=f"credentials-review/{partition}/{surface}", language="en", region="US",
            text=text, gold=gold, decoys=decoys,
        ))

    rng = api.Rng(api.PARTITION_SEEDS[partition], "credential-code-review")

    def pick(kind):
        pool = pools[kind]
        return pool[rng.below(len(pool))]

    for index in range(api.DOCS_PER_FAMILY):
        for family, value in hashes(rng).items():
            if family not in ("sha256crypt", "sha512crypt", "md5crypt",
                              "django_pbkdf2_sha256", "django_pbkdf2_sha1", "django_scrypt"):
                continue
            # Alter only salt components; checksum alphabets/widths stay fixed.
            parts = value.split("$")
            slot = 3 if family == "sha512crypt" else 2
            for mark in ("-", "_") if "django" not in family else ("+", "@", " ", ":"):
                salt = ("dv" if partition == "dev" else "ts") + mark + rng.digits(4)
                parts[slot] = salt
                encoded = "$".join(parts)
                for surface, template in TEMPLATES[partition].items():
                    emit("A", family, "salt_" + surface + "_" + str(ord(mark)), index,
                         template, encoded, "PASSWORD")
                    emit("R", family, "salt_" + surface + "_" + str(ord(mark)), index,
                         template + templates["repeat"], encoded, "PASSWORD")
                emit("D", family, "salt_truncated_" + str(ord(mark)), index,
                     templates["hash_truncated"], encoded[:-1], "decoy:incomplete_hash")
                invalid_parts = parts.copy()
                invalid_parts[slot] = "" if "django" in family else "bad:salt"
                emit("D", family, "salt_invalid_" + str(ord(mark)), index,
                     templates["hash_invalid"],
                     "$".join(invalid_parts), "decoy:malformed_hash")
        for key, label in (("password", "PASSWORD"), ("api_token", "SECURITYTOKEN"),
                           ("remember_token", "SECURITYTOKEN")):
            value = pick("array_prefix") + rng.digits(6)
            for surface in ("array_compact", "array_first", "array_final", "array_nested"):
                template = templates[surface].replace("{K}", key)
                emit("A", key, surface, index, template, value, label)
                emit("R", key, surface, index, template + templates["repeat"], value, label)
        weak_string, weak_stars = pick("schema"), "*" * pick("stars")
        for family, value in (
            ("spaced_passphrase", pick("passphrase_prefix") + rng.digits(6)),
            ("pipe_password", pick("pipe_prefix") + rng.digits(6)),
            ("weak_string", weak_string),
            ("weak_stars", weak_stars),
        ):
            template = templates["password"]
            emit("A", family, "password_" + family, index, template, value, "PASSWORD")
            emit("R", family, "password_" + family, index, template + templates["repeat"], value, "PASSWORD")
        # These are genuine credentials under an ambiguous generic key. Keeping
        # them gold makes the deliberate generic-token recall cost measurable.
        value = pick("token_prefix") + rng.digits(8)
        emit("A", "generic_token", "generic_token", index,
             templates["generic_token"], value, "SECURITYTOKEN")
        emit("R", "generic_token", "generic_token", index,
             templates["generic_token"] + templates["repeat"], value, "SECURITYTOKEN")
        for family, value in (
            ("logprob", pick("logprob")), ("tokenizer", pick("tokenizer")),
            ("token_schema", weak_string), ("password_schema", weak_string),
            ("validation", pick("validation")), ("language", pick("language")),
            ("masked", weak_stars),
        ):
            emit("D", family, "metadata_" + family, index,
                 templates[family], value, "decoy:credential_metadata")
    return result



def source_escape_records(api, partition):
    """v23 source-unit boundaries, with independent salt/digest draws per split."""
    result = []
    rng = api.Rng(api.PARTITION_SEEDS[partition], "credential-source-escapes")
    templates = TEMPLATES[partition]
    review = REVIEW_TEMPLATES[partition]

    def emit(layer, family, surface, index, value):
        template = (review["hash_invalid"] if layer == "D" else
                    templates["bare" if surface == "salt_raw" else "sql" if surface.startswith("salt_") else "json"])
        if layer == "R":
            template += review["repeat"]
        text, gold, decoys = api._fill_with_decoys(
            template, {"V": (value, None if layer == "D" else "PASSWORD")})
        result.append(api.Record(
            uid=f"agentic-{partition}-{layer}-credentials-escapes-{family}-{surface}-{index}",
            partition=partition, layer=layer, family=family,
            surface="credentials_escapes_" + surface,
            validity=api.BENIGN if layer == "D" else api.UNCHECKED,
            group=f"{partition}-credentials-escapes-{family}-{index}",
            template=f"credentials-escapes/{partition}/{surface}", language="en", region="US",
            text=text, gold=gold, decoys=decoys,
        ))

    for index in range(api.DOCS_PER_FAMILY):
        values = hashes(rng)
        for family, kind, limit in (("md5crypt", "1", 8), ("sha256crypt", "5", 16),
                                     ("sha512crypt", "6", 16)):
            digest = values[family].rsplit("$", 1)[1]
            salt = ("dv" if partition == "dev" else "ts") + rng.digits(limit - 3) + "'"
            for surface, escape in (("salt_raw", "'"), ("salt_sql", "''"), ("salt_mysql", r"\'")):
                encoded_salt = salt.replace("'", escape)
                value = f"${kind}${encoded_salt}${digest}"
                for layer in ("A", "R"):
                    emit(layer, family, surface, index, value)
                emit("D", family, surface + "_overlong", index, f"${kind}$a{encoded_salt}${digest}")
                emit("D", family, surface + "_truncated", index, value[:-1])
                emit("D", family, surface + "_checksum_overlong", index, value + "A")
        for family in ("bcrypt", "argon2id", "scrypt_phc", "scrypt_crypt", "yescrypt",
                       "phpass_p", "django_pbkdf2_sha256", "django_pbkdf2_sha1", "django_scrypt"):
            # Force an escaped alphabet member in both fixed and variable fields;
            # the rest of each value remains independently generated.
            parts = values[family].split("$")
            for slot in (-1, -2) if family in ("argon2id", "scrypt_phc", "yescrypt") else (-1,):
                component = parts[slot]
                parts[slot] = component[:4] + "/" + component[5:]
            value = "$".join(parts).replace("/", r"\/")
            for layer in ("A", "R"):
                emit(layer, family, "json_slash", index, value)
            for surface, broken in (("escape_tail", value + r"\q"),
                                     ("glued", value + "_tail"),
                                     ("truncated", value.rsplit("$", 1)[0] if family == "argon2id" else value[:-1])):
                emit("D", family, "json_" + surface, index, broken)
            if family != "argon2id":
                emit("D", family, "json_overlong", index, value.replace("=", "") + "A" + ("==" if family == "django_scrypt" else "=" if "pbkdf2" in family else ""))
    return result
