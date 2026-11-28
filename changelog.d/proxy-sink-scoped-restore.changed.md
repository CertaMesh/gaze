- Breaking: proxy restore is sink-scoped, like the MCP bridge. The assistant
  text the client shows its user restores; a tool call's arguments restore only
  for tools declared with `[proxy] restore_tools` or
  `ProxyConfig::with_restore_tools`. Undeclared tools' arguments, tool results,
  documents, reasoning and Gemini thought summaries keep their tokens, so a
  placeholder the model writes there cannot pull an original into an undeclared
  tool or service. `PiiSurface` gains a `sink` field (`RestoreSink`), and
  `ProviderAdapter` gains `sse_event_tool_calls`.
