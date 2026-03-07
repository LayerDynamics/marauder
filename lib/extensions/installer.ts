// lib/extensions/installer.ts
// Extension installation: local path linking and git-based install.

import { validateManifest } from "./loader.ts";
import type { ExtensionManifest } from "./types.ts";

/** Default user extensions directory. */
function userExtensionDir(): string {
  const home = Deno.env.get("HOME") ?? "";
  return `${home}/.config/marauder/extensions`;
}

/** Result of an install operation. */
export interface InstallResult {
  success: boolean;
  manifest?: ExtensionManifest;
  dir?: string;
  error?: string;
}

/**
 * Install an extension from a local path.
 * Validates the manifest and symlinks the directory into the user extensions dir.
 */
export async function installFromPath(sourcePath: string): Promise<InstallResult> {
  // Read and validate manifest
  const manifestPath = `${sourcePath}/extension.json`;
  let raw: string;
  try {
    raw = await Deno.readTextFile(manifestPath);
  } catch {
    return { success: false, error: `No extension.json found at ${manifestPath}` };
  }

  let json: Record<string, unknown>;
  try {
    json = JSON.parse(raw) as Record<string, unknown>;
  } catch {
    return { success: false, error: `Invalid JSON in ${manifestPath}` };
  }

  const manifest = validateManifest(json);
  if (!manifest) {
    return { success: false, error: `Invalid manifest in ${manifestPath}` };
  }

  // Ensure user extensions dir exists
  const targetDir = userExtensionDir();
  try {
    await Deno.mkdir(targetDir, { recursive: true });
  } catch {
    // Already exists
  }

  // Create symlink
  const linkPath = `${targetDir}/${manifest.name}`;
  try {
    // Remove existing link if present
    try {
      await Deno.remove(linkPath);
    } catch {
      // Doesn't exist — fine
    }
    await Deno.symlink(sourcePath, linkPath);
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    return { success: false, error: `Failed to create symlink: ${msg}` };
  }

  return { success: true, manifest, dir: linkPath };
}

/**
 * Install an extension from a git repository URL.
 * Clones the repo into the user extensions dir and validates the manifest.
 */
export async function installFromGit(url: string): Promise<InstallResult> {
  const targetDir = userExtensionDir();
  try {
    await Deno.mkdir(targetDir, { recursive: true });
  } catch {
    // Already exists
  }

  // Extract a directory name from the URL
  const urlParts = url.replace(/\.git$/, "").split("/");
  const repoName = urlParts[urlParts.length - 1] ?? "unknown-extension";
  const cloneDir = `${targetDir}/${repoName}`;

  // Clone
  try {
    const cmd = new Deno.Command("git", {
      args: ["clone", "--depth", "1", url, cloneDir],
      stdout: "piped",
      stderr: "piped",
    });
    const { code, stderr } = await cmd.output();
    if (code !== 0) {
      const errMsg = new TextDecoder().decode(stderr);
      return { success: false, error: `git clone failed: ${errMsg}` };
    }
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    return { success: false, error: `git clone failed: ${msg}` };
  }

  // Validate manifest
  const manifestPath = `${cloneDir}/extension.json`;
  let raw: string;
  try {
    raw = await Deno.readTextFile(manifestPath);
  } catch {
    // Clean up on failure
    try {
      await Deno.remove(cloneDir, { recursive: true });
    } catch {
      // Best effort
    }
    return { success: false, error: `Cloned repo has no extension.json` };
  }

  let json: Record<string, unknown>;
  try {
    json = JSON.parse(raw) as Record<string, unknown>;
  } catch {
    return { success: false, error: `Invalid JSON in cloned extension.json` };
  }

  const manifest = validateManifest(json);
  if (!manifest) {
    return { success: false, error: `Invalid manifest in cloned repo` };
  }

  return { success: true, manifest, dir: cloneDir };
}
