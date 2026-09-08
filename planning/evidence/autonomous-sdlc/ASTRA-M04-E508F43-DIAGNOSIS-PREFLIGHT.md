# ASTRA M04: Diagnosis protocol timeout

Status: protocol prerequisite failed; primary semantic diagnostic and matched control remain undispatched. No provider, policy or timeout changed.

On source `e508f430e9f006c5f3e9f74fdd120d275605d85c`, `test-diagnosis-nemotron-v2@v1` targets `fireworks-nemotron-lightning-3p5-30b-a3b@v1`. The [native preflight receipt](ASTRA-M04-E508F43-TEST-DIAGNOSIS-PRIMARY-PROTOCOL.json), verification `inferverify_01a08211935575129894e2d5f71a9263`, records `status: failed` and `gateway protocol verification timed out`. The operation ran 17:29:44.998–17:34:45.087 UTC on 2026-09-08. Protocol calibration is null; no 30/30 result or qualification exists.

The native timeout occurs while awaiting the gateway HTTP response under the target's existing 300-second first-response limit. The failed wrapper marks reachability, visibility and tool compatibility false together; these are not independent observations proving a missing model, invalid token or an incompatible tool schema. The preceding readiness, registry-alignment and target-alias checks must pass to reach this timeout. The exact upstream cause and any incurred usage are not established by this receipt. Do not classify this as a failed semantic diagnosis or lower acceptance requirements.

The external observation script saved the completed native failure before its summary printer encountered null calibration data. That display-only exception has been corrected; the saved operation was reconciled without another POST. No automatic retry or longer deadline was used.

The separately registered Verifier remains eligible for its declared independent preflight and canaries. A fresh successful preflight is required before Diagnosis can proceed. Its future matched control needs an actual primary report on the same source/suite/inputs; do not fabricate a reference or silently replace the candidate.

See [M04](../../programs/autonomous-sdlc/ASTRA-04-CODING-RELIABILITY-QUALIFICATION.md). The timeout is a concrete runtime dependency, while the [Planner correction](ASTRA-M04-E508F43-PLANNER-ANALYSIS.md) and other independent implementation work can continue.
