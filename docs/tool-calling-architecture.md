# Tool-calling architecture

This document summarizes the chat tool-calling path centred on
`ChatDelegate::execute_tool_calls`. It is intended as a compact reference for
reviewers and maintainers who need to understand how preflight checks,
execution, approvals, and post-flight folding interact.

**Figure 1. Tool-calling sequence from `ChatDelegate` entry through preflight,
execution, post-flight folding, and loop outcome selection. The flow records
redacted tool calls on the active turn, checks hooks and approvals before
execution, runs tools inline or in parallel depending on batch size, sanitizes
and records outputs, and may return either a deferred auth response, a pending
approval, or no special loop outcome.**

```mermaid
sequenceDiagram
    participant Delegate as ChatDelegate
    participant ToolExec as tool_exec_module
    participant Session as Session
    participant Thread as Thread
    participant Turn as Turn
    participant Channels as ChannelManager
    participant Tools as ToolRegistry
    participant Safety as SafetyLayer
    participant JobCtx as JobContext
    participant Agent as Agent
    participant reason_ctx as reason_ctx

    Note over Delegate,ToolExec: Entry: NativeLoopDelegate.execute_tool_calls
    Delegate->>ToolExec: execute_tool_calls(delegate, tool_calls, content, reason_ctx)
    ToolExec->>reason_ctx: messages.push(assistant_with_tool_calls)
    ToolExec->>Channels: send_status(Thinking("Executing N tool(s)..."))
    ToolExec->>ToolExec: record_redacted_tool_calls(delegate, tool_calls)
    ToolExec->>Session: lock()
    Session->>Thread: get_mut(thread_id)
    Thread->>Turn: last_turn_mut().record_tool_call(redacted_args)
    Session-->>ToolExec: unlock()

    Note over ToolExec: Phase 1: Preflight
    ToolExec->>ToolExec: group_tool_calls(delegate, tool_calls)
    ToolExec->>Tools: get(tc.name)
    ToolExec->>Safety: redact_params(tc.arguments, sensitive)
    ToolExec->>Agent: hooks().run(HookEvent::ToolCall)
    alt hook rejects
        ToolExec->>ToolExec: preflight.push(Rejected(msg))
    else needs approval
        ToolExec->>ToolExec: approval_needed = Some(ApprovalCandidate)
        ToolExec-->>Delegate: return NeedApproval
    else runnable
        ToolExec->>ToolExec: preflight.push(Runnable), runnable.push
    end

    Note over ToolExec: Phase 2: Execution
    ToolExec->>ToolExec: run_phase2(delegate, preflight.len, runnable)
    alt small batch
        ToolExec->>ToolExec: run_tool_batch_inline
        loop each runnable tc
            ToolExec->>Delegate: execute_one_tool(tc)
            Delegate->>Channels: send_status(ToolStarted)
            Delegate->>Agent: execute_chat_tool(name, args, job_ctx)
            Agent->>Tools: tools()
            Tools-->>Agent: Tool
            Agent-->>Delegate: Result<String,Error>
            Delegate->>Channels: send_status(ToolCompleted)
            Delegate-->>ToolExec: result
        end
    else large batch
        ToolExec->>ToolExec: run_tool_batch_parallel
        par each runnable tc
            ToolExec->>Channels: send_status(ToolStarted)
            ToolExec->>Tools: execute_chat_tool_standalone(ToolCallSpec)
            Tools->>Safety: execute_tool_with_safety
            Safety-->>Tools: Result<String,Error>
            Tools-->>ToolExec: result
            ToolExec->>Channels: send_status(ToolCompleted)
        end
        ToolExec->>ToolExec: fill missing exec_results with ToolError
    end

    Note over ToolExec: Phase 3: Post-flight
    ToolExec->>ToolExec: run_postflight(delegate, preflight, exec_results)
    loop for each preflight entry
        alt PreflightOutcome::Rejected
            ToolExec->>Session: lock()
            Session->>Thread: get_mut(thread_id)
            Thread->>Turn: last_turn_mut().record_tool_error(msg)
            Session-->>ToolExec: unlock()
            ToolExec->>reason_ctx: messages.push(tool_result error)
        else PreflightOutcome::Runnable
            ToolExec->>ToolExec: process_runnable_tool(delegate, tc, result)
            alt result is Err
                ToolExec->>ToolExec: fold_into_context(error, is_tool_error=true)
            else result is Ok
                ToolExec->>ToolExec: maybe_emit_image_sentinel
                ToolExec->>Safety: sanitize_tool_output or is_valid_json
                ToolExec->>Channels: send_status(ToolResult preview)
                ToolExec->>ToolExec: check_auth_required + parse_auth_result
                alt awaiting token
                    ToolExec->>Session: lock() and enter_auth_mode
                    ToolExec->>Channels: send_status(AuthRequired)
                    ToolExec->>ToolExec: deferred_auth = Some(instructions)
                end
                ToolExec->>JobCtx: tool_output_stash.insert(tc.id, output)
                ToolExec->>ToolExec: fold_into_context(result_content, is_tool_error)
                ToolExec->>Session: lock()
                Session->>Thread: last_turn_mut().record_tool_result or record_tool_error
                Session-->>ToolExec: unlock()
                ToolExec->>reason_ctx: messages.push(tool_result)
            end
        end
    end

    alt deferred_auth is Some
        ToolExec-->>Delegate: LoopOutcome::Response(instructions)
    else approval_needed is Some
        ToolExec->>ToolExec: build_pending_approval(delegate, candidate, tool_calls, reason_ctx)
        ToolExec-->>Delegate: LoopOutcome::NeedApproval(PendingApproval)
    else
        ToolExec-->>Delegate: None
    end
```
