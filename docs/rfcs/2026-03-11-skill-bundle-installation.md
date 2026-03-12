# Skill Bundle Installation And Bundled File Access

**Status:** Proposed
**Author:** Codex
**Date:** 2026-03-11

## Summary

IronClaw's current skill system is effectively single-file. It discovers and
injects `SKILL.md`, but installation from a ZIP currently keeps only the
top-level `SKILL.md` and discards the rest of the archive. That means skills
cannot reliably ship bundled reference material, templates, or images.

This RFC proposes a first-class multi-file skill bundle format:

1. Introduce `.skill` files, which are ZIP archives with `SKILL.md` at the
   archive root.
2. Allow optional `references/` and `assets/` directories inside the bundle.
3. Support installation from either an uploaded `.skill` file or an HTTPS URL.
4. Expose a dedicated read-only interface so the model can read bundled files
   from an installed skill without requiring general filesystem access.
5. Mirror the progressive-discovery model used by the codex harness: advertise
   the `SKILL.md` path, inject only `SKILL.md` content when a skill is active,
   and load ancillary files lazily when they are actually needed.

Phase 1 explicitly does not support bundled scripts or executables.

## Problem

### IronClaw currently drops ancillary skill files

The current install flow can fetch a raw `SKILL.md` or a ZIP, but when given a
ZIP it extracts only the root `SKILL.md`. Sibling files such as
`references/guide.md` or `assets/logo.png` are ignored.

This creates an awkward mismatch:

1. Skill authors naturally want to ship curated reference material beside the
   prompt.
2. The runtime prompt only contains `SKILL.md`, so any ancillary material must
   be pasted into the prompt body or lost.
3. The model has no bounded interface for reading a file from an installed
   skill.

### The current prompt contract is too narrow

IronClaw's skill runtime currently injects only the parsed prompt body from
`SKILL.md`. It does not inject the on-disk skill path, and it does not provide
any skill-scoped file-reading interface.

As a result, even if `references/` or `assets/` were preserved during install,
the model would not have a safe, explicit way to retrieve them.

## Reference Model

The codex harness already demonstrates the right high-level interaction model,
even though it relies on local file tools rather than a dedicated skill-file
API:

1. Skills are discovered by locating `SKILL.md` files and recording their
   canonical paths.
2. The global instructions list each available skill with the `SKILL.md` path
   and tell the model to resolve relative references from there.
3. When a skill is explicitly selected, the harness injects the full
   `SKILL.md` contents plus the skill path.
4. Ancillary files are read lazily only when the skill instructions point to
   them.

This is the right progressive-discovery model for IronClaw too. The difference
is that IronClaw should not depend on raw filesystem tools for this. It should
provide an explicit, skill-scoped, read-only interface.

## Goals

1. Support multi-file skill installation with a narrow, easy-to-validate
   archive format.
2. Preserve bundled `references/` and `assets/` during installation.
3. Support installation from both HTTPS URL and direct upload.
4. Allow the model to read bundled skill files on demand through a dedicated
   tool.
5. Keep the prompt contract progressive: advertise a skill, inject only
   `SKILL.md`, load ancillary files lazily.
6. Avoid expanding skill bundles into an execution surface.

## Non-goals

1. Running bundled scripts.
2. Supporting bundled binaries or executables.
3. Supporting arbitrary top-level directories inside the bundle.
4. Exposing raw local filesystem paths or generic file reads to the model.
5. Solving every future asset use case, such as image rendering, in phase 1.

## Proposed Bundle Format

### Archive type

The new distributable format is a `.skill` file, implemented as a ZIP archive.

### Required structure

Every bundle must contain:

```text
SKILL.md
```

### Optional structure

The bundle may also contain:

```text
references/<files...>
assets/<files...>
```

Examples:

```text
SKILL.md
references/usage.md
references/troubleshooting/api-errors.md
assets/logo.png
assets/prompt-template.txt
```

