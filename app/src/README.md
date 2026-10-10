# UI module

Five neutral screens share `tokens.css`. The system color scheme selects light or dark; reduced motion removes transitions. Product naming comes from generated `product.ts`.

From `app`, run `bun run dev`, then open `/?demo=1&screen=interactive`. The same route accepts `overlay`, `history`, `settings` and `setup`; `controls=0` hides the development toolbar. Next event advances a finite synthetic stream without microphone, clipboard, hotkeys or desktop input. Production builds exclude the fake stream and show the disconnected state until the host supplies core data.

`source.ts` is the generated-type adapter. `createCoreSource(send)` accepts the host's typed action dispatcher. Feed `receive({ sessions, history, settings, devices })` from host subscriptions, and render `<App source={source} />`. The aggregate is a UI subscription interface, not a new Rust/wire contract. IPC command and event names belong to the host integration. Dispatch preserves action order, including delayed edits. The host's send promise resolves after the action has been applied and its snapshot delivered; errors reject it. Screens import contract types through this adapter.

The unedited editor displays committed bytes plus the current provisional tail. Once edited, core owns the editor value in `SessionView.edited`, including an empty string. Edited regions stay intact while untouched provisional text and subsequent chunks stream according to core snapshots. Original shows raw recognition separately. Optimistic drafts survive navigation and failed actions, and hand back to core when the edit is accepted. Enter always adds a newline. Ctrl+Enter or Cmd+Enter requests Insert after recognition completes; composition shortcuts never insert. Cancel recording is available while recording; Discard appears after release. Each explicit insertion waits for the latest edit to reach core. The UI never invokes clipboard, focus, paste or input APIs.

The host integration supplies the formerly missing operations through generated `HostAction` and
`HostSnapshot` contracts: numbered subscription/bootstrap, ordered action replies, settings and
vocabulary/app saves, model manifest/progress/license, permissions and platform instructions,
scratch-only setup, range-based whole-clip audio playback, SQLite FTS search, retry/delete/retention,
disk usage and overlay notifications/indicators. `main.tsx` uses Tauri Channels and invoke; normal
browser builds stay disconnected. See `../src-tauri/README.md` for IPC names and acceptance scope.

Checks from `app`:

```sh
CARGO_BUILD_JOBS=2 bun run typecheck
CARGO_BUILD_JOBS=2 bun run build
bun test src/editor.test.ts src/source.test.ts
bash src/verify-ui.sh http://127.0.0.1:1422 /path/outside/repo/ui-evidence
```

The browser check requires a running Vite development server. It uses `bunx agent-browser` in an isolated headless session, exercises editing and keyboard/history behavior, writes fifteen screenshots plus accessibility reports, and closes its browser. Set `AGENT_BROWSER_EXECUTABLE_PATH` for an existing Chromium binary. Inspect every screenshot after running it; capture alone is not visual verification. Browser checks cover renderer behavior; native permissions and delivery use the host/platform acceptance checks.
