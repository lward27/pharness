```bash
./bin/llama-server -m ~/Personal/llama.cpp/models/gpt-oss-120b-UD-Q4_K_XL-00001-of-00002.gguf --port 8080 --jinja  --n-gpu-layers 999 --ctx-size 65536 --batch-size 2048 --ubatch-size 512 --threads 16 --n-predict -1 --flash-attn on --host 192.168.10.71
```

Cleaned command:
```bash
./bin/llama-server \
  -m "$HOME/Personal/llama.cpp/models/gpt-oss-120b-UD-Q4_K_XL-00001-of-00002.gguf" \
  --alias gpt-oss-120b-local \
  --host 192.168.10.71 \
  --port 8080 \
  --api-key-file "$HOME/.config/pharness/llama-api-key" \
  --jinja \
  --parallel 1 \
  --n-gpu-layers 999 \
  --ctx-size 65536 \
  --batch-size 2048 \
  --ubatch-size 512 \
  --threads 16 \
  --n-predict -1 \
  --flash-attn on
```
