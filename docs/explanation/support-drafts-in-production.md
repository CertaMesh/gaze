# AI support drafts that never see the customer

[`CertaMesh/gaze-ghostwriter`](https://github.com/CertaMesh/gaze-ghostwriter) is a Laravel package that watches a support inbox over IMAP and drafts replies with an LLM. The application does the data lookup. Gaze pseudonymizes the resulting context. The LLM only composes prose.

```text
1. Customer email arrives via IMAP:
   "Hi Support, I'm Alice Schmidt, order #INV-2026-04-1872,
    my refund of €128.40 hasn't shown up..."
       ↓
2. App parses email → extracts identifiers:
   sender=customer@..., order_id=INV-2026-04-1872, amount=€128.40
       ↓
3. App looks up order in DB (real PII, no LLM involved):
   order #INV-2026-04-1872 → refund processed 2026-05-12,
   customer = Alice Schmidt
       ↓
4. App builds context bundle (still real PII):
   { name: "Alice Schmidt", order_id: "INV-2026-04-1872",
     amount: "€128.40", refund_processed: "2026-05-12",
     issue: "delayed refund" }
       ↓
5. gaze clean — pseudonymizes the bundle:
   { name: "<Name_1>", order_id: "<OrderId_1>",
     amount: "<Amount_1>", refund_processed: "<Date_1>",
     issue: "delayed refund" }
   + per-session manifest stored
       ↓
6. LLM drafts reply (sees only tokens + facts):
   "Hi <Name_1>, your refund of <Amount_1> for order <OrderId_1>
    was processed on <Date_1>. Please allow 3-5 business days to
    appear on your statement."
       ↓
7. gaze restore rehydrates draft:
   "Hi Alice Schmidt, your refund of €128.40 for order
    #INV-2026-04-1872 was processed on 2026-05-12. Please allow
    3-5 business days to appear on your statement."
       ↓
8. Support agent reviews → approves → reply sent.

LLM never saw "Alice Schmidt", "#INV-2026-04-1872", "€128.40", "2026-05-12".
App owns the lookup. gaze owns the manifest. LLM owns the prose.
Each layer's role is what it is built for.
```

`OrderId` and refund-amount shapes are tenant-specific custom recognizers in the host policy; email, names, IBAN, phone, postal, and credit-card shapes come from the bundled `core` rulepack.

## Try the loop

- Drop-in Laravel package: [`CertaMesh/gaze-ghostwriter`](https://github.com/CertaMesh/gaze-ghostwriter)

Agentic workflows (browser automation, tool execution) hook the same restore boundary at tool-call args, on the same manifest contract — the agent stays on tokens end-to-end.

CLI surface (`gaze clean`, `gaze restore`, audit, policy TOML): [Quickstart](../../README.md#quickstart), [`gaze-cli` README](../../crates/gaze-cli/README.md).
