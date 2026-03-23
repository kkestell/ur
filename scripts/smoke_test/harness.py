"""Shared harness for CLI smoke tests."""

from __future__ import annotations

import os
import shlex
import shutil
import subprocess
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable


@dataclass(frozen=True)
class BuildArtifact:
    manifest_path: str
    build_args: tuple[str, ...]
    artifact_path: str
    install_dir: str


EXTENSION_ARTIFACTS: tuple[BuildArtifact, ...] = (
    BuildArtifact(
        manifest_path="extensions/system/session-jsonl/Cargo.toml",
        build_args=("--target", "wasm32-wasip2", "--release"),
        artifact_path="extensions/system/session-jsonl/target/wasm32-wasip2/release/session_jsonl.wasm",
        install_dir="extensions/system/session-jsonl",
    ),
    BuildArtifact(
        manifest_path="extensions/system/compaction-llm/Cargo.toml",
        build_args=("--target", "wasm32-wasip2", "--release"),
        artifact_path="extensions/system/compaction-llm/target/wasm32-wasip2/release/compaction_llm.wasm",
        install_dir="extensions/system/compaction-llm",
    ),
    BuildArtifact(
        manifest_path="extensions/system/llm-google/Cargo.toml",
        build_args=("--target", "wasm32-wasip2", "--release"),
        artifact_path="extensions/system/llm-google/target/wasm32-wasip2/release/llm_google.wasm",
        install_dir="extensions/system/llm-google",
    ),
    BuildArtifact(
        manifest_path="extensions/system/llm-openrouter/Cargo.toml",
        build_args=("--target", "wasm32-wasip2", "--release"),
        artifact_path="extensions/system/llm-openrouter/target/wasm32-wasip2/release/llm_openrouter.wasm",
        install_dir="extensions/system/llm-openrouter",
    ),
    BuildArtifact(
        manifest_path="extensions/workspace/test-extension/Cargo.toml",
        build_args=("--target", "wasm32-wasip2", "--release"),
        artifact_path="extensions/workspace/test-extension/target/wasm32-wasip2/release/test_extension.wasm",
        install_dir=".ur/extensions/test-extension",
    ),
    BuildArtifact(
        manifest_path="extensions/workspace/llm-test/Cargo.toml",
        build_args=("--target", "wasm32-wasip2", "--release"),
        artifact_path="extensions/workspace/llm-test/target/wasm32-wasip2/release/llm_test.wasm",
        install_dir=".ur/extensions/llm-test",
    ),
)


