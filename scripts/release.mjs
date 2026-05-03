#!/usr/bin/env node
import { execFileSync } from "node:child_process";

const bump = process.argv[2] ?? "patch";
const allowed = new Set(["patch", "minor", "major"]);

if (!allowed.has(bump)) {
  throw new Error("用法: pnpm release[:patch|:minor|:major]");
}

function git(args) {
  return execFileSync("git", args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
}

function run(command, args) {
  execFileSync(command, args, { stdio: "inherit" });
}

const dirty = git(["status", "--porcelain"]);
if (dirty) {
  throw new Error("工作区存在未提交改动，请先提交或暂存后再发版。\n" + dirty);
}

const tags = git(["tag", "--sort=-v:refname"])
  .split("\n")
  .filter(Boolean)
  .filter((tag) => /^v\d+\.\d+\.\d+$/.test(tag));

const latest = tags[0] ?? "v0.0.0";
const parts = latest.slice(1).split(".").map(Number);

if (bump === "major") {
  parts[0] += 1;
  parts[1] = 0;
  parts[2] = 0;
} else if (bump === "minor") {
  parts[1] += 1;
  parts[2] = 0;
} else {
  parts[2] += 1;
}

const nextTag = `v${parts.join(".")}`;

run("node", ["scripts/sync-version.mjs", nextTag]);
run("git", ["add", "package.json", "src-tauri/tauri.conf.json", "src-tauri/Cargo.toml", "src-tauri/Cargo.lock"]);
run("git", ["commit", "-m", `chore: release ${nextTag}`]);
run("git", ["tag", "-a", nextTag, "-m", `${nextTag}: release`]);

console.log(`\n发版提交和 tag 已创建: ${nextTag}`);
console.log("推送并触发 GitHub Actions:");
console.log("  git push && git push origin " + nextTag);
