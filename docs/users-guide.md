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

### Minimal workflow example

```text
1. Worker starts and fetches the hosted-visible remote catalogue.
2. Worker registers one local proxy per advertised remote tool.
3. Model selects a hosted-visible remote tool such as `github_search`.
4. Worker sends one generic remote-tool execution request:
   POST /worker/{job_id}/tools/execute
   {
     "tool_name": "github_search",
     "params": { "query": "hosted worker transport" }
   }
5. Orchestrator validates the params against the advertised schema, executes
   the tool, and returns the normal `ToolOutput` payload.
```

Not every orchestrator-owned tool is visible in hosted mode. Tools may still
be hidden when they:

### Visibility rules & defaults

| Case | Default visibility | Expected behaviour when selected |
| --- | --- | --- |
| Requires interactive approval | Hidden | Omitted from the remote catalogue because hosted workers cannot satisfy interactive approval prompts |
| Depends on worker-local execution | Hidden | Omitted because the orchestrator cannot safely proxy a worker-local dependency |
| Other ineligible or protected cases | Hidden | Omitted from the remote catalogue and rejected if called directly |
| Hosted-visible orchestrator-owned tool | Visible | Advertised unchanged in the remote catalogue and executed through the generic remote-tool request |

- require interactive approval that hosted workers cannot satisfy
- depend on worker-local execution rather than orchestrator-owned execution
- are otherwise not eligible for the hosted remote-tool path

If a hosted-visible remote tool is selected, the worker sends one generic
execution request to the orchestrator rather than using tool-family-specific
proxy routes.
