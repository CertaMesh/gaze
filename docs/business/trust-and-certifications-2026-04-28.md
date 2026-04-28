# Vertrauens-Architektur — Zertifikate & Trust-Signale

**Datum:** 2026-04-28
**Status:** Übersicht / Diskussionsgrundlage
**Verwandt:** [`positioning-2026-04-28.md`](./positioning-2026-04-28.md), [`honest-assessment-2026-04-28.md`](./honest-assessment-2026-04-28.md)

> **Kernthese:** Bei Security-Tools ist Vertrauen das Produkt. Zertifikate sind ein Hebel, aber nicht der einzige — und nicht alle gleich wertvoll. Strategie schlägt "wir holen alles".

## Phasenmodell — Empfehlung im Überblick

| Phase | Zeitfenster | Investment | Output |
|---|---|---|---|
| **A — Free / Tag 1** | Monat 0–3 | 0 € (nur Zeit) | Tech-Buyer-Vertrauen, Open-Source-Glaubwürdigkeit |
| **B — Günstig** | Monat 3–9 | 15–35k einmalig | Glaubwürdigkeit gegen Skeptiker, Pen-Test-Report |
| **C — Enterprise-Standard** | Monat 9–18 | 60–120k initial + 30k/J | Enterprise-Tor offen |
| **D — Branchen-spezifisch** | Monat 18+ | je nach Markt | Sektor-Erschließung |

Phase A **immer** angehen. Phasen B–D nur wenn konkreter Kunden-Bedarf signalisiert.

---

## Tier 0 — Free / Tag-1, hoher Impact

Kostet nur Zeit. Setzt Vertrauen-Anker, ohne Audit-Firma zu bezahlen. **Hier sofort einsteigen.**

| Signal | Was | Aufwand |
|---|---|---|
| **OpenSSF Scorecard Badge** | Automatisierte Code-Hygiene-Prüfung (Branch-Protection, Pinned-Deps, Code-Review-Regel, Vuln-Scan). Public Score. | 1 Tag Setup |
| **OpenSSF Best Practices Badge** | Selbst-Erklärung gegen ~30 Security-Best-Practices. Gold/Silver/Passing. | 2 Tage |
| **Sigstore / cosign signed releases** | Jedes Binary kryptografisch signiert, Public-Key checkable. | 1 Tag CI |
| **Reproducible Builds** | Jeder kann Binary aus Source rebuilden, Hash matcht. Beweis "kein heimlicher Code drin". | 1–2 Wochen |
| **SBOM (CycloneDX / SPDX)** | Software Bill of Materials — auflistet alle Dependencies versioniert. Pflicht in vielen Ausschreibungen. | 1 Tag CI |
| **SLSA Level 2–3** | Supply-Chain-Sicherheits-Framework, basiert auf Reproducible Builds + signierter Provenance. | 1 Woche |
| **GitHub Security Advisory** | Eigener Reporting-Channel für Bugs, CVE-Vergabe-Prozess. | 1 Tag |

→ Diese 7 Punkte = **"wir nehmen Security ernst"-Signal**, das jeder technische Buyer in 30s checken kann. Mehr wert als jedes Marketing-Statement.

---

## Tier 1 — Günstig (~5–30k), großer Trust-Sprung

| Signal | Was | Kosten | Wann |
|---|---|---|---|
| **Independent Pen-Test + Public Report** | Cure53 / Trail of Bits / NCC Group machen Code-Audit, Report wird published. Riesige Glaubwürdigkeit. | 10–30k einmalig | Nach v1.0 stabil, vor erstem großen Kunden |
| **Bug-Bounty-Programm** | HackerOne / Intigriti / huntr.dev. Externe Hacker melden Bugs gegen Bounty. | 0–5k Setup + variable Bounties | Sobald Public-Tap live |
| **TÜV / DEKRA Spot-Audit** | Punktuelle Prüfung ohne volle ISO-Zertifizierung. "Geprüft durch TÜV Rheinland" Siegel. | 5–15k | Marketing-Hebel DACH |
| **GDD-Mitgliedschaft** | Gesellschaft für Datenschutz und Datensicherheit. DACH-DPO-Netzwerk. | 1–2k/Jahr | Sales-Türöffner DACH |

---

## Tier 2 — Standard-Enterprise-Pakete

Brauchst du **erst** für Enterprise-Kunden. Vorher Geldverbrennen.

