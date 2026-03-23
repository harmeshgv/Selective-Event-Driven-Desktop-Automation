from __future__ import annotations

import json
import os
import urllib.error
import urllib.request
from dataclasses import dataclass
from typing import Any, Optional


@dataclass(frozen=True)
class LlmConfig:
    provider: str
    model: str
    base_url: str
    timeout_seconds: int
    groq_api_key: Optional[str]

    @classmethod
    def from_env(cls) -> "LlmConfig":
        provider = (os.environ.get("SEDA_LLM_PROVIDER") or "disabled").strip().lower()
        model = (os.environ.get("SEDA_LLM_MODEL") or "").strip()
        base_url = (os.environ.get("SEDA_LLM_BASE_URL") or "").strip()
        timeout = int(os.environ.get("SEDA_LLM_TIMEOUT_SECONDS") or "30")
        groq_api_key = (os.environ.get("SEDA_GROQ_API_KEY") or os.environ.get("GROQ_API_KEY") or "").strip() or None

        if provider == "ollama":
            if not base_url:
                base_url = "http://127.0.0.1:11434"
            if not model:
                model = "llama3.1:8b"
        elif provider == "groq":
            if not model:
                model = "llama-3.1-8b-instant"
        else:
            provider = "disabled"

        return cls(
            provider=provider,
            model=model,
            base_url=base_url,
            timeout_seconds=max(5, timeout),
            groq_api_key=groq_api_key,
        )


def _post_json(url: str, payload: dict[str, Any], timeout_seconds: int, headers: dict[str, str]) -> dict[str, Any]:
    data = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(
        url,
        data=data,
        headers={
            "Content-Type": "application/json",
            "Accept": "application/json",
            # Some networks/CDNs block requests with missing/odd UA. Use a stable UA.
            "User-Agent": "SEDA-Agent/0.1 (python urllib)",
            **headers,
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout_seconds) as resp:  # nosec - local/known endpoints
            raw = resp.read().decode("utf-8", errors="ignore")
    except urllib.error.HTTPError as e:
        raw = ""
        try:
            raw = e.read().decode("utf-8", errors="ignore")
        except Exception:
            raw = ""
        return {
            "error": f"HTTP {getattr(e, 'code', 'unknown')}",
            "raw": raw,
            "status_code": getattr(e, "code", None),
        }
    except Exception as e:
        return {"error": f"{type(e).__name__}: {e}", "raw": None, "status_code": None}
    try:
        return json.loads(raw)
    except Exception:
        return {"raw": raw}


def _build_prompt(bundle: dict[str, Any]) -> str:
    sequence = bundle.get("sequence") or []
    label = bundle.get("sequence_label") or ""
    freq = bundle.get("frequency")
    sample_run = bundle.get("sample_run") or []

    return (
        "You are helping a user understand a repeated desktop workflow discovered from privacy-safe, symbolic events.\n"
        "Explain in clear, concise English what the workflow does.\n"
        "Be specific, but do not invent details that aren't present.\n\n"
        f"Frequency: {freq}\n"
        f"Sequence label: {label}\n"
        f"Steps ({len(sequence)}):\n"
        + "\n".join(f"- {s}" for s in sequence[:60])
        + "\n\n"
        "Representative run (JSON array of actions):\n"
        + json.dumps(sample_run[:200], ensure_ascii=False, indent=2)
        + "\n\n"
        "Return:\n"
        "1) A 2-4 sentence summary\n"
        "2) A bullet list of the steps in plain language\n"
        "3) Any detected apps/sites/queries mentioned\n"
    )


def explain_bundle(bundle: dict[str, Any]) -> dict[str, Any]:
    cfg = LlmConfig.from_env()
    if cfg.provider == "disabled":
        return {
            "enabled": False,
            "provider": "disabled",
            "model": "n/a",
            "explanation": None,
            "error": "LLM is disabled. Set SEDA_LLM_PROVIDER=ollama or groq to enable explanations.",
        }

    prompt = _build_prompt(bundle)

    if cfg.provider == "ollama":
        url = cfg.base_url.rstrip("/") + "/api/generate"
        payload = {"model": cfg.model, "prompt": prompt, "stream": False}
        data = _post_json(url, payload, cfg.timeout_seconds, headers={})
        if data.get("error"):
            raw = data.get("raw")
            raw_hint = ""
            if isinstance(raw, str) and raw.strip():
                raw_hint = raw.strip()[:1200]
            return {
                "enabled": False,
                "provider": "ollama",
                "model": cfg.model,
                "explanation": None,
                "error": str(data.get("error") or "Ollama request failed")
                + (f"\n\nDetails:\n{raw_hint}" if raw_hint else ""),
            }
        text = str(data.get("response") or data.get("message") or data.get("raw") or "")
        return {"enabled": True, "provider": "ollama", "model": cfg.model, "explanation": text, "error": None}

    if cfg.provider == "groq":
        if not cfg.groq_api_key:
            return {
                "enabled": False,
                "provider": "groq",
                "model": cfg.model,
                "explanation": None,
                "error": "Groq selected but no API key found. Set SEDA_GROQ_API_KEY (or GROQ_API_KEY).",
            }

        # Prefer the official Groq SDK to avoid CDN blocks
        # that can occur with urllib (e.g. Cloudflare 1010).
        try:
            from groq import Groq  # type: ignore[import-not-found]

            client = Groq(api_key=cfg.groq_api_key)
            resp = client.chat.completions.create(
                model=cfg.model,
                messages=[
                    {"role": "system", "content": "You are a helpful assistant."},
                    {"role": "user", "content": prompt},
                ],
                temperature=0.2,
            )
            text = ""
            try:
                text = resp.choices[0].message.content or ""
            except Exception:
                text = str(resp)
            return {"enabled": True, "provider": "groq", "model": cfg.model, "explanation": text, "error": None}
        except Exception as sdk_err:
            # Fall back to raw HTTP if SDK isn't available, but keep error surfaced.
            url = "https://api.groq.com/openai/v1/chat/completions"
            payload = {
                "model": cfg.model,
                "messages": [
                    {"role": "system", "content": "You are a helpful assistant."},
                    {"role": "user", "content": prompt},
                ],
                "temperature": 0.2,
            }
            data = _post_json(
                url,
                payload,
                cfg.timeout_seconds,
                headers={"Authorization": f"Bearer {cfg.groq_api_key}"},
            )
            if data.get("error"):
                raw = data.get("raw")
                raw_hint = ""
                if isinstance(raw, str) and raw.strip():
                    raw_hint = raw.strip()[:1200]
                return {
                    "enabled": False,
                    "provider": "groq",
                    "model": cfg.model,
                    "explanation": None,
                    "error": (
                        f"SDK error: {type(sdk_err).__name__}: {sdk_err}\n"
                        f"HTTP fallback: {data.get('error')}"
                        + (f"\n\nDetails:\n{raw_hint}" if raw_hint else "")
                    ),
                }
            try:
                text = data["choices"][0]["message"]["content"]
            except Exception:
                text = str(data.get("raw") or data)
            return {"enabled": True, "provider": "groq", "model": cfg.model, "explanation": text, "error": None}

    return {
        "enabled": False,
        "provider": cfg.provider,
        "model": cfg.model,
        "explanation": None,
        "error": f"Unsupported provider: {cfg.provider}",
    }

