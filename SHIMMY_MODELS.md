# Managing LLMs in Shimmy

Shimmy is a single-binary GGUF + LoRA server running locally. This doc covers how to manage models.

## Model Directory

Models live in:

```
/var/home/a/models/
```

Shimmy auto-discovers `.gguf` files from this directory on startup. No config file edits needed.

## Adding a Model

1. Download a GGUF model (Q4_K_M recommended for 8B models):
   ```bash
   hf download <repo_id> <filename> --local-dir /var/home/a/models
   ```
2. Restart shimmy to pick it up:
   ```bash
   systemctl --user restart shimmy.service
   ```
3. Verify it's loaded:
   ```bash
   curl -s http://127.0.0.1:11436/v1/models | grep <model_name>
   ```

## Removing a Model

1. Delete the `.gguf` file:
   ```bash
   rm /var/home/a/models/<model-name>.gguf
   ```
2. Restart shimmy:
   ```bash
   systemctl --user restart shimmy.service
   ```

## Recommended Models: Tool Calling + Voice

For full OpenLibertas functionality (MCP tools + voice), you need models that support **both** tool calling and audio/voice modalities. The app auto-detects these capabilities from model names.

### Multimodal + Tool Models

| Model | Params | Size | Tools | Voice | Notes |
|---|---|---|---|---|---|
| **Qwen2.5-Omni** | 7B | 4.5 GB | ✅ | ✅ | Best local multimodal+tool model |
| **Phi-4-Multimodal** | 5.6B | 3.5 GB | ✅ | ✅ | Microsoft's latest multimodal |
| **Gemma-3-4B-IT** | 4B | 2.6 GB | ✅ | ✅ | Google's multimodal (4B variant) |
| **Gemma-3-12B-IT** | 12B | 7.2 GB | ✅ | ✅ | Larger variant if VRAM allows |
| **Ultravox** | 7B | 4.2 GB | ❓ | ✅ | Speech-focused; tool support varies by quant |

### Download Examples

```bash
# Qwen2.5-Omni (recommended)
hf download Qwen/Qwen2.5-Omni-7B-GGUF \
  qwen2.5-omni-7b-q4_k_m.gguf \
  --local-dir /var/home/a/models

# Phi-4 Multimodal
hf download microsoft/Phi-4-multimodal-instruct-GGUF \
  phi-4-multimodal-instruct-q4_k_m.gguf \
  --local-dir /var/home/a/models

# Gemma-3 4B
hf download google/gemma-3-4b-it-GGUF \
  gemma-3-4b-it-q4_k_m.gguf \
  --local-dir /var/home/a/models
```

## Model Capability Detection

OpenLibertas detects capabilities from model filenames:

- **Tool support**: `qwen`, `phi-4`, `gemma-3`, `omni`, `llama-3`, `command-r`, `mistral`
- **Voice support**: `omni`, `multimodal`, `ultravox`, `speech`, `audio`, `gemma-3`

Set `filter_require_voice_and_tools = true` in `~/.config/openlibertas/config.toml` to hide models missing either capability.

## Deprecated: Old Models

These models were previously listed but **do NOT support voice/audio**:

| Model | Why Removed |
|---|---|
| Llama-3.2-1B | Text-only, no voice |
| NeuralDaredevil-8B | Text-only, no voice |
| Phi3-LoRA | Text-only, no voice |
| Qwen-Coder-32b | Code-only, no voice |
| Qwen2.5-Coder-* | Code-only, no voice |
| Qwen3.5-9B | Text-only, no voice |
| Trinity-Nano | Text-only, no voice |

## Service Management

```bash
# Status
systemctl --user status shimmy.service

# Restart
systemctl --user restart shimmy.service

# Enable auto-start on login
systemctl --user enable shimmy.service

# View logs
journalctl --user -u shimmy.service -f
```

## Auto-Restart on Crashes

The systemd service has `Restart=on-failure` with `RestartSec=5`, so shimmy auto-restarts if it crashes. It does **not** restart on graceful shutdown (`SIGTERM`).

## Downloading from HuggingFace

The old `huggingface-cli` is deprecated. Use `hf` instead:

```bash
# Download specific file
hf download <repo_id> <filename> --local-dir /var/home/a/models

# Example
hf download Qwen/Qwen2.5-Omni-7B-GGUF \
  qwen2.5-omni-7b-q4_k_m.gguf \
  --local-dir /var/home/a/models
```

## Server Endpoint

Shimmy exposes an OpenAI-compatible API:

- Base URL: `http://127.0.0.1:11436`
- Models: `GET /v1/models`
- Generate: `POST /api/generate`
- Health: `GET /health`

## Port Binding

Shimmy binds to `127.0.0.1:11436` by default. Use `--bind` to change:

```bash
shimmy serve --bind 0.0.0.0:8080
```

## GPU Backend

Shimmy auto-detects CUDA. Override with:

```bash
shimmy serve --gpu-backend cuda   # or cpu, vulkan, opencl
```
