// lib/extensions/installer.ts
// Extension installation: local path linking and git-based install.

import { validateManifest } from "./loader.ts";
import type { ExtensionManifest } from "./types.ts";

/** Result of an uninstall operation. */
export interface UninstallResult {
  success: boolean;
  error?: string;
}

/** Default user extensions directory. */
function userExtensionDir(): string {
  const xdg = Deno.env.get("XDG_CONFIG_HOME");
  if (xdg) return `${xdg}/marauder/extensions`;
  const home = Deno.env.get("HOME");
  if (!home) {
    throw new Error("Neither HOME nor XDG_CONFIG_HOME is set. Cannot determine config directory.");
  }
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

  // Create symlink — resolve to absolute path so it works regardless of CWD
  const absoluteSource = await Deno.realPath(sourcePath);
  const linkPath = `${targetDir}/${manifest.name}`;
  try {
    // Remove existing link if present
    try {
      await Deno.remove(linkPath);
    } catch {
      // Doesn't exist — fine
    }
    await Deno.symlink(absoluteSource, linkPath);
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

  /** Clone timeout in milliseconds (60 seconds). */
  const CLONE_TIMEOUT_MS = 60_000;

  // Clone with timeout
  try {
    const cmd = new Deno.Command("git", {
      args: ["clone", "--depth", "1", "--single-branch", url, cloneDir],
      stdout: "piped",
      stderr: "piped",
    });
    const child = cmd.spawn();

    const timeoutId = setTimeout(() => {
      try { child.kill("SIGTERM"); } catch { /* already exited */ }
    }, CLONE_TIMEOUT_MS);

    const { code, stderr } = await child.output();
    clearTimeout(timeoutId);

    if (code !== 0) {
      const errMsg = new TextDecoder().decode(stderr);
      // Clean up partial clone
      try { await Deno.remove(cloneDir, { recursive: true }); } catch { /* best effort */ }
      return { success: false, error: `git clone failed: ${errMsg}` };
    }
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    // Clean up partial clone
    try { await Deno.remove(cloneDir, { recursive: true }); } catch { /* best effort */ }
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
    try { await Deno.remove(cloneDir, { recursive: true }); } catch { /* best effort */ }
    return { success: false, error: `Invalid manifest in cloned repo` };
  }

  return { success: true, manifest, dir: cloneDir };
}

/**
 * Uninstall an extension by name.
 * Removes the extension directory (or symlink) from the user extensions dir.
 */
export async function uninstallExtension(
  name: string,
  opts?: { loader?: { unload(name: string): Promise<void>; isLoaded?(name: string): boolean } },
): Promise<UninstallResult> {
  // Unload the extension first if a loader is provided and the extension is active
  if (opts?.loader) {
    try {
      await opts.loader.unload(name);
    } catch {
      // Best effort — extension may not be loaded
    }
  }

  const extPath = `${userExtensionDir()}/${name}`;

  try {
    const stat = await Deno.lstat(extPath);
    if (stat.isSymlink) {
      await Deno.remove(extPath);
    } else if (stat.isDirectory) {
      await Deno.remove(extPath, { recursive: true });
    } else {
      return { success: false, error: `${extPath} is not a directory or symlink` };
    }
  } catch {
    return { success: false, error: `Extension "${name}" not found at ${extPath}` };
  }

  return { success: true };
}
