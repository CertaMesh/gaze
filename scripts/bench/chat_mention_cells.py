"""CC0 chat-platform user mentions, generator v33.

Slack user IDs, enterprise user IDs and Discord snowflakes are minted per
partition and name no real account; display names are invented. Layer A carries
EN/DE chat, tool-result JSON and agent prose with Slack `<@U…>` / `<@W…>`
mentions (some with the client's `|display.name`) and Discord `<@…>` / `<@!…>`
mentions; the value inside the mention markup is gold. Layer R repeats each
mention. Layer D carries the same markup with no person in it: channel, user
group and broadcast mentions, Discord role, channel, emoji and timestamp
markup, slash-command markup, workspace IDs, Java type annotations inside generics,
HTML tags, team `@` handles, decorators and Slack app IDs in angle brackets.
"""

SURFACE_PREFIX = "chat_mentions_"

UPPER_DIGITS = "ABCDEFGHJKLMNPQRSTUVWXYZ0123456789"

GIVEN = {
    "dev": ["lina", "tomasz", "ayo", "greta", "ravi", "mireille", "jonte", "sanna"],
    "test": ["edda", "kofi", "ilse", "marek", "noor", "bastian", "yuki", "oriane"],
}
FAMILY = {
    "dev": ["varnholt", "quellmann", "okonedo", "brisk", "talmeyer", "dunsworth"],
    "test": ["ferrowitz", "lindqvist", "achterberg", "osei", "plumridge", "kessendorf"],
}

TEMPLATES = {
    "dev": {
        "slack_en": [
            "<@{V}> can you take a look at the failing deploy before five?",
            "Thanks <@{V}>, merged and closing the thread.",
            '{"type":"message","channel":"C04QK2M8Z","text":"<@{V}> approved the rollout plan","ts":"1767225600.000200"}',
            "Summary: <@{V}> owns the migration, I will draft the changelog.",
        ],
        "slack_de": [
            "<@{V}> kannst du bitte den Build noch einmal starten?",
            "Danke <@{V}>, ich übernehme das Review.",
        ],
        "discord_en": [
            "<@{V}> joined the voice channel for the playtest.",
            '{"content":"<@{V}> the patch notes are up","channel_id":"1189044412340000001"}',
        ],
        "discord_de": [
            "Kurze Frage an <@{V}>: läuft der Server wieder?",
        ],
        "benign": [
            ("Alerts go to {D} from now on.", "channel"),
            ("{D} standup starts in five minutes.", "broadcast"),
            ("Paging {D}, please acknowledge the incident.", "subteam"),
            ("{D} the raid starts at eight.", "role"),
            ("Moved the thread to {D}.", "discord_channel"),
            ("Nice work {D}", "emoji"),
            ("Release window opens {D}.", "timestamp"),
            ("Run {D} to redeploy.", "command"),
            ("Workspace {D} was migrated to the enterprise grid.", "team"),
            ("public Map<@NonNull String, Long> counts() { return {D}; }", "annotation"),
            ("<div class=\"{D}\">Deploy finished</div>", "html"),
            ("Ask {D} in the team channel.", "handle"),
            ("Slack app IDs such as {D} are listed in the admin console.", "bare_id"),
            ("@Override\npublic String {D}() { return name; }", "decorator"),
        ],
    },
    "test": {
        "slack_en": [
            "<@{V}> could you check why the nightly job timed out?",
            "Appreciate it <@{V}>, shipping it now.",
            '{"type":"message","channel":"C07HV3R1P","text":"<@{V}> signed off on the release","ts":"1767312000.000300"}',
            "Notes: <@{V}> is on point for the database upgrade, I will update the tracker.",
        ],
        "slack_de": [
            "<@{V}> könntest du die Pipeline bitte neu anstoßen?",
            "Merci <@{V}>, ich kümmere mich um den Rest.",
        ],
        "discord_en": [
            "<@{V}> is streaming the tournament tonight.",
            '{"content":"<@{V}> your ticket has been answered","channel_id":"1201550093370000002"}',
        ],
        "discord_de": [
            "Hey <@{V}>, bist du heute Abend beim Training dabei?",
        ],
        "benign": [
            ("Please post deploy notes in {D}.", "channel"),
            ("{D} the office closes early today.", "broadcast"),
            ("Escalating to {D} for the database alert.", "subteam"),
            ("{D} patch day is tomorrow.", "role"),
            ("Continue the discussion in {D}.", "discord_channel"),
            ("Ship it {D}", "emoji"),
            ("Maintenance starts {D}.", "timestamp"),
            ("Use {D} to open a ticket.", "command"),
            ("Workspace {D} enabled SSO last week.", "team"),
            ("Callable<@UIThread Void> task = () -> {D};", "annotation"),
            ("<span data-role=\"{D}\">Build passed</span>", "html"),
            ("Route the question to {D} please.", "handle"),
            ("The bot is installed as app {D} in every workspace.", "bare_id"),
            ("@dataclass\nclass {D}:\n    retries: int = 3", "decorator"),
        ],
    },
}


