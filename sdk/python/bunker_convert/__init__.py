from __future__ import annotations

import subprocess
from pathlib import Path
from typing import Iterable, Mapping, Sequence


def _build_args(extra_args: Mapping[str, str] | Sequence[str] | None) -> list[str]:
    if extra_args is None:
        return []
    if isinstance(extra_args, Mapping):
        result: list[str] = []
        for key, value in extra_args.items():
            flag = key if key.startswith("-") else f"--{key}"
            result.extend([flag, str(value)])
        return result
    return [str(item) for item in extra_args]


def run_recipe(
    recipe_path: str | Path,
    *,
    bunker_convert_bin: str = "bunker-convert",
    extra_args: Mapping[str, str] | Sequence[str] | None = None,
    capture_output: bool = True,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    cmd = [bunker_convert_bin, "run", str(Path(recipe_path))]
    cmd.extend(_build_args(extra_args))
    result = subprocess.run(cmd, text=True, capture_output=capture_output)
    if check and result.returncode != 0:
        raise RuntimeError(
            f"bunker-convert run failed with code {result.returncode}: {result.stderr.strip()}"
        )
    return result


def lint_recipes(
    recipes: Sequence[str | Path],
    *,
    bunker_convert_bin: str = "bunker-convert",
    extra_args: Iterable[str] | None = None,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    cmd = [bunker_convert_bin, "recipe", "lint", *map(lambda p: str(Path(p)), recipes)]
    if extra_args:
        cmd.extend(map(str, extra_args))
    result = subprocess.run(cmd, text=True, capture_output=True)
    if check and result.returncode != 0:
        raise RuntimeError(
            f"bunker-convert recipe lint failed with code {result.returncode}: {result.stderr.strip()}"
        )
    return result


__all__ = ["run_recipe", "lint_recipes"]
