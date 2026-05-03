#!/usr/bin/env node
import { readFileSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";

const SEMVER = /^v?(\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?)$/;

function resolveVersion() {
  const input = process.argv[2] ?? process.env.RELEASE_VERSION ?? process.env.GITHUB_REF_NAME;

  if (input) {
    const match = input.match(SEMVER);
    if (!match) {
      throw new Error(`版本号必须是 v1.2.3 或 1.2.3，当前为: ${input}`);
    }
    return match[1];
  }

  const tag = execFileSync("git", ["describe", "--tags", "--exact-match"], {
    encoding: "utf8",
  }).trim();
  const match = tag.match(SEMVER);
  if (!match) {
    throw new Error(`当前 tag 不是语义化版本: ${tag}`);
  }
  return match[1];
}

function updateJson(file, updater) {
  const data = JSON.parse(readFileSync(file, "utf8"));
  updater(data);
  writeFileSync(file, `${JSON.stringify(data, null, 2)}\n`);
}

function updateTomlVersion(file, version) {
  const lines = readFileSync(file, "utf8").split("\n");
  let inPackage = false;
  let updated = false;

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    if (/^\[.*\]$/.test(line)) {
      inPackage = line === "[package]";
      continue;
    }
    if (inPackage && /^version\s*=\s*"[^"]+"/.test(line)) {
      lines[index] = line.replace(/"[^"]+"/, `"${version}"`);
      updated = true;
      break;
    }
  }

  if (!updated) {
    throw new Error(`未找到 ${file} 的 [package] version 字段`);
  }
  writeFileSync(file, lines.join("\n"));
}

function updateCargoLock(file, version) {
  const content = readFileSync(file, "utf8");
  const next = content.replace(
    /(^\[\[package\]\]\nname = "dh-transshot"\nversion = )"[^"]+"/m,
    `$1"${version}"`,
  );
  if (next !== content) {
    writeFileSync(file, next);
  }
}

const version = resolveVersion();

updateJson("package.json", (data) => {
  data.version = version;
});

updateJson("src-tauri/tauri.conf.json", (data) => {
  data.version = version;
});

updateTomlVersion("src-tauri/Cargo.toml", version);
updateCargoLock("src-tauri/Cargo.lock", version);

console.log(`已同步版本号: ${version}`);
