# ASTRA: Minisforum local models for PHarness

Windows 11 · 128 GB memory · 2026-09-07. Assumes the **MS-S1 Max with Ryzen AI Max+ 395 / Radeon 8060S**; confirm the processor in Settings → System → About. This is a setup guide, not evidence that the new machine has passed PHarness qualification.

## Recommended starting point

Use **LM Studio on Windows, its Vulkan llama.cpp runtime, one loaded model, and its authenticated local API**. You already know the interface, and PHarness already speaks its API. Keep model serving on the Minisforum and execution of repository tools in PHarness:

`PHarness worker → PHarness gateway in lucas_engineering → LM Studio on the Minisforum`

The gateway owns the server token and approved endpoint. LM Studio generates responses/tool requests; PHarness owns file access, tests, permissions and approvals. An additional proxy framework or model-serving Kubernetes cluster is unnecessary for this first connection.

LM Studio offers Vulkan and ROCm runtimes for AMD hardware. Vulkan is my recommended initial baseline; compare ROCm later with the same model and inputs if there is a measured performance problem. This recommendation is about setup simplicity, not a claim that Vulkan is always faster. [AMD serving guide](https://developer.amd.com/playbooks/lmstudio-rocm-llms/)

## 1. Prepare Windows and choose a model

1. Install the current stable Minisforum/AMD-supported graphics driver and [LM Studio](https://lmstudio.ai/download). Use LM Studio **0.4.0 or newer** for API-token authentication. Record the actual app, runtime and driver versions before benchmarking.
2. Use wired Ethernet and reserve the machine's IPv4 address in DHCP. Keep it awake while plugged in; the display may sleep. Do not expose the inference port through the router or Cloudflare.
3. In LM Studio, select the Vulkan runtime and verify that it sees the Radeon GPU. Download **GGUF**, not Apple-only MLX weights. Start with the model's supplied chat template and native tool support.
4. Load **one model at a time**, initially with a **32,768-token context**, GPU offload enabled and one concurrent request. Leave Windows and runtime memory headroom. 128 GB shared memory is not 128 GB freely available for model weights: context caches and runtime buffers also consume it. Check actual GPU allocation before increasing it; do not copy Linux memory-tuning instructions into Windows.

| Candidate | Starting quantization | Why use it |
| --- | --- | --- |
| **Qwen3-Coder-30B-A3B-Instruct** | Q4_K_M GGUF | My first choice for proving the connection and measuring latency with modest memory pressure. |
| **Qwen3-Coder-Next** | Q4_K_M GGUF | My next coding candidate for this 128 GB machine once the smaller baseline works. It has 80B total parameters / 3B active; all weights still need memory. |

These are candidates, not qualified replacements for the hosted model. Avoid downloading a collection of 70B–120B models before measuring the first one. Model size and chat quality alone do not establish reliable coding/tool use. [Qwen 30B model card](https://huggingface.co/Qwen/Qwen3-Coder-30B-A3B-Instruct), [Qwen Coder Next official GGUF](https://huggingface.co/Qwen/Qwen3-Coder-Next-GGUF)

For reproducibility, record the exact GGUF repository, revision, quantization and file SHA-256 (all parts for a split model). Keep the same loaded file behind an API identifier during a test. `Get-FileHash -Algorithm SHA256 'C:\path\to\model.gguf'` reads the file without changing it.

## 2. Start the authenticated server

In **Developer → Server Settings**:

- Port: **1234**.
- **Require Authentication: on**. Create a dedicated PHarness token; grant inference and model-list access, not MCP access. Store it privately.
- **Serve on Local Network: on** when ready for PHarness to connect.
- **CORS: off**. The browser does not call LM Studio directly.
- **Allow per-request MCPs** and **Allow calling servers from mcp.json: off**. PHarness supplies and executes its own tools.
- For the initial test, manually load and keep the selected model loaded. Disable automatic JIT eviction/loading so a cold load or model switch does not obscure latency.

Start the server. Its upstream base URL will be `http://YOUR_RESERVED_IPV4:1234/v1/`. `127.0.0.1` works only for a test on the Minisforum itself; inside Kubernetes it refers to the calling container. [Server settings](https://lmstudio.ai/docs/developer/core/server/settings), [authentication](https://lmstudio.ai/docs/developer/core/authentication), [LAN serving](https://lmstudio.ai/docs/developer/core/server/serve-on-network)

Use Windows Firewall's advanced inbound rule editor to allow TCP 1234 only from the verified cluster egress addresses and your operator computer, on the trusted network profile. Check for an existing broad LM Studio allow rule, since a narrow second rule does not narrow an existing broad one. We will establish the exact cluster source addresses before enabling the target; do not guess the Pod CIDR or permit the entire Internet. The approved PHarness LAN exception uses an exact private IPv4 `/32` and port. HTTP does not encrypt the token or prompts; a trusted HTTPS endpoint is the later alternative if the LAN boundary is unsuitable.

## 3. Check the API from PowerShell

This first test runs **on the Minisforum**, prompts for the token without putting it in shell history, lists model identifiers and requests an inert tool call. Copy the identifier of the model you actually loaded; do not assume a display name is its API identifier.

```powershell
$base = 'http://127.0.0.1:1234/v1'
$secret = Read-Host 'LM Studio PHarness token' -AsSecureString
$credential = [System.Net.NetworkCredential]::new('', $secret)
$headers = @{ Authorization = 'Bearer ' + $credential.Password }
try {
    $models = Invoke-RestMethod -Uri "$base/models" -Headers $headers -TimeoutSec 20
    $models.data | Select-Object id
    $model = Read-Host 'Exact loaded model identifier from the list'
    $body = @{
        model = $model
        messages = @(@{ role = 'user'; content = 'Call report_ready with ready=true.' })
        tools = @(@{
            type = 'function'
            function = @{
                name = 'report_ready'; description = 'Report readiness; performs no action.'
                parameters = @{
                    type = 'object'
                    properties = @{ ready = @{ type = 'boolean' } }
                    required = @('ready'); additionalProperties = $false
                }
            }
        })
        tool_choice = 'required'; parallel_tool_calls = $false
        stream = $false; temperature = 0.1; max_tokens = 256
    }
    $reply = Invoke-RestMethod -Uri "$base/chat/completions" -Method Post `
        -Headers $headers -ContentType 'application/json' `
        -Body ($body | ConvertTo-Json -Depth 12) -TimeoutSec 180
    $calls = @($reply.choices[0].message.tool_calls)
    if ($calls.Count -ne 1 -or $calls[0].function.name -ne 'report_ready') {
        throw 'Expected one structured report_ready tool call.'
    }
    $arguments = $calls[0].function.arguments | ConvertFrom-Json
    if ($arguments.ready -ne $true) { throw 'Unexpected tool arguments.' }
    'Local API and one structured tool call passed. PHarness qualification is still pending.'
} finally {
    Remove-Variable headers, credential, secret -ErrorAction SilentlyContinue
}
```

This simple test deliberately uses a complete JSON response. PHarness additionally requires streamed tool calls, continuation after tool results, correct context handling and its existing qualification gates. Plain text that resembles a tool call is not sufficient. [LM Studio tool support](https://lmstudio.ai/docs/developer/openai-compat/tools)

## 4. Connect PHarness and qualify the candidate

Send the **reserved IP, port, exact loaded model identifier, GGUF identity and configured context size**. Do not paste the token into chat. We will store it using the existing cluster Secret process, mount it only in the gateway, add a new immutable target/policy revision and permit only that IP/port in the gateway's NetworkPolicy. Existing workflow defaults stay unchanged during diagnostics.

The 32K context is for initial setup. Existing M04 profiles can require 65,536 input plus 8,192 output tokens, and Builder/Repair can need more. Before a matched diagnostic, load a context large enough for the unchanged policy and runtime overhead, measure memory and prompt-processing latency, and record it. Do not silently truncate inputs or shrink a benchmark to make the local model pass.

Acceptance proceeds through: authenticated LAN connectivity → streamed protocol/tool round trip → one bounded stage diagnostic → controlled model comparison → connected coding/repair → the unchanged full qualification gates. Measure time to first response, total duration, token usage, context size, tool failures and corrections. A model fitting in memory or passing the small PowerShell test does not close M04.

## 5. Unattended use after the first successful test

LM Studio can run in the background and start at login. That is a convenient first step, but **login startup is not proof of recovery after an unattended reboot**. Confirm server, authentication and the pinned model after a reboot before relying on it for overnight runs. LM Studio also provides the **llmster** headless daemon for Windows/Linux; use that later if an independent background service is needed. Keep the same API contract instead of changing PHarness. [Headless operation](https://lmstudio.ai/docs/developer/core/headless)

Troubleshooting: a timeout usually means sleep, firewall/routing or a cold load; 401 means authentication; missing model means the identifier/load changed; prose instead of `tool_calls` means a model/template/protocol problem. Fix the observed boundary before changing models or increasing budgets.
