#!/usr/bin/env bash
#
# Synchronize locally built WASM tool and channel overrides into the per-user
# IronClaw extension directories under ~/.ironclaw.
#
# This is useful when you have already installed a local override and want to
# refresh it from the latest build output without touching extensions that are
# not currently installed. The script prefers `wasm32-wasip2` release artifacts
# and falls back to `wasm32-wasip1`, replaces only matching installed
# overrides, and updates the adjacent capabilities manifests at the same time so
# the host metadata stays aligned with the deployed WASM binary.

set -euo pipefail
shopt -s nullglob

cd "$(dirname "$0")/.."

SHARED_WASM_TARGET_DIR="${CARGO_TARGET_DIR:-target/wasm-extensions}"

resolve_installed_wasm() {
    local kind_dir="$1"
    local raw_name="$2"
    local normalized_name="${raw_name//_/-}"

    if [[ -f "${HOME}/.ironclaw/${kind_dir}/${raw_name}.wasm" ]]; then
        printf '%s\n' "${HOME}/.ironclaw/${kind_dir}/${raw_name}.wasm"
        return 0
    fi

    if [[ -f "${HOME}/.ironclaw/${kind_dir}/${normalized_name}.wasm" ]]; then
        printf '%s\n' "${HOME}/.ironclaw/${kind_dir}/${normalized_name}.wasm"
        return 0
    fi

    return 1
}

sync_matching_wasm_artifacts() {
    local kind_dir="$1"
    local source_root="$2"
    local suffix="$3"
    local primary_capabilities_suffix="$4"
    local fallback_capabilities_suffix="${5:-}"
    local target_name source_name wasm_path
    declare -A synced_items=()

    for target_name in wasm32-wasip2 wasm32-wasip1; do
        for wasm_path in "${source_root}"/*/target/"${target_name}"/release/*"${suffix}.wasm"; do
            local wasm_file item_name target_wasm source_capabilities target_capabilities

            wasm_file=$(basename "$wasm_path")
            item_name=${wasm_file%"${suffix}.wasm"}
            if [[ -n "${synced_items[$item_name]:-}" ]]; then
                continue
            fi
            source_name="${item_name//_/-}"

            if ! target_wasm=$(resolve_installed_wasm "$kind_dir" "$item_name"); then
                continue
            fi

            source_capabilities="${source_root}/${source_name}/${source_name}${primary_capabilities_suffix}"
            if [[ -n "$fallback_capabilities_suffix" && ! -f "$source_capabilities" ]]; then
                source_capabilities="${source_root}/${source_name}/${source_name}${fallback_capabilities_suffix}"
            fi
            target_capabilities="${target_wasm%.wasm}.capabilities.json"

            cp -v "$wasm_path" "$target_wasm"
            if [[ -f "$source_capabilities" ]]; then
                cp -v "$source_capabilities" "$target_capabilities"
            fi
            synced_items["$item_name"]=1
        done
        for wasm_path in "${SHARED_WASM_TARGET_DIR}/${target_name}/release/"*"${suffix}.wasm"; do
            local wasm_file item_name target_wasm source_capabilities target_capabilities

            wasm_file=$(basename "$wasm_path")
            item_name=${wasm_file%"${suffix}.wasm"}
            if [[ -n "${synced_items[$item_name]:-}" ]]; then
                continue
            fi
            source_name="${item_name//_/-}"

            if ! target_wasm=$(resolve_installed_wasm "$kind_dir" "$item_name"); then
                continue
            fi

            source_capabilities="${source_root}/${source_name}/${source_name}${primary_capabilities_suffix}"
            if [[ -n "$fallback_capabilities_suffix" && ! -f "$source_capabilities" ]]; then
                source_capabilities="${source_root}/${source_name}/${source_name}${fallback_capabilities_suffix}"
            fi
            target_capabilities="${target_wasm%.wasm}.capabilities.json"

            cp -v "$wasm_path" "$target_wasm"
            if [[ -f "$source_capabilities" ]]; then
                cp -v "$source_capabilities" "$target_capabilities"
            fi
            synced_items["$item_name"]=1
        done
    done
}

sync_tools() {
    sync_matching_wasm_artifacts "tools" "tools-src" "_tool" \
        "-tool.capabilities.json" \
        ".capabilities.json"
}

sync_channels() {
    sync_matching_wasm_artifacts "channels" "channels-src" "_channel" \
        "-channel.capabilities.json" \
        ".capabilities.json"
}

sync_tools
sync_channels
