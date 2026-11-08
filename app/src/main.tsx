import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./app";
import type { HostSnapshot } from "./contracts";
import { product } from "./product";
import { createCoreSource, isUiAction } from "./source";

import "./tokens.css";
import "./style.css";

document.title = product.name;

const mount = document.querySelector("#root");

if (mount === null) {
  throw new Error("index.html needs a #root element");
}

const root = createRoot(mount);
const params = new URLSearchParams(window.location.search);

if (import.meta.env.DEV && params.get("demo") === "1") {
  const { createFakeSource } = await import("./fake");

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
    source.receive(
      await invoke<HostSnapshot>(isUiAction(action) ? "ui_action" : "host_action", { action }),
    );
  });

  const updates = new Channel<HostSnapshot>((snapshot) => {
    source.receive(snapshot);
  });

  try {
    await listen<string>("host-error", (event) => {
      source.disconnect(event.payload);
    });
    source.receive(await invoke<HostSnapshot>("connect", { updates }));
    root.render(
      <StrictMode>
        <App source={source} />
      </StrictMode>,
    );
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
