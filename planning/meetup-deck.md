# Decisions

- Create a Marp-compatible Markdown deck at `planning/pharness-meetup-slides.md` so it can be presented directly or adapted into another slide tool.
- Keep the meetup story casual but technical: pharness is framed as an agent SDLC control plane, not a chat UI or plugin marketplace.
- Use the actual current artifact state as the source of truth: local-first Rust runtime, Fireworks native tool calling, durable events, approvals, typed read-only cluster capabilities, observations, incidents, remediation plans, work plans, change sets, approval gates, permission grants, and the PHarness UI prototype.
- Include the UI screenshot as a product/operator slide because the prototype already communicates the intended control-plane surface better than a fresh abstract diagram.
- Use Mermaid diagrams for the SDLC resource model and runtime flow.
- Explicitly call out current limitations and risks so the presentation does not overstate V1 as production-autonomous.
- Generate an editable PowerPoint deck at `planning/pharness-meetup.pptx` while preserving the Markdown deck at `planning/pharness-meetup-slides.md`.
- Use a dark PHarness-style visual system and native editable slide elements for the PowerPoint version, with the UI prototype screenshot embedded as product proof.

# Backlog

- Add speaker notes after a dry run if the talk needs timing cues or less dense slides.
- Consider adding a live demo script beside the deck once the exact meetup demo environment is chosen.
- Consider rendering the deck to PDF after content review if the venue expects projected slides rather than Markdown.
- Add a one-slide backup appendix with exact CLI commands only after deciding whether the presentation will include a live demo.
- If the talk needs a stronger visual punch, convert the dense SDLC resource diagram into two slides: current V1 resources and future CRD direction.
