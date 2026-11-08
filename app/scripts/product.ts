const root = new URL("../../", import.meta.url);

const isText = (value: unknown): value is string => typeof value === "string" && value !== "";

const isObject = (value: unknown): value is object => typeof value === "object" && value !== null;

const toml = new Map<string, unknown>(
  Object.entries(Bun.TOML.parse(await Bun.file(new URL("product.toml", root)).text())),
);

const field = (key: string) => {
  const value = toml.get(key);

  if (!isText(value)) {
    throw new Error(`product.toml needs a nonempty ${key}`);
  }

  return value;
};

const writeIfChanged = async (path: URL, content: string) => {
  const file = Bun.file(path);

  if (!(await file.exists()) || (await file.text()) !== content) {
    await Bun.write(file, content);
  }
};

const product = {
  name: field("name"),
  bundle_id: field("bundle_id"),
  storage_key: field("storage_key"),
};

const base: unknown = await Bun.file(new URL("app/src-tauri/tauri.base.json", root)).json();

if (!isObject(base)) {
  throw new Error("tauri.base.json must hold an object");
}

await writeIfChanged(
  new URL("app/src-tauri/tauri.conf.json", root),
  `${JSON.stringify({ ...base, productName: product.name, identifier: product.bundle_id }, null, 2)}\n`,
);

await writeIfChanged(
  new URL("app/src/product.ts", root),
  `export const product = ${JSON.stringify(product, null, 2)} as const;\n`,
);
