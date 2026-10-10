import { readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const root = new URL("../../", import.meta.url);
const parsed = Bun.TOML.parse(await readFile(new URL("product.toml", root), "utf8")) as Record<string, unknown>;
const product = {
  name: field("name"),
  bundle_id: field("bundle_id"),
  storage_key: field("storage_key"),
};
const base = JSON.parse(await readFile(new URL("app/src-tauri/tauri.base.json", root), "utf8"));
const config = { ...base, productName: product.name, identifier: product.bundle_id };
await writeIfChanged(new URL("app/src-tauri/tauri.conf.json", root), `${JSON.stringify(config, null, 2)}\n`);
await writeIfChanged(new URL("app/src/product.ts", root), `export const product = ${JSON.stringify(product, null, 2)} as const;\n`);

function field(key: string): string {
  const value = parsed[key];
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(`product.toml needs a nonempty ${key}`);
  }
  return value;
}

async function writeIfChanged(path: URL, content: string) {
  const previous = await readFile(path, "utf8").catch((error: NodeJS.ErrnoException) => {
    if (error.code !== "ENOENT") throw error;
    return undefined;
  });
  if (previous !== content) await writeFile(fileURLToPath(path), content);
}
