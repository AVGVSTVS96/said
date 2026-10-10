import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { product } from "./product";
import { App } from "./App";
import "./tokens.css";
import "./style.css";

document.title = product.name;
const root = createRoot(document.getElementById("root")!);
if (
  import.meta.env.DEV &&
  new URLSearchParams(window.location.search).get("demo") === "1"
) {
  const { createFakeSource } = await import("./fake");
  const demo = createFakeSource();
  root.render(
    <StrictMode>
      <App source={demo} demo={demo} />
    </StrictMode>,
  );
} else {
  root.render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}
