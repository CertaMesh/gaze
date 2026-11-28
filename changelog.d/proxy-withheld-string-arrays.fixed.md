- The Anthropic response codec no longer refuses a string array in a field that
  keeps its tokens (an undeclared tool's `input`, `tool_result.content`, search
  results and documents) when it holds format-preserving fakes. The array proof
  now checks the restored reading each element's own check proves, so a
  provider-origin value in the array still refuses.
