#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
tmp_root="$(mktemp -d "${TMPDIR:-/tmp}/nan-harness-uninstall-smoke.XXXXXX")"
trap 'rm -rf -- "$tmp_root"' EXIT

fake_bin="$tmp_root/bin"
fake_home="$tmp_root/home"
mkdir -p "$fake_bin" "$fake_home"

# Keep package-manager probes deterministic and offline. The scripts must not
# attempt to install, update, or remove anything from the host environment.
for command_name in npm brew pipx python3; do
    cat >"$fake_bin/$command_name" <<'EOF'
#!/usr/bin/env bash
exit 1
EOF
    chmod +x "$fake_bin/$command_name"
done

scripts=()
for script in "$script_dir"/uninstall-*.sh; do
    [[ "$(basename "$script")" == uninstall-common.sh ]] && continue
    scripts+=("$script")
done

expected_count=13
if [[ "${#scripts[@]}" -ne "$expected_count" ]]; then
    printf 'Expected %s harness scripts, found %s.\n' "$expected_count" "${#scripts[@]}" >&2
    exit 1
fi

for script in "${scripts[@]}"; do
    [[ -x "$script" ]] || { printf 'Not executable: %s\n' "$script" >&2; exit 1; }
    HOME="$fake_home" PATH="$fake_bin:/usr/bin:/bin" "$script" --help >/dev/null
    HOME="$fake_home" PATH="$fake_bin:/usr/bin:/bin" "$script" --dry-run --purge --yes >/dev/null
    if HOME="$fake_home" PATH="$fake_bin:/usr/bin:/bin" "$script" --not-an-option >/dev/null 2>&1; then
        printf 'Unknown option unexpectedly accepted: %s\n' "$script" >&2
        exit 1
    fi
done

# Verify the shared destructive contract on a representative standalone state
# path: --purge requires confirmation, --yes removes it, and a second run is
# idempotent. The fake HOME keeps the test entirely isolated.
mkdir -p "$fake_home/.kimi-code"
printf 'fixture\n' >"$fake_home/.kimi-code/config.toml"
if printf 'n\n' | HOME="$fake_home" PATH="$fake_bin:/usr/bin:/bin" \
    "$script_dir/uninstall-kimi.sh" --purge >/dev/null 2>&1; then
    :
else
    printf 'Cancelled purge must be successful.\n' >&2
    exit 1
fi
[[ -f "$fake_home/.kimi-code/config.toml" ]] || { printf 'Cancelled purge removed state.\n' >&2; exit 1; }
HOME="$fake_home" PATH="$fake_bin:/usr/bin:/bin" "$script_dir/uninstall-kimi.sh" --purge --yes >/dev/null
[[ ! -e "$fake_home/.kimi-code" ]] || { printf 'Confirmed purge retained state.\n' >&2; exit 1; }
HOME="$fake_home" PATH="$fake_bin:/usr/bin:/bin" "$script_dir/uninstall-kimi.sh" --purge --yes >/dev/null

printf 'uninstall script smoke tests passed (%s scripts).\n' "${#scripts[@]}"
