# Gaze — Laravel Integration (v0.3 Pipe Mode)

**Status:** Roadmap / targets Gaze v0.3 — pipe mode is not shipped in v0.1. This document describes the planned integration surface so the v0.1 anonymizer core can be designed to support it without rework.

---

## Scope

This guide shows how to use Gaze as a PII-anonymization layer inside a Laravel application that calls an LLM (OpenAI, Anthropic, local model) on user-submitted data. The canonical example is **Ghostwriter-style inbound email AI**: an incoming email arrives, the app drafts a reply with an LLM, the reply is sent back to the user. Gaze sits in front of the LLM to strip PII on the way in and restore it on the way out.

The same pattern applies to any Laravel feature that passes user content through an LLM: chat assistants, summarization, classification, categorization — anything where the prompt contains PII and the response must contain the real names and addresses.

## Architecture

```
┌──────────────┐      ┌──────────────────────────────────────────────┐
│  Inbox /     │      │  Laravel                                     │
│  Webhook     │──────▶                                              │
│  (IMAP, API) │      │  ┌──────────────┐        ┌────────────────┐  │
└──────────────┘      │  │ GhostwriterService   │ Gaze facade     │  │
                      │  │              │◀──────▶│ (Process::run) │  │
                      │  │  1. gaze clean         │                │  │
                      │  │  2. LLM call           │  gaze clean    │  │
                      │  │  3. gaze restore       │  gaze restore  │  │
                      │  └──────┬───────┘        └───────┬────────┘  │
                      │         │                        │           │
                      │         ▼                        ▼           │
                      │  ┌──────────────┐        ┌────────────────┐  │
                      │  │ Queue        │        │ gaze binary    │  │
                      │  │ (Redis/DB)   │        │ (Rust subproc) │  │
                      │  │ Encrypted    │        └────────────────┘  │
                      │  │ session blob │                            │
                      │  └──────────────┘                            │
                      └──────────────────────────────────────────────┘
                                        │
                                        ▼
                              ┌────────────────────┐
                              │  OpenAI / Anthropic │
                              │  (receives CLEAN    │
                              │   text only)        │
                              └────────────────────┘
```

Two Gaze invocations per request, one LLM call in between. Session state travels with the request as an encrypted blob; nothing lives on the Gaze side between calls.

## Prerequisites

