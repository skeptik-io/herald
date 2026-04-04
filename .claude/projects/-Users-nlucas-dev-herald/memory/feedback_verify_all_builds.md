---
name: Verify all builds locally before pushing
description: Must run ALL build steps locally (Rust, all SDKs, admin UI) before git push — pushing broken builds is not acceptable.
type: feedback
---

Always verify ALL build steps pass locally before pushing to remote. This includes Rust (fmt, clippy, tests), all SDK type checks (TS browser, TS admin, Go, Python, Ruby), AND the admin UI (`herald-admin-ui` tsc --noEmit).

**Why:** User explicitly said "Please ensure things build locally before pushing. This is not acceptable." after an unused import in the admin UI broke the Docker build in CI.

**How to apply:** Before every `git push`, run the full CI check matrix locally: `cargo fmt --check`, `cargo clippy`, `cargo test --workspace --lib`, `cargo test --test integration`, and `npx tsc --noEmit` / `go build` / `python -c "import"` / `ruby -e "require"` for each SDK. Include `herald-admin-ui` in the check.
