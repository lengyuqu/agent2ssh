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

const inputDirectory = valueFor("--input-dir");
const output = valueFor("--output");
const fragments = fs
  .readdirSync(inputDirectory)
  .filter((name) => name.startsWith("updater-") && name.endsWith(".json"))
  .sort();
if (fragments.length === 0) {
  throw new Error(`no updater fragments found in ${inputDirectory}`);
}

const requiredPlatforms = ["darwin-aarch64", "linux-x86_64", "windows-x86_64"];
let version;
const platforms = {};
for (const fragment of fragments) {
  const manifest = JSON.parse(fs.readFileSync(path.join(inputDirectory, fragment), "utf8"));
  if (typeof manifest.version !== "string" || !manifest.platforms) {
    throw new Error(`invalid updater fragment: ${fragment}`);
  }
  if (version && version !== manifest.version) {
    throw new Error(`updater version mismatch in ${fragment}`);
  }
  version = manifest.version;
  for (const [platform, update] of Object.entries(manifest.platforms)) {
    if (platforms[platform]) {
      throw new Error(`duplicate updater platform: ${platform}`);
    }
    if (
      !update ||
      typeof update.url !== "string" ||
      update.url.length === 0 ||
      typeof update.signature !== "string" ||
      update.signature.length === 0
    ) {
      throw new Error(`invalid updater platform entry: ${fragment}:${platform}`);
    }
    platforms[platform] = update;
  }
}

const missingPlatforms = requiredPlatforms.filter((platform) => !platforms[platform]);
if (missingPlatforms.length > 0) {
  throw new Error(`missing required updater platforms: ${missingPlatforms.join(", ")}`);
}

fs.writeFileSync(
  output,
  `${JSON.stringify({ version, platforms }, null, 2)}\n`,
);
