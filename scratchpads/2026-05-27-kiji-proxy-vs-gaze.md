# Kiji Proxy vs Gaze

Date: 2026-05-27

Compared:

- Gaze local repo at `/Users/krishankoenig/Workspace/EmpireTwo/gaze`
- Dataiku Kiji Privacy Proxy at `https://github.com/dataiku/kiji-proxy`, cloned shallowly to `/tmp/kiji-proxy`

## Kurzfazit

Kiji und Gaze verfolgen denselben Grundzweck: PII soll vor dem LLM-Provider lokal abgefangen, ersetzt und nach der Antwort wiederhergestellt werden. Die Produktform ist aber deutlich anders.

Kiji ist ein endnutzerorientierter Privacy Proxy mit Desktop-App, transparentem MITM/PAC-Browserpfad, Chrome Extension und einem ML-first ONNX-Detektor. Es ersetzt PII durch realistisch wirkende Dummy-Werte und restauriert diese ueber lokale Mapping-Tabellen.

Gaze ist ein agentic-first Pseudonymisierungsruntime: Rust-Library, CLI, MCP-Chokepoint, Dokument-Ingestion und API-key-basierter LLM-Proxy. Der Kern ist manifestbasierte, auditable, deterministische und reversible Tokenisierung. Neural/ML ist bei Gaze Defense-in-depth, nicht der mutierende Kern.

## Gemeinsamkeiten

- Beide laufen lokal und sollen verhindern, dass echte PII an externe AI APIs gesendet wird.
- Beide haben einen Proxy vor LLM-Provider-APIs.
- Beide unterstuetzen mehrere Provider:
  - Kiji: OpenAI, Anthropic, Gemini, Mistral, Custom Provider im Go-Backend.
  - Gaze: OpenAI, Anthropic, Gemini im `gaze-proxy`.
- Beide behandeln Request- und Response-Seite: vor dem Upstream maskieren/pseudonymisieren, danach restaurieren.
- Beide verwenden Provider-Adapter statt ein einziges generisches JSON-Verfahren.
- Beide nutzen das Kiji/DistilBERT/ONNX-Modell im weiteren Oekosystem:
  - Kiji als Hauptdetektor.
  - Gaze als optionaler observer-only SafetyNet-Backend.
- Beide haben lokale Modell-/Hash-/Signaturthemen:
  - Kiji hat `model/quantized/model_manifest.json` mit Datei-Hashes.
  - Gaze verifiziert gepinnte SafetyNet-Bundles und fail-closed bei Mismatch.
- Beide haben Tests fuer Provider-/Proxy-/PII-Flows.

## Zentrale Unterschiede

### 1. Product shape

Kiji:

- Go-Backend plus Electron/React-Frontend plus Chrome Extension plus Python-Modelltraining.
- Desktop-App fuer macOS und Standalone-Server fuer Linux.
- Automatische Browser-Konfiguration per PAC und transparenter MITM-Proxy.
- Chrome Extension warnt in ChatGPT/Claude/Gemini/Copilot/etc. vor dem Absenden.

Gaze:

- Rust-Workspace mit separaten Crates: Core, Types, Recognizers, Audit, Assembly, CLI, Document, MCP Core/RMCP, Proxy.
- Library-first und process-boundary integrations via CLI/daemon/MCP/proxy.
- Kein Desktop-UI- oder Browser-extension-first Produkt.
- Kein transparenter browserweiter MITM im aktuellen `gaze-proxy`; API-key SDK/Base-URL-Swap ist explizit der Scope.

### 2. Detection philosophy

Kiji:

