---
description: "ConsensFlow: list the curated agent presets"
disable-model-invocation: true
---

Run the ConsensFlow CLI via the Bash tool and relay its output as-is:

```bash
node "${CONSENSFLOW_HOST_ROOT}/bin/cf.mjs" agents presets
```

If the user wants one configured, suggest `/consensflow:agents add <preset>` (or `add all`).
