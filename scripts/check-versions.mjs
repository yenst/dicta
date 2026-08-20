import fs from "node:fs";

const readJson = (path) => JSON.parse(fs.readFileSync(path, "utf8"));
const cargoVersion = (path) => {
  const contents = fs.readFileSync(path, "utf8");
  const packageSection = contents.match(/\[package\]([\s\S]*?)(?:\n\[|$)/)?.[1];
  const version = packageSection?.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  if (!version) throw new Error(`Could not read package.version from ${path}`);
  return version;
};

const packageJson = readJson("package.json");
const packageLock = readJson("package-lock.json");
const expected = packageJson.version;
const versions = new Map([
  ["package.json", expected],
  ["package-lock.json", packageLock.version],
  ["package-lock.json packages['']", packageLock.packages?.[""]?.version],
  ["src-tauri/tauri.conf.json", readJson("src-tauri/tauri.conf.json").version],
  ["src-tauri/Cargo.toml", cargoVersion("src-tauri/Cargo.toml")],
  ["mcp/Cargo.toml", cargoVersion("mcp/Cargo.toml")],
  ["crates/dicta-core/Cargo.toml", cargoVersion("crates/dicta-core/Cargo.toml")],
]);

const mismatches = [...versions].filter(([, version]) => version !== expected);
if (mismatches.length) {
  for (const [path, version] of mismatches) {
    console.error(`${path} has version ${version ?? "<missing>"}; expected ${expected}`);
  }
  process.exit(1);
}

if (process.argv.includes("--print")) {
  process.stdout.write(expected);
} else {
  console.log(`Version consistency: ${expected} across ${versions.size} declarations`);
}
