#!/usr/bin/env bash
set -euo pipefail

origin=${1:?Usage: bash src/verify-ui.sh http://127.0.0.1:1422 /path/to/evidence}
evidence=${2:?Provide an evidence directory outside the repository}
mkdir -p "$evidence/tmp"
evidence=$(cd "$evidence" && pwd)
export TMPDIR="$evidence/tmp"
export AGENT_BROWSER_SESSION
AGENT_BROWSER_SESSION=$(bunx agent-browser session id --scope worktree --prefix voice-ui-check)
export AGENT_BROWSER_HEADED=false
printf '{}\n' > "$evidence/browser-config.json"

browser() {
  bunx agent-browser --config "$evidence/browser-config.json" --headed false "$@"
}
trap 'browser close >/dev/null 2>&1' EXIT

browser open "$origin/?demo=1&screen=interactive"
browser wait --fn "document.querySelector('textarea')?.value === 'um, no, keep the the old value'"
browser find label 'Dictation text' fill '  my edit stays  '
browser press Enter
browser eval --stdin <<'JS'
(() => {
  if (document.querySelector('textarea').value !== '  my edit stays  \n' || document.querySelector('.action-feedback')) throw Error('Enter submitted or changed literal text');
  return {pass: 'Enter adds a literal newline'};
})()
JS
for event in 1 2 3 4 5 6; do browser find role button click --name 'Next event'; done
browser eval --stdin <<'JS'
(() => {
  if (document.querySelector('textarea').value !== "  my edit stays  \nuse cache_key, no no don't rename it.") throw Error('Late recognition overwrote the draft');
  return {pass: 'Draft survives late partials, commits and final'};
})()
JS
browser find role button click --name 'Original +'
browser eval --stdin <<'JS'
(() => {
  if (document.querySelector('.original-panel .verbatim').textContent !== "um, no, keep the the old value, actually no, use cache_key, no no don't rename it.") throw Error('Original text was rewritten');
  return {pass: 'Original recognition remains byte-identical'};
})()
JS
browser find role button click --name 'History'
browser find label 'Search history' fill 'my edit'
browser wait --fn "document.querySelectorAll('.history-entry').length === 1"
browser eval --stdin <<'JS'
(() => {
  if (document.querySelector('.history-text').textContent !== "  my edit stays  \nuse cache_key, no no don't rename it.") throw Error('History lost the edited bytes');
  return {pass: 'History searches edited text'};
})()
JS
browser find role button click --name 'Edit'
browser eval --stdin <<'JS'
(() => {
  if (document.activeElement !== document.querySelector('textarea') || document.querySelector('textarea').value !== "  my edit stays  \nuse cache_key, no no don't rename it.") throw Error('Edit did not restore and focus the draft');
  return {pass: 'History editor restores and focuses the saved draft'};
})()
JS
browser press Tab
browser eval --stdin <<'JS'
(() => {
  if (!document.activeElement.textContent.includes('Original') || getComputedStyle(document.activeElement).outlineStyle === 'none') throw Error('Keyboard focus is not visible');
  return {pass: 'Keyboard navigation has a visible focus ring'};
})()
JS
browser find label 'Dictation text' click
browser press Enter
browser eval --stdin <<'JS'
(() => {
  if (document.querySelector('.action-feedback')) throw Error('Plain Enter in a saved editor submitted');
  return {pass: 'Enter remains a newline after recognition finishes'};
})()
JS
browser press Control+Enter
browser wait --fn "document.querySelector('.action-feedback')?.textContent.includes('insert action recorded')"
browser eval "({pass: 'Ctrl+Enter explicitly requests insertion'})"

browser eval --stdin <<'JS'
document.querySelector('textarea').dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }));
JS
browser press Control+Enter
browser eval --stdin <<'JS'
(() => {
  if (document.querySelector('.action-feedback')) throw Error('A composition shortcut inserted text');
  document.querySelector('textarea').dispatchEvent(new CompositionEvent('compositionend', { bubbles: true }));
  return {pass: 'Composition shortcuts do not insert'};
})()
JS
browser find role button click --name Interactive
browser set media light reduced-motion
browser eval --stdin <<'JS'
(() => {
  if (getComputedStyle(document.querySelector('.status')).animationName !== 'none') throw Error('Reduced motion still animates');
  return {pass: 'Reduced motion disables status animation'};
})()
JS
browser set media light

browser set viewport 1280 800
for theme in light dark; do
  browser set media "$theme"
  for screen in overlay interactive history settings setup; do
    browser open "$origin/?demo=1&screen=$screen&controls=0"
    browser wait --fn "document.title.startsWith('${screen^}')"
    browser eval --stdin <<'JS'
(() => {
  if (document.documentElement.scrollWidth > innerWidth) throw Error('Horizontal overflow');
  return {title: document.title, width: innerWidth, horizontalOverflow: false};
})()
JS
    browser screenshot --full "$evidence/$screen-$theme.png"
    browser a11y --tags wcag2a,wcag2aa --json > "$evidence/$screen-$theme-a11y.json"
  done
done

browser set viewport 390 844
for screen in overlay interactive history settings setup; do
  browser open "$origin/?demo=1&screen=$screen&controls=0"
  browser wait --fn "document.title.startsWith('${screen^}')"
  browser eval --stdin <<'JS'
(() => {
  if (document.documentElement.scrollWidth > innerWidth) throw Error('Narrow window overflows horizontally');
  return {pass: 'Narrow window fits', title: document.title};
})()
JS
  browser screenshot --full "$evidence/$screen-narrow.png"
done
browser errors --json > "$evidence/browser-errors.json"

bun - "$evidence" <<'JS'
const evidence = process.argv.at(-1);
let reports = 0;
let incomplete = 0;
for (const name of new Bun.Glob("*-a11y.json").scanSync(evidence)) {
  const report = await Bun.file(`${evidence}/${name}`).json();
  if (!report.success || report.data.counts.violations !== 0) throw Error(`Accessibility check failed: ${name}`);
  reports++;
  incomplete += report.data.counts.incomplete;
}
const errors = await Bun.file(`${evidence}/browser-errors.json`).json();
if (!errors.success || errors.data.errors.length !== 0) throw Error("Browser reported an error");
console.log(JSON.stringify({reports, violations: 0, incomplete, browserErrors: 0}));
JS
