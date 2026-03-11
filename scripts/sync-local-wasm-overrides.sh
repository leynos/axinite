#!/usr/bin/env bash
#
# Synchronize locally built WASM tool and channel overrides into the per-user
# IronClaw extension directories under ~/.ironclaw.
#
# This is useful when you have already installed a local override and want to
# refresh it from the latest build output without touching extensions that are
# not currently installed. The script scans wasm32-wasip1 release artifacts for
# tools and channels, replaces only matching installed overrides, and updates
# the adjacent capabilities manifests at the same time so the host metadata
# stays aligned with the deployed WASM binary.

set -euo pipefail
shopt -s nullglob

cd "$(dirname "$0")/.."

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

sync_tools() {
    local wasm_path
    for wasm_path in tools-src/*/target/wasm32-wasip1/release/*_tool.wasm; do
        local wasm_file toolname source_name target_wasm source_capabilities target_capabilities

        wasm_file=$(basename "$wasm_path")
        toolname=${wasm_file%_tool.wasm}
        source_name="${toolname//_/-}"

        if ! target_wasm=$(resolve_installed_wasm "tools" "$toolname"); then
            continue
        fi

        source_capabilities="tools-src/${source_name}/${source_name}-tool.capabilities.json"
        target_capabilities="${target_wasm%.wasm}.capabilities.json"

        cp -v "$wasm_path" "$target_wasm"
        cp -v "$source_capabilities" "$target_capabilities"
    done
}

sync_channels() {
    local wasm_path
    for wasm_path in channels-src/*/target/wasm32-wasip1/release/*_channel.wasm; do
        local wasm_file channelname source_name target_wasm source_capabilities target_capabilities

        wasm_file=$(basename "$wasm_path")
        channelname=${wasm_file%_channel.wasm}
        source_name="${channelname//_/-}"

        if ! target_wasm=$(resolve_installed_wasm "channels" "$channelname"); then
            continue
        fi

        source_capabilities="channels-src/${source_name}/${source_name}-channel.capabilities.json"
        if [[ ! -f "$source_capabilities" ]]; then
            source_capabilities="channels-src/${source_name}/${source_name}.capabilities.json"
        fi
        target_capabilities="${target_wasm%.wasm}.capabilities.json"

        cp -v "$wasm_path" "$target_wasm"
        cp -v "$source_capabilities" "$target_capabilities"
    done
}

sync_tools
sync_channels
