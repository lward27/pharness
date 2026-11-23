# Campus lab harness

Lab sandboxes ship with [OpenCode](https://opencode.ai) as the default coding
harness. Nothing here is specific to OpenCode: any harness that speaks the
OpenAI chat-completions API works the same way.

- `campus-model-proxy` runs in every lab on `http://127.0.0.1:4791/v1`. It
  forwards model requests to the Campus broker with the session's current
  broker token, read from `/var/run/campus-broker/token` on every request. A
  harness can use any API key; it never sees a Campus credential and never
  has to handle token rotation.
- The broker maps Campus model aliases (`campus-default`) to the
  institution's model provider and holds the provider key. The sandbox never
  does.
- `opencode/` is the lab's OpenCode configuration, selected by
  `OPENCODE_CONFIG` and `OPENCODE_CONFIG_DIR` in the lab provider's
  environment. It defaults to the `tutor` agent, which explains and hints but
  cannot edit files or run commands; Tab switches to the `build` agent, which
  asks before every edit and command.

To use another harness, install it in the lab image and point it at the
proxy, for example Aider:

    aider --openai-api-base http://127.0.0.1:4791/v1 --openai-api-key campus-lab \
      --model openai/campus-default