class SmokeHarness:
    """Shared state and helpers for smoke tests."""

    def __init__(self, root: Path):
        self.root = root
        self.ur = root / "target" / "debug" / "ur"
        self.tmpdir: Path | None = None
        self.ur_root: Path | None = None
        self.workspace: Path | None = None
        self._env = os.environ.copy()
        self._tempdir: tempfile.TemporaryDirectory[str] | None = None

    def __enter__(self) -> "SmokeHarness":
        self._load_dotenv()
        self._build_artifacts()
        self._prepare_directories()
        self._install_wasm_artifacts()
        return self

    def __exit__(self, *exc_info: object) -> None:
        if self._tempdir is not None:
            self._tempdir.cleanup()
            self._tempdir = None

    def run(
        self,
        *args: str,
        env: dict[str, str] | None = None,
    ) -> subprocess.CompletedProcess[str]:
        """Print and execute: ur <args>. Raises on non-zero exit."""
        return self._run_ur(args, check=True, env_overrides=env)

    def run_err(
        self,
        *args: str,
        env: dict[str, str] | None = None,
    ) -> subprocess.CompletedProcess[str]:
        """Print and execute: ur <args>. Continues only on non-zero exit."""
        result = self._run_ur(args, check=False, env_overrides=env)
        if result.returncode == 0:
            joined = shlex.join(("ur", *args))
            raise RuntimeError(f"expected non-zero exit from: {joined}")
        return result

    def run_allow_error(
        self,
        *args: str,
        env: dict[str, str] | None = None,
    ) -> subprocess.CompletedProcess[str]:
        """Print and execute: ur <args>. Never raises for exit status."""
        return self._run_ur(args, check=False, env_overrides=env)

    def cat(self, path: Path) -> None:
        """Print file contents."""
        print()
        print(f"$ cat {path}")
        if not path.exists():
            print("(missing)")
            return
        print(path.read_text(encoding="utf-8"), end="")

    def getenv(self, name: str) -> str | None:
        return self._env.get(name)

    @property
    def config_path(self) -> Path:
        if self.ur_root is None:
            raise RuntimeError("smoke harness has not been entered")
        return self.ur_root / "config.toml"

    def _load_dotenv(self) -> None:
        env_file = self.root / ".env"
        if not env_file.exists():
            return

        for raw_line in env_file.read_text(encoding="utf-8").splitlines():
            line = raw_line.strip()
            if not line or line.startswith("#"):
                continue
            if line.startswith("export "):
                line = line[len("export ") :].lstrip()
            if "=" not in line:
                continue

            key, value = line.split("=", 1)
            key = key.strip()
            value = value.strip()
            if len(value) >= 2 and value[0] == value[-1] and value[0] in {"'", '"'}:
                value = value[1:-1]
            self._env[key] = value

    def _build_artifacts(self) -> None:
        self._run(
            ("cargo", "build", "--manifest-path", "Cargo.toml"),
            ("cargo", "build", "--manifest-path", "Cargo.toml"),
            env=self._env,
            check=True,
        )
        for artifact in EXTENSION_ARTIFACTS:
            self._run(
                ("cargo", "build", "--manifest-path", artifact.manifest_path, *artifact.build_args),
                ("cargo", "build", "--manifest-path", artifact.manifest_path, *artifact.build_args),
                env=self._env,
                check=True,
            )

    def _prepare_directories(self) -> None:
        self._tempdir = tempfile.TemporaryDirectory(prefix="ur-smoke-test-")
        self.tmpdir = Path(self._tempdir.name)
        self.ur_root = self.tmpdir / "ur-root"
        self.workspace = self.tmpdir / "workspace"
        self.ur_root.mkdir(parents=True, exist_ok=True)
        self.workspace.mkdir(parents=True, exist_ok=True)

    def _install_wasm_artifacts(self) -> None:
        workspace = self._require_workspace()
        for artifact in EXTENSION_ARTIFACTS:
            destination = self._install_base(artifact.install_dir).resolve()
            destination.mkdir(parents=True, exist_ok=True)
            source = self.root / artifact.artifact_path
            shutil.copy2(source, destination / source.name)

        # Ensure the workspace-local .ur directory exists even if the first copy
        # target changes in the future.
        (workspace / ".ur").mkdir(exist_ok=True)

    def _install_base(self, install_dir: str) -> Path:
        if install_dir.startswith(".ur/"):
            workspace = self._require_workspace()
            return workspace / install_dir
        ur_root = self._require_ur_root()
        return ur_root / install_dir

    def _run_ur(
        self,
        args: Iterable[str],
        *,
        check: bool,
        env_overrides: dict[str, str] | None = None,
    ) -> subprocess.CompletedProcess[str]:
        workspace = self._require_workspace()
        ur_root = self._require_ur_root()
        display_command = ("ur", *args)
        command = (str(self.ur), "-w", str(workspace), *args)
        env = self._env | {"UR_ROOT": str(ur_root)}
        if env_overrides is not None:
            env |= env_overrides
        return self._run(display_command, command, env=env, check=check)

    def _run(
        self,
        display_command: Iterable[str],
        command: Iterable[str],
        *,
        env: dict[str, str],
        check: bool,
    ) -> subprocess.CompletedProcess[str]:
        display = tuple(display_command)
        args = tuple(command)
        print()
        print(f"$ {shlex.join(display)}")
        run_args = tuple(str(part) for part in args)
        result = subprocess.run(
            run_args,
            cwd=self.root,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            encoding="utf-8",
            errors="replace",
            check=False,
        )
        if result.stdout:
            print(result.stdout, end="" if result.stdout.endswith("\n") else "\n")
        if check and result.returncode != 0:
            raise subprocess.CalledProcessError(
                result.returncode,
                run_args,
                output=result.stdout,
            )
        return result

    def _require_workspace(self) -> Path:
        if self.workspace is None:
            raise RuntimeError("smoke harness has not been entered")
        return self.workspace

    def _require_ur_root(self) -> Path:
        if self.ur_root is None:
            raise RuntimeError("smoke harness has not been entered")
        return self.ur_root
