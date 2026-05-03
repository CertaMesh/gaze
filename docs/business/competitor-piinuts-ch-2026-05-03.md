# Wettbewerbs-Analyse — piinuts.ch + breitere Landschaft

**Datum:** 2026-05-03 (initial), erweitert 2026-05-03 mit Markt-Scan
**Status:** Diskussionsgrundlage
**Verwandt:** [`positioning-2026-04-28.md`](./positioning-2026-04-28.md), [`honest-assessment-2026-04-28.md`](./honest-assessment-2026-04-28.md), [`trust-and-certifications-2026-04-28.md`](./trust-and-certifications-2026-04-28.md)

> **Kernthese (Update 2026-05-03):** piinuts.ch ist marginal. Aber die breitere Konkurrenz hat zwischen März und April 2026 mehrere Pivot-Trigger aus `honest-assessment` realisiert. Story-Reset wichtiger als Brand-Reset.

---

## Executive Summary

1. **piinuts.ch** = kleines Schweizer Agentur-Side-Produkt, kein Funding, Tracxn-Score 16/100. Brand-Kollision real (UTY3 Sàrl betreibt es), aber finanziell und strategisch schwacher Player.
2. **Echte Konkurrenz hat sich verschärft.** OpenAI Privacy Filter (Apr 2026, Apache-2.0), Tonic Textual MCP Server (Mär 2026), NoPII (reverse-proxy mit deterministischer reversibler Tokenization), MCP-Gateway-Kategorie explodiert (Kong, Cloudflare, Pangea/LiteLLM/Portkey, Bifrost, Obot, MCP Manager).
3. **Differenzierungs-Pillars von April 2026 sind teilweise commodity.** "MCP-nativ + reversibel + open-source-detection" eingeholt. Verbleibender Moat: signierte Compliance-Pipeline, DACH-Locale-Tiefe, Laravel-Beachhead, on-prem ohne Vault-Friction.
4. **Pivot-Trigger gefeuert in 30 Tagen.** Honest-Assessment-Konstanten zur Disposition.

---

## ⚠️ Brand-Kollision — vor allem anderen klären

`piinuts.ch` (CH) ≠ wir (`PIInuts` GitHub-Org, geplante Domain `piinuts.io`). Gleiche Schreibweise, verschiedene Marken, gleiche Branche.

Risiken:

- **SEO-Verwechslung.** Such "piinuts" → CH-Site dominiert (live, indexiert) bevor wir launchen.
- **Marken-Streit möglich.** CH ist EU-nah, gleiche Branche (PII-Anonymisierung), Käufer-Persona-Überschneidung am Rand.
- **Trust-Schaden bilateral.** Bug bei denen → Käufer denkt es ist uns. Umgekehrt genauso.
- **Vertriebs-Friction.** "Hatten wir schon mal — die Schweizer Firma?" → Discovery-Cycle verlängert.

Optionen:

1. **Rebrand jetzt.** Billigste Lösung, solange keine Marketing-Investments.
2. **`.io` schnell sichern + Trademark anmelden DE/CH/EU.** Klassen 9 (Software) + 42 (SaaS).
3. **Bewusste Differenzierung.** Domain primär `gaze.dev` / `usegaze.io`, "Gaze by [Org-Name]" als Klammer. Marken-Konflikt vermeiden indem Produkt-Name dominiert.

**Empfehlung (verschärft 2026-05-03):** **Brand-Reset weg von "PIInuts" durchziehen.** Org umbenennen (z.B. `gaze-labs`), Produkt-Marke "Gaze" vorziehen, Trademark-Suche **Gaze** in Klasse 9/42 EU+CH+US starten.

---

## Was piinuts.ch wirklich verkauft

