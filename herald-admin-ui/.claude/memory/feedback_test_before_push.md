---
name: Test before push
description: Always verify changes locally (docker build, run, etc.) before pushing to remote
type: feedback
---

Always test changes locally before pushing. Don't iterate via CI/CD.

**Why:** User was frustrated by multiple broken pushes (nginx crash, envsubst issues) that could have been caught with a local docker build + run.

**How to apply:** Before any `git push`, run the docker build locally and verify the container starts and works. For nginx configs, test that the container boots without errors.
