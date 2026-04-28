# PIInuts — Business-Positionierung (Diskussionsgrundlage)

**Datum:** 2026-04-28
**Status:** Entwurf, noch nicht entschieden

## Produktfamilie (Stand heute)

Alle Repos privat. Org: [PIInuts](https://github.com/orgs/PIInuts/repositories).

| Repo | Sprache | Rolle | Stand |
|---|---|---|---|
| `gaze` | Rust | Reversible PII-Pseudonymisierung, Workspace mit 7 Crates (`gaze-types`, `gaze`, `gaze-audit`, `gaze-recognizers`, `gaze-assembly`, `gaze-cli`, `xtask`) | v0.5.0 |
| `gaze-lens` | Rust | MCP-Server: Agent → Prod-DB/Logs Read-Access mit pseudonymisierten Ergebnissen, Replay lokal | v1.0 |
| `gaze-laravel` | PHP | Laravel-Adapter, Composer-Plugin, Pin auf `gaze` v0.4.5 | aktiv |
| `gaze-website` | JS | Marketing-Site | unklar |

**North Star (`gaze` README):** "Most reliable, reversible PII pseudonymization runtime for agentic workflows. Zero PII leaks between agent and data owner — ever."

**Differenzierung:**
- Reversibilität (GDPR Art. 4(5) Pseudonymisierung), nicht nur Redaction.
- Agentic-first (MCP-nativ via `gaze-lens`).
- Manifest + Audit-Trail → legal defensible.
- **Compliance-Beweis-Pipeline** → signierte Reports für DPO/Behörde, nicht nur Filter-Layer.

---

## Compliance-Hebel — warum der DPO zahlt

> **Kernthese:** Der stärkste Verkaufs-Hebel ist nicht "wir filtern PII raus" — sondern "**wir liefern den Nachweis, den der Datenschutzbeauftragte gegenüber Behörden braucht**, automatisch und signiert."
>
> Genau deshalb öffnet ein DPO Budget. Das Filter-Feature ist Mittel zum Zweck. Der **Beweis** ist das Produkt.

### GDPR-Nachweispflichten (Art. 5(2) "Accountability")

Verantwortlicher (= Kunde) muss **belegen** können, dass Datenverarbeitung GDPR-konform läuft. Nicht "wir machen das schon richtig" — Dokumentation auf den Tisch.

| Artikel | Pflicht | Was Gaze liefert |
|---|---|---|
| **Art. 5(2)** | Rechenschaftspflicht — Konformität nachweisen | Signierte Compliance-Reports (PDF + JSON) |
| **Art. 30** | Verzeichnis von Verarbeitungstätigkeiten (VVT) | Welche PII-Klassen, welche Recognizer-Versionen, welche LLM-Empfänger |
| **Art. 32** | Sicherheit der Verarbeitung — Pseudonymisierung explizit erwähnt | Beweis: Daten **wurden** pseudonymisiert, mit welchen Tokens, wann, durch welche Regel |
| **Art. 33/34** | Meldepflicht bei Datenpanne | Forensik: was hat Agent gesehen, war PII drin, wer hat restored |
| **Art. 35** | Datenschutz-Folgenabschätzung (DSFA) | Risiko-Bewertung der KI-Nutzung mit Pipeline-Architektur |

### Wer fragt wann

Drei typische Trigger:

1. **Routine-Audit** durch internen DPO (jährlich, oder vom Aufsichtsrat angeordnet).
2. **Behörden-Anfrage** (LfDI, BfDI, Landesdatenschutz) — routinemäßig oder nach Beschwerde / Vorfall.
3. **Kunden-Anfrage** (B2B-Vertrag mit Auftragsverarbeitung — Kunde will sehen, was Sub-Verarbeiter macht).

**Frist typisch: 30 Tage.** Wer Doku nicht hat → Bußgeld bis **4 % Konzernumsatz oder 20 Mio. €** (Art. 83). Diese Zahl ist der Schmerz, der Budget bewegt.

### Warum klassische App-Logs nicht reichen

Standard-Logs sind compliance-untauglich:

- **Nicht signiert** → manipulationsverdächtig, vor Gericht schwach.
- **Enthalten oft selbst PII** → schaffen neues Compliance-Problem statt es zu lösen.
- **Kein Pseudonymisierungs-Schema** → Auditor versteht Token-Mapping nicht.
- **Keine Reversibility-Manifest-Bindung** → Restore-Vorgänge nicht nachvollziehbar.

Auditor will sehen: "Token `<EMAIL_001>` entstand am 12.04. 14:23, durch Recognizer-Regel `eu.email.v3`, Mapping liegt verschlüsselt im Manifest Y, Restore-Befugnis hat Rolle Z." — vollständig **deterministisch nachvollziehbar**, ohne dass Auditor selbst PII sieht.

### Beispiel-Compliance-Report (Skizze)

Das, was Kunde am Quartalsende automatisch generiert und an DPO / Auditor mailt:

```
GDPR Compliance Report — Acme GmbH
Period: 2026-03-01 — 2026-03-31

[Art. 30 VVT-Auszug]
PII-Klassen verarbeitet: EMAIL, NAME_FULL, IBAN, PHONE_DE
Recognizer-Versionen: eu.core@v3.2, finance@v1.4
Empfänger (LLM-Provider): Anthropic Claude 4.7 (EU-Region)

[Art. 32 Pseudonymisierungs-Nachweis]
Total Sessions: 47.213
Total Tokens generated: 218.402
Reversal-Operations: 18 (durch Rolle "incident-responder")
Kein Klartext-Leak detektiert (Canary-Tests bestanden 100 %)

[Audit-Stichprobe]
Session ULID 01HZX...4F9
  - 12 Tokens emittiert
  - Recognizer-Quelle: eu.core regex.email_v3
  - Manifest-Signatur verifiziert: ed25519:abc123...
  - Original-Restore: nicht durchgeführt

[Anomalien]
Keine.

Signatur Bericht: ed25519:def456...
Erstellt: 2026-04-01T08:00:00Z
```

### Warum das ein Verkaufs-Hammer ist

- **DPO kennt Compliance sonst als Quartals-Albtraum.** Excel-Tabellen, manuelle Doku, Anwalts-Reviews. Wir liefern automatisch generierten, signierten Report → ihre Arbeit halbiert.
- **Schmerz beim Käufer = direkte Zahlungsbereitschaft.** Anders als "Tech-Schönheit" hat Compliance-Pain einen klar identifizierbaren Käufer mit Budget.
- **Audit-Pass = Vertragsverlängerung.** Ein Quartal mit erfolgreichem Behörden-Audit dank unserem Report → Kunde kündigt nie.
- **Differenzierung gegen Presidio / Tonic.** Die liefern Filter, kein Compliance-Beweis. Selbst wenn die morgen Reversal nachbauen — Audit-Pipeline + Manifest-Signaturen sind 12+ Monate Arbeit.

### Quer-Wirkung auf alle drei Strategie-Optionen

| Option | Wie Compliance-Hebel wirkt |
|---|---|
| **1 Open-Core** | Audit-Cloud ist Premium-Layer #1 (höchste Margen). Open-Source-Filter zieht Trust, Compliance-Layer zieht Geld. |
| **2 Laravel-Vertical** | Hebt ACV von ~588 €/Jahr (Pro-Tier) auf ~3000+ €/Jahr (Compliance-Pack). Mittelständische Laravel-Shops mit DPO sind Hauptziel, nicht Solo-Devs. |
| **3 Compliance-Enterprise** | **Ist** das Produkt. Lens + Audit-Cloud + Compliance-Reports + AVV-Vorlagen + DSFA-Templates = das, wofür Konzern 50–150k €/Jahr zahlt. |

### Erweiterungs-Pfade jenseits GDPR

Die Pipeline-Mechanik (signierter Audit-Trail + Manifest-Restore) ist regulierungs-agnostisch. Spiegelbildliche Märkte:

- **HIPAA** (US Health, 45 CFR § 164.312) → Health-Recognizer-Pack + HIPAA-Report-Variante.
- **EU AI Act** (ab 2026 in Stufen) → Logging-Pflichten für Hochrisiko-KI-Systeme, Art. 12.
- **PCI-DSS** (Zahlungsdaten) → Finance-Recognizer + PCI-Audit-Layer.
- **BaFin / MaRisk** (DE Finanz-Aufsicht) → KI-Governance-Anforderungen.

Jede neue Regulierung = neues Recognizer-Pack + neuer Report-Template = neuer Premium-SKU.

### Caveat

Wir sind **keine Anwälte**. Konkrete Auslegung von GDPR-Artikeln muss DPO / Anwaltskanzlei machen. Aber Architektur + Pipeline-Design sind genau auf diese Pflichten ausgelegt. TÜV-/Zertifizierungs-Stempel kommt später — Substanz ist da.

---

## Wichtiges Grundprinzip — wo Gaze läuft

Egal welche Option wir wählen: **Gaze läuft immer beim Kunden.** Nicht bei uns.

### Warum nicht "Gaze als Cloud-API"?

Gaze sitzt zwischen KI-Agent und Kundendaten. Wenn echte PII (Namen, Mails, Adressen) erst zu uns auf einen Server fliegen müsste:

1. **Kunde will das nicht.** Datenschutzbeauftragter sagt nein. GDPR-Auftragsverarbeitung-Vertrag, Audit-Recht, EU-Hosting — Riesen-Theater.
2. **Latenz.** Jeder LLM-Call doppelt durchs Netz → langsam.
3. **Wir wären selbst Datenrisiko.** Ein Hack bei uns = alle Kunden-PII weg. Will niemand haftbar sein.
4. **Widerspricht eigenem Versprechen.** "PII verlässt nie deine Infrastruktur" — wenn's auf unseren Server geht, stimmt das nicht mehr.

**Konsequenz:** `gaze` Binary läuft on-prem / in der Kunden-Cloud / im Kunden-Container. **PII bleibt dort.**

### Wo kommt dann das Geld her? — Drei Hebel

Diese drei Hebel funktionieren in **jeder** Option (1, 2, 3) — nur die Mischung ändert sich.

#### Hebel A — Lizenz / Subscription für die Software selbst

Kunde lädt Gaze runter, installiert lokal, braucht aber einen **License-Key** zum Starten.

```
gaze clean --license-key=ABC123...
```

- Ohne Key → Binary läuft nicht (oder nur Free-Tier).
- Mit Key → Pro-Features, Updates, Support.
- Kunde zahlt z.B. **500 €/Monat pro Server** oder **pro Entwickler-Seat**.

**Beispiele real:** GitLab, Sentry, Cal.com — alle so. Software beim Kunden, Lizenz bei der Firma.

#### Hebel B — Premium-Recognizer-Pakete

Gaze erkennt PII über "Recognizer" (= Regeln, was als PII gilt).

- **Free:** Basis-Recognizer (Email, Telefon, IBAN, Standard-EU)
- **Premium:** Branchen-Pakete:
  - Health-Pack (HL7, ICD-10, Patientennummern) → Krankenhäuser
  - Finance-Pack (SWIFT, Steuernummern aller EU-Länder, KYC-Felder) → Banken
  - Telco, Versicherung, Public-Sector …

Kunde zahlt Abo, lädt Pakete runter, läuft lokal weiter. **Wir verkaufen Wissen, nicht Server-Kapazität.**

#### Hebel C — Audit-Cloud (optional, Push-Modell)

**Einziger** Teil, der bei uns liegt — und nur **Logs**, **keine PII**.

Gaze produziert Audit-Logs lokal beim Kunden ("Heute 12:34 hat Agent X Token `<EMAIL_001>` gesehen"). Keine echten Mails! Nur Token + Metadaten.

Kunde pusht diese anonymen Audit-Logs zu uns → wir bauen **Compliance-Reports** ("für GDPR-Audit nächsten Monat: 47.000 Agent-Zugriffe, alle pseudonymisiert, hier PDF-Export für den Auditor").

→ Kunde zahlt für Cloud-Service, der ihm Behörden-Papier spart.

### Pricing-Modell — Usage-Based mit Budget-Cap (orthogonal zu Optionen 1/2/3)

Idee: Subscription-Modell wie OpenAI / Anthropic — Kunde definiert monatliches Budget, zahlt nach tatsächlichem Verbrauch, mit Self-Service-Cap.

Funktioniert in Kombination mit **allen** drei Strategie-Optionen — ist nicht entweder/oder, sondern **wie** wir abrechnen, nachdem die Strategie steht.

#### Pro

- **Value-aligned.** Mehr verarbeitete PII = mehr Risiko-Reduktion = mehr Wert.
- **Niedrige Einstiegshürde.** Dev startet mit 20 €/Monat, wächst rein. Kein Sales-Call.
- **Bekanntes Muster.** Jeder Dev kennt OpenAI/Anthropic-Billing. Null Erklärbedarf.
- **Audit-Cloud sowieso da.** Counter-Pings = minimaler Mehraufwand.
- **Self-Service-Skalierung.** Kunde wächst alleine, ohne Sales-Engineer.
- **Frühe Churn-Signale.** Usage-Drop sichtbar bevor Kündigung.

#### Stolpersteine

##### 1. Air-Gapped Enterprise sagt nein
Banken, Krankenhäuser, BSI-Kunden haben **keine Internet-Verbindung** im Production-Pfad. Online-Counter-Sync = Ausschluss.

**Lösung:** Hybrid — Cloud-Tier (Pro/Team/Business) Usage-Based online, Enterprise-Tier mit Offline-Lizenz + fixem Volumen-Pool, optional Counter-Sync per File-Drop.

##### 2. Privacy-Paradox

Wir verkaufen "PII verlässt nie deine Infra." Wenn Counter zur Cloud — was genau zählen wir?

| Was zählen | Privacy-Status | OK? |
|---|---|---|
| Bytes / Tokens verarbeitet | neutral | ✅ ja |
| Anzahl Sessions / Aufrufe | neutral | ✅ ja |
| Anzahl PII-Treffer (aggregiert) | grenzwertig | 🟡 grob, keine Klasse pro Match |
| PII-Klassen-Histogramm | aussagekräftig | 🟡 OK in Audit-Cloud, nicht Billing |
| Echte Token-Strings | Bruch des Versprechens | ❌ niemals |

**Regel:** Bill nach **Throughput** (Tokens / Bytes), nicht nach Match-Anzahl. Sonst hat Kunde Anreiz, PII zu verstecken — paradox zum Produkt.

##### 3. Was tun bei Cloud-Ausfall / Offline-Kunde

| Modell | Verhalten | Risiko |
|---|---|---|
| Hard-Fail | Binary verweigert Service ohne Cloud | Production-Killer. **Tabu.** |
| Soft-Fail | Läuft weiter, billed nachträglich | Manipulation, Trust-Risiko |
| Pre-Paid Buckets + Grace | Lokaler Counter, periodisch sync, 7d Grace | Best Practice |

**Empfehlung:** Pre-Paid Buckets + 7-Tage Grace-Period.

##### 4. Predictability für Enterprise

Procurement **hasst** variable Bills. Wollen Capex-Budget für 1 Jahr fest.

**Lösung:** Volumen-Commitment-Vertrag — "100M Tokens/Jahr für 50k €", Overage zu vereinbartem Preis.

##### 5. Counter-Manipulation

- Counter signiert per License-Key, Cloud verifiziert Signatur.
- Audit-Logs unabhängig vom Billing-Counter → Cross-Check.
- Manipulation = Vertragsbruch, nachweisbar.

##### 6. Eigene Cloud-Kosten

Mehr Usage → mehr Audit-Storage / DB / Compute bei uns. Faustregel: Cloud-Kosten ≤ 20 % der Subscription-Revenue.

#### Bill-Unit (Empfehlung)

**1k Input-Tokens** (alternativ 1 KB UTF-8).

- Konsistent mit LLM-Welt → Kunde versteht sofort.
- Linear skalierbar, deterministisch.
- Kein Anreiz, PII zu verstecken (Counter unabhängig von Match-Rate).

#### Pricing-Sketch (Hypothese, nicht final)

| Tier | Base / Monat | Inkludiert | Overage | Online-Pflicht |
|---|---|---|---|---|
| Free | 0 € | 100k Tokens, Basis-Recognizer | n/a | optional |
| Pro | 49 € | 5M Tokens, Email-Support | 0,02 € / 1k | ja (7d Grace) |
| Team | 199 € | 30M Tokens, 1 Premium-Pack, Priority | 0,015 € / 1k | ja (7d Grace) |
| Business | 999 € | 200M Tokens, alle Premium-Packs, SLA | 0,010 € / 1k | ja (7d Grace) |
| Enterprise | Custom | Volumen-Commit, On-Prem-Audit, Air-Gap-Lizenz | per Vertrag | optional |

**Budget-Cap im Dashboard** ("max 500 €/Monat"):
- Hard-Stop (sicher, aber Outage-Risiko)
- Soft-Warn (Mail ab 80 %, Throttle ab 100 %)
- Auto-Upgrade (zustimmungspflichtig)

#### Tech-Aufwand für Usage-Based-Billing

- Lokaler Token-Counter im Binary (Pipeline durchläuft eh)
- Signierter Counter-Push (stündlich + bei Shutdown)
- Cloud-Side Quota-Engine (Stripe Metered Billing / Lago / Eigenbau)
- Self-Service-Dashboard mit Budget-Cap
- 7-Tage Offline-Grace (lokaler Buffer, Sync auf Reconnect)
- Stripe / Paddle Integration

#### 3-Schichten-Modell (Verdict)

1. **Free** → Adoption / Trust-Building
2. **Self-Service Pro / Team / Business** → Usage-Based mit Budget-Cap (Kern-Hebel für Bottom-Up-Wachstum)
3. **Enterprise** → Volumen-Commit-Vertrag + Air-Gap-Option

Funktioniert in Kombination mit Option 1, 2 oder 3 — Strategie-Wahl bleibt offen.

---

### Typische Kunden-Topologie

```
[Kunden-Server / Kunden-Cloud]
  └─ Kunden-App (Laravel / Python / Node / Go)
      └─ Adapter (gaze-laravel, gaze-py, ...)
          └─ ruft `gaze` Binary lokal auf  ← läuft hier, sieht PII
              ├─ License-Check beim Start (online, einmal/Tag)
              ├─ Recognizer-Pakete (lokal, signiert)
              └─ optional: pusht Audit-Metadaten an [PIInuts Audit-Cloud]
                                                    ↑
                                       NUR hier sind wir im Bild —
                                       und zwar ohne PII.
```

---

## Positionierungs-Optionen

### Option 1 — Open-Core (Empfehlung)

#### Schulkind-Erklärung 🍫

Stell dir vor, du verkaufst **Schokoriegel**.

Du gibst die **Schokolade gratis raus**. Jeder darf sie essen, kopieren, weitergeben. Klingt blöd? Ist es nicht — denn:

- Alle Welt sieht: "Krass, die Schokolade ist gut, da ist nix Komisches drin." → **Vertrauen**
- Leute reden drüber, posten auf Reddit → **kostenlose Werbung**
- Andere Firmen probieren's aus → **manche kommen wieder und wollen mehr**

Geld machst du dann mit den **Extras** drumherum:
- Hübsche Geschenkbox (= `gaze-audit` Compliance-Reports)
- Lieferservice nach Hause (= gemanagte Cloud)
- Schoki mit Logo der Firma drauf (= Enterprise-Features wie Login, Rechte-Verwaltung)

**Warum bei Sicherheits-Tools so wichtig:** Würdest du einem **Türschloss** vertrauen, das niemand reingucken darf? Nein. Du willst sehen "ja, Schloss ist gut gebaut, kein heimlicher Zweitschlüssel drin." Genau so bei PII-Filtern für KI: Firmen kaufen das nur, wenn sie reingucken dürfen.

**Nachteil:** Jemand könnte unsere Schokolade nehmen, eigenen Riegel draus machen, uns Konkurrenz machen. Aber: die **Box, der Lieferservice und das Logo** kann er nicht so einfach kopieren — da liegt unser Geld.

#### Modell technisch

- **Öffentlich (MIT / Apache-2.0):** `gaze`, `gaze-types`, `gaze-recognizers` (Basis-Pakete), `gaze-cli`, `gaze-assembly`.
- **Kommerziell (proprietäre Lizenz):**
  - `gaze-audit` Compliance-Pack (gemanagte Reports, Retention, Export)
  - `gaze-lens` Enterprise (RBAC, SSO, Multi-Tenant, gemanagte Recognizer-Updates)
  - Premium-Recognizer-Pakete (Health, Finance, Telco, …)
  - Cloud-Audit-Sink (SaaS)

#### Geld-Hebel-Mix

| Hebel | Anteil | Beispiel |
|---|---|---|
| A — Lizenz | mittel | Lens Pro: 49 €/Seat/Monat |
| B — Recognizer | hoch | Finance-Pack: 800 €/Monat |
| C — Audit-Cloud | mittel | 0,10 €/GB ingest |
| Support / SLA | niedrig | 1.500 €/Monat Pro-Support |

#### Why
- Security-Tools ohne Auditierbarkeit verkaufen sich nicht. Niemand traut Closed-Source PII-Filter im LLM-Pfad.
- Moat ist nicht der Code, sondern Reversibility-Contract + Recognizer-Pakete + Audit-Workflow.
- OSS = Trust + SEO + Design-Partner-Pipeline.

#### Tradeoff
- Forks möglich. Mitigation: Recognizer-Pakete + Compliance-Reports + Lens-MCP-Tooling als Premium-Layer.
- Erfordert Community-Investment (Issues, Releases, Docs, Tap, Crates.io).

---

### Option 2 — Vertical-SaaS Laravel/PHP

#### Schulkind-Erklärung 🥨

Stell dir vor, du verkaufst **Brezeln**, aber **nur in Bayern**.

Bayern liebt Brezeln. Du kennst dort schon viele Bäcker (= Naoray ist im Laravel-/PHP-Land bekannt). Die kaufen **sofort**, du brauchst keinen großen Werbe-Spruch.

`gaze-laravel` wird Hauptprodukt. Wir verkaufen **nur** an PHP-/Laravel-Firmen. Schnell Geld, klare Kunden.

**Wichtig zur Lieferung:** Auch hier liegt Gaze **beim Kunden**, nicht bei uns. `gaze-laravel` (das Composer-Package) ist nur **Glue-Code**, der die `gaze`-Binary lokal aufruft. Der Kunde lädt:

1. `composer require naoray/gaze-laravel` → das PHP-Package
2. Composer-Plugin lädt automatisch `gaze` Binary nach `vendor/bin/`
3. Binary prüft License-Key online (einmal/Tag)
4. PHP-App schickt Texte → Binary cleant lokal → Pseudonyme zurück

**Nachteil:** Bayern ist klein. Du verkaufst keine Brezeln in Hamburg, Berlin, USA. Die ganze tolle Rust-Maschine dahinter (`gaze`-Core) bringt nix, wenn nur Laravel-Leute sie nutzen. Wenn morgen jemand in Python oder Java ein KI-Tool baut — **kann er uns nicht kaufen**, weil wir nur PHP machen.

#### Modell technisch

- Alles **closed source**, `gaze-laravel` als Hero-Produkt.
- `gaze` Binary nur als signiertes Release-Artifact (kein Code-Zugang).
- Distribution: Composer-Plugin + Lizenz-Server.

#### Geld-Hebel-Mix

| Hebel | Anteil | Beispiel |
|---|---|---|
| A — Lizenz | **dominant** | 99 €/Monat pro App, 299 €/Monat pro Multi-App |
| B — Recognizer | niedrig (PHP-Welt eher generisch) | EU-Pack inkludiert |
| C — Audit-Cloud | niedrig | optional |
| Support | mittel | Forum gratis, Priority-Support 500 €/Monat |

#### Why
- Schneller Revenue, klarer ICP (Laravel-Shops mit AI-Features).
- Naoray hat bereits Reichweite in der Community.
- Geringe Sales-Komplexität (Self-Service, Stripe-Checkout reicht).

#### Tradeoff
- TAM klein (vermutlich 50–500 zahlende Kunden Maximum).
- Rust-Core-Investment unausgeschöpft.
- Kein Hebel für Java/Python/Node/Go — riesiger Markt liegt brach.
- Bei Closed-Source: Vertrauensproblem für Security-Tool ("wieso soll ich das in meinen LLM-Pfad lassen?").

---

### Option 3 — Compliance-Plattform Enterprise

#### Schulkind-Erklärung 🏦

Stell dir vor, du verkaufst **Tresore an Banken**.

Banken haben **viel Geld** und **brauchen unbedingt** sichere Tresore (Gesetz!). Du verkaufst **einen** Tresor → kassierst sehr viel.

Wir gehen zu großen Firmen (Versicherungen, Krankenhäuser, Banken). Wir reden mit dem **Datenschutzbeauftragten** — das ist der, der sagen muss "ja, wir dürfen KI auf Kundendaten loslassen, weil GDPR/HIPAA passt." Der hat Budget. Der zahlt **viel** pro Vertrag.

**Wichtig zur Lieferung:** Auch bei Enterprise liegt Gaze **beim Kunden** — sogar **noch strenger**: oft komplett air-gapped, ohne Internet-Lizenz-Check (Offline-Lizenzen mit Hardware-Bindung). Was Enterprise zusätzlich kauft:

- **`gaze-lens` Enterprise** als MCP-Server beim Kunden, mit SSO (Okta, Azure AD), RBAC, Multi-Tenant.
- **Compliance-Cloud** mit Reports, Retention, Auditor-Login (oder On-Prem-Variante davon).
- **Support-Vertrag** (24/7, SLA mit Strafen bei Verletzung, dedizierter Engineer).
- **Onboarding-Workshops** (= Beratung, gut bezahlt: 2.000 €/Tag).
- **Legal-Pakete:** AVV-Vorlagen, Data-Processing-Agreements, ISMS-Mapping.

**Nachteil:**
- **Banken kaufen langsam.** 6–12 Monate Verhandlungen, Anwälte, Prüfungen, Meetings, bis ein Vertrag steht. Du musst ein Jahr ohne Geld überleben.
- Du brauchst selbst **Zertifikate** (SOC2, ISO27001) — kostet 50.000–200.000 € und dauert Monate. Banken kaufen sonst nicht.
- **Frühphase ungeeignet ohne Geld im Rücken** (Investoren). Bootstrap geht da nicht.

#### Modell technisch

- `gaze-lens` Enterprise als Lead-Produkt.
- Kern (`gaze`) kann offen oder closed sein (für Enterprise sekundär — die wollen Vertrag, nicht Code).
- Audit-Cloud ist Pflicht-Bestandteil, oft als On-Prem-Variante zusätzlich.

#### Geld-Hebel-Mix

| Hebel | Anteil | Beispiel |
|---|---|---|
| A — Lizenz | **dominant** | Enterprise-Lizenz: 30k–150k €/Jahr |
| B — Recognizer | mittel | Branchen-Pakete inkludiert oder +20–50k |
| C — Audit-Cloud | mittel-hoch | inkludiert in ACV oder On-Prem-Add-on |
| Support / SLA | hoch | 20–30 % der Lizenz, 24/7 Premium |
| Beratung | mittel | Onboarding 20–50k pro Projekt |

ACV (Annual Contract Value) bei Enterprise typisch **30k–150k €**. 20–50 Kunden reichen für Series-A-Story.

#### Why
- Höchste ACVs.
- Datenschutz-Officer ist klarer Käufer mit Budget und Schmerzen.
- KI-Compliance ist 2026 ein erzwungener Markt (EU AI Act, BSI, BaFin).

#### Tradeoff
- Lange Sales-Cycles (6–12 Monate).
- Erfordert SOC2 / ISO27001 — Cert-Audit teuer (50k–200k).
- Frühphase ungeeignet ohne Funding.
- Kunden-Akquise erfordert Sales-Engineer und Branchen-Netzwerk.

---

## Konkurrenz

| Player | Was sie machen | Lücke, die wir füllen |
|---|---|---|
| Microsoft Presidio (OSS) | PII-Detection | Kein Reversal, kein Manifest, kein Agent-Fokus |
| Tonic.ai | Synthetic Data, Vault | Batch-Anonymisierung, nicht inline im Agent-Pfad |
| Skyflow | Data-Vault-API | Vault-Pattern, kein MCP, kein lokales Replay |
| Privacera | Data-Governance | Enterprise-heavy, kein Agent-First |

**Eindeutige Differenzierung:** reversibel + agentic-first + MCP-nativ. Gibt's so nicht am Markt.

---

## Cheat-Sheet — schnelle Orientierung

| Option | Zeit bis erstes Geld | € pro Kunde / Jahr | Kunden-Zahl realistisch | Risiko | Funding nötig |
|---|---|---|---|---|---|
| 1 Open-Core | 3–6 Monate | 5k–50k | hunderte–tausende | mittel | Bootstrap möglich |
| 2 Laravel-Vertical | 1–3 Monate | 1k–10k | 50–500 | gering | Bootstrap |
| 3 Compliance-Enterprise | 9–18 Monate | 30k–150k | 20–100 | hoch | VC nötig |

**Hybrid denkbar:** Open-Core (Option 1) als Basis, **plus** Laravel-Adapter (Option 2 als Bottom-up-Land) **plus** Lens-Enterprise (Option 3 als Top-Down) — auf einer gemeinsamen Code-Basis. Das ist genau das, was die aktuelle Repo-Struktur schon hergibt.

---

## Offene strategische Fragen

1. **Funding-Pfad:** Bootstrap → Vertical-SaaS sinnvoll. VC → Compliance-Plattform sinnvoll. Open-Core funktioniert in beiden Welten.
2. **Repo-Sichtbarkeit:** Wann `piinuts/gaze` public? Vor oder nach erstem Design-Partner?
3. **Naoray-Rolle:** Co-Founder, Advisor, oder reiner Laravel-Champion?
4. **Lizenz für Kern:** MIT (max. Adoption) vs. Apache-2.0 (Patent-Grant) vs. BSL (Commercial-Schutz mit zeitversetztem OSS)?
5. **License-Key-Infrastruktur:** Wer baut den Lizenz-Server? Off-the-shelf (Keygen.sh, Cryptolens) vs. Eigenbau?
6. **Pricing-Hypothesen:**
   - Core: gratis
   - `gaze-lens` Pro: $X / Seat / Monat
   - Audit-Cloud: $Y / GB ingest
   - Recognizer-Pakete: $Z / Paket / Monat
7. **Erste 2 Design-Partner:** Profile? Wo finden?

## Nächster Schritt — Co-Founder-Gespräch

Bevor wir weiter ausarbeiten: **Konvergenz mit Naoray suchen.** Risiko sonst: stundenlang Welt bauen, die er ganz anders sieht. Besser jetzt billig falsifizieren.

### Agenda (60 min)

Reihenfolge bewusst — Bauchgefühl **vor** Argumenten, sonst Anchoring.

1. **Bauchgefühl pro Option (1 / 2 / 3)** — ungefiltert, vor jeder Diskussion
2. **Roter Faden:** wem wollen wir in 3 Jahren morgens beim Aufstehen helfen — Solo-Devs, Mittelstand, Konzerne? *(Diese Frage entscheidet ~70 % des Rests.)*
3. **Funding-Realität:** Bootstrap, Side-Project, oder VC anpeilen?
4. **Naoray-Commitment:** Vollzeit, Teilzeit, Advisor?
5. **Repo-Sichtbarkeit:** `piinuts/gaze` public ja/nein — Trust-Gewinn vs. Code-Schutz, sein Standpunkt?
6. **Hybrid-Pfad:** ist die Kombi 1+2+3 auf gemeinsamer Code-Basis attraktiv oder zu zerfasert?
7. **Usage-Based-Pricing:** OpenAI-/Anthropic-Style-Subscriptions mit Budget-Cap — Bauchgefühl?

### Was im Gespräch **nicht** tun

- Keine Pricing-Zahlen festlegen.
- Keine Lizenz-Wahl entscheiden.
- Keinen "wir starten Option X heute"-Beschluss erzwingen.

### Ziel des Gesprächs

**Konvergenz auf 1–2 Optionen** — nicht finaler Plan. Eine oder zwei Optionen rausstreichen ist Erfolg.

### Nach dem Gespräch (Doc-Update)

- Was war **Konsens**
- Was war **Dissens** (wichtig — nicht wegputzen, ehrlich dokumentieren)
- Welche Fragen sind entschieden, welche neu aufgekommen
- 1 oder 2 Optionen markieren als "abgewählt" (nicht löschen — Rationale für später)

---

## Folge-Schritte (NACH Co-Founder-Alignment, nicht vorher)

Erst angehen, wenn Richtung steht. Sonst dreifacher Aufwand.

- [ ] `piinuts/gaze` public + Public-Homebrew-Tap → Trust + SEO *(nur falls Option 1 oder Hybrid)*
- [ ] gaze-website Landing: **eine** Botschaft ("GDPR-safe AI agents on production data"), nicht drei
- [ ] Design-Partner-Pipeline: 2 Laravel-Shops (via Naoray) + 2 Rust/Go-Teams (Lens-MCP)
- [ ] Pricing-Test mit Design-Partnern
- [ ] Lizenz-Entscheidung (Code-Lizenz + License-Key-Modell) dokumentieren
- [ ] License-Server-Tooling evaluieren (Keygen.sh / Cryptolens / Eigenbau)
