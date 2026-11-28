- The proxy no longer restores a streamed tool call's arguments when the fields
  that name the call disagree. Previously an OpenAI Responses argument delta was
  bound to a tool only by `output_index`, so a delta whose `item_id` named an
  undeclared call restored into it for a client that joins deltas by `item_id`.
  Now the Responses `item_id` and item `id` and the Chat Completions tool call
  `id` must name the same declared tool as the call's index, or the arguments
  keep their tokens. `SseStreamKey` gains `with_call_id` for adapters whose
  events name a call by an id.
