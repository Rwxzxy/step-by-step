// 编译 steppy-hook 并按 Tauri sidecar 的命名规则放到 src-tauri/binaries/steppy-hook-<目标三元组>[.exe]。
// 用法：node scripts/build-hook.mjs [--debug] [--target <triple>]
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, chmodSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const debug = args.includes("--debug");
const ti = args.indexOf("--target");
const host = /host: (\S+)/.exec(execFileSync("rustc", ["-vV"], { encoding: "utf8" }))[1];
const target = ti >= 0 ? args[ti + 1] : process.env.TAURI_ENV_TARGET_TRIPLE || host;

const cargoArgs = ["build", "-p", "steppy-hook"];
if (!debug) cargoArgs.push("--release");
if (target !== host) cargoArgs.push("--target", target);
execFileSync("cargo", cargoArgs, { cwd: root, stdio: "inherit" });

const exe = target.includes("windows") ? ".exe" : "";
const profile = debug ? "debug" : "release";
const built = target !== host ? join(root, "target", target, profile, `steppy-hook${exe}`) : join(root, "target", profile, `steppy-hook${exe}`);
if (!existsSync(built)) throw new Error(`找不到编译产物 ${built}`);
const outDir = join(root, "src-tauri", "binaries");
mkdirSync(outDir, { recursive: true });
const out = join(outDir, `steppy-hook-${target}${exe}`);
copyFileSync(built, out);
if (!exe) chmodSync(out, 0o755);
console.log(`steppy-hook -> ${out}`);
