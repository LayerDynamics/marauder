#!/usr/bin/env -S deno run --allow-read --allow-write --allow-run --allow-env
// lib/cli/ext.ts — Marauder extension management CLI.

import { parseArgs } from "https://deno.land/std@0.224.0/cli/parse_args.ts";
import { installFromPath, installFromGit, uninstallExtension } from "../extensions/installer.ts";
import { discover, validateManifest } from "../extensions/loader.ts";

const TEMPLATE_DIR = new URL("./templates/extension-template/", import.meta.url).pathname;

function userExtensionDir(): string {
  const xdg = Deno.env.get("XDG_CONFIG_HOME");
  if (xdg) return `${xdg}/marauder/extensions`;
  const home = Deno.env.get("HOME");
  if (!home) {
    console.error("Neither HOME nor XDG_CONFIG_HOME is set. Cannot determine config directory.");
    Deno.exit(1);
  }
  return `${home}/.config/marauder/extensions`;
}

function usage(): void {
  console.log(`marauder-ext — Marauder Extension Manager

Usage:
  ext install <source>   Install extension from path or git URL
  ext uninstall <name>   Remove an installed extension
  ext list               List all installed extensions
  ext create <name>      Scaffold a new extension from template
  ext info <name>        Show details for an installed extension
  ext enable <name>      Enable an extension
  ext disable <name>     Disable an extension
  ext help               Show this help message`);
}

async function cmdInstall(source: string): Promise<void> {
  const isGit = source.startsWith("https://") || source.startsWith("http://") || source.startsWith("git@");
  const result = isGit ? await installFromGit(source) : await installFromPath(source);

  if (result.success) {
    console.log(`Installed "${result.manifest!.name}" v${result.manifest!.version} → ${result.dir}`);
  } else {
    console.error(`Install failed: ${result.error}`);
    Deno.exit(1);
  }
}

async function cmdUninstall(name: string): Promise<void> {
  const result = await uninstallExtension(name);
  if (result.success) {
    console.log(`Uninstalled "${name}"`);
  } else {
    console.error(`Uninstall failed: ${result.error}`);
    Deno.exit(1);
  }
}

async function cmdList(): Promise<void> {
  const extensions = await discover(["extensions", userExtensionDir()]);

  if (extensions.length === 0) {
    console.log("No extensions found.");
    return;
  }

  // Compute dynamic column widths
  const nameW = Math.max(6, ...extensions.map(({ manifest }) => manifest.name.length)) + 2;
  const verW = Math.max(9, ...extensions.map(({ manifest }) => manifest.version.length)) + 2;
  const stateW = 10;

  console.log(
    "Name".padEnd(nameW) + "Version".padEnd(verW) + "Source".padEnd(stateW) + "Description"
  );
  console.log("-".repeat(nameW + verW + stateW + 30));

  for (const { manifest, dir } of extensions) {
    const source = dir.startsWith("extensions/") ? "bundled" : "user";
    console.log(
      manifest.name.padEnd(nameW) +
      manifest.version.padEnd(verW) +
      source.padEnd(stateW) +
      manifest.description
    );
  }
}

/** Validate extension name: alphanumeric, dots, hyphens, underscores only. */
function isValidExtensionName(name: string): boolean {
  return /^[a-z0-9][a-z0-9._-]*$/i.test(name) && !name.includes("..") && !name.includes("/") && !name.includes("\\");
}

/** Atomically write a config file via temp + rename. */
async function atomicWriteConfig(configPath: string, data: string): Promise<void> {
  const tmpPath = `${configPath}.${Date.now()}.tmp`;
  await Deno.writeTextFile(tmpPath, data);
  await Deno.rename(tmpPath, configPath);
}

async function cmdCreate(name: string): Promise<void> {
  if (!isValidExtensionName(name)) {
    console.error(`Invalid extension name "${name}". Use only alphanumeric characters, dots, hyphens, and underscores.`);
    Deno.exit(1);
  }
  const targetDir = `extensions/${name}`;

  try {
    await Deno.stat(targetDir);
    console.error(`Directory "${targetDir}" already exists.`);
    Deno.exit(1);
  } catch {
    // Doesn't exist — good
  }

  await Deno.mkdir(targetDir, { recursive: true });

  // Copy and template each file
  const files = ["extension.json", "mod.ts", "README.md"];
  for (const file of files) {
    let content = await Deno.readTextFile(`${TEMPLATE_DIR}/${file}`);
    content = content.replaceAll("{{name}}", name);
    content = content.replaceAll("{{description}}", `A Marauder extension: ${name}`);
    await Deno.writeTextFile(`${targetDir}/${file}`, content);
  }

  // Verify the generated manifest is valid
  const generatedJson = JSON.parse(await Deno.readTextFile(`${targetDir}/extension.json`)) as Record<string, unknown>;
  const validatedManifest = validateManifest(generatedJson);
  if (!validatedManifest) {
    console.error("Generated manifest failed validation — this is a bug in the template.");
    Deno.exit(1);
  }

  console.log(`Created extension scaffold at ${targetDir}/`);
  console.log(`  Name:    ${validatedManifest.name}`);
  console.log(`  Version: ${validatedManifest.version}`);
  console.log("\nNext steps:");
  console.log(`  1. Edit ${targetDir}/mod.ts with your extension logic`);
  console.log(`  2. Update ${targetDir}/extension.json with required permissions`);
  console.log(`  3. Install: marauder-ext install ./${targetDir}`);
}

