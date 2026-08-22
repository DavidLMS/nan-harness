#!/usr/bin/env bash
set -euo pipefail

for label in \
  dev.nan-harness.canary-daily \
  dev.nan-harness.canary-weekly \
  dev.nan-harness.release-gate
do
  launchctl bootout "gui/$(id -u)/$label" 2>/dev/null || true
  rm -f "$HOME/Library/LaunchAgents/$label.plist"
done