- Gaze v0.3 binary on the Laravel server's `PATH` (install via `cargo install gaze` or the Homebrew tap).
- A `policy.toml` in the Laravel project root (or configurable via `config/gaze.php`).
- Laravel 11+ (for the `Process` facade's piping support).
- `APP_KEY` set to a strong random value — used to encrypt session blobs in flight.

## Installing the Wrapper Package

The Gaze wrapper for Laravel is a thin package — not a reimplementation. It shells out to the `gaze` binary and manages session-blob serialization and encryption.

```bash
composer require gaze/laravel
```

```bash
php artisan vendor:publish --tag=gaze-config
```

This publishes `config/gaze.php`:

```php
return [
    'binary' => env('GAZE_BINARY', 'gaze'),
    'policy_path' => env('GAZE_POLICY_PATH', base_path('policy.toml')),
    'timeout_seconds' => env('GAZE_TIMEOUT', 30),
    'fail_closed' => env('GAZE_FAIL_CLOSED', true),
];
```

## The `Gaze` Facade

```php
// vendor/gaze/laravel/src/Gaze.php
namespace Gaze\Laravel;

use Illuminate\Support\Facades\Process;

class Gaze
{
    public function __construct(
        private string $binary,
        private string $policyPath,
        private int $timeoutSeconds,
        private bool $failClosed,
    ) {}

    /** Anonymize raw text. Returns clean text + an opaque session blob. */
    public function clean(string $rawText): GazeSession
    {
        $result = Process::timeout($this->timeoutSeconds)
            ->input($rawText)
            ->run([
                $this->binary,
                'clean',
                '--format=json',
                '--policy=' . $this->policyPath,
            ]);

        if ($result->failed()) {
            throw $this->buildException('gaze clean', $result);
        }

        $payload = json_decode($result->output(), flags: JSON_THROW_ON_ERROR);
        return new GazeSession(
            cleanText: $payload->clean_text,
            blob: $payload->session_blob,
        );
    }

    /** De-anonymize a response using a session blob from a prior clean(). */
    public function restore(GazeSession $session, string $llmResponse): string
    {
        $input = json_encode([
            'session_blob' => $session->blob,
            'text' => $llmResponse,
        ], JSON_THROW_ON_ERROR);

        $result = Process::timeout($this->timeoutSeconds)
            ->input($input)
            ->run([
                $this->binary,
                'restore',
                '--format=json',
            ]);

        if ($result->failed()) {
            throw $this->buildException('gaze restore', $result);
        }

        $payload = json_decode($result->output(), flags: JSON_THROW_ON_ERROR);
        return $payload->text;
    }

    /**
     * Build a GazeException without leaking stderr content into the exception message.
     *
     * Gaze's Rust binary is expected to actively sanitize its own stderr (see
     * the spec's "Active error sanitization" section), but this wrapper adds a
     * second line of defense: we never forward raw stderr into an exception
     * message that might end up in `failed_jobs.exception`, Sentry, Horizon,
     * or a Laravel log line. Instead we store a SHA-256 hash of stderr so
     * operators can correlate failures without exposing content.
     */
    private function buildException(string $stage, ProcessResult $result): GazeException
    {
        $stderrHash = hash('sha256', $result->errorOutput() ?: '');
        Log::warning("{$stage} failed", [
            'exit_code' => $result->exitCode(),
            'stderr_sha256' => $stderrHash,
        ]);
        return new GazeException(
            "{$stage} failed (exit={$result->exitCode()}, stderr_sha256={$stderrHash})",
            $result->exitCode(),
        );
    }
}
```

The key rule: `Process::errorOutput()` never appears in an exception message or a log entry — only its SHA-256. If a developer needs to debug a specific failure, they correlate the hash against the Gaze binary's own logs (which live inside Gaze's sanitized audit log). This prevents a chain like: Gaze emits a stderr line with a sanitization bug → Laravel wrapper throws with that line in the exception message → Laravel's exception handler writes it to `failed_jobs.exception` → Horizon renders it → operator sees PII in the dashboard.

```php
// vendor/gaze/laravel/src/GazeSession.php
namespace Gaze\Laravel;

final class GazeSession
{
    public function __construct(
        public readonly string $cleanText,
        public readonly string $blob,
    ) {}
}
```

## Encryption-in-Flight — mandatory

The `GazeSession::blob` contains a plaintext JSON mapping of anonymized tokens back to real PII. While the Laravel worker holds it in memory, that is acceptable — the worker already processed the raw email. But as soon as the blob enters a **queue payload**, **cache entry**, **log line**, or **failed-jobs table**, it must be encrypted.

This step is **mandatory, not best-practice**. Gaze deliberately does not sign the blob itself; integrity comes from Laravel's AEAD envelope. A plaintext blob sitting in Redis is simultaneously a confidentiality bug (raw PII map exposed) and an integrity bug (a tampered blob produces wrong restores with no detection). Skipping encryption does not just weaken privacy — it breaks correctness.

Laravel's built-in `Crypt::encryptString()` (AES-256-CBC + HMAC-SHA256, keyed on `APP_KEY`) is exactly the right primitive: it authenticates the ciphertext, so `Crypt::decryptString` fails *before* a tampered blob reaches `gaze restore`.

```php
use Illuminate\Support\Facades\Crypt;

// When storing the session blob in any persistence layer:
$encryptedBlob = Crypt::encryptString($session->blob);

// When reading it back before restore():
$plaintext = Crypt::decryptString($encryptedBlob);
$session = new GazeSession('', $plaintext); // cleanText not needed for restore
```

**Rule of thumb:** if the blob is about to leave the current function's local variable scope, encrypt it first.

### Optional: dedicated `GAZE_ENCRYPTION_KEY`

For teams that rotate `APP_KEY` frequently, or that want to scope blob encryption independently from session-cookie / general-purpose encryption, Laravel supports additional encrypter instances. Configure a second key in `config/gaze.php`:

