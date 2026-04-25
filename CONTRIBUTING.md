# Contributing

## Tenant class names in tests

Test fixtures and benchmark labels MUST use neutral class names (e.g. `class_alpha`, `tenant_class_a`, `dict_alpha`), never tenant-specific patterns like `order_id`, `Order_42`, `Song_42`, `User_7`. Rationale: drawer `eac549ae` — gaze core has no built-in tenant knowledge.

Forward-reference: a future `// gaze-allow-tenant-knowledge: <reason>` comment marker may be introduced for legitimate cases (drawer `eac549ae` + planned v0.4.3 lint gate). Use neutral placeholders by default.
