# User's guide

This guide captures operator-visible behaviour for the current Axinite
runtime.

## Hosted workers and remote tools

Hosted workers now advertise two tool families to the model:

- container-local development tools such as `shell`, `read_file`,
  `write_file`, `list_dir`, and `apply_patch`
- orchestrator-owned hosted-visible tools fetched from the worker-authenticated
  remote catalogue at startup

When the worker receives the remote catalogue, it registers a local proxy for
each advertised remote tool. The model therefore sees the orchestrator-owned
tool's `name`, `description`, and JSON Schema parameters unchanged, even though
execution still happens in the orchestrator process.

Not every orchestrator-owned tool is visible in hosted mode. Tools may still
be hidden when they:

- require interactive approval that hosted workers cannot satisfy
- depend on worker-local execution rather than orchestrator-owned execution
- are otherwise not eligible for the hosted remote-tool path

If a hosted-visible remote tool is selected, the worker sends one generic
execution request to the orchestrator rather than using tool-family-specific
proxy routes.