```php
return [
    'binary' => env('GAZE_BINARY', 'gaze'),
    'policy_path' => env('GAZE_POLICY_PATH', base_path('policy.toml')),
    'timeout_seconds' => env('GAZE_TIMEOUT', 30),
    'fail_closed' => env('GAZE_FAIL_CLOSED', true),
    'blob_encryption_key' => env('GAZE_ENCRYPTION_KEY'), // base64-encoded 32 bytes, optional
];
```

Validate the key at boot (inside the service provider's `boot()` method) before any job can use it — a misconfigured key must fail loudly, not silently fall back:

```php
use Illuminate\Encryption\Encrypter;

public function boot(): void
{
    $raw = config('gaze.blob_encryption_key');

    if ($raw === null || $raw === '') {
        // No dedicated key configured — fall back to Laravel's default `Crypt` facade.
        return;
    }

    $decoded = base64_decode($raw, true);
    if ($decoded === false || strlen($decoded) !== 32) {
        throw new \RuntimeException(
            'GAZE_ENCRYPTION_KEY must be base64-encoded 32 bytes (run `php artisan gaze:key:generate`).'
        );
    }

    $this->app->singleton('gaze.encrypter', fn () => new Encrypter($decoded, 'AES-256-CBC'));
}
```

Then inside the facade:

```php
$encrypter = app()->bound('gaze.encrypter')
    ? app('gaze.encrypter')
    : app('encrypter'); // falls back to Laravel's default Crypt

$encryptedBlob = $encrypter->encryptString($session->blob);
```

The explicit fallback rule — **"if `GAZE_ENCRYPTION_KEY` is set it MUST be valid; if absent the wrapper uses `Crypt`"** — prevents the half-configured failure mode where a typo in `.env` silently disables encryption.

**Why bother with a dedicated key.** `APP_KEY` rotation currently invalidates cookies, queued-job payloads, cached data, *and* Gaze blobs simultaneously — one rotation event becomes a coordinated outage. A dedicated `GAZE_ENCRYPTION_KEY` lets you rotate each domain on its own schedule. The tradeoff is one extra key to protect and rotate, so this is opt-in, not the default.

### Redis / MySQL backup retention

Laravel queues backed by Redis have AOF (append-only file) and RDB (snapshot) persistence turned on by default. Laravel's queue backed by MySQL stores payloads in the `jobs` and `failed_jobs` tables, which go into nightly database backups. Both persistence paths keep ciphertext around **beyond** the lifetime of the in-flight job:

- A Redis AOF file rewritten after a job completes still contains the ciphertext until the rewrite compaction removes it.
- A nightly MySQL dump taken while a blob sits in `failed_jobs` captures the ciphertext and ships it to whatever backup target you use (S3, Backblaze, a cold-storage volume).
- If you later rotate `APP_KEY` (or `GAZE_ENCRYPTION_KEY`), old backups still contain ciphertext encrypted under the **old** key. If the old key is ever compromised, those backups become readable.

**Operational implication:** backup retention policy is part of Gaze's threat model whether you meant it to be or not. Two practical options:

1. **Short retention + aggressive rotation** — keep backups for the minimum period your business needs, rotate keys on a known cadence, accept that the rotation window is your real residual-risk window.
2. **Encrypt backups independently** — most DB backup tooling already supports this (e.g., `mysqldump | gpg` or S3 SSE-KMS); doing it at the backup layer means the Gaze blob ciphertext is double-wrapped and a single-key compromise does not decrypt backups.

Neither is a Gaze feature — they are operational-hygiene decisions the integrator owns. The Laravel wrapper cannot fix them, and pretending otherwise would be misleading.

## End-to-End Example: Ghostwriter Reply Flow

### The Job

```php
// app/Jobs/DraftEmailReplyJob.php
namespace App\Jobs;

use App\Services\Ghostwriter;
use Gaze\Laravel\Gaze;
use Gaze\Laravel\GazeSession;
use Illuminate\Bus\Queueable;
use Illuminate\Contracts\Queue\ShouldQueue;
use Illuminate\Foundation\Bus\Dispatchable;
use Illuminate\Queue\InteractsWithQueue;
use Illuminate\Queue\SerializesModels;
use Illuminate\Support\Facades\Crypt;

class DraftEmailReplyJob implements ShouldQueue
{
    use Dispatchable, InteractsWithQueue, Queueable, SerializesModels;

    public int $tries = 3;
    public int $timeout = 120;

    public function __construct(
        public readonly int $emailId,
        public readonly string $cleanPrompt,
        public readonly string $encryptedSessionBlob,
    ) {}

    public function handle(Gaze $gaze, Ghostwriter $ghostwriter): void
    {
        // 1. Ask the LLM to draft a reply using ONLY the anonymized prompt.
        $draftWithTokens = $ghostwriter->generateReply($this->cleanPrompt);

        // 2. Decrypt the session blob, restore PII, then immediately discard the plaintext.
        $blob = Crypt::decryptString($this->encryptedSessionBlob);
        $session = new GazeSession(cleanText: '', blob: $blob);
        $draftWithRealPii = $gaze->restore($session, $draftWithTokens);
        unset($blob, $session);

        // 3. Persist the final draft for human review before sending.
        EmailReplyDraft::create([
            'email_id' => $this->emailId,
            'body' => $draftWithRealPii,
        ]);
    }
}
```

### The Dispatcher

```php
// app/Services/InboundEmailHandler.php
namespace App\Services;

use App\Jobs\DraftEmailReplyJob;
use App\Models\InboundEmail;
use Gaze\Laravel\Gaze;
use Illuminate\Support\Facades\Crypt;

class InboundEmailHandler
{
    public function __construct(private Gaze $gaze) {}

    public function handle(InboundEmail $email): void
    {
        // Clean the raw email body before it ever enters a queue payload.
        $session = $this->gaze->clean($email->raw_body);

        // Encrypt the session blob before handing it to the queue.
        $encryptedBlob = Crypt::encryptString($session->blob);

        // At this point the plaintext $session->blob still lives in $session;
        // let it go out of scope so the raw map is garbage-collectable.
        $cleanPrompt = $session->cleanText;
        unset($session);

        DraftEmailReplyJob::dispatch(
            emailId: $email->id,
            cleanPrompt: $cleanPrompt,
            encryptedSessionBlob: $encryptedBlob,
        );
    }
}
```

### The Ghostwriter Service

```php
// app/Services/Ghostwriter.php
namespace App\Services;

use OpenAI\Client;

class Ghostwriter
{
    public function __construct(private Client $openai) {}

    public function generateReply(string $cleanPrompt): string
    {
        $response = $this->openai->chat()->create([
            'model' => 'gpt-4.1',
            'messages' => [
                ['role' => 'system', 'content' => 'You are a polite German customer support agent. Reply in German.'],
                ['role' => 'user',   'content' => $cleanPrompt],
            ],
        ]);

        return $response->choices[0]->message->content;
    }
}
```

### What goes over the wire to OpenAI

Given an inbound email:

```
Hallo, mein Name ist Krishan Koenig, Bestellung #1234, Lieferung an
Musterstraße 5, 10115 Berlin. Wann kommt die Ware an?
```

OpenAI sees only:

```
Hallo, mein Name ist Person_7, Bestellung #1234, Lieferung an
Musterstraße 7, 00000 Berlin. Wann kommt die Ware an?
```

The LLM drafts something like:

```
Hallo Person_7, Ihre Bestellung #1234 wird am Donnerstag an
Musterstraße 7, 00000 Berlin ausgeliefert.
```

And after `gaze restore`, the saved draft is:

```
Hallo Krishan Koenig, Ihre Bestellung #1234 wird am Donnerstag an
Musterstraße 5, 10115 Berlin ausgeliefert.
```

OpenAI never saw the real name or address. The session blob — which does contain that mapping — stayed on the Laravel server, encrypted with `APP_KEY` the entire time it was in the queue.

## Failure Modes and Fail-Closed Behavior

| Failure | Configured behavior |
|---|---|
| `gaze clean` exits non-zero | Throw `GazeException`. Do not dispatch the job. Retry with exponential backoff. |
| `gaze` binary not on PATH | Throw `GazeException` at boot (health check). Application refuses to start. |
| Encrypted blob decryption fails in job | `Crypt::decryptString` throws. Job fails; moves to `failed_jobs` (where the ciphertext is still safe). |
| `Crypt::decryptString` fails (tampered or truncated blob) | Laravel throws `DecryptException` *before* bytes reach `gaze restore`. This is the only tamper-detection path — Gaze does not sign the blob itself; integrity is provided entirely by Laravel's AEAD envelope (`Crypt::encryptString` = AES-256-CBC + HMAC-SHA256). Job fails without returning the LLM response. |
| `gaze restore` fails with `UnknownToken` | LLM hallucinated a token shape (e.g., `user_99@example.com`) that isn't in the session map — treat as draft corruption. Flag for human review, do not send. *(Note: `UnknownToken` is the pipe-mode error variant; the MCP-mode filter-value path uses a different internal variant that is collapsed to a generic `InvalidFilterValue` on the surface.)* |
| `gaze restore` fails with `BlobExpired` | Session blob TTL elapsed before restore. Usually means the job sat in the queue too long or the worker clock drifted — re-run `clean` from scratch on the original input. |
| LLM call fails | Standard OpenAI retry logic. The session blob is still valid for the next attempt. |
| Laravel worker crash between clean and restore | Job retries from scratch. The encrypted blob in the failed payload is still ciphertext — no PII leak. |

The rule: if any step in the pipeline cannot complete cleanly, the user never sees a reply. Half-anonymized output is worse than no output.

## Testing Strategy

1. **Canary test.** Inject a unique marker (`CANARY_EMAIL_DO_NOT_LEAK@test.local`) into the raw email body. After `clean`, assert the marker does not appear in the clean prompt. After `restore`, assert it is back in the final draft.
2. **Cross-session isolation.** Run two parallel `InboundEmailHandler` calls with different emails. Assert the token sets do not overlap (no accidental cross-session contamination).
3. **Tamper detection.** Manually modify a byte in an encrypted session blob before dispatching the job. Assert `Crypt::decryptString` throws a `DecryptException` — integrity is owned by Laravel's AEAD envelope, not by Gaze.
4. **Encryption-at-rest.** Dispatch a job, inspect the Redis/MySQL payload, assert no PII appears in plaintext — only ciphertext.
5. **Fail-closed verification.** Stub the `gaze` binary to exit 1. Assert `InboundEmailHandler::handle` throws and does not enqueue the job.

## Operational Notes

- **Worker memory discipline.** Call `unset($blob, $session)` immediately after `restore()`. PHP's garbage collector is lazy; explicit unset reduces the window that plaintext sits in RAM.
- **Log scrubbing.** Never log the `$session->blob` or `$encryptedSessionBlob` at any level. Even ciphertext leaks metadata (blob length correlates with PII count). Apply the same rule to `dd()`, `dump()`, and `Log::debug()` calls in non-production environments — habits learned locally leak into staging.
- **Telescope / Pulse exclusion.** Laravel Telescope records every job dispatch with the job's constructor arguments, and those entries sit in `telescope_entries` with long retention (often weeks) browsable by any developer with dashboard access. The `encryptedSessionBlob` constructor argument would therefore live in Telescope's database as ciphertext, alongside its length (which correlates with PII count). Exclude Gaze-carrying jobs explicitly:
  ```php
  // app/Providers/TelescopeServiceProvider.php
  public function register(): void
  {
      Telescope::filter(function (IncomingEntry $entry) {
          if ($entry->type === 'job' && in_array(
              $entry->content['name'] ?? '',
              [DraftEmailReplyJob::class],
              true,
          )) {
              return false;
          }
          return $this->shouldRecord($entry);
      });
  }
  ```
  The same rule applies to Laravel Pulse, any custom audit-log package, or Sentry breadcrumb integrations that capture job payloads.
- **Failed-jobs cleanup.** Schedule `php artisan queue:prune-failed --hours=24` so orphaned encrypted blobs expire on a known cadence. Pair with the backup-retention note above — pruning the `failed_jobs` table does not purge already-taken MySQL or Redis backups.
- **APP_KEY rotation.** On rotation, in-flight jobs with the old key will fail to decrypt. Job retries from scratch — this is the correct behavior. If you adopted the optional dedicated `GAZE_ENCRYPTION_KEY`, rotating it has the same in-flight cost but does not invalidate cookies or cached data simultaneously.
- **Timeouts.** Keep `timeout_seconds` small (≤30s). A hung Gaze process should kill the job quickly rather than holding a worker.
- **`SerializesModels` caution.** The example job uses `SerializesModels` (inherited from the standard job template), but deliberately takes only scalar and already-encrypted constructor arguments. Do **not** extend this pattern to accept `InboundEmail` or `User` Eloquent models — when Laravel serializes a job, it pulls the model's current attributes into the payload, which would put raw PII back into the queue. If you need to pass related data, pass IDs and re-hydrate inside `handle()` after the blob has been decrypted and the session restored.

## GDPR Positioning Notes for the Customer

Using this integration, the Laravel app sends **pseudonymized and minimized content** to the LLM vendor — names, emails, and addresses are replaced with session tokens before the LLM ever sees the prompt. That changes the compliance story, but (per the main spec's Legal Basis section) it does **not** eliminate customers' GDPR obligations. Positioning this for a DSB or a B2B customer:

- **Art. 5(1)(c) minimization:** the app demonstrably minimizes. The `policy.toml` + the canary test + the `failed_jobs` inspection are your evidence. This is the strongest single claim Gaze enables.
- **Art. 25 privacy by design:** the architecture makes raw PII physically unable to reach the LLM vendor — not a policy rule, a type-system and process boundary. DSBs specifically look for Art. 25 framing in DPIA review.
- **Art. 13/14 transparency:** the privacy policy can honestly say "AI-generated draft replies are produced using pseudonymized content: names, addresses, and contact details are replaced with session tokens before being sent to our AI provider, and restored locally only after the reply is generated." This is softer than "we send your data to Anthropic" without overclaiming "never transmitted."
- **Art. 35 DPIA:** with Gaze in place, the subprocessor receives pseudonymized rather than raw personal data, which strengthens the DPIA risk-mitigation section and may reduce the overall risk rating. Whether a DPIA is required at all still depends on the broader processing activity — confirm with your DSB.
- **B2B contracts:** if your customers' DPAs restrict subprocessors or forbid data transmission to AI vendors, the pseudonymized-content story typically makes those clauses satisfiable where they previously were not. A clause-by-clause reading is still required — "no PII to subprocessors" means different things in different contracts.

None of this eliminates baseline obligations (click-through DPA with the LLM vendor, Verarbeitungsverzeichnis entry, lawful basis documentation), but it does shrink the scope those obligations need to cover. See the main spec's Legal Basis section for the full framing and the dual Art. 25 / Art. 32 anchoring.

**One phrase to avoid.** Earlier drafts of this doc used "no personal data transmitted to the LLM vendor." That phrase is inconsistent with the pseudonymous-while-session-live position taken in the spec and should not appear in marketing or DSB-facing materials. The LLM receives pseudonymized content, which is still personal data from the controller's perspective even if it is not directly identifying for the recipient.

## Open Questions

- **Should the Laravel package own the encryption step, or should Gaze emit ciphertext directly?** Current design: Gaze emits plaintext, host encrypts. This keeps Gaze's key-management surface zero, but relies on every integration doing encryption correctly. Alternative: Gaze accepts a `--encrypt-blob-with=<path-to-key-file>` flag and emits ciphertext. Revisit after first real integration feedback.
- **Session blob size limits.** Large emails with many PII tokens could produce large blobs. Cap at what — 1 MB? 64 KB? Needs a real-world measurement from Artistfy's inbound mail corpus.
- **Persistent tokens across jobs.** Currently each `gaze clean` produces a fresh session. If the same email thread triggers multiple jobs over days, each reply gets a different token for the same person. Usually fine (the end user only sees the restored text). If cross-job consistency is required, v0.3 persistent-key mode applies — with the associated legal caveats.
