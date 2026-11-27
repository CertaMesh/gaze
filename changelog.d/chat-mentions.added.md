- `username.chat_mention` (in `core`) protects Slack and Discord user mentions
  as `custom:username`: the Slack user or enterprise-user ID inside the mention
  markup, a display name after the vertical bar included, and the Discord user
  snowflake. The value inside the markup becomes one token. Channel, group,
  broadcast, role, emoji, slash-command and timestamp markup stays raw.
