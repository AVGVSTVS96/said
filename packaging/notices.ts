import { createHash } from "node:crypto";
import { existsSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import path from "node:path";

interface Crate {
  id: string;
  name: string;
  version: string;
  source: string | null;
  manifest_path?: string;
  license?: string | null;
  license_file?: string | null;
}

interface CrateNode {
  id: string;
  dependencies: string[];
}

interface LicenseSource {
  file: string;
  url: string;
  sha256: string;
}

interface NpmPackage {
  name: string;
  version: string;
  license?: string;
  dependencies?: Record<string, string>;
}

interface Metadata {
  packages: Crate[];
  resolve: { nodes: CrateNode[] };
}

const isText = (value: unknown): value is string => typeof value === "string";

const isTextOrNull = (value: unknown): value is string | null => value === null || isText(value);

const isListOf =
  <T>(check: (item: unknown) => item is T) =>
  (value: unknown): value is T[] =>
    Array.isArray(value) && value.every(check);

const isTextMap = (value: unknown): value is Record<string, string> =>
  typeof value === "object" && value !== null && Object.values(value).every(isText);

const isCrateIdentity = (
  value: unknown,
): value is Omit<Crate, "manifest_path" | "license" | "license_file"> =>
  typeof value === "object" &&
  value !== null &&
  "id" in value &&
  isText(value.id) &&
  "name" in value &&
  isText(value.name) &&
  "version" in value &&
  isText(value.version) &&
  "source" in value &&
  isTextOrNull(value.source);

const isCrate = (value: unknown): value is Crate =>
  isCrateIdentity(value) &&
  (!("manifest_path" in value) || isText(value.manifest_path)) &&
  (!("license" in value) || isTextOrNull(value.license)) &&
  (!("license_file" in value) || isTextOrNull(value.license_file));

const isCrateNode = (value: unknown): value is CrateNode =>
  typeof value === "object" &&
  value !== null &&
  "id" in value &&
  isText(value.id) &&
  "dependencies" in value &&
  isListOf(isText)(value.dependencies);

const isMetadata = (value: unknown): value is Metadata =>
  typeof value === "object" &&
  value !== null &&
  "packages" in value &&
  isListOf(isCrate)(value.packages) &&
  "resolve" in value &&
  typeof value.resolve === "object" &&
  value.resolve !== null &&
  "nodes" in value.resolve &&
  isListOf(isCrateNode)(value.resolve.nodes);

const isLicenseSource = (value: unknown): value is LicenseSource =>
  typeof value === "object" &&
  value !== null &&
  "file" in value &&
  isText(value.file) &&
  "url" in value &&
  isText(value.url) &&
  "sha256" in value &&
  isText(value.sha256);

const isUpstream = (value: unknown): value is Record<string, LicenseSource[]> =>
  typeof value === "object" &&
  value !== null &&
  Object.values(value).every(isListOf(isLicenseSource));

const isNpmPackage = (value: unknown): value is NpmPackage =>
  typeof value === "object" &&
  value !== null &&
  "name" in value &&
  isText(value.name) &&
  "version" in value &&
  isText(value.version) &&
  (!("license" in value) || isText(value.license)) &&
  (!("dependencies" in value) || isTextMap(value.dependencies));

const isAppPackage = (value: unknown): value is { dependencies: Record<string, string> } =>
  typeof value === "object" &&
  value !== null &&
  "dependencies" in value &&
  isTextMap(value.dependencies);

const parse = <T>(text: string, check: (value: unknown) => value is T, what: string): T => {
  const parsed: unknown = JSON.parse(text);

  if (!check(parsed)) {
    throw new Error(`Unexpected ${what}`);
  }

  return parsed;
};

const root = path.resolve(import.meta.dir, "..");
const [output] = process.argv.slice(2);

if (output === undefined || output === "") {
  throw new Error("Supply a dependency notice output file");
}

const hash = (bytes: string | Uint8Array) => createHash("sha256").update(bytes).digest("hex");

const upstream = new Map(
  Object.entries(
    parse(
      readFileSync(path.join(root, "licenses/upstream.json"), "utf-8"),
      isUpstream,
      "licenses/upstream.json",
    ),
  ),
);

const cargoMetadata = (platform: string) => {
  const result = Bun.spawnSync(
    [
      "cargo",
      "metadata",
      "--locked",
      "--format-version",
      "1",
      "--features",
      "said-app/custom-protocol",
      "--filter-platform",
      platform,
    ],
    { cwd: root },
  );

  if (result.exitCode !== 0) {
    throw new Error(result.stderr.toString());
  }

  const metadata = parse(result.stdout.toString(), isMetadata, "cargo metadata");

  return { packages: metadata.packages, nodes: metadata.resolve.nodes };
};

const packages = new Map<string, Crate>();

for (const platform of ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin"]) {
  const metadata = cargoMetadata(platform);
  const nodes = new Map(metadata.nodes.map((node) => [node.id, node.dependencies]));
  const pending = metadata.packages.filter((pkg) => pkg.name === "said-app").map((pkg) => pkg.id);
  const reached = new Set<string>();

  for (let id = pending.pop(); id !== undefined; id = pending.pop()) {
    if (!reached.has(id)) {
      reached.add(id);
      pending.push(...(nodes.get(id) ?? []));
    }
  }

  for (const pkg of metadata.packages) {
    if (pkg.source !== null && reached.has(pkg.id)) {
      packages.set(pkg.id, pkg);
    }
  }
}

const texts = new Map<string, { text: string; components: Set<string> }>();
const inventory: string[] = [];

const retain = (component: string, file: string, expected?: string) => {
  const bytes = readFileSync(file);
  const digest = hash(bytes);

  if (expected !== undefined && expected !== digest) {
    throw new Error(`License hash mismatch: ${component}`);
  }

  const entry = texts.get(digest) ?? {
    text: bytes.toString("utf-8"),
    components: new Set<string>(),
  };

  entry.components.add(component);
  texts.set(digest, entry);
};

const licenseFiles = (directory: string) =>
  [...new Bun.Glob("**/*").scanSync({ cwd: directory, onlyFiles: true, followSymlinks: true })]
    .filter((file) =>
      /^(?:licen[cs]e|copying|copyright|notice|ofl)(?:[._-]|$)/iu.test(path.basename(file)),
    )
    .filter((file) => !file.endsWith(".spdx"))
    .toSorted();

const crates = [...packages.values()].toSorted((a, b) =>
  `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`, "en"),
);

