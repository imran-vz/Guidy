"""Refresh the LLM model registry bundled with the app.

Usage: python3 scripts/update_llm_registry.py [api.json] > assets/llm-registry.json

Reads models.dev's catalog (https://models.dev/api.json, or a saved copy)
and keeps only what Guidy uses: the providers Guidy can call, their models
that output text, and the fields the model picker shows. The result keeps
models.dev's schema, so the app parses it with the same code it uses for
the copy it downloads at runtime (crates/guidy-core/src/llm/registry.rs).
Keep PROVIDERS in sync with `models_dev_provider` there.
"""
import json
import sys
import urllib.request

URL = "https://models.dev/api.json"
PROVIDERS = [
    "anthropic",
    "cerebras",
    "cloudflare-ai-gateway",
    "cloudflare-workers-ai",
    "groq",
    "openai",
    "openrouter",
    "vercel",
    "zai",
]
MODEL_FIELDS = ["id", "name", "reasoning", "tool_call", "structured_output", "release_date"]


def trim_model(m):
    out = {k: m[k] for k in MODEL_FIELDS if k in m}
    if "modalities" in m:
        out["modalities"] = m["modalities"]
    limit = {k: v for k, v in m.get("limit", {}).items() if k in ("context", "output")}
    if limit:
        out["limit"] = limit
    cost = {k: v for k, v in m.get("cost", {}).items() if k in ("input", "output")}
    if cost:
        out["cost"] = cost
    return out


def main():
    if len(sys.argv) > 1:
        with open(sys.argv[1]) as f:
            data = json.load(f)
    else:
        with urllib.request.urlopen(URL, timeout=60) as r:
            data = json.load(r)
    out = {}
    for pid in PROVIDERS:
        p = data.get(pid)
        if not p:
            print(f"warning: models.dev has no provider {pid}", file=sys.stderr)
            continue
        models = {
            mid: trim_model(m)
            for mid, m in sorted(p.get("models", {}).items())
            if "text" in m.get("modalities", {}).get("output", ["text"])
        }
        out[pid] = {"id": pid, "name": p.get("name", pid), "models": models}
    json.dump(out, sys.stdout, ensure_ascii=False, separators=(",", ":"), sort_keys=True)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
