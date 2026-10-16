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
