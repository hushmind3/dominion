# Sub2 single-pass per-round context and usage (2026-11-17)

The model-turn call previously repeatedly estimated the same fixed
`wire_messages + round_tools` input for the room receipt, Home SDK task
limit, owner job budget and fallback token metadata, and separately read
`session_usage` for both Home SDK token and round checks. The same model
round now performs one prompt input estimate and (only when an SDK limit
is active) one persisted usage snapshot. No cached estimate crosses model
rounds, provider changes or tool-set changes.

Admission still reserves the input estimate before a billable provider
request and caps the model output tokens by remaining budget. No
completion/evidence policy or tool authorization was weakened.
No speedup factor is claimed without a model workload benchmark.