def records(api, partition):
    rng = api.Rng(api.PARTITION_SEEDS[partition], "chat-mentions-v33")
    templates = TEMPLATES[partition]
    result = []

    def emit(layer, family, surface, index, template, fields, language):
        text, gold, decoys = api._fill_with_decoys(template, fields)
        result.append(api.Record(
            uid=f"agentic-{partition}-{layer}-mention-{family}-{surface}-{index}",
            partition=partition, layer=layer, family=family, surface=SURFACE_PREFIX + surface,
            validity=api.BENIGN if layer == "D" else api.UNCHECKED,
            group=f"{partition}-mention-{family}-{index}",
            template=f"mention/{partition}/{surface}", language=language,
            region="DE" if language == "de" else "US", text=text, gold=gold, decoys=decoys))

    def chars(size):
        return "".join(UPPER_DIGITS[rng.below(len(UPPER_DIGITS))] for _ in range(size))

    def snowflake():
        return str(rng.between(1, 9)) + rng.digits(rng.between(16, 18))

    def slack():
        value = rng.choice("UUUW") + "0" + chars(rng.between(7, 9))
        if rng.below(3) == 0:
            value += "|" + rng.choice(GIVEN[partition]) + "." + rng.choice(FAMILY[partition])
        return value

    def discord():
        return ("!" if rng.below(3) == 0 else "") + snowflake()

    def decoy(kind):
        if kind == "channel":
            return f"<#C0{chars(8)}|{rng.choice(['alerts', 'deploys', 'general', 'ops-room'])}>"
        if kind == "broadcast":
            return rng.choice(["<!here>", "<!channel>", "<!everyone>"])
        if kind == "subteam":
            return f"<!subteam^S0{chars(8)}|@{rng.choice(['oncall', 'sre', 'db-team'])}>"
        if kind == "role":
            return f"<@&{snowflake()}>"
        if kind == "discord_channel":
            return f"<#{snowflake()}>"
        if kind == "emoji":
            return f"<:{rng.choice(['partyparrot', 'shipit', 'tada'])}:{snowflake()}>"
        if kind == "timestamp":
            return f"<t:{rng.between(1767225600, 1798761600)}:{rng.choice('RFfdt')}>"
        if kind == "command":
            return f"</{rng.choice(['deploy', 'ticket', 'status'])}:{snowflake()}>"
        if kind == "team":
            return "T0" + chars(8)
        if kind == "html":
            return rng.choice(["status-ok", "toast", "badge-info"])
        if kind == "handle":
            return rng.choice(["@oncall", "@sre-team", "@release-bot", "@here"])
        if kind == "bare_id":
            return "<A0" + chars(8) + ">"
        if kind == "decorator":
            return rng.choice(["toString", "RetryPolicy", "describe"])
        return rng.choice(["null", "Long.valueOf(0)", "Void.TYPE.cast(null)"])

    for index in range(api.DOCS_PER_FAMILY):
        for family, mint in (("slack", slack), ("discord", discord)):
            for language in ("en", "de"):
                for surface, template in enumerate(templates[f"{family}_{language}"]):
                    value = mint()
                    # The mention markup stays raw text; only the user value inside it is gold.
                    gold_value = value[1:] if value.startswith("!") else value
                    filled = template.replace("<@{V}>", "<@!{V}>" if value.startswith("!") else "<@{V}>")
                    emit("A", f"{family}_{language}", f"{family}_{surface}", index, filled,
                         {"V": (gold_value, "USERNAME")}, language)
                    repeat = "\nAgain: " if language == "en" else "\nNochmal: "
                    repeat += "<@!{V}>" if value.startswith("!") else "<@{V}>"
                    emit("R", f"{family}_{language}", f"{family}_repeat_{surface}", index,
                         filled + repeat, {"V": (gold_value, "USERNAME")}, language)
        for surface, (template, kind) in enumerate(templates["benign"]):
            emit("D", "benign", f"benign_{surface}", index, template, {"D": (decoy(kind), None)}, "en")
    return result