Beobachtungen von [piinuts.ch](https://piinuts.ch) (Stand 2026-05-03):

- **End-User-SaaS.** Login, Dashboard. Kein API/CLI/SDK/GitHub-Link sichtbar.
- **Headline:** "Unleash data privacy like never before"
- **Sub-Headline:** "Powerful AI. Effortless Compliance."
- **Pricing:**
  - Free: 30k chars, kein Credit-Card
  - Pro: CHF 49/Monat (CHF 42 jährlich), 2M+ chars/Monat
  - Enterprise: on-prem **"coming soon"**
- **Features:** 20+ Entitäten, 4 Sprachen (welche nicht offengelegt), Real-time Anonymisierung, "Re-identify data once your work is done".
- **Hosting:** CH, nFADP + GDPR-Claim, "no third-party data access".
- **Persona-Sprache:** "seamlessly", "effortlessly" → Knowledge-Worker / Compliance-Officer, nicht Dev.

**Was fehlt:** kein MCP, kein signierter Audit-Trail, keine Framework-Adapter, kein on-prem heute, kein OSS, keine Dev-Persona.

**Kategorisch:** Compliance-Chat-Tool für Knowledge-Worker. Vergleichbar mit Pangea/Nightfall-Lite oder hauseigenen "Privacy-Layer"-Wrappern um ChatGPT.

### Finanzielle Stärke / Erfolg

**Rechtsträger:** UTY3 Sàrl, Route du Petit-Moncor 1a, 1752 Villars-sur-Glâne (Kanton Fribourg). Sàrl = Schweizer GmbH, Mindestkapital CHF 20'000.

**Piinuts.ch ist Side-Produkt einer kleinen Digital-Agentur:**
- UTY3 Sàrl = Spin-off von "Up to you"-Agentur.
- Gründer: Vincent Brülhart, Ludovic Chenaux (digital marketing), Alec von Barnekow (Software-Engineer).
- Produktportfolio breit: KI-Lösungen, Automation, Trainings, **plus** Piinuts. Piinuts = einer von vier Geschäftsbereichen.
- Agentur-Mandate (mehrheitlich Werbe-/Web-Mandate, **nicht** Piinuts-Käufer): Elle Suisse, Alaia, Canton Jura, Swiss Watch, FriUp, HEIA-FR, UPCF, HC Gottéron — typisches Romandie-Agentur-Portfolio.

**Tracxn-Daten (Stand 2026-02-25):**
- Gegründet 2025
- **Funding: 0 €.** Bootstrapped.
- Mitarbeiter: nicht offengelegt
- Tracxn-Score: **16/100, Rang 39 von 39** im Wettbewerbs-Set
- Keine Revenue-/Traction-Signale offengelegt
- Konkurrenten haben Funding: $3.6M (Data Sentinel) bis $131M (Intralinks). Piinuts unten ohne.

**Praktische Konsequenz:**
- Kein VC-Druck → langsame Roadmap. "Enterprise on-prem coming soon" steht wohl länger.
- Klein-Team, geteilte Aufmerksamkeit über 4 Produkte → wenig dedizierte Eng-Kapazität.
- Romandie-Agentur-Setup → starker FR-Markt-Fokus, geringer DACH-/EU-Reach.
- Bootstrap = kann Jahre überleben, aber wächst nicht aggressiv.

**Bedrohungs-Niveau:** **Niedrig finanziell, niedrig strategisch.** Eher Romandie-Agentur-Beifang als ernster Wettbewerber. Reales Risiko bleibt Brand-Kollision (nDSG/GDPR-SEO + .ch-Domain) und Discovery-Verwirrung.

---

## Wo Gaze steht (zur Erinnerung)

Aus Repos + Docs (`gaze`, `gaze-laravel`, `gaze-website`, externer `gaze-lens`):

- **Developer-Runtime, on-prem first.** Rust-Binary läuft beim Kunden, PII verlässt nie Infra.
- **MCP-nativ** via `gaze-lens` (eigener Server) — agentic-Pipeline statt Chat-UI.
- **Manifest-bound Reversal**, Recognizer-Registry, Conflict-Resolution, fail-closed.
- **Audit-Trail-Crate** (`gaze-audit`) mit `SqliteLogger`, `AuditFilter`. Signierte Reports geplant.
- **Framework-Adapter** (`gaze-laravel` v0.3, Composer-Plugin, Binary-Auto-Install). Naoray-Beachhead.
- **Open-Core-Pfad** geplant (Apache-2.0 Core + Premium-Recognizer + Audit-Cloud).
- **Reife:** v0.5.0 Core, Lens v1.0, Laravel-Adapter aktiv, kein public Design-Partner.

**Persona:** Backend-Dev / SRE / Platform-Team mit DPO im Nacken.

---

## piinuts.ch direkter Vergleich

| Achse | piinuts.ch | Gaze |
|---|---|---|
| Form-Faktor | SaaS-Web-App | Embedded Runtime + CLI + MCP-Server |
| Wo läuft PII | CH-Cloud (deren Server) | Beim Kunden, nie raus |
| Persona | End-User, Knowledge-Worker, Compliance-Officer | Dev, SRE, DPO |
| Use-Case | Doku/Chat anonymisieren vor ChatGPT-Paste | Inline in agentic LLM-Pipeline mit Tool-Calls |
| Reversibilität | Ja, im UI-Workflow | Manifest-Contract, multi-turn, programmatisch |
| Audit-Trail | Nicht beworben | Architektur-Pflicht-Achse, signierte Reports geplant |
| MCP / agentic | Nein | Kern-Differenzierung (`gaze-lens`) |
| Framework-Adapter | Nein | Laravel live, weitere geplant |
| Open-Source | Nein erkennbar | Open-Core geplant |
| On-Prem | "Coming soon" Enterprise | Default-Mode |
| Sprachen | 4 (welche geheim) | Locale-chain 4-tier, eu/de/en heute |
| Entry-Price | CHF 49/Monat 2M chars | Hypothese 49 €/Monat 5M Tokens |
| Trust-Signale | CH-Hosting, GDPR-Claim | Reproducible Builds, OpenSSF, Sigstore *(geplant)* |
| Reife | Live, zahlende Kunden möglich | v0.5 Core, kein public Design-Partner |

---

## Breiterer Markt-Scan (ergänzt 2026-05-03)

### Tier 1 — Direkter Hit auf Gaze-Differenzierung

| Player | Was | Treffer-Achse |
|---|---|---|
| **NoPII** ([nopii.co](https://www.nopii.co)) | Reverse-Proxy für OpenAI/Anthropic-SDKs. Deterministische, format-erhaltende, **reversible** Tokenization. Backend = PCI-L1 + SOC2 Vault. HIPAA/GDPR/PCI-DSS-compliant. Fail-safe by default. Drop-in via `base_url`-Switch. | **Reversibilität + agentic-first** — exakt unser Pitch. Vault ist hosted, nicht on-prem — bleibt unser Vorteil. |
| **Tonic Textual MCP Server** (März 2026) | Tonic.ai's PII-Redaction jetzt als MCP-Server. | **MCP-nativ** — exakt `gaze-lens`. Tonic hat Marketing-Muskel + bestehende Enterprise-Kunden. |
| **OpenAI Privacy Filter** (22. Apr 2026) | Open-weight 1.5B-Param MoE-Modell, Apache-2.0, läuft lokal, 96% F1, 8 PII-Kategorien, 128k context. | **Plattform-Risiko aus Honest-Assessment ist Realität.** Untergräbt OSS-Glaubwürdigkeits-Hebel von Detection-Layer. |

### Tier 2 — Gateway-Kategorie schluckt PII-Filter als Feature

| Player | Was |
|---|---|
| **Kong AI Gateway** | PII-Sanitization für LLMs/agentic AI eingebaut. |
| **Cloudflare AI Gateway** | DLP-Scan auf MCP + Agent-Traffic. 100+ Detection-Typen. |
| **Pangea AI Guard** | PII-Redaction-Service. Bereits in **LiteLLM + Portkey** integriert. |
| **LiteLLM Proxy** | Presidio + Pangea als built-in Guardrails. OSS. |
| **Portkey** | Pangea-Integration. AI-Gateway. |
| **Bifrost MCP Gateway** | MCP-Governance + PII-Filter. |
| **Obot** | MCP-Filtering, Compliance-Logs. |
| **MCP Manager** | PII-Redaction für MCP-Server. |
| **Cequence** | MCP-Security + Governance (CIS-Companion-Guide-konform). |

→ **PII am Gateway ist commodity geworden.** Devs bauen heute nichts neu — sie aktivieren Pangea-Guardrail in LiteLLM mit drei Zeilen YAML.

### Tier 3 — Klassische Detection / DLP-Player mit LLM-Story

| Player | Was |
|---|---|
| **Microsoft Presidio** | OSS-Detection, kein Reversal, sehr breit verbreitet. |
| **Protecto** | Enterprise-NER, Presidio-Alternative, hohe Genauigkeit. |
| **Nightfall** | AI-DLP-Plattform, 100+ Modelle, 95% Accuracy. |
| **Private AI** | API-basiert, multi-language. |
| **Skyflow** | Vault-Pattern, Enterprise. |
| **John Snow Labs** | Medical de-identification, Health-spezifisch. |
| **LLM Guard** | OSS-Toolkit, 35+ Scanner. |
| **Strac, DataFog, IRI, K2view** | Tokenization-Tools, breiter Markt. |

### Tier 4 — Compliance-Nische / Adjacent

| Player | Was |
|---|---|
| **Comply** (ComplyAI MCP) | Financial-Services-Compliance MCP. Sektor-spezifisch. |
| **piinuts.ch** (UTY3 Sàrl) | Doku-/Chat-Anonymisierung Knowledge-Worker. Kein Funding. |

### Regulierungs-Kontext

- **CIS MCP Companion Guide** publiziert 20. Apr 2026 — MCP-Governance ist jetzt offizielles Framework, kein Greenfield mehr.
- **EU AI Act High-Risk-Provisions** voll erzwingbar ab **August 2026** — pusht Käufer in Compliance-Tools, **aber** in etablierte (Kong, Cloudflare, Tonic) zuerst.

---

## Pivot-Trigger-Realitätscheck

Aus `honest-assessment-2026-04-28.md`:

> "Anthropic / OpenAI können in 6 Monaten 'PII-aware mode' als Feature shippen."

→ ✅ **Gefeuert.** OpenAI Privacy Filter, **5 Tage** nach Veröffentlichung des Honest-Assessments (22. Apr 2026).

> "Skyflow / Tonic können MCP-Layer dazu basteln."

→ ✅ **Gefeuert.** Tonic Textual MCP Server, **vor** dem Honest-Assessment (März 2026).

> "Microsoft Presidio + Copilot-Integration ist ein Federstrich entfernt."

→ 🟡 Teil-realisiert. Presidio läuft in LiteLLM-Proxy als Guardrail.

**Fazit:** Was die Differenzierungs-Story von April 2026 noch versprach (MCP-nativ + reversibel + manifest-bound) ist heute teilweise commodity oder wird es in 6 Monaten sein.

---

## Was bleibt als echte Differenzierung

1. **Signierter Compliance-Report mit deterministischer Token-Herkunft (Art. 5(2)).** Kein Konkurrent hat das so geschnürt. DPO-Verkaufs-Hammer.
2. **DACH-Locale-Tiefe + Mehrsprachigkeit per Recognizer-Pack** (eu/de heute, klar erweiterbar). Tonic + NoPII = US-zentriert.
3. **Laravel-Beachhead via Naoray.** Niemand sonst spricht PHP.
4. **Dependency-zero Rust-Binary, on-prem, kein Vault-Zwang.** NoPII nutzt hosted Vault — DPO-Friction. Tonic = SaaS-cloud.
5. **Audit-Trail-Crate als Architektur-Pflicht-Achse.** Andere logged ad-hoc. Bei uns ist Audit Erste-Klasse-Bürger.

## Was weg ist

- "MCP-nativ" als allein-stehender Pitch → Tonic + Bifrost + Obot + Kong + Cloudflare haben es.
- "Reversibel" als allein-stehender Pitch → NoPII hat es deterministisch.
- "Open-Source-PII-Detection" als Trust-Hebel → OpenAI Privacy Filter (Apache-2.0) deckt 96% F1 lokal.

---

## Strategische Empfehlung — Update

### Sollen wir trotzdem durchstarten?

**Mit veränderter Story — ja. Mit der April-2026-Story — nein.**

#### Ja, weil

- Compliance-Report-Pipeline ist real-defensible, kein Konkurrent hat dasselbe Schnürpaket.
- Laravel-Beachhead unverändert exklusiv. PHP-Welt von keinem Tier-1/Tier-2-Player adressiert.
- DACH-DPO-Markt versteht "Schweizer Tonic" oder "US-Cloudflare" oft schlechter als deutschsprachiges Tool — Sprach-/Kultur-Vorteil real.
- Bootstrap-Risiko asymmetrisch: maximaler Verlust = Zeit. Maximaler Gewinn = Mikro-SaaS mit 5–10k €/Monat realistisch in 12 Monaten.
- Gegner-Schwächen real: NoPII-Vault-Friction, Tonic-Enterprise-Preise, Kong/Cloudflare-Generic-Layer ohne GDPR-Doku, OpenAI-Privacy-Filter ohne Reversibility.

#### Nein, wenn

- Wenn Strategie unverändert "MCP-PII + reversibel" bleibt → Story eingeholt, Verkauf wird hart.
- Wenn `gaze-lens` Hauptpitch ist → Tonic-Textual-MCP frisst direkt diese Wiese.
- Wenn Funding-Pfad "Wir sind die OSS-Alternative zu Presidio" lautet → OpenAI Privacy Filter macht Pitch-Deck obsolet.

### Konkrete Konsequenzen für Repositionierung

1. **Headline-Pivot:** Statt "MCP-native PII pseudonymization" → **"Signed GDPR Art. 5(2) compliance pipeline for AI agents — auditable from token to report"**.
2. **Hero-Demo:** Nicht der Pseudonymisierungs-Schritt, sondern der **signierte PDF-Compliance-Report**. Das hat sonst keiner.
3. **Brand-Reset:** Brand-Kollision (piinuts.ch) ist jetzt Pflicht-Reset. Empfehlung: **Gaze als Produkt-Marke vorziehen, "PIInuts" als Org streichen.** `gaze.dev` / `usegaze.io` / `gazepii.com` prüfen. Org wird `gaze-labs` o.ä. Trademark-Suche **Gaze** in Klasse 9/42 EU+CH+US.
4. **Beachhead-Doppel:** Laravel (Naoray) + **Compliance-Auditor-Persona** (DPO-Beratungen, TÜV-Affiliates, GDD-Mitgliedschaft). Letzteres weil Tier-2-Gateways DPOs gar nicht kennen.
5. **Kill `gaze-lens`-First-Marketing.** Lens bleibt als technisches Asset, aber nicht als Hero-Story. Tonic hat zu viel Marketing-Vorsprung.
6. **Honest-Assessment-Update fällig.** "8 Wochen kein Design-Partner" Pivot-Trigger sollte verschärft werden auf **6 Wochen**, weil Markt-Fenster sich schließt.
7. **OpenAI-Privacy-Filter integrieren statt bekämpfen.** Optionaler Recognizer-Backend in `gaze-recognizers`. Trust-Hebel: "Wir kombinieren OpenAI's 96% F1 mit deterministischer Manifest-Reversibilität und Compliance-Reports."

### Verdict

Brand-Reset weg von "Piinuts" → **dringend, aber zweitrangig**.

Story-Reset weg von "MCP-native reversible PII" → **ersttrangig**. Markt hat das in 30 Tagen kommodifiziert.

Compliance-Pipeline ist die noch unbesetzte Spielwiese. Wenn wir innerhalb 6 Monaten erstes signiertes Report-Demo + erstem DPO-Design-Partner haben → es geht. Wenn nicht → Tier-2-Gateways bauen Compliance-Layer drauf, dann ist auch das weg.

---

## Offene Fragen

1. Wer ist hinter piinuts.ch konkret? UTY3 Sàrl-Handelsregister + UID-Recherche im Schweizer Zefix.
2. Trademark-Status "Gaze" in DE / CH / EU / US? Klassen 9 + 42.
3. Eigene Marken-Anmeldung wirtschaftlich vor erstem Kunden? Realistisch 2–5k EUR pro Jurisdiktion.
4. Domain-Strategie: `piinuts.io` halten oder zu `gaze.dev` / `usegaze.io` wechseln? Verfügbarkeits-Check + Preis prüfen.
5. Wie reagieren wir, wenn piinuts.ch in 6 Monaten Dev-API shippt? (geringes Risiko, aber Plan haben)
6. **Neu:** OpenAI-Privacy-Filter als optionaler `gaze-recognizers`-Backend integrieren — Aufwand-Schätzung?
7. **Neu:** Tonic Textual MCP Server detail-evaluieren (eigene Tests) — wie nah am `gaze-lens`-Feature-Set?
8. **Neu:** NoPII-Pricing + Vault-Architektur recherchieren — Vergleichs-Pitch für DPO-Käufer.
9. **Neu:** Hero-Demo "signierter Compliance-Report" — wie schnell baubar mit aktueller `gaze-audit`-Crate?

---

## Quellen (Stand 2026-05-03)

### piinuts.ch / UTY3
- [piinuts.ch](https://piinuts.ch)
- [piinuts.ch Conditions (UTY3 Sàrl Fribourg)](https://piinuts.ch/conditions)
- [piinuts.ch Privacy](https://piinuts.ch/privacy)
- [UTY3.ai — Anbieter-Site](https://uty3.ai)
- [Tracxn — Piinuts profile](https://tracxn.com/d/companies/piinuts/__AW29nTjYLLdEbto1J8p8vaQfn0yCEzHr-fXAW-4JyLE)

### Tier 1 — Direkter Hit
- [NoPII — PII tokenizing reverse proxy](https://www.nopii.co)
- [NoPII — DEV blog "109 tests"](https://dev.to/nopii_hq/we-ran-109-tests-to-measure-how-pii-protection-methods-affect-llm-output-quality-heres-what-we-1k2f)
- [Introducing OpenAI Privacy Filter](https://openai.com/index/introducing-openai-privacy-filter/)
- [OpenAI Privacy Filter Model Card (PDF)](https://cdn.openai.com/pdf/c66281ed-b638-456a-8ce1-97e9f5264a90/OpenAI-Privacy-Filter-Model-Card.pdf)
- [VentureBeat — OpenAI Privacy Filter launch](https://venturebeat.com/data/openai-launches-privacy-filter-an-open-source-on-device-data-sanitization-model-that-removes-personal-information-from-enterprise-datasets)
- [openai/privacy-filter on Hugging Face](https://huggingface.co/openai/privacy-filter)
- [Tonic Textual MCP server announcement (Mar 2026)](https://securityboulevard.com/2026/03/announcing-the-tonic-textual-mcp-server-pii-redaction-meets-ai-agents/)
- [Tonic.ai — Benchmarking OpenAI Privacy Filter](https://www.tonic.ai/blog/benchmarking-openai-privacy-filter-pii-detection)

### Tier 2 — Gateway-Kategorie
- [Kong — PII Sanitization for LLMs and Agentic AI](https://konghq.com/blog/enterprise/building-pii-sanitization-for-llms-and-agentic-ai)
- [LiteLLM — Pangea guardrail integration](https://docs.litellm.ai/docs/proxy/guardrails/pangea)
- [LiteLLM — Presidio PII masking](https://docs.litellm.ai/docs/proxy/guardrails/pii_masking_v2)
- [Portkey — Pangea integration](https://portkey.ai/docs/integrations/guardrails/pangea)
- [Best MCP Gateways and AI Agent Security Tools (2026) — Integrate.io](https://www.integrate.io/blog/best-mcp-gateways-and-ai-agent-security-tools/)
- [Obot — MCP Filtering](https://obot.ai/blog/mcp-filtering/)
- [MCP Manager — PII redaction for MCP servers](https://mcpmanager.ai/blog/pii-redaction-for-mcp-servers/)
- [Cequence — CIS MCP Security Guide](https://www.cequence.ai/blog/ai/cis-mcp-security-guide-how-to-govern-ai-agent-access-in-enterprise-environments/)
- [Bifrost MCP Gateway Governance — DEV](https://dev.to/kuldeep_paul/bifrost-mcp-gateway-governance-compliance-requirements-for-regulated-ai-agents-41jg)
- [Comply — ComplyAI MCP for financial services](https://www.comply.com/resource/comply-launches-financial-services-first-agentic-compliance-platform-mcp-server-enabling-teams-to-build-custom-ai-agents-without-developers/)

### Tier 3 — Detection / DLP
- [Microsoft Presidio on GitHub](https://github.com/microsoft/presidio)
- [Protecto vs Microsoft Presidio](https://www.protecto.ai/protecto-vs-microsoft-presidio/)
- [Nightfall AI Review 2026](https://aiflowreview.com/nightfall-ai-llm-prompt-dlp-review/)
- [John Snow Labs vs Presidio — Medical de-identification](https://www.johnsnowlabs.com/comparing-john-snow-labs-medical-text-de-identification-with-microsoft-presidio/)
- [Grepture — Best PII Redaction APIs for LLMs (2026)](https://grepture.com/compare/best-pii-redaction-apis-for-llms)

---

## Caveat

Snapshot 2026-05-03. Konkurrenz-Landschaft bewegt sich wöchentlich. Rechtliche Bewertung der Marken-Kollision **muss** Anwalt machen — diese Notiz ersetzt keine Trademark-Recherche. Ranking-Tier-Einschätzungen basieren auf Marketing-Material + Tracxn — eigene technische Evaluation der Top-3-Konkurrenten (NoPII, Tonic Textual MCP, OpenAI Privacy Filter) noch ausstehend.
