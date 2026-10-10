import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { product } from "./product";
import { App } from "./App";
import "./style.css";

document.title = product.name;
createRoot(document.getElementById("root")!).render(
  <StrictMode><App /></StrictMode>,
);
