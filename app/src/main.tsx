import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { product } from "./product";
import { App } from "./App";
import { createCoreSource } from "./source";
import type { HostSnapshot } from "./contracts";
import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./tokens.css";
import "./style.css";

document.title = product.name;
const root = createRoot(document.getElementById("root")!);
if (
  import.meta.env.DEV &&
  new URLSearchParams(window.location.search).get("demo") === "1"
) {
  const { createFakeSource } = await import("./fake");
  const params = new URLSearchParams(window.location.search);
  const demo = createFakeSource({
    mac: params.get("os") === "mac",
    model: params.get("model") === "downloading" ? "downloading" : "ready",
  });
  root.render(
    <StrictMode>
      <App source={demo} demo={demo} />
    </StrictMode>,
  );
 } else if (isTauri()) {
  const source = createCoreSource(async (action) => {
    const command = ["insert", "copy", "edit", "discard", "cancel", "insert_last"].includes(action.type) ? "ui_action" : "host_action";
    source.receive(await invoke<HostSnapshot>(command, { action }));
  });
  const updates = new Channel<HostSnapshot>();
  updates.onmessage = (snapshot) => source.receive(snapshot);
  try {
    await listen<string>("host-error", (event) => source.disconnect(event.payload));
    source.receive(await invoke<HostSnapshot>("connect", { updates }));
    root.render(<StrictMode><App source={source} /></StrictMode>);
  } catch (error) {
    root.render(<main role="alert">Host connection failed: {String(error)}</main>);
  }
} else {
  root.render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}
