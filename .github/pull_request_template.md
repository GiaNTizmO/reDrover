## Summary

<!-- One or two sentences. What does this PR do and why? -->

## Type of change

- [ ] Bug fix
- [ ] New feature
- [ ] Refactor / cleanup
- [ ] Documentation
- [ ] CI / build

## Checklist

- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] `cargo test --workspace` passes
- [ ] If I added a `drover.ini` field: Rust types, C++ types, and `dist/drover.ini` are all updated
- [ ] If I added a `drover.ini` field that should be user-visible: GUI form has a control for it
- [ ] If I added a Windows hook: also considered POSIX (`posix_preload.cpp`)
- [ ] Docs updated (`AGENTS.md`, `ARCHITECTURE.md`, or relevant `docs/*.md`)

## Testing

<!--
How did you verify this works? For native changes, please mention:
- OS + Discord build tested on
- Whether voice / chat were exercised
- drover.log excerpt showing the relevant event lines
-->

## Related issues

<!-- Closes #N, refs hdrover/discord-drover#65, etc. -->