### Disallowed content in phase 1

The installer must reject bundles containing any of the following:

1. `scripts/` or `bin/` directories.
2. Symlinks, hard links, or archive entries with special file types.
3. Absolute paths or traversal paths such as `../foo`.
4. Files outside the allowed top-level locations.
5. Executable files, including common script extensions such as `.sh`, `.py`,
   `.js`, `.ps1`, `.bat`, `.cmd`, `.rb`, and `.pl`.
6. Duplicate normalized paths, including case-fold collisions on
   case-insensitive filesystems.

The narrow rule is deliberate: a skill bundle is documentation plus passive
assets, not a plugin or command package.

## Installation Flows

### 1. Upload install

The Skills page should support uploading a local `.skill` file.

Recommended request shape:

- `multipart/form-data`
- one file part containing the archive
- optional display metadata if the UI needs it before parsing

The server validates the archive before writing anything to the installed skill
directory.

### 2. HTTPS URL install

The existing URL install flow should be extended so the URL may point to either:

1. a raw `SKILL.md`, preserving current behaviour
2. a `.skill` ZIP archive

The existing HTTPS-only and SSRF protections should remain in force.

### 3. Unified install endpoint

IronClaw should keep one logical install endpoint that accepts exactly one of:

1. inline `content` for raw `SKILL.md`
2. `url` for remote `SKILL.md` or `.skill`
3. uploaded file data for local `.skill`

This keeps UI and API behaviour aligned while preserving current single-file
installs.

## Validation And Extraction

### Validation rules

Before extraction, the installer should validate:

1. the archive contains exactly one root `SKILL.md`
2. every other entry is under `references/` or `assets/`
3. no entry exceeds a per-file size cap
4. the whole archive stays under a total size cap
5. file count stays under a bounded limit
6. all text files that must be parsed as text are valid UTF-8

### Extraction rules

The installer should extract the bundle into the installed skill root:

```text
<install-root>/<skill-name>/SKILL.md
<install-root>/<skill-name>/references/...
<install-root>/<skill-name>/assets/...
```

Extraction should be staged in a temporary directory and then committed
atomically, so a failed install does not leave a partial skill tree behind.

## Runtime Model Interface

### Design principle

Mirror the codex harness discovery flow, but replace generic local file reads
with an explicit read-only skill-file tool.

### 1. Discovery surface

The general skill instructions shown to the model should continue to list
available skills by name, description, and canonical `SKILL.md` path.

The usage rules should be updated so that when `SKILL.md` references
`references/...` or `assets/...`, the model is told to use the skill-file tool
to retrieve only the specific files it needs.

### 2. Active-skill injection

When a skill is activated, IronClaw should inject:

1. the skill name
2. the canonical `SKILL.md` path
3. the full `SKILL.md` contents

This is the same progressive-discovery shape used by the codex harness. The
model gets the main instructions and the root path, but not the entire bundle
eagerly.

An illustrative injected block:

```xml
<skill name="openai-docs" path="/home/user/.ironclaw/skills/openai-docs/SKILL.md">
...contents of SKILL.md...
</skill>
```

### 3. New tool: `skill_read_file`

IronClaw should expose a new read-only tool for reading installed skill files.

Suggested schema:

```json
{
  "type": "object",
  "properties": {
    "skill": {
      "type": "string",
      "description": "Installed skill name exactly as advertised to the model."
    },
    "path": {
      "type": "string",
      "description": "Bundle-relative path, such as `SKILL.md` or `references/usage.md`."
    }
  },
  "required": ["skill", "path"],
  "additionalProperties": false
}
```

Suggested successful response for text files:

```json
{
  "skill": "openai-docs",
  "path": "references/latest-model.md",
  "mime_type": "text/markdown",
  "content": "# Latest model\n..."
}
```

Suggested error shape:

