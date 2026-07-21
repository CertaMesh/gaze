/* Dev-only synthetic HTTP fixture server.
 *
 * Serves the real production assets byte-for-byte with the full production
 * security-header set, and mocks ONLY the closed typed API surface with the
 * synthetic fixtures. It is never shipped; it exists so the browser contract
 * runs against the exact assets under the exact headers. Routes beginning
 * with /__ are test-control routes and do not exist in production.
 */

import { createServer } from "node:http";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

import { FIXTURES, LAUNCH_TOKEN } from "./fixtures.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const ASSETS = join(HERE, "..", "assets");

const INDEX_HTML = readFileSync(join(ASSETS, "index.html"));
const APP_CSS = readFileSync(join(ASSETS, "app.css"));
const APP_JS = readFileSync(join(ASSETS, "app.js"));

/* Deterministic dev-only secondary secrets (32 bytes each). */
const PAGE_SECRET = Buffer.alloc(32, 0xa1);
const CSRF_SECRET = Buffer.alloc(32, 0xb2);

function b64url(buf) {
  return buf.toString("base64").replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export const PAGE_SESSION_HEADER = b64url(PAGE_SECRET);
export const CSRF_HEADER = b64url(CSRF_SECRET);

const CSP_FULL =
  "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; " +
  "img-src 'none'; font-src 'none'; object-src 'none'; frame-src 'none'; " +
  "worker-src 'none'; manifest-src 'none'; base-uri 'none'; form-action 'none'; " +
  "frame-ancestors 'none'; require-trusted-types-for 'script'; trusted-types 'none'";

/* Identical policy minus the Trusted Types directives; used only for the axe
 * injection pass, because axe cannot be injected under trusted-types 'none'. */
const CSP_AXE = CSP_FULL
  .replace("; require-trusted-types-for 'script'", "")
  .replace("; trusted-types 'none'", "");

export { CSP_FULL, CSP_AXE };

function securityHeaders(cspMode, contentType) {
  return {
    "content-type": contentType,
    "cache-control": "no-store",
    "pragma": "no-cache",
    "expires": "0",
    "x-content-type-options": "nosniff",
    "referrer-policy": "no-referrer",
    "x-frame-options": "DENY",
    "cross-origin-opener-policy": "same-origin",
    "cross-origin-embedder-policy": "require-corp",
    "cross-origin-resource-policy": "same-origin",
    "permissions-policy": "accelerometer=(), camera=(), geolocation=(), microphone=(), payment=(), usb=()",
    "connection": "close",
    "content-security-policy": cspMode === "axe" ? CSP_AXE : CSP_FULL,
  };
}

function constantReject(res, status, cspMode) {
  res.writeHead(status, securityHeaders(cspMode, "text/plain; charset=utf-8"));
  res.end("rejected");
}

function payloadEnvelope(domainTag, stageTag, text) {
  const body = Buffer.from(text, "utf-8");
  const head = Buffer.alloc(11);
  head.write("GZPL", 0, "ascii");
  head[4] = 0x01;
  head[5] = domainTag;
  head[6] = stageTag;
  head.writeUInt32BE(body.length, 7);
  return Buffer.concat([head, body]);
}

function bootstrapEnvelope() {
  const head = Buffer.alloc(6);
  head.write("GZDB", 0, "ascii");
  head[4] = 0x01;
  head[5] = 0x02;
  return Buffer.concat([head, PAGE_SECRET, CSRF_SECRET]);
}

const STAGE_TAGS = {
  OwnerRequest: 1,
  ProviderRequest: 2,
  ProviderResponse: 3,
  OwnerRestoredResponse: 4,
};

async function readBody(req) {
  const chunks = [];
  for await (const chunk of req) {
    chunks.push(chunk);
    if (chunks.reduce((n, c) => n + c.length, 0) > 65536) { throw new Error("body too large"); }
  }
  return Buffer.concat(chunks);
}

export function startFixtureServer(options = {}) {
  const state = {
    fixtureId: options.fixtureId || "fx-default",
    cspMode: options.cspMode || "full",
    authMode: "accept",
    pairAttempts: 0,
  };

  const server = createServer(async (req, res) => {
    const url = new URL(req.url, "http://localhost");
    const path = url.pathname;
    const mode = state.cspMode;

    try {
      /* ---- test control (dev-only, not part of the contract) ---- */
      if (path === "/__fixture" && req.method === "POST") {
        const body = JSON.parse((await readBody(req)).toString("utf-8"));
        if (!FIXTURES[body.id]) { constantReject(res, 404, mode); return; }
        state.fixtureId = body.id;
        res.writeHead(200, securityHeaders(mode, "application/json"));
        res.end(JSON.stringify({ ok: true }));
        return;
      }
      if (path === "/__authmode" && req.method === "POST") {
        const body = JSON.parse((await readBody(req)).toString("utf-8"));
        state.authMode = body.mode === "reject" ? "reject" : "accept";
        res.writeHead(200, securityHeaders(mode, "application/json"));
        res.end(JSON.stringify({ ok: true }));
        return;
      }
      if (path === "/__csp" && req.method === "POST") {
        const body = JSON.parse((await readBody(req)).toString("utf-8"));
        state.cspMode = body.mode === "axe" ? "axe" : "full";
        res.writeHead(200, securityHeaders(state.cspMode, "application/json"));
        res.end(JSON.stringify({ ok: true }));
        return;
      }
      if (path === "/__axe.js" && req.method === "GET") {
        const axeSource = readFileSync(join(HERE, "node_modules", "axe-core", "axe.min.js"));
        res.writeHead(200, securityHeaders(mode, "text/javascript; charset=utf-8"));
        res.end(axeSource);
        return;
      }

      /* ---- static shell/assets (byte-invariant) ---- */
      if (req.method === "GET") {
        if (path === "/") {
          res.writeHead(200, securityHeaders(mode, "text/html; charset=utf-8"));
          res.end(INDEX_HTML);
          return;
        }
        if (path === "/assets/app.css") {
          res.writeHead(200, securityHeaders(mode, "text/css; charset=utf-8"));
          res.end(APP_CSS);
          return;
        }
        if (path === "/assets/app.js") {
          res.writeHead(200, securityHeaders(mode, "text/javascript; charset=utf-8"));
          res.end(APP_JS);
          return;
        }
        constantReject(res, 404, mode);
        return;
      }

      if (req.method !== "POST") { constantReject(res, 405, mode); return; }

      /* Ambient credentials are rejected before anything else. */
      if (req.headers.cookie || req.headers.cookie2 || req.headers["proxy-authorization"]) {
        constantReject(res, 400, mode);
        return;
      }

      const fixture = FIXTURES[state.fixtureId];

      if (path === "/api/v1/session/pair") {
        state.pairAttempts += 1;
        const auth = req.headers.authorization || "";
        if (auth !== "GazeDashboardV1 " + LAUNCH_TOKEN) {
          constantReject(res, 401, mode);
          return;
        }
        await readBody(req);
        res.writeHead(200, securityHeaders(mode, "application/octet-stream"));
        res.end(bootstrapEnvelope());
        return;
      }

      /* All other API routes require the secondary secrets. */
      const page = req.headers["x-gaze-page-session"] || "";
      const csrf = req.headers["x-gaze-csrf"] || "";
      if (state.authMode === "reject" || page !== PAGE_SESSION_HEADER || csrf !== CSRF_HEADER) {
        constantReject(res, 401, mode);
        return;
      }

      if (path === "/api/v1/events/snapshot" || path === "/api/v1/events/follow") {
        await readBody(req);
        res.writeHead(200, securityHeaders(mode, "application/json"));
        res.end(JSON.stringify(fixture.snapshot()));
        return;
      }

      if (path === "/api/v1/events/provider-visible") {
        const body = JSON.parse((await readBody(req)).toString("utf-8"));
        const text = fixture.payloads.providerVisible;
        if (!text) { constantReject(res, 404, mode); return; }
        res.writeHead(200, securityHeaders(mode, "application/octet-stream"));
        res.end(payloadEnvelope(1, STAGE_TAGS[body.stage] || 2, text));
        return;
      }

      if (path === "/api/v1/reveal/owner-raw") {
        const body = JSON.parse((await readBody(req)).toString("utf-8"));
        const text = fixture.payloads.ownerRaw;
        if (!text) { constantReject(res, 404, mode); return; }
        res.writeHead(200, securityHeaders(mode, "application/octet-stream"));
        res.end(payloadEnvelope(2, STAGE_TAGS[body.stage] || 1, text));
        return;
      }

      if (path === "/api/v1/reveal/owner-restored") {
        const body = JSON.parse((await readBody(req)).toString("utf-8"));
        const text = fixture.payloads.ownerRestored;
        if (!text) { constantReject(res, 404, mode); return; }
        res.writeHead(200, securityHeaders(mode, "application/octet-stream"));
        res.end(payloadEnvelope(3, STAGE_TAGS[body.stage] || 4, text));
        return;
      }

      if (path === "/api/v1/purge") {
        await readBody(req);
        state.fixtureId = "fx-purged";
        res.writeHead(200, securityHeaders(mode, "application/json"));
        res.end(JSON.stringify({ ok: true }));
        return;
      }

      constantReject(res, 404, mode);
    } catch (_err) {
      constantReject(res, 500, state.cspMode);
    }
  });

  return new Promise((resolve) => {
    server.listen(0, "127.0.0.1", () => {
      const port = server.address().port;
      resolve({
        port,
        origin: "http://127.0.0.1:" + port,
        state,
        close: () => new Promise((r) => server.close(r)),
      });
    });
  });
}
