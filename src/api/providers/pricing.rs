use serde_json::Value;

/// Returns (free?, source). `None` means the provider did not publish enough
/// information to decide; the UI must show "확인 필요", never a fake FREE.
pub(super) fn model_free_status(
    _provider: &Value,
    kind: &str,
    base: &str,
    item: &Value,
    id: &str,
) -> (Option<bool>, &'static str) {
    // Local runtimes on their standard ports are free. Any other localhost
    // proxy may front a paid provider, so it is not automatically free.
    if kind == "ollama" || base.contains(":11434") || base.contains(":1234") {
        return (Some(true), "local-runtime");
    }
    if base.contains("z.ai") {
        return (
            Some(matches!(
                id,
                "glm-4.7-flash" | "glm-4.5-flash" | "glm-4.6v-flash"
            )),
            "z.ai-free-list",
        );
    }
    if base.contains("oai.endpoints.kepler.ai.cloud.ovh.net") {
        return (Some(true), "ovh-free-endpoint");
    }
    // Groq's Free Plan has per-model rate limits. /models lists the models
    // accessible to the key, not a "free-only" catalog.
    // Source: https://console.groq.com/docs/rate-limits
    if base.contains("api.groq.com") {
        return (
            if matches!(
                id,
                "openai/gpt-oss-120b"
                    | "openai/gpt-oss-20b"
                    | "openai/gpt-oss-safeguard-20b"
                    | "qwen/qwen3.8-27b"
            ) {
                Some(true)
            } else {
                None
            },
            "groq-free-list",
        );
    }
    // Cloudflare Workers Free includes 10,000 Neurons per day shared across
    // supported models. The catalogue does not publish per-model free status,
    // so only models confirmed by the official pricing page are FREE; the
    // rest are "unknown", never a guessed FREE.
    if base.contains("api.cloudflare.com") && base.contains("/ai/") {
        if !id.starts_with("@cf/") {
            return (Some(false), "not-workers-ai");
        }
        if matches!(
            id,
            "@cf/moonshotai/kimi-k2.6"
                | "@cf/moonshotai/kimi-k2.7-code"
                | "@cf/zai-org/glm-5.2"
                | "@cf/zai-org/glm-5.3"
                | "@cf/zai-org/glm-5.3-flash"
                | "@cf/deepseek-ai/deepseek-v4-flash-0731"
                | "@cf/deepseek-ai/deepseek-v4-pro-0813"
        ) {
            return (Some(false), "cloudflare-paid-only");
        }
        if matches!(
            id,
            "@cf/zai-org/glm-4.7-flash"
                | "@cf/google/gemma-4-26b-a4b-it"
                | "@cf/nvidia/nemotron-3-120b-a12b"
                | "@cf/deepseek-ai/deepseek-v4.1-flash-1017"
        ) {
            return (Some(true), "cloudflare-confirmed-free");
        }
        return (None, "cloudflare-no-free-metadata");
    }
    if base.contains("127.0.0.1:31415") || base.contains("127.0.0.1:3001") {
        return (Some(true), "freellmapi-hub");
    }
    if item["free"].as_bool() == Some(true) || item["is_free"].as_bool() == Some(true) {
        return (Some(true), "provider-metadata");
    }
    // Price metadata wins over a suffix. Do not label a billable endpoint free
    // just because its model ID contains a historical promotional name.
    if let Some(pricing) = item.get("pricing") {
        let zero = |key: &str| {
            pricing
                .get(key)
                .and_then(|value| {
                    value
                        .as_str()
                        .and_then(|text| text.parse::<f64>().ok())
                        .or_else(|| value.as_f64())
                })
                .is_some_and(|value| value == 0.0)
        };
        return (
            Some(zero("prompt") && zero("completion")),
            "provider-pricing",
        );
    }
    if id.to_ascii_lowercase().ends_with(":free") {
        return (Some(true), "openrouter-free-suffix");
    }
    // OpenCode Go publishes free variants with the `-free` suffix, not `:free`.
    // Scope this fallback to OpenCode's API rather than unrelated providers.
    if (base.contains("opencode.ai/inference/") || base.contains("opencode.ai/zen/"))
        && id.to_ascii_lowercase().ends_with("-free")
    {
        return (Some(true), "opencode-free-suffix");
    }
    (None, "no-pricing-metadata")
}

pub(super) fn model_is_free(
    provider: &Value,
    kind: &str,
    base: &str,
    item: &Value,
    id: &str,
) -> bool {
    model_free_status(provider, kind, base, item, id).0 == Some(true)
}
