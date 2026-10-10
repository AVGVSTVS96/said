import { product } from "./product";

const screens = ["overlay", "interactive", "history", "settings", "setup"] as const;

export function App() {
  const requested = new URLSearchParams(window.location.search).get("screen");
  const screen = screens.find((screen) => screen === requested) ?? "setup";
  return (
    <main>
      <h1>{product.name}</h1>
      <p>{screen}</p>
      <p>Milestone 0: shared contracts and app shell. Dictation is not connected yet.</p>
    </main>
  );
}
