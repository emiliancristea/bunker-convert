import { spawnSync } from "node:child_process";
import { resolve } from "node:path";

function runCommand(args, options = {}) {
  const bin = options.bin ?? "bunker-convert";
  const result = spawnSync(bin, args, {
    encoding: "utf8",
    ...options.spawn
  });
  if ((options.check ?? true) && result.status !== 0) {
    const error = new Error(
      `bunker-convert exited with code ${result.status}: ${result.stderr?.trim()}`
    );
    error.stdout = result.stdout;
    error.stderr = result.stderr;
    error.exitCode = result.status;
    throw error;
  }
  return result;
}

export function runRecipe(recipePath, { bin, extraArgs, check = true, spawn } = {}) {
  const args = ["run", resolve(recipePath)];
  if (Array.isArray(extraArgs)) {
    args.push(...extraArgs.map(String));
  } else if (extraArgs && typeof extraArgs === "object") {
    for (const [key, value] of Object.entries(extraArgs)) {
      const flag = key.startsWith("-") ? key : `--${key}`;
      args.push(flag, String(value));
    }
  }
  return runCommand(args, { bin, check, spawn });
}

export function lintRecipes(recipes, { bin, check = true, spawn } = {}) {
  const args = ["recipe", "lint", ...recipes.map((p) => resolve(p))];
  return runCommand(args, { bin, check, spawn });
}
