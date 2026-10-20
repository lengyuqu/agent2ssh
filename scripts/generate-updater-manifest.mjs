import fs from "node:fs";
import path from "node:path";

const args = process.argv.slice(2);
const valueFor = (name) => {
  const index = args.indexOf(name);
  if (index < 0 || !args[index + 1]) {
    throw new Error(`missing ${name}`);
  }
  return args[index + 1];
};

const target = valueFor("--target");
const output = valueFor("--output");
const targetInfo = {
  "aarch64-apple-darwin": { platform: "darwin-aarch64", directory: "macos", suffix: ".app.tar.gz" },
  "x86_64-unknown-linux-gnu": { platform: "linux-x86_64", directory: "appimage", suffix: ".AppImage" },
  "x86_64-pc-windows-msvc": { platform: "windows-x86_64", directory: "nsis", suffix: ".exe" },
}[target];

if (!targetInfo) {
  throw new Error(`unsupported updater target: ${target}`);
}

const bundleDirectory = path.join("src-tauri", "target", target, "release", "bundle", targetInfo.directory);
const artifacts = fs.existsSync(bundleDirectory)
  ? fs.readdirSync(bundleDirectory).filter((name) => name.endsWith(targetInfo.suffix))
  : [];
if (artifacts.length !== 1) {
  throw new Error(`expected one ${targetInfo.suffix} updater artifact in ${bundleDirectory}, found ${artifacts.length}`);
}

const artifact = artifacts[0];
const signaturePath = path.join(bundleDirectory, `${artifact}.sig`);
if (!fs.existsSync(signaturePath)) {
  throw new Error(`missing updater signature: ${signaturePath}`);
}

const repository = process.env.GITHUB_REPOSITORY;
const server = process.env.GITHUB_SERVER_URL || "https://github.com";
const tag = process.env.GITHUB_REF_NAME;
if (!repository || !tag) {
  throw new Error("GITHUB_REPOSITORY and GITHUB_REF_NAME are required");
}

const config = JSON.parse(fs.readFileSync("src-tauri/tauri.conf.json", "utf8"));
const version = config.version;
const url = `${server}/${repository}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(artifact)}`;
const manifest = {
  version,
  platforms: {
    [targetInfo.platform]: {
      signature: fs.readFileSync(signaturePath, "utf8").trim(),
      url,
    },
  },
};

fs.mkdirSync(path.dirname(output), { recursive: true });
fs.writeFileSync(output, `${JSON.stringify(manifest, null, 2)}\n`);
