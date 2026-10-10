# UI module

Five neutral screens share `tokens.css`. The system color scheme selects light or dark; reduced motion removes transitions. Product naming comes from generated `product.ts`.

From `app`, run `bun run dev`, then open `/?demo=1&screen=interactive`. The same route accepts `overlay`, `history`, `settings` and `setup`; `controls=0` hides the development toolbar. Next event advances a finite synthetic stream without microphone, clipboard, hotkeys or desktop input. Production builds exclude the fake stream and show the disconnected state until the host supplies core data.

`source.ts` is the generated-type adapter. `createCoreSource(send)` accepts the host's typed action dispatcher. Feed `receive({ sessions, history, settings, devices })` from host subscriptions, and render `<App source={source} />`. The aggregate is a UI subscription interface, not a new Rust/wire contract. IPC command and event names belong to the host integration. Dispatch preserves action order, including delayed edits. Screens import contract types through this adapter.

The unedited editor displays committed bytes plus the current provisional tail. Once edited, the draft belongs to the user; late recognition continues under Original. The adapter already recognizes the incoming `SessionView.edited` field planned by core, including an empty string. Local drafts survive navigation and failed actions. Enter always adds a newline. Ctrl+Enter or Cmd+Enter requests Insert after recognition completes; composition shortcuts never insert. Each explicit insertion waits for the latest edit to reach core. The UI never invokes clipboard, focus, paste or input APIs.

The frozen contracts still need these additions from the integrator:

- Host state subscription/bootstrap and an action dispatch connection in `main.tsx`. Production intentionally does not fabricate state or invent IPC names.
- Settings save, microphone selection and vocabulary/app-rule updates; an engine manifest with local/cloud provenance and vocabulary capability.
- Model size/license/download progress; permission state/request actions; platform setup instructions; a scratch-only dictation target and setup completion.
- Audio playback state/actions or a host-provided audio resource, with optional word timestamps.
- History query/results, retry/delete/retention actions and disk usage. Current search covers loaded original and edited text. Copy, edit, discard, cancel and insert use existing `UiAction` variants.
- Overlay outcome/notification data for disconnect, restart, empty/canceled/error distinctions and cloud/vocabulary/auto-send indicators. Overlay window focus, monitor placement and native dismissal belong to the platform/host.

Unavailable operations are disabled and labeled. Settings/setup forms preview values but do not apply them. No shared contracts or generated files were modified.

Checks from `app`:

```sh
CARGO_BUILD_JOBS=2 bun run typecheck
CARGO_BUILD_JOBS=2 bun run build
bun test src/editor.test.ts src/source.test.ts
bash src/verify-ui.sh http://127.0.0.1:1422 /path/outside/repo/ui-evidence
```

The browser check requires a running Vite development server. It uses `bunx agent-browser` in an isolated headless session, exercises editing and keyboard/history behavior, writes fifteen screenshots plus accessibility reports, and closes its browser. Set `AGENT_BROWSER_EXECUTABLE_PATH` for an existing Chromium binary. Inspect every screenshot after running it; capture alone is not visual verification. Audio playback, native focus behavior, permissions, downloads and actual delivery cannot be verified until their host/core contracts exist.
