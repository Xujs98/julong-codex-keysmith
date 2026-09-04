import { cpSync, mkdirSync, rmSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const resourcesDir = resolve(projectRoot, "src-tauri", "resources");
const skillsDir = resolve(resourcesDir, "codex-skills");
const mcpToolsDir = resolve(resourcesDir, "mcp-tools");

mkdirSync(resourcesDir, { recursive: true });
rmSync(skillsDir, { recursive: true, force: true });
cpSync(resolve(projectRoot, "bridge.md"), resolve(resourcesDir, "bridge.md"));
cpSync(resolve(projectRoot, "codex-skills"), skillsDir, { recursive: true });
rmSync(mcpToolsDir, { recursive: true, force: true });
cpSync(resolve(projectRoot, "mcp-tools"), mcpToolsDir, { recursive: true });

console.log(`Resources copied to ${resourcesDir} (skills + mcp-tools)`);
