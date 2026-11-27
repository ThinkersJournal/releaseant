kind: untrack a stray compiled Python file
---
Stop tracking `tools/__pycache__/cite-selftest.cpython-315.pyc`, which slipped into the cite-update merge, and ignore `__pycache__/` and `*.pyc`. Verified: `git ls-files | grep pycache` is empty on the branch.