async function cmdInfo(name: string): Promise<void> {
  const extensions = await discover(["extensions", userExtensionDir()]);
  const ext = extensions.find((e) => e.manifest.name === name);

  if (!ext) {
    console.error(`Extension "${name}" not found.`);
    Deno.exit(1);
  }

  const m = ext.manifest;
  console.log(`Name:         ${m.name}`);
  console.log(`Version:      ${m.version}`);
  console.log(`Description:  ${m.description}`);
  console.log(`Entry:        ${m.entry}`);
  console.log(`Directory:    ${ext.dir}`);
  if (m.permissions?.length) console.log(`Permissions:  ${m.permissions.join(", ")}`);
  if (m.engines) console.log(`Engines:      ${JSON.stringify(m.engines)}`);
  if (m.repository) console.log(`Repository:   ${m.repository}`);
  if (m.activationEvents?.length) console.log(`Activation:   ${m.activationEvents.join(", ")}`);
  if (m.dependencies) console.log(`Dependencies: ${JSON.stringify(m.dependencies)}`);
}

/** Resolve the marauder config directory, respecting XDG_CONFIG_HOME. */
function configDir(): string {
  const xdg = Deno.env.get("XDG_CONFIG_HOME");
  if (xdg) return `${xdg}/marauder`;
  const home = Deno.env.get("HOME");
  if (!home) {
    console.error("Neither HOME nor XDG_CONFIG_HOME is set. Cannot determine config directory.");
    Deno.exit(1);
  }
  return `${home}/.config/marauder`;
}

/** Read the user config file, returning a parsed object. */
async function readConfig(): Promise<Record<string, unknown>> {
  try {
    return JSON.parse(await Deno.readTextFile(`${configDir()}/config.json`)) as Record<string, unknown>;
  } catch {
    return {};
  }
}

/** Get the disabledExtensions array from config with runtime type safety. */
function getDisabledList(config: Record<string, unknown>): string[] {
  const raw = config.disabledExtensions;
  return Array.isArray(raw) ? raw.filter((v): v is string => typeof v === "string") : [];
}

async function cmdEnable(name: string): Promise<void> {
  const dir = configDir();
  const configPath = `${dir}/config.json`;
  const config = await readConfig();

  config.disabledExtensions = getDisabledList(config).filter((n) => n !== name);

  await Deno.mkdir(dir, { recursive: true });
  await atomicWriteConfig(configPath, JSON.stringify(config, null, 2) + "\n");
  console.log(`Enabled extension "${name}"`);
}

async function cmdDisable(name: string): Promise<void> {
  const dir = configDir();
  const configPath = `${dir}/config.json`;
  const config = await readConfig();

  const disabled = getDisabledList(config);
  if (!disabled.includes(name)) {
    disabled.push(name);
  }
  config.disabledExtensions = disabled;

  await Deno.mkdir(dir, { recursive: true });
  await atomicWriteConfig(configPath, JSON.stringify(config, null, 2) + "\n");
  console.log(`Disabled extension "${name}"`);
}

// --- Main ---
const parsed = parseArgs(Deno.args, {
  boolean: ["help", "force", "quiet"],
  alias: { h: "help", f: "force", q: "quiet" },
  "--": false,
});

const [subcommand, ...positional] = parsed._ as string[];

if (parsed.help && !subcommand) {
  usage();
  Deno.exit(0);
}

function requireArg(cmd: string): string {
  if (!positional[0]) {
    console.error(`Usage: ext ${cmd} <${cmd === "install" ? "source" : "name"}>`);
    Deno.exit(1);
  }
  return String(positional[0]);
}

switch (subcommand) {
  case "install":
    await cmdInstall(requireArg("install"));
    break;
  case "uninstall":
    await cmdUninstall(requireArg("uninstall"));
    break;
  case "list":
    await cmdList();
    break;
  case "create":
    await cmdCreate(requireArg("create"));
    break;
  case "info":
    await cmdInfo(requireArg("info"));
    break;
  case "enable":
    await cmdEnable(requireArg("enable"));
    break;
  case "disable":
    await cmdDisable(requireArg("disable"));
    break;
  case "help":
  case undefined:
    usage();
    break;
  default:
    console.error(`Unknown subcommand: ${subcommand}`);
    usage();
    Deno.exit(1);
}