for (const pkg of crates) {
  const component = `${pkg.name}@${pkg.version}`;

  if (pkg.manifest_path === undefined) {
    throw new Error(`Missing manifest path: ${component}`);
  }

  const directory = path.dirname(pkg.manifest_path);
  const files = licenseFiles(directory);

  inventory.push(
    `${component} | ${pkg.license ?? "see supplied terms"} | https://crates.io/crates/${pkg.name}/${pkg.version}`,
  );

  for (const file of files) {
    retain(`${component}: ${file}`, path.join(directory, file));
  }

  if (
    pkg.license_file !== undefined &&
    pkg.license_file !== null &&
    pkg.license_file !== "" &&
    !files.includes(pkg.license_file)
  ) {
    retain(`${component}: ${pkg.license_file}`, path.join(directory, pkg.license_file));
    files.push(pkg.license_file);
  }

  if (files.length === 0) {
    const sources = upstream.get(component) ?? [];

    if (sources.length === 0) {
      throw new Error(`Missing license texts: ${component}`);
    }

    for (const source of sources) {
      retain(`${component}: ${source.url}`, path.join(root, source.file), source.sha256);
    }
  }
}

const packageDirectory = (name: string, from: string) => {
  for (let directory = from; ; directory = path.dirname(directory)) {
    const candidate = path.join(directory, "node_modules", name);

    if (path.basename(directory) !== "node_modules" && existsSync(candidate)) {
      return realpathSync(candidate);
    }

    if (path.dirname(directory) === directory) {
      throw new Error(`Cannot find ${name} from ${from}`);
    }
  }
};

const readPackage = (directory: string) =>
  parse(
    readFileSync(path.join(directory, "package.json"), "utf-8"),
    isNpmPackage,
    `package.json in ${directory}`,
  );

const app = path.join(root, "app");

const appPackage = parse(
  readFileSync(path.join(app, "package.json"), "utf-8"),
  isAppPackage,
  "app/package.json",
);

const pendingPackages = Object.keys(appPackage.dependencies).map((name) => ({
  name,
  from: app,
}));

const reachedPackages = new Set<string>();

for (
  let dependency = pendingPackages.pop();
  dependency !== undefined;
  dependency = pendingPackages.pop()
) {
  const directory = packageDirectory(dependency.name, dependency.from);

  if (!reachedPackages.has(directory)) {
    reachedPackages.add(directory);
    const pkg = readPackage(directory);
    const component = `${pkg.name}@${pkg.version}`;

    inventory.push(
      `${component} | ${pkg.license ?? "see supplied terms"} | https://www.npmjs.com/package/${pkg.name}/v/${pkg.version}`,
    );

    const files = licenseFiles(directory).filter((file) => !file.startsWith("node_modules/"));

    if (files.length === 0) {
      throw new Error(`Missing license texts: ${component}`);
    }

    for (const file of files) {
      retain(`${component}: ${file}`, path.join(directory, file));
    }

    pendingPackages.push(
      ...Object.keys(pkg.dependencies ?? {}).map((name) => ({ name, from: directory })),
    );
  }
}

const header = `Third-party Rust and JavaScript dependency notices\n\nGenerated by packaging/notices.ts.\nCargo.lock SHA-256: ${hash(readFileSync(path.join(root, "Cargo.lock")))}\nbun.lock SHA-256: ${hash(readFileSync(path.join(root, "bun.lock")))}\n\nThis is the union of the said-app dependency graphs on Linux x86_64 and Apple Silicon,\nincluding build/test dependencies, plus production frontend dependencies. It can\ninclude components absent from an individual binary. Upstream license expressions\nand texts are retained; this inventory does not change their licensing.\nMPL-covered source is available from the versioned crate source links below.\nSource archives can be obtained at https://crates.io/api/v1/crates/NAME/VERSION/download.\n\n${inventory.toSorted().join("\n")}\n\n`;

writeFileSync(
  output,
  header +
    [...texts.values()]
      .map(
        (entry) =>
          `${"=".repeat(72)}\n${[...entry.components].toSorted().join("\n")}\n\n${entry.text}\n`,
      )
      .join("\n"),
);

console.log(
  `Retained ${inventory.length} dependencies and ${texts.size} distinct license/notice texts`,
);