- ML-first: ONNX DistilBERT Modell erkennt 26 PII-Typen.
- Customization laeuft ueber synthetische/annotierte Trainingsdaten, Label Studio, Metaflow Training und neues Modell.
- Gezielter Code-Check auf deterministic detection: Im aktuellen Backend liegt unter `src/backend/pii/detectors` nur das generische `Detector` interface plus `ONNXModelDetectorSimple`. `ModelManager` konstruiert ausschliesslich `NewONNXModelDetectorSimple`, und `MaskingService.MaskText` ruft nur `detector.Detect(...)` auf.
- Dependency-Check: `go.mod` enthaelt fuer Runtime-Erkennung `github.com/daulet/tokenizers` und `github.com/yalue/onnxruntime_go`; dazu SQLite, dotenv, Sentry, uuid, rate limiting. Keine offensichtliche Presidio-/scrubber-/validator-/phonenumbers-/stdnum-/Luhn-/Regex-Recognizer-Library ist im Go-Runtime-Graph eingebunden. Die Python-Abhaengigkeiten (`torch`, `transformers`, `sentencepiece`, `datasets`, `seqeval`, `pytorch-crf`, `onnxruntime` etc.) gehoeren zur Modell-/Training-/Benchmark-Pipeline, nicht zu einer deterministischen Runtime-Detector-Schicht im Go-Proxy.
- Caveat: `src/backend/config/README.md` beschreibt `use_regex_detectors`, `regex_config.custom_patterns` und `PII_USE_REGEX`, aber im aktuellen Repo wurden keine implementierenden Regex-Detector-Dateien, Builder oder Aufrufer gefunden. Das wirkt wie stale/aspirational documentation, nicht wie aktiver Detection-Code.
- Fehlerpfad in `MaskingService.MaskText`: wenn Detector nicht geladen ist oder Detection fehlschlaegt, wird der Originaltext unveraendert zurueckgegeben. Das ist fuer Gaze-Achse "fail closed" ein harter Unterschied.

Gaze:

- Deterministische Rule-/Regex-/Dictionary-/Validator-Detektoren sind der Trust-Floor.
- NER ist optional; SafetyNet ist observer-only nach Tokenisierung.
- Unknown validators/normalizers und SafetyNet-Striktheit koennen fail-closed gehen.
- Conflict resolution ist dokumentiert deterministisch: class priority, rule priority, score, span length, recognizer id.

### 3. Replacement and restore contract

Kiji:

- Ersetzt PII durch realistisch wirkende Dummy-Werte.
- Restore basiert auf `map[masked]original` und `strings.ReplaceAll` in der Antwort.
- Es gibt auch eine persistente SQLite Mapping DB plus in-memory Cache.
- Die Dummy-Werte koennen semantisch natuerlicher sein, aber das Restore ist string-substitution-basiert und nicht token-/manifestvertraglich abgesichert.

Gaze:

- Ersetzt PII durch session-scoped Tokens wie `<{session_hex}:Email_1>`.
- Restore laeuft ueber `Session`/`SensitiveSnapshot`/Manifest und Token-Pattern-Scan.
- Snapshot ist die einzige Restore-Quelle; Audit ist explizit nicht Restore-Quelle.
- Tokenemissionen sind auditable und an Recognizer/Version gebunden.

### 4. Proxy architecture and coverage

Kiji:

- Hat Forward/transparent proxy, CONNECT/MITM, CertManager, PAC server, system proxy configuration und CORS injection fuer browserbasierte API Calls.
- Intercept basiert auf Domains, z.B. `api.openai.com`.
- OpenAI-Provider-Code fokussiert sichtbar auf `/v1/chat/completions` und `messages[].content`.
- Hat Mistral und custom provider.

Gaze:

- `gaze-proxy` ist ein Axum/Reqwest pass-through fuer native Provider-Pfade.
- Matcht Pfade statt MITM-Domains: OpenAI `/v1/chat/completions`, `/v1/completions`, `/v1/responses`; Anthropic `/v1/messages`; Gemini `generateContent`, `streamGenerateContent`, `countTokens`.
- Adapter sammeln PII-bearing surfaces inklusive tool calls, function arguments, tool results, SSE deltas und Gemini function call/response args.
- Kein lokaler Auth-Boundary; Headers werden weitergereicht, loopback bind empfohlen.

### 5. Agentic workflow fit

Kiji:

- Stark fuer "user/browser/app sends prompt to AI service" und "desktop privacy proxy" positioniert.
- Chrome Extension deckt web-chat Eingaben vor dem Absenden ab.
- Weniger explizite Architektur fuer MCP/tool-call-source-system-Chokepoints.

Gaze:

- Agentic-first: MCP Core/RMCP, tool-call JSON, structured tool results, multi-turn Session isolation, document SafeBundle.
- Restore boundary wird als Vertrag zwischen Datenbesitzer und Agent/LLM modelliert.
- Custom recognizers/policies fuer tenant-spezifische IDs sind Teil der Hauptstory.

### 6. Audit, trust, and failure semantics

Kiji:

- SQLite Mapping/Logging fuer lokale App-/UI-Transparenz.
- Frontend zeigt Request Monitoring und minimale PII details.
- Fehler beim Speichern eines Mappings werden geloggt, Cache kann weiterlaufen.
- Detection-Fehler koennen originalen Text passieren lassen.

Gaze:

- Audit ist metadata-only, SQLite isoliert in `gaze-audit`.
- Core darf keine `rusqlite` dependency bekommen; Dylint schuetzt Modulgrenzen.
- Restore-Snapshot darf nicht zum LLM, in Logs oder Browser-Clients.
- SafetyNet-suspects enthalten backend/version/field path, aber keine raw bytes.

### 7. Model ownership and extensibility

Kiji:

- Bietet ein komplettes Modelltrainingssystem: synthetische Samples, HuggingFace dataset, Label Studio Review, Metaflow training, ONNX export/quantization.
- Erweiterung neuer PII-Typen bedeutet meist Labelschema + Training/Fine-tuning.

Gaze:

- Erweiterung neuer PII-Typen ist primaer policy/rulepack/custom recognizer.
- ML-Modelle koennen als NER oder SafetyNet eingebunden werden, aber sie definieren nicht den Restore-Vertrag.

## Was Gaze von Kiji lernen koennte

- Desktop-/Browser-Ergonomie: Kijis PAC/MITM/Chrome-extension Story ist deutlich naeher am nicht-technischen Endnutzer.
- Visual request monitoring: Kijis Electron UI macht lokale Schutzwirkung sichtbarer.
- Model customization workflow: Kiji hat eine umfassende Pipeline fuer Daten, Review, Training und Deployment eigener PII-Modelle.
- Mistral/custom provider support koennte als Vergleichspunkt fuer Gaze-Proxy-Adapter dienen.

## Was Kiji von Gaze lernen koennte

- Fail-closed semantics: Detector-Fehler sollten nicht still Originaltext weiterreichen.
- Manifest-first Restore: String-Replacement ueber Dummy-Werte ist weniger robust als session-scoped Tokens mit signiertem Snapshot.
- Deterministische, auditable Detection als Trust-Floor statt ML als mutierende Hauptinstanz.
- Tool-call/MCP/source-system Chokepoints fuer agentische Workflows.
- Metadata-only Audit-Trennung von Restore-Material.

## Source anchors

Gaze:

- `README.md`
- `ARCHITECTURE.md`
- `crates/gaze-proxy/README.md`
- `crates/gaze-proxy/src/adapter.rs`
- `crates/gaze-proxy/src/server.rs`
- `crates/gaze-proxy/src/adapters/{openai,anthropic,gemini}.rs`
- `crates/gaze/src/pipeline.rs`
- `crates/gaze/src/session.rs`
- `docs/architecture/safety-nets.md`

Kiji:

- `README.md`
- `docs/05-advanced-topics.md`
- `docs/06-chrome-extension.md`
- `docs/07-customizing-pii-model.md`
- `src/backend/pii/masking_service.go`
- `src/backend/pii/mapper.go`
- `src/backend/pii/detectors/onnx_model_detector.go`
- `src/backend/providers/{openai,anthropic,gemini,mistral,custom}.go`
- `src/backend/proxy/{handler,transparent,certmanager,pac_server,router}.go`
- `chrome-extension/content.js`
- `model/quantized/model_manifest.json`
