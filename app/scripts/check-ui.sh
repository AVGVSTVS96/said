#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
export AGENT_BROWSER_SESSION="said-ui-regressions-$$" AGENT_BROWSER_HEADED=false
scratch="$(mktemp -d "${TMPDIR:-/var/tmp}/said-ui.XXXXXX")"
printf '{}\n' > "$scratch/config.json"
browser() { bunx agent-browser --config "$scratch/config.json" "$@"; }
trap 'browser close >/dev/null 2>&1; rm -rf "$scratch"' EXIT
browser open "${1:-http://127.0.0.1:1420}/?screen=setup"
for viewport in '640 680' '640 860' '390 600'; do
    read -r width height <<< "$viewport"
    browser set viewport "$width" "$height"
    browser eval --stdin <<'JS'
(async () => {
  const tests = await import('/scripts/ui-regressions.tsx');
  return { linux: await tests.setupRegression(), mac: await tests.setupRegression(true), review: await tests.interactiveRegression() };
})()
JS
done
