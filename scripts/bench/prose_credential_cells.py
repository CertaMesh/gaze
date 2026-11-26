"""CC0 prose credential disclosures and base64-encoded private keys, generator v31.

Passwords, security codes and tokens are invented per partition and cannot
authenticate anywhere; key bodies are random filler inside real armour lines.
Layer A carries EN/DE chat, email and tool-call prose that names a credential
and gives its value, including phrasings a cue grammar does not reach, so the
residual stays measured. Layer R repeats each value bare. Layer D carries the
same cue words with no credential: password rules and lengths, credential
names, hints, rotation schedules, card verification digits, tokenizer names,
and base64-encoded public certificates and keys.
"""

import base64
import json

SURFACE_PREFIX = "prose_credentials_"

TEMPLATES = {
    "dev": {
        "password_en": [
            "Heads-up: I switched my password to {V} last night, keep it to yourself.",
            "The password for the build dashboard is {V}.",
            "My new passphrase, for now, is \"{V}\".",
            "Reminder that the password we agreed on for the team inbox is '{V}'.",
            '{"name":"post_reply","input":{"body":"The password is {V}, please rotate it."}}',
            "I think I typed {V} as my password, can you check?",
        ],
        "password_de": [
            "Zur Info: Mein Passwort für das Mitgliederkonto lautet {V}.",
            "Das neue Kennwort ist „{V}“, bitte beim ersten Login ändern.",
            "Ich habe das Router-Passwort auf {V} geändert.",
            '{"role":"user","content":"Mein Passwort ist {V}"}',
            "Als Passwort habe ich {V} genommen.",
        ],
        "token_en": [
            "Your security code is {V}; it is valid for ten minutes.",
            "Please pass the access token {V} with the next request.",
            "The authentication code, {V}, was texted to you.",
            "Here is the session key {V} for the support call.",
        ],
        "token_de": [
            "Ihr Sicherheitstoken lautet {V}.",
            "Der Zugangscode für das Mieterportal ist {V}.",
            "Hier der Einmal-Sicherheitscode: {V}",
        ],
        "benign_en": [
            ("Your password must be at least {D} characters long.", "digits"),
            ("Password policy {D} now requires one symbol.", "reference"),
            ("The password reset ticket {D} was closed yesterday.", "reference"),
            ("API key name: {D}", "slug"),
            ("Security token rotation runs every {D} days.", "digits"),
            ("The tokenizer {D} splits this prompt into tokens.", "tokenizer"),
            ("Enter the 3-digit security code {D} from the back of the card.", "cvc"),
            ("Verification code template {D} is used for sign-in mails.", "slug"),
        ],
        "benign_de": [
            ("Das Passwort muss mindestens {D} Zeichen haben.", "digits"),
            ("Passwort vergessen? Den Link finden Sie unter {D}.", "url"),
            ("Der Passwort-Hinweis für Konto {D} wurde entfernt.", "reference"),
            ("Sicherheitscode (CVC) auf der Kartenrückseite: {D}", "cvc"),
            ("Zugangscode-Länge: {D} Stellen", "digits"),
        ],
    },
    "test": {
        "password_en": [
            "Quick update: I changed my password to {V} this morning, please don't share it.",
            "The password for the staging portal is {V}.",
            "Hi team, my new passphrase is \"{V}\" until Friday.",
            "For the shared mailbox, the password, as agreed, is '{V}'.",
            '{"tool":"send_message","arguments":{"text":"Your password is {V}, change it after login."}}',
            "I may have entered {V} as the password, could you look?",
        ],
        "password_de": [
            "Kurze Info: Mein Passwort für das Kundenkonto lautet {V}.",
            "Das neue Kennwort lautet „{V}“, bitte nach der Anmeldung ändern.",
            "Ich habe das WLAN-Passwort auf {V} gesetzt.",
            '{"role":"user","content":"Das Passwort lautet {V}"}',
            "Als Passwort habe ich {V} gewählt.",
        ],
        "token_en": [
            "Your security token is {V}; it expires in one hour.",
            "Use the access code {V} for the next API call.",
            "The verification code, {V}, was sent to the device.",
            "Here is the API key {V} for the integration test.",
        ],
        "token_de": [
            "Ihr Sicherheitscode lautet {V}.",
            "Der Zugangscode für das Bewerberportal ist {V}.",
            "Hier der Einmal-Sicherheitstoken: {V}",
        ],
        "benign_en": [
            ("Passwords need {D} characters and a digit.", "digits"),
            ("The password policy {D} applies to every account.", "reference"),
            ("Password reset request {D} has been resolved.", "reference"),
            ("Access token label: {D}", "slug"),
            ("Session token rotation happens every {D} hours.", "digits"),
            ("We counted tokens with {D} before sending the prompt.", "tokenizer"),
            ("Type the security code {D} printed next to the signature strip.", "cvc"),
            ("The API key hint {D} is shown in the console.", "slug"),
        ],
        "benign_de": [
            ("Ein Passwort braucht mindestens {D} Zeichen.", "digits"),
            ("Passwort zurücksetzen über {D}.", "url"),
            ("Die Passwort-Richtlinie {D} gilt ab sofort.", "reference"),
            ("Der Sicherheitscode auf der Karte lautet {D}.", "cvc"),
            ("Sicherheitstoken-Laufzeit: {D} Minuten", "digits"),
        ],
    },
}

