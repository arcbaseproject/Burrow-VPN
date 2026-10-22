// Builds burrow-helper and places it where Tauri's externalBin expects it.
import { execSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";

const triple = process.env.TAURI_ENV_TARGET_TRIPLE
  || execSync("rustc -vV").toString().match(/host: (\S+)/)[1];
const ext = triple.includes("windows") ? ".exe" : "";
execSync(`cargo build -p burrow-helper --release --target ${triple}`, { stdio: "inherit" });
mkdirSync("src-tauri/binaries", { recursive: true });
copyFileSync(`target/${triple}/release/burrow-helper${ext}`, `src-tauri/binaries/burrow-helper-${triple}${ext}`);