```json
{
  "error": "File is not available for reading",
  "skill": "openai-docs",
  "path": "scripts/install.sh"
}
```

### Tool semantics

`skill_read_file` must:

1. resolve `path` relative to the installed root for the named skill
2. reject absolute paths and traversal
3. allow only `SKILL.md`, `references/**`, and `assets/**`
4. enforce a maximum returned file size
5. return text content only in phase 1

For binary assets under `assets/`, the tool should return a typed error or a
non-inline metadata result rather than base64-encoding large blobs into the
prompt path. Phase 1 should optimize for text references, not bulk binary
transport.

## Why A Dedicated Tool Instead Of Raw File Paths

The codex harness can rely on ordinary local file reads because the agent runs
with direct repository and local-path access. IronClaw should not couple skill
usage to general filesystem visibility.

A dedicated `skill_read_file` tool has cleaner boundaries:

1. it is read-only
2. it is scoped to installed skills
3. it naturally enforces path and size validation
4. it works the same way in local and hosted execution modes
5. it gives the model a stable contract independent of local filesystem layout

## Data Model Changes

IronClaw should extend the loaded skill model so the runtime retains:

1. the canonical path to `SKILL.md`
2. the canonical skill root directory
3. whether the skill was installed as a single file or as a bundle

The existing prompt-only `prompt_content` field is not sufficient for
progressive file access because the runtime also needs a canonical root for
`skill_read_file`.

## UI Changes

The Skills page should support two install inputs:

1. HTTPS URL
2. local `.skill` upload

Recommended UI behaviour:

1. keep the current URL field
2. add a file picker for `.skill`
3. show a clear note that phase 1 supports bundled `references/` and `assets/`
   but not scripts or executables

## Security Considerations

1. Preserve the current HTTPS-only and SSRF protections for remote installs.
2. Reject traversal, links, and unsupported entry types during ZIP validation.
3. Reject executables and script-like file extensions in phase 1.
4. Keep `skill_read_file` read-only and skill-scoped.
5. Apply conservative response-size limits so large reference files cannot blow
   up prompt size unexpectedly.

## Compatibility

Single-file `SKILL.md` installs remain supported and should continue to work
unchanged.

The new bundle flow adds capability without requiring existing skills to
repackage immediately.

## Testing

### Unit tests

Add coverage for:

1. valid `.skill` archive with root `SKILL.md` plus `references/` and `assets/`
2. rejection of `scripts/`
3. rejection of traversal paths
4. rejection of duplicate normalized paths
5. rejection of executable extensions
6. `skill_read_file` success for `SKILL.md` and `references/...`
7. `skill_read_file` rejection for files outside the allowed roots

### Behavioural tests

Add end-to-end coverage for:

1. install by upload
2. install by HTTPS URL
3. activated skill prompt includes canonical `SKILL.md` path
4. model can read a referenced bundled text file through `skill_read_file`
5. plain `SKILL.md` installs still work

## Rollout Plan

1. Implement archive validator and extractor.
2. Extend install API and web UI for upload plus `.skill` URL handling.
3. Store canonical skill root and `SKILL.md` path in the loaded skill model.
4. Inject the canonical skill path into active-skill context.
5. Add `skill_read_file`.
6. Add tests for installation, validation, and runtime reads.
7. Update user-facing skill authoring documentation with the new bundle format.

## Alternatives Considered

### 1. Keep skills single-file only

Rejected because it forces authors to inline all supporting material into
`SKILL.md`, which scales badly and makes curated reference packs impossible.

### 2. Inject the whole bundle into the prompt

Rejected because it is token-expensive and defeats progressive discovery.

### 3. Expose raw filesystem reads to the model

Rejected because it creates broader capability exposure than the skill system
actually needs. A skill-scoped read tool is the narrower contract.

### 4. Support `scripts/` immediately

Rejected for phase 1 because it expands the design from passive resource bundle
to executable package. That needs separate review.