| Cert | Wofür | Kosten Initial | Jährlich |
|---|---|---|---|
| **ISO/IEC 27001** | Information-Security-Management. Quasi-Pflicht für Konzern-Kunden. | 30–60k | 10–20k Surveillance |
| **ISO/IEC 27701** | Privacy-Add-on zu 27001. Direkt-Fit für Gaze (PIMS = Privacy Info Management System). | +10–20k auf 27001 | +5k |
| **SOC 2 Type II** | US-Standard. Nötig für US-Enterprise. Type II = 6+ Monate Beobachtung. | 30–80k | 30–80k jährlich |
| **TISAX** | Automotive DE. Nur wenn Automotive-Kunden im Visier. | 15–40k | 5–15k |
| **BSI C5** | Cloud-Testat DE-Bundesbehörden. | 30–80k | 15–30k |

→ **Faustregel:** Erst dann anfangen, wenn ein konkreter Kunde sagt "ich kaufe, sobald ihr X habt." Sonst Fehlinvestition.

---

## Tier 3 — AI-spezifisch, Early-Mover-Vorteil 🆕

**Strategisch besonders interessant für Gaze:**

| Cert | Status | Warum strategisch |
|---|---|---|
| **ISO/IEC 42001** (AI Management System) | Seit Dez 2023, Auditor-Pipeline gerade rollend | **Frühe Zertifizierung = Marketing-Gold.** "Erstes PII-Pseudonymisierungs-Tool mit ISO 42001" ist eine Headline. Kosten ähnlich 27001 (~30–60k). |
| **NIST AI RMF Alignment** | Selbst-Erklärung möglich, kein Cert | Bringt US-Federal-Audience-Punkte. Kostenlos. |
| **EU AI Act Compliance Self-Declaration** | Pflicht für Hochrisiko-KI ab 2026 | Wir sind PII-Layer, nicht KI selbst — aber unsere **Kunden** brauchen das, und unser Audit-Trail hilft ihnen. **Vermarktbar.** |

→ **Strategie-Tipp:** ISO 42001 sobald wirtschaftlich tragbar anvisieren. Markt ist neu, Konkurrenz hat's noch nicht, Differenzierungs-Potenzial erheblich.

---

## Tier 4 — Spezial / nur bei Bedarf

| Cert | Wann |
|---|---|
| **HIPAA-ready** (kein formales Cert, aber BAA-Verträge + Audit-Report) | Wenn US-Health-Markt aktiv |
| **PCI DSS** | Nur wenn Zahlungsdaten verarbeitet — eher unwahrscheinlich für Gaze |
| **FedRAMP** | Nur US-Federal. Brutal teuer (~500k–2M, 12+ Monate). Frühphase: ignorieren. |
| **Common Criteria EAL4+** | Nur Defense / Geheimdienst. Bottomless-pit-teuer. Ignorieren. |
| **FIPS 140-3** | Nur bei Krypto-Modul-Anspruch. Wenn eigene Krypto: ja. Sonst delegieren auf BoringSSL/OpenSSL FIPS-Builds. |

---

## GDPR-spezifische Siegel

Komplizierte Lage:

| Siegel | Status |
|---|---|
| **GDPR Art. 42 Certification** | Offizielles Schema. Aber: Auditor-Pipeline noch dünn, Markt erkennt's noch nicht stark. Eher in 2–3 Jahren relevant. |
| **EuroPriSe** | Privates Privacy-Siegel, etabliert in DACH. ~20–40k. Marketing-Wert in Behörden-Kontext. |
| **GDPR Code of Conduct** (Art. 40) | Sektorale Codes — z.B. für Cloud-Provider. Nichts spezifisch für uns. |

→ **Empfehlung:** EuroPriSe ist nice-to-have ab Tier-2-Phase. Vorher OpenSSF + Pen-Test reichen.

---

## Empfohlener Pfad (Phasen-Detail)

### Phase A (Monat 0–3, vor erstem Kunden)

Checkliste:
- [ ] OpenSSF Scorecard Badge
- [ ] OpenSSF Best Practices Badge (Passing → Silver)
- [ ] Sigstore signed releases
- [ ] SBOM in jeder Release
- [ ] Reproducible Builds dokumentiert
- [ ] Public Security-Reporting-Channel (`SECURITY.md`)
- [ ] SLSA Level 2

**Cost: 0 €. Impact: enorm.** Macht in jedem Tech-Vetting sofort sympathisch.

### Phase B (Monat 3–9, vor erstem Enterprise-Pitch)

Checkliste:
- [ ] Independent Pen-Test (Cure53 oder NCC Group), Report public
- [ ] Bug-Bounty-Programm (huntr.dev als Start, billig)
- [ ] GDD-Mitgliedschaft DACH