STEMS = {
    "dev": ["Harbor", "Pinecone", "Saffron", "Tundra", "Walnut", "Fjell", "Lupine", "Mosaik"],
    "test": ["Lantern", "Quarry", "Thistle", "Marigold", "Juniper", "Kranich", "Bernstein", "Holunder"],
}
SYMBOLS = "!#$%&*+?@"
UPPER_DIGITS = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789"
BASE64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
PRIVATE_LABELS = ["PRIVATE KEY", "RSA PRIVATE KEY", "EC PRIVATE KEY", "OPENSSH PRIVATE KEY"]
PUBLIC_LABELS = ["CERTIFICATE", "PUBLIC KEY"]


def records(api, partition):
    rng = api.Rng(api.PARTITION_SEEDS[partition], "prose-credentials-v31")
    templates = TEMPLATES[partition]
    result = []

    def emit(layer, family, surface, index, template, fields, language):
        text, gold, decoys = api._fill_with_decoys(template, fields)
        result.append(api.Record(
            uid=f"agentic-{partition}-{layer}-prosecred-{family}-{surface}-{index}",
            partition=partition, layer=layer, family=family, surface=SURFACE_PREFIX + surface,
            validity=api.BENIGN if layer == "D" else api.UNCHECKED,
            group=f"{partition}-prosecred-{family}-{index}",
            template=f"prosecred/{partition}/{surface}", language=language,
            region="DE" if language == "de" else "US", text=text, gold=gold, decoys=decoys))

    def chars(size, alphabet):
        return "".join(alphabet[rng.below(len(alphabet))] for _ in range(size))

    def password():
        value = rng.choice(STEMS[partition]) + rng.digits(rng.between(2, 4)) + rng.choice(SYMBOLS)
        return value if rng.below(2) else rng.choice(SYMBOLS) + value

    def token():
        groups = [chars(rng.between(4, 6), UPPER_DIGITS) for _ in range(rng.between(1, 3))]
        value = "-".join(groups)
        while len(value) < 8 or not any(c.isdigit() for c in value) or not any(c.isalpha() for c in value):
            value += chars(1, UPPER_DIGITS)
        return value

    def decoy(kind):
        if kind == "digits":
            return str(rng.between(8, 24))
        if kind == "reference":
            return f"{'PWD' if partition == 'test' else 'SEC'}-{rng.digits(4)}"
        if kind == "slug":
            return f"{rng.choice(['ci', 'backend', 'billing', 'mailer'])}-{rng.choice(['deploy', 'sync', 'export'])}-{rng.digits(2)}"
        if kind == "tokenizer":
            return rng.choice(["cl100k_base", "o200k_base", "p50k_base"])
        if kind == "cvc":
            return rng.digits(3)
        return f"https://example.invalid/reset/{rng.digits(4)}"

    def pem(label):
        body = "\n".join(chars(64, BASE64) for _ in range(rng.between(2, 4)))
        return f"-----BEGIN {label}-----\n{body}\n-----END {label}-----\n"

    def b64(text):
        return base64.b64encode(text.encode()).decode()

    for index in range(api.DOCS_PER_FAMILY):
        for language in ("en", "de"):
            for family, mint in (("password", password), ("token", token)):
                label = "PASSWORD" if family == "password" else "SECURITYTOKEN"
                for surface, template in enumerate(templates[f"{family}_{language}"]):
                    value = mint()
                    emit("A", f"{family}_{language}", f"{family}_{surface}", index, template,
                         {"V": (value, label)}, language)
                    emit("R", f"{family}_{language}", f"{family}_repeat_{surface}", index,
                         template + ("\nAgain: {V}" if language == "en" else "\nNochmal: {V}"),
                         {"V": (value, label)}, language)
            for surface, (template, kind) in enumerate(templates[f"benign_{language}"]):
                emit("D", f"benign_{language}", f"benign_{surface}", index, template,
                     {"D": (decoy(kind), None)}, language)
        # Base64 private keys: whole files, files inside encoded JSON at the other two byte
        # alignments, and line-wrapped `base64` output.
        label = PRIVATE_LABELS[index % len(PRIVATE_LABELS)]
        private = pem(label)
        public = pem(PUBLIC_LABELS[index % len(PUBLIC_LABELS)])
        whole = b64(private)
        aligned_one = b64(json.dumps({"private_key": private}))
        aligned_two = b64(json.dumps({"privkey": private}))
        wrapped = "\n".join(whole[start:start + 76] for start in range(0, len(whole), 76))
        secret = ("apiVersion: v1\nkind: Secret\ntype: kubernetes.io/tls\ndata:\n  tls.crt: {C}\n"
                  "  tls.key: {V}\n" if partition == "test" else
                  "kind: Secret\ndata:\n  tls.key: {V}\n  tls.crt: {C}\n")
        cases = [
            ("secret_yaml", secret, whole),
            ("env", "SIGNING_KEY_B64={V}\n" if partition == "test" else "export TLS_KEY_BASE64={V}\n", whole),
            ("json_alignment_1", '{"config":"{V}"}' if partition == "test" else '{"blob":"{V}"}', aligned_one),
            ("json_alignment_2", '{"payload":"{V}"}' if partition == "test" else '{"data":"{V}"}', aligned_two),
            ("wrapped", "$ base64 server.key\n{V}\n$ echo done\n" if partition == "test"
             else "$ base64 id.pem\n{V}\n", wrapped),
        ]
        for surface, template, value in cases:
            # Layer A and D records carry no decoys: every protected D byte, and every
            # protected non-gold A byte, already counts as a false positive.
            fields = {"V": (value, "SECURITYTOKEN"), "C": (b64(public), None)}
            emit("A", "pem_base64", surface, index, template, fields, "en")
        emit("R", "pem_base64", "repeat", index, "tls.key: {V}\n---\nprivateKey: {V}\n",
             {"V": (whole, "SECURITYTOKEN")}, "en")
        for surface, value in [("certificate", b64(public)),
                               ("prose", b64("Never commit a private key to the repository.\n")),
                               ("public_key", b64(pem("PUBLIC KEY")))]:
            emit("D", "pem_base64", surface, index, "tls.crt: {D}\n" if partition == "test" else "ca.crt: {D}\n",
                 {"D": (value, None)}, "en")
    return result
