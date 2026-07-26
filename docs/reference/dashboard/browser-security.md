# Dashboard browser security reference

## Origin and transport

- Bind a CSPRNG-selected literal IPv4 address in 127.0.0.0/8 with port zero.
- Require the exact bound literal authority in Host and the exact http:// origin in Origin.
- Loopback position is not authentication.
- Accept at most one canonical HTTP/1.1 origin-form request per connection, then close.
- The raw gate owns the stream before any framework parser, router, middleware, body extractor,
  tracing layer, or access logger.
- Reject absolute/authority forms, CONNECT, query/fragment targets, normalization ambiguity,
  duplicate singleton headers, obs-fold/control bytes, Transfer-Encoding, ambiguous
  Content-Length, Upgrade, h2c, pipelining, trailing requests, forwarded headers, ambient cookies,
  and proxy authorization.
- Emit no CORS headers; deny preflight.

Every response uses no-store/no-cache, nosniff, no-referrer, frame denial, same-origin
COOP/COEP/CORP, a deny-by-default Permissions-Policy, Connection: close, Clear-Site-Data, and this
CSP:

    default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self';
    img-src 'none'; font-src 'none'; object-src 'none'; frame-src 'none';
    worker-src 'none'; manifest-src 'none'; base-uri 'none'; form-action 'none';
    frame-ancestors 'none'; require-trusted-types-for 'script'; trusted-types 'none'

## Credentials

The launch credential has one spelling: GazeDashboardV1 followed by one ASCII space and exactly 43
unpadded base64url bytes encoding 32 random bytes. Reject padding, alternate alphabets, case
changes, additional whitespace, and extra fields.

After launch pairing, a private manual bootstrap envelope supplies separate 32-byte page-session
and CSRF secrets once. The three secret types have no Debug, Display, Clone, Serialize, Deref,
AsRef, or implicit conversion surface. Server state retains only validation digests bound to
authentication generation and page session.

Only the pair request may carry launch `Authorization`. Every post-pair request must omit it and
carry the page-session and CSRF headers from page memory, with `credentials: omit`, `cache:
no-store`, `redirect: error`, and `referrerPolicy: no-referrer`. Reload, pagehide, freeze, hidden visibility, network/follow
loss, authentication failure, rotation, and bfcache restoration must abort requests, remove payload
DOM, and clear mutable secrets where possible.

## Reveal and response leases

ProviderVisible selection is exact logical ID, provider stage, and emission ID. Owner reveal adds
the exact startup-captured owner domain and consumes a 30-second permit. There is no reveal-all,
hover/focus reveal, export, download, copy, or clipboard operation.

Sensitive responses use separate provider-visible, OwnerRaw, and OwnerRestored manual encoders.
The wire envelope is exactly `GZPL`, version, domain tag, stage tag, big-endian payload length, and
UTF-8 payload. A single zeroizing envelope is reserved against the global byte cap before copying;
no second payload/envelope/chunk buffer is created.
Before header commit and every application write, the child revalidates authentication generation,
inspection epoch, logical ID, stage, emission ID, domain, insertion generation, and deadline.
Purge, TTL, rotation, conceal, authentication loss, disconnect, failure, and shutdown prevent later
application writes and zeroize owned buffers.

Concealment means byte absence from the DOM, attributes, safe snapshots, follow responses, and
nonmatching payload responses. Hiding with CSS is not concealment.

## Safe view constraints

Safe snapshot/follow models never hold payload bytes, object keys/values, raw paths, credentials,
numeric endpoints, or arbitrary error text. Configured ports render only their closed category.
Queue telemetry renders UNAVAILABLE / NOT MEASURED, not zero or healthy.

MetadataOnly has no content-derived measurements or projections. Every absent projection shows the
producer's exact closed omission reason. ProjectionFailedClosed remains coarse.

An SSE row contains only ordinal, event kind, optional delta kind, and optional content-block index.
It contains no byte count or timing. Browser code must not derive bytes, timestamps, latency,
cadence, or relative time for an entry.