**Cost: 15–35k. Impact: Glaubwürdigkeit gegen Skeptiker.**

### Phase C (Monat 9–18, mit erstem zahlenden Enterprise / nach Seed-Funding)

Checkliste:
- [ ] ISO 27001 + 27701 (gemeinsam auditiert, billiger)
- [ ] ISO 42001 (AI-Management) — **early-mover-Hebel**
- [ ] SOC 2 Type II nur wenn US-Markt aktiv

**Cost: 60–120k initial + 30k/Jahr. Impact: Enterprise-Tor offen.**

### Phase D (Monat 18+, branchen-spezifisch)

- C5 wenn Behörden-Kunden
- TISAX wenn Automotive
- HIPAA-Pfad wenn US-Health
- EuroPriSe wenn DACH-Datenschutz-Marketing

---

## Trust-Multiplikatoren ohne Cert (kosten nichts, wirken oft mehr)

Für **technische Buyer** sind diese oft wertvoller als Papier-Zertifikate:

- **Reproducible Builds** + öffentlicher Build-Hash → "ich kann selbst prüfen, kein Backdoor drin".
- **Public Roadmap** mit Security-Decisions transparent erklärt (`docs/research/` ist genau richtig).
- **Public Incident-Reports** wenn was schiefgeht — Postmortem-Kultur baut Vertrauen.
- **Open-Source-Beiträge** zu nachgelagerten Projekten (Presidio? Anthropic-SDK?).
- **Conference-Talks** auf FOSDEM / RustConf / re:publica → "die Leute hinter Gaze sind echt".
- **Public Threat-Model** im Repo → "ihr habt drüber nachgedacht".

---

## Anti-Patterns (was **nicht** tun)

- ❌ **"Wir holen alles."** Geld + Zeit raus, kein Differenzierungs-Gewinn. ISO 27001 + 42001 + Pen-Test sind 90 % des Sales-Werts.
- ❌ **FedRAMP / Common Criteria zu früh.** 500k+ versenkt, 12+ Monate Aufwand, ohne dass Kunde wartet.
- ❌ **Selbst-erfundene Siegel.** "Gaze Certified Privacy" mit eigenem Logo wirkt amateurhaft.
- ❌ **Cert-Dauerbeschäftigung.** Mehr Audits ≠ mehr Kunden. Im Zweifel Sales statt Audit.
- ❌ **Audit ohne klares Sales-Ziel.** Kein "der Kunde wartet drauf"-Trigger → kein Cert kaufen.

---

## Wirkung pro Strategie-Option

Wie verschiebt sich die Cert-Priorität je nach gewähltem Pfad?

| Cert | Option 1 (Open-Core) | Option 2 (Laravel) | Option 3 (Compliance-Enterprise) |
|---|---|---|---|
| Phase A (OpenSSF, Sigstore, SBOM) | **Pflicht** — Trust-Signal für OSS | Hilfreich, weniger kritisch | Pflicht |
| Pen-Test public | Hoch — OSS-Glaubwürdigkeit | Optional | Pflicht |
| ISO 27001/27701 | Mittel — erst bei Enterprise-Tier | Niedrig — Self-Service-Kunden fragen nicht | **Pflicht** |
| ISO 42001 | Hoch — Marketing-Differenzierung | Mittel | **Pflicht** |
| SOC 2 Type II | Mittel — wenn US-Markt | Niedrig | Pflicht für US-Enterprise |
| BSI C5 / TISAX | Niedrig | Niedrig | Selektiv pro Branche |
| EuroPriSe | Optional | Niedrig | Mittel — DACH-DPO-Marketing |

---

## Offene Fragen

1. Welche Phase-A-Items setzen wir **vor** Public-Release des `gaze`-Repos um?
2. Wer macht intern Cert-Pflege? Externer Consultant ab Phase C?
3. Frage an Naoray: gibt's in der Laravel-Community Cert-Erwartungen, die wir ignorieren?
4. ISO 42001 — frühestmöglicher realistischer Termin? Auditoren listen lassen.
5. Pen-Test-Vendor: Cure53 (DE, exzellent, teuer) vs. Radically Open Security (NL, gut, günstiger) vs. NCC Group (UK, etabliert)?

---

## Caveat

Wir sind **keine Anwälte oder Auditoren**. Cert-Anforderungen ändern sich. Konkrete Wahl sollte mit Compliance-Berater + DPO + ggf. Anwalt geprüft werden, bevor Geld fließt.
