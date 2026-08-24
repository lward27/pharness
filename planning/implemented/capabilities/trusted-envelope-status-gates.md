# Decisions

- Trusted envelopes now require the underlying SDLC resource to be `approved`.
- WorkPlan and ChangeSet envelope creation returns conflict for draft, proposed, stale, rejected, executing, completed, and blocked states.
- ChangeSet trusted-envelope creation also requires the parent WorkPlan to be `approved`.
- No operator override was added in this slice. Pre-approval trusted envelopes would weaken the review boundary and should be a separate audited policy if needed later.

# Backlog

- Add a machine-readable conflict payload once API errors grow beyond a simple message string.
- Consider allowing envelope creation during `executing` only if execution state becomes a direct continuation of a previously approved envelope rather than a fresh grant.
- Add UI affordances that disable trusted-envelope actions until the resource is approved.
