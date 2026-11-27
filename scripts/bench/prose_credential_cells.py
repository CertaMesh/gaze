"""CC0 prose credential disclosures and base64-encoded private keys, generator v31.

Passwords, security codes and tokens are invented per partition and cannot
authenticate anywhere; key bodies are random filler inside real armour lines.
Layer A carries EN/DE chat, email and tool-call prose that names a credential
and gives its value, including phrasings a cue grammar does not reach, so the
residual stays measured. Layer R repeats each value bare. Layer D carries the
same cue words with no credential: password rules and lengths, credential
names, hints, rotation schedules, the digit count of a card security code,
tokenizer names, credential-shaped build tags, article numbers and campaign
names under non-credential cues, and base64-encoded public certificates and
keys.
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
            ("Enter the {D}-digit security code from the back of the card.", "cvclen"),
            ("Build tag {D} shipped to staging.", "tokenshape"),
            ("Our spring campaign is called {D} this year.", "pwshape"),
            ("Verification code template {D} is used for sign-in mails.", "slug"),
        ],
        "benign_de": [
            ("Das Passwort muss mindestens {D} Zeichen haben.", "digits"),
            ("Passwort vergessen? Den Link finden Sie unter {D}.", "url"),
            ("Der Passwort-Hinweis für Konto {D} wurde entfernt.", "reference"),
            ("Der Sicherheitscode (CVC) auf der Kartenrückseite hat {D} Ziffern.", "cvclen"),
            ("Die Artikelnummer {D} ist ausverkauft.", "tokenshape"),
            ("Unser Kampagnenname lautet {D}.", "pwshape"),
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
            ("Type the {D}-digit security code printed next to the signature strip.", "cvclen"),
            ("Release tag {D} went out on Tuesday.", "tokenshape"),
            ("The product line {D} launches next month.", "pwshape"),
            ("The API key hint {D} is shown in the console.", "slug"),
        ],
        "benign_de": [
            ("Ein Passwort braucht mindestens {D} Zeichen.", "digits"),
            ("Passwort zurücksetzen über {D}.", "url"),
            ("Die Passwort-Richtlinie {D} gilt ab sofort.", "reference"),
            ("Der Sicherheitscode auf der Karte besteht aus {D} Ziffern.", "cvclen"),
            ("Die Chargennummer {D} ist freigegeben.", "tokenshape"),
            ("Der Messeslogan lautet {D}.", "pwshape"),
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
PRODUCT_STEMS = {"dev": ["Aurora", "Basalt", "Cobalt", "Delta"], "test": ["Ember", "Falke", "Granit", "Helix"]}
PASSPHRASE_WORDS = {
    "dev": ["copper", "meadow", "violin", "glacier", "pepper", "orbit", "lantern", "mosaic"],
    "test": ["harbor", "thimble", "saffron", "quartz", "willow", "canyon", "biscuit", "nebula"],
}


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

    def password(template, stems=STEMS[partition]):
        if "passphrase" in template and rng.below(2):
            words = PASSPHRASE_WORDS[partition]
            return " ".join(rng.choice(words) for _ in range(4)) + " " + rng.digits(2)
        value = rng.choice(stems) + rng.digits(rng.between(2, 4)) + rng.choice(SYMBOLS)
        return value if rng.below(2) else rng.choice(SYMBOLS) + value

    def token(template):
        # One-time codes are often all digits; API keys are often lowercase hex.
        if "code" in template.lower() and not rng.below(3):
            return rng.digits(rng.between(6, 8))
        if "API key" in template and rng.below(2):
            return chars(32, "0123456789abcdef")
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
        if kind == "cvclen":
            return str(rng.between(3, 4))
        if kind == "tokenshape":
            return token("")
        if kind == "pwshape":
            return password("", PRODUCT_STEMS[partition])
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
                    value = mint(template)
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
                               ("prose", b64("Never commit a private key to the repository.\n" if partition == "test"
                                         else "Keep signing keys out of version control.\n")),
                               ("public_key", b64(pem("PUBLIC KEY")))]:
            emit("D", "pem_base64", surface, index, "tls.crt: {D}\n" if partition == "test" else "ca.crt: {D}\n",
                 {"D": (value, None)}, "en")
    return result


# Generator v32: credential forms the v31 cells do not write, and agent and coding text that
# names a credential beside values that are not one.
AGENT_TEXT_PREFIX = "credential_agent_text_"
AGENT_TEXT = {
    # Dev carries the review's phrasings, which the rules were fitted on; test is written apart.
    "dev": {
        "password_en": [
            ("My passphrase is \"{V}\".", "phrase"),
            ("The password is {V} as of today.", "inner_quote"),
            ('{"message":"the password is \\"{V}\\" ok"}', "plain"),
            ("His password is {V} for the VPN.", "bracket"),
            ('{"messages":[{"role":"user","content":"my new password is {V}"}]}', "plain"),
            ('msg = "the password is {V}";', "plain"),
            ("The password is {V} until Monday.", "dollar"),
            ("The password for my email is {V}.", "plain"),
            ("The password is \"{V}\" in the fixture.", "cue_word"),
            ("My passphrase is \"{V}\".", "sentence"),
            ("Server=db;Password={V};User Id=sa;", "plain"),
            ("The password is {V} ok", "hyphen_stem"),
        ],
        "password_de": [
            ("Das Passwort lautet \"{V}\".", "phrase"),
            ('{"msg":"Mein Passwort ist \\"{V}\\"."}', "plain"),
            ('{"content":"Das Passwort lautet {V}","b":"x y"}', "plain"),
            ("Das Passwort für die Seite lautet {V}.", "plain"),
            ("Das Passwort lautet \"{V}\".", "sentence_de"),
        ],
        "benign_en": [
            ("The API key works with {D} and the embeddings model.", "model"),
            ("Rotate the API key before deploying release {D}.", "version"),
            ("The security code check failed on commit {D}.", "sha"),
            ("Access token lifetime is configured in {D}.", "file"),
            ("Our API key for the payment provider lives in {D} now.", "region"),
            ("The session token is refreshed by {D} every hour.", "snake"),
            ("The password is read from {D} at startup.", "env"),
            ("Store the password as {D} in the users table.", "column"),
            ("Load the password from {D} before the deploy.", "path"),
            ("The security code {D} was patched last week.", "cve"),
            ("The password prompt is {D}.", "ui"),
            ("Change the error for a wrong password to {D}.", "ui"),
            ("The password is {D} for every admin.", "hyphen"),
        ],
        "benign_de": [
            ("Der API-Key fehlt in Version {D}.", "version"),
            ("Das Passwort wird beim Start aus {D} geladen.", "env"),
            ("Das Zugangstoken wird in {D} konfiguriert.", "file"),
            ("Der Sicherheitscode ist {D}.", "hyphen"),
        ],
    },
    "test": {
        "password_en": [
            ("Okay, the wifi passphrase was \"{V}\" last I checked.", "phrase"),
            ("Note that my email password is now {V} after the lockout.", "inner_quote"),
            ('{"role":"user","content":"set the password: \\"{V}\\" please"}', "plain"),
            ("Sure, the password for the NAS is {V} today.", "bracket"),
            ('{"input":{"notes":["the shared password is {V}"]}}', "plain"),
            ('reply("ok, the password is {V}")', "plain"),
            ("Heads up, the router password is now {V} again.", "dollar"),
            ("FYI the password on the kiosk screen is {V} this week.", "plain"),
            ("Weak, I know, but the password is '{V}' for the demo box.", "cue_word"),
            ("Honestly my passphrase was '{V}' all along.", "sentence"),
            ('"ConnectionString": "Host=db;Password={V};Username=app"', "plain"),
            ("Told them the VPN password is {V} for now.", "hyphen_stem"),
        ],
        "password_de": [
            ("Das WLAN-Passwort war zuletzt \"{V}\".", "phrase"),
            ('{"text":"Das neue Kennwort lautet \\"{V}\\", danke"}', "plain"),
            ('{"note":"Kennwort ist {V}","id":"n1 n2"}', "plain"),
            ("Das Kennwort für meine E-Mail ist {V}.", "plain"),
            ("Mein Passwort war ehrlich gesagt '{V}'.", "sentence_de"),
        ],
        "benign_en": [
            ("Since the upgrade our access token only gets accepted by {D} endpoints.", "model"),
            ("Per the changelog the API key header changed in {D}, see the notes.", "version"),
            ("I bisected the session token bug down to {D} in the repo history.", "sha"),
            ("The API key loader reads defaults out of {D} first.", "file"),
            ("We pinned the access token issuer to {D} for latency.", "region"),
            ("Each night {D} replaces the session token for the bot.", "snake"),
            ("In CI the password gets injected via {D} automatically.", "env"),
            ("Migration 12 renamed the password column to {D} last sprint.", "column"),
            ("The deploy script copies the password into {D} before boot.", "path"),
            ("A security code review flagged {D} in the auth library.", "cve"),
            ("For a failed sign-in the password banner now reads {D} in bold.", "ui"),
            ("QA wants the forgot-password dialog to say {D} instead.", "ui"),
            ("Our VPN password is {D} on every laptop.", "hyphen"),
        ],
        "benign_de": [
            ("Seit {D} prüft der Server den API-Key strenger.", "version"),
            ("Im Container kommt das Passwort aus {D} vom Orchestrator.", "env"),
            ("Die Sicherheitstoken-Prüfung liegt jetzt in {D} im Repo.", "file"),
            ("Unser Zugangscode ist {D}.", "hyphen"),
        ],
    },
}
# Digit-free passphrases built from ordinary words, function words included.
SENTENCES = {
    "dev": {"sentence": ["the moon is blue", "it is raining cats", "one for all and all for one"],
            "sentence_de": ["der Hund bellt laut", "die Katze schläft gern", "ein Baum im Wind"]},
    "test": {"sentence": ["a fox under the bridge", "we sail at the dawn", "the owl and the lamp"],
             "sentence_de": ["das Boot im Hafen", "ein Lied für den Mond", "die Brücke am Fluss"]},
}
AGENT_VALUES = {
    "dev": {
        "model": ["gpt-4-turbo", "claude-3-opus", "mistral-7b-instruct"],
        "region": ["us-east-1", "eu-central-1", "ap-northeast-1"],
        "file": ["settings_v2.py", "token3.json", "auth_v4.yml"],
        "snake": ["auth_service_v2", "token_worker_3", "session_sync_v5"],
        "env": ["DB_PASSWORD", "REDIS_PASS_1", "API_SECRET_V3"],
        "column": ["bcrypt_hash", "password_hash", "pw_digest"],
        "path": ["~/.config/app/pw", "secret/data/api", "../keys/admin"],
        "words": ["copper", "lagoon", "walrus", "meteor", "crimson", "thistle"],
        "ui": ['"Please enter it again"', '"Incorrect password, try again"', '"Reset your account"'],
        "hyphen": ["2fa-enabled", "SHA256-basiert", "e2e-tested"],
        "sibling": ["databaseConnectionName", "replicaHostOverride", "sessionAffinityMode"],
    },
    "test": {
        "model": ["gemini-1.5-pro", "o3-mini", "phi-3-medium"],
        "region": ["westeurope", "sa-east-1", "me-south-1"],
        "file": ["defaults_2024.ini", "keys.v2.json", "app-config3.yaml"],
        "snake": ["rotate_tokens_job7", "nightly_refresh_v9", "bot_token_sync_4"],
        "env": ["NAS_ADMIN_PW", "SMTP_PASSWORD_2", "KEYCLOAK_PASS"],
        "column": ["passwd_bcrypt", "secret_digest", "pwd_hash_v2"],
        "path": ["/run/secrets/db_pw", "/etc/app/secret3", "C:\\ProgramData\\app\\pw"],
        "words": ["birch", "lantern", "pebble", "orchid", "harbor", "violet"],
        "ui": ['"Something went wrong on our side"', '"Did you forget it?"', "'Not the right one, sorry'"],
        "hyphen": ["mfa-protected", "AES256-signed", "SSO-only"],
        "sibling": ["clusterIssuerReference", "rotationWindowHours", "metricsScrapeEnabled"],
    },
}


def agent_text_records(api, partition):
    rng = api.Rng(api.PARTITION_SEEDS[partition], "credential-agent-text-v32")
    values = AGENT_VALUES[partition]
    result = []

    def emit(layer, family, surface, index, template, fields, language):
        text, gold, decoys = api._fill_with_decoys(template, fields)
        result.append(api.Record(
            uid=f"agentic-{partition}-{layer}-credagent-{family}-{surface}-{index}",
            partition=partition, layer=layer, family=family, surface=AGENT_TEXT_PREFIX + surface,
            validity=api.BENIGN if layer == "D" else api.UNCHECKED,
            group=f"{partition}-credagent-{family}-{index}",
            template=f"credagent/{partition}/{surface}", language=language,
            region="DE" if language == "de" else "US", text=text, gold=gold, decoys=decoys))

    def stem():
        word = rng.choice(STEMS[partition])
        return word + rng.digits(rng.between(2, 3))

    def password(kind):
        if kind == "phrase":
            # Spaced passphrases that end in a word, the shape a last-word check misreads.
            words = [rng.choice(values["words"]) for _ in range(3)]
            words.insert(rng.between(1, 2), rng.digits(2))
            return " ".join(words)
        if kind == "inner_quote":
            return stem()[:-1] + "'" + rng.choice(STEMS[partition]) + rng.digits(2)
        if kind == "cue_word":
            return rng.choice(["Passw0rd", "Kennwort", "MyPassword", "Password"]) + rng.digits(rng.between(1, 4)) + rng.choice("!#")
        if kind in ("sentence", "sentence_de"):
            words = SENTENCES[partition][kind]
            return rng.choice(words)
        if kind == "hyphen_stem":
            return rng.choice(STEMS[partition]) + rng.digits(4) + "-" + rng.choice(["only", "safe", "ready"])
        if kind == "dollar":
            return "$" + stem() + rng.choice("#%&*+@")
        if kind == "bracket":
            opener, closer = rng.choice(["()", "[]", "{}"])
            return stem() + opener + rng.choice(values["words"]) + closer
        return stem() + rng.choice("#$%&*+@")

    def benign(kind):
        if kind == "version":
            return f"v{rng.between(1, 9)}.{rng.between(0, 19)}.{rng.between(0, 9)}-rc{rng.between(1, 4)}"
        if kind == "sha":
            return "".join(rng.choice("0123456789abcdef") for _ in range(8))
        if kind == "cve":
            return f"CVE-{rng.between(2019, 2026)}-{rng.between(10000, 49999)}"
        return rng.choice(values[kind])

    def pem_wrapped():
        body = "\n".join("".join(rng.choice(BASE64) for _ in range(64)) for _ in range(3))
        armour = f"-----BEGIN PRIVATE KEY-----\n{body}\n-----END PRIVATE KEY-----\n"
        whole = base64.b64encode(armour.encode()).decode()
        return [whole[start:start + 76] for start in range(0, len(whole), 76)]

    for index in range(api.DOCS_PER_FAMILY):
        for language in ("en", "de"):
            for surface, (template, kind) in enumerate(AGENT_TEXT[partition][f"password_{language}"]):
                emit("A", f"password_{language}", f"{kind}_{surface}", index, template,
                     {"V": (password(kind), "PASSWORD")}, language)
            for surface, (template, kind) in enumerate(AGENT_TEXT[partition][f"benign_{language}"]):
                emit("D", f"benign_{language}", f"{kind}_{surface}", index, template,
                     {"D": (benign(kind), None)}, language)
        # Wrapped base64 private keys in an indented YAML block scalar and behind JSON-escaped
        # line breaks: the whole run, indentation and escapes included, is one gold value.
        lines = pem_wrapped()
        yaml = ("data:\n  tls.key: |\n    {V}\n  tls.crt: none\n" if partition == "test"
                else "stringData:\n  key.b64: |\n    {V}\n")
        emit("A", "pem_base64", "yaml_block", index, yaml,
             {"V": ("\n    ".join(lines), "SECURITYTOKEN")}, "en")
        whole = "".join(pem_wrapped())
        sibling = ("data:\n  tls_key: {V}\n  " + rng.choice(values["sibling"]) + ": primary\n" if partition == "test"
                   else "key:\n  tls_key: {V}\n  " + rng.choice(values["sibling"]) + ": true\n")
        emit("A", "pem_base64", "yaml_sibling_key", index, sibling, {"V": (whole, "SECURITYTOKEN")}, "en")
        lines = pem_wrapped()
        escaped = '{"secret":"{V}"}' if partition == "test" else '{"key_b64":"{V}"}'
        emit("A", "pem_base64", "json_escaped_lines", index, escaped,
             {"V": ("\\n".join(lines), "SECURITYTOKEN")}, "en")
    return result
