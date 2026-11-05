#!/usr/bin/env python3
"""End-to-end export smoke test through AT-SPI.

Launches rossresscut inside a headless cage compositor, opens a generated
media file with two chapters (so the timeline has two segments), starts an
export and asserts the running, completed and cancelled states.

This is a development tool, not part of the application runtime. It requires:

- a rossresscut build with eframe's `accesskit` feature (the default),
- PyGObject Atspi bindings (`python-gobject`, `at-spi2-core`),
- `cage`, `ffmpeg` and `ffprobe` in PATH.

Typical use:

    ./scripts/ui_export_smoke.py
    ./scripts/ui_export_smoke.py --keep
    ./scripts/ui_export_smoke.py --binary target/debug/rossresscut
"""

from __future__ import annotations

import argparse
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.dont_write_bytecode = True

from ui_a11y import (  # noqa: E402 - sys.path is prepared first
    Atspi,
    accessibility_enabled,
    set_accessibility,
    state_names,
)

APP_NAME = "rossresscut"
EXPORTING_LABEL = "Exporting..."
COMPLETED_LABEL = "Export completed:"
CANCELLED_LABEL = "Export cancelled"
FAILED_LABEL = "Export failed:"
CANCEL_BUTTON = "Cancel export"


class SmokeError(RuntimeError):
    """Raised when the UI does not reach the expected state."""


def run(*args: str, **kwargs: object) -> subprocess.CompletedProcess[str]:
    return subprocess.run(args, capture_output=True, text=True, **kwargs)


def require_tool(name: str) -> None:
    if shutil.which(name) is None:
        sys.exit(f"error: {name} is not in PATH")


def iter_nodes(node: Atspi.Accessible):
    """Yields every AT-SPI node below `node`, including `node`."""
    yield node
    for index in range(node.get_child_count()):
        child = node.get_child_at_index(index)
        if child is not None:
            yield from iter_nodes(child)


def find_nodes(
    node: Atspi.Accessible,
    *,
    name: str | None = None,
    role: str | None = None,
    contains: bool = False,
) -> list[Atspi.Accessible]:
    found = []
    for candidate in iter_nodes(node):
        candidate_name = candidate.get_name() or ""
        candidate_role = candidate.get_role_name() or ""
        if name is not None:
            if contains:
                if name not in candidate_name:
                    continue
            elif candidate_name != name:
                continue
        if role is not None and role.lower() not in candidate_role.lower():
            continue
        found.append(candidate)
    return found


def wait_for_app(timeout: float) -> Atspi.Accessible:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        desktop = Atspi.get_desktop(0)
        for index in range(desktop.get_child_count()):
            app = desktop.get_child_at_index(index)
            if (app.get_name() or "").lower() == APP_NAME:
                return app
        time.sleep(0.1)
    raise SmokeError(f"no {APP_NAME} window appeared on the AT-SPI bus")


def wait_until(
    app: Atspi.Accessible,
    predicate,
    description: str,
    timeout: float,
    interval: float = 0.02,
) -> Atspi.Accessible:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        match = predicate(app)
        if match is not None:
            return match
        time.sleep(interval)
    raise SmokeError(f"timed out waiting for {description}; {describe(app)}")


def describe(app: Atspi.Accessible) -> str:
    labels = [n.get_name() for n in find_nodes(app, role="label")]
    buttons = [n.get_name() for n in find_nodes(app, role="button")]
    return f"labels={labels!r} buttons={buttons!r}"


def label_prefix(app: Atspi.Accessible, prefix: str) -> Atspi.Accessible | None:
    for node in find_nodes(app, role="label"):
        if (node.get_name() or "").startswith(prefix):
            return node
    return None


def enabled_button(app: Atspi.Accessible, name: str) -> Atspi.Accessible | None:
    for node in find_nodes(app, name=name, role="button"):
        if "enabled" in state_names(node):
            return node
    return None


def click(app: Atspi.Accessible, name: str, timeout: float = 30.0) -> None:
    node = wait_until(
        app,
        lambda tree: enabled_button(tree, name),
        f"an enabled {name!r} button",
        timeout,
    )
    if node.get_n_actions() == 0:
        raise SmokeError(f"{name!r} has no AT-SPI action")
    if not node.do_action(0):
        raise SmokeError(f"the AT-SPI action for {name!r} was rejected")


def wait_ready(app: Atspi.Accessible, timeout: float = 60.0) -> None:
    wait_until(
        app,
        lambda tree: enabled_button(tree, "Export…"),
        "the media to open (enabled Export… button)",
        timeout,
    )


def start_export(app: Atspi.Accessible) -> None:
    click(app, "Export…")
    wait_until(
        app,
        lambda tree: enabled_button(tree, "Export"),
        "the export panel (enabled Export button)",
        30.0,
    )
    click(app, "Export")


def wait_running(app: Atspi.Accessible, timeout: float = 15.0) -> None:
    wait_until(
        app,
        lambda tree: enabled_button(tree, CANCEL_BUTTON)
        or label_prefix(tree, EXPORTING_LABEL),
        "the running export state",
        timeout,
    )


def wait_finished(app: Atspi.Accessible, timeout: float) -> str:
    def finished(tree: Atspi.Accessible) -> Atspi.Accessible | None:
        for prefix in (COMPLETED_LABEL, FAILED_LABEL):
            node = label_prefix(tree, prefix)
            if node is not None:
                return node
        return None

    return wait_until(app, finished, "the export to finish", timeout).get_name() or ""


def probe_duration(path: Path) -> float:
    result = run(
        "ffprobe",
        "-v",
        "error",
        "-show_entries",
        "format=duration",
        "-of",
        "default=noprint_wrappers=1:nokey=1",
        str(path),
    )
    if result.returncode != 0:
        raise SmokeError(f"ffprobe failed on {path}: {result.stderr.strip()}")
    return float(result.stdout.strip())


def count_chapters(path: Path) -> int:
    result = run("ffprobe", "-v", "error", "-show_chapters", "-of", "json", str(path))
    if result.returncode != 0:
        raise SmokeError(f"ffprobe failed on {path}: {result.stderr.strip()}")
    import json

    return len(json.loads(result.stdout).get("chapters", []))


def generate_media(ffmpeg: str, directory: Path) -> Path:
    """Builds a 1080p30 clip whose chapters start inside GOPs.

    Segment starts therefore need smart-cut encoding, which keeps the export
    running long enough to observe progress and cancel it deterministically.
    """
    video = directory / "video.mp4"
    metadata = directory / "chapters.txt"
    output = directory / "source.mkv"
    result = run(
        ffmpeg,
        "-nostdin",
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=1920x1080:rate=30:duration=9",
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-g",
        "120",
        "-keyint_min",
        "120",
        "-sc_threshold",
        "0",
        "-pix_fmt",
        "yuv420p",
        str(video),
    )
    if result.returncode != 0:
        raise SmokeError(f"ffmpeg could not generate the smoke fixture: {result.stderr}")
    metadata.write_text(
        ";FFMETADATA1\n"
        "\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=1000\nEND=4000\ntitle=One\n"
        "\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=4500\nEND=8000\ntitle=Two\n",
        encoding="utf-8",
    )
    result = run(
        ffmpeg,
        "-nostdin",
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-i",
        str(video),
        "-i",
        str(metadata),
        "-map_metadata",
        "1",
        "-c",
        "copy",
        str(output),
    )
    if result.returncode != 0:
        raise SmokeError(f"ffmpeg could not add chapters: {result.stderr}")
    return output


def write_config(directory: Path) -> Path:
    config = directory / "rossresscut" / "config.toml"
    config.parent.mkdir(parents=True, exist_ok=True)
    config.write_text(
        '[general]\nlanguage = "en"\nautosave_projects = false\n\n'
        "[export]\nchapters = true\nto_source_dir = true\nsmart_cut = true\n",
        encoding="utf-8",
    )
    return directory


class CageApp:
    """Runs rossresscut inside a headless cage compositor."""

    def __init__(self, binary: Path, media: Path, config_home: Path, log: Path):
        self.binary = binary
        self.media = media
        self.config_home = config_home
        self.log = log
        self.process: subprocess.Popen[bytes] | None = None
        self._log_file = None

    def __enter__(self) -> Atspi.Accessible:
        env = os.environ.copy()
        env.update(
            {
                "WLR_BACKENDS": "headless",
                "WLR_HEADLESS_OUTPUTS": "1",
                "WLR_LIBINPUT_NO_DEVICES": "1",
                "WLR_RENDERER": "vulkan",
                "XDG_CONFIG_HOME": str(self.config_home),
                "RUST_LOG": "rossresscut=warn",
            }
        )
        self._log_file = self.log.open("wb")
        self.process = subprocess.Popen(
            ["cage", "--", str(self.binary), str(self.media)],
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=self._log_file,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        try:
            app = wait_for_app(30.0)
            wait_ready(app, 90.0)
            return app
        except Exception:
            self._dump_log()
            self._stop()
            raise

    def _stop(self) -> None:
        if self.process is None:
            return
        try:
            os.killpg(self.process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(self.process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            self.process.wait(timeout=5)
        self.process = None

    def __exit__(self, *_exc: object) -> None:
        self._stop()
        if self._log_file is not None:
            self._log_file.close()
            self._log_file = None

    def _dump_log(self) -> None:
        try:
            text = self.log.read_text(encoding="utf-8", errors="replace")
        except OSError:
            return
        lines = text.strip().splitlines()[-20:]
        if lines:
            print("--- cage log ---", file=sys.stderr)
            print("\n".join(lines), file=sys.stderr)


def completion_case(args: argparse.Namespace, media: Path, workspace: Path) -> None:
    config_home = write_config(workspace / "config")
    output = media.with_name(f"{media.stem}_export{media.suffix}")
    output.unlink(missing_ok=True)

    print("completion: launching the app")
    with CageApp(args.binary, media, config_home, workspace / "cage-complete.log") as app:
        start_export(app)
        wait_running(app)
        status = wait_finished(app, args.timeout)
        if status.startswith(FAILED_LABEL):
            raise SmokeError(status)
        if not status.startswith(COMPLETED_LABEL):
            raise SmokeError(f"unexpected final status: {status!r}")
        if not output.is_file() or output.stat().st_size == 0:
            raise SmokeError(f"the completed export is missing: {output}")
        duration = probe_duration(output)
        if not 5.5 <= duration <= 7.5:
            raise SmokeError(f"unexpected export duration {duration:.3f}s")
        chapters = count_chapters(output)
        if chapters != 2:
            raise SmokeError(f"expected 2 chapters, got {chapters}")
        print(
            f"completion: ok, {output.name} is {duration:.3f}s with {chapters} chapters"
        )


def cancellation_case(args: argparse.Namespace, media: Path, workspace: Path) -> None:
    config_home = write_config(workspace / "config-cancel")
    output = media.with_name(f"{media.stem}_export{media.suffix}")
    output.unlink(missing_ok=True)

    print("cancellation: launching the app")
    with CageApp(args.binary, media, config_home, workspace / "cage-cancel.log") as app:
        start_export(app)
        wait_until(
            app,
            lambda tree: enabled_button(tree, CANCEL_BUTTON),
            "the Cancel export button",
            30.0,
        )
        click(app, CANCEL_BUTTON)
        cancelled = label_prefix(app, CANCELLED_LABEL)
        if cancelled is None:
            cancelled = wait_until(
                app,
                lambda tree: label_prefix(tree, CANCELLED_LABEL),
                "the cancelled state",
                30.0,
            )
        if output.exists():
            raise SmokeError(f"cancelled export left an output file: {output}")
        print("cancellation: ok, no output file was left behind")


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Drive the rossresscut export UI through AT-SPI.",
    )
    parser.add_argument(
        "--binary",
        type=Path,
        help="rossresscut binary (default: target/release, then target/debug)",
    )
    parser.add_argument(
        "--work-dir",
        type=Path,
        help="working directory (default: a temporary directory)",
    )
    parser.add_argument(
        "--keep",
        action="store_true",
        help="keep the working directory and its log files",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=180.0,
        help="seconds to wait for the completed export",
    )
    args = parser.parse_args()

    require_tool("cage")
    require_tool("ffmpeg")
    require_tool("ffprobe")
    running = run("pgrep", "-x", "rossresscut")
    if running.returncode == 0:
        sys.exit("error: another rossresscut instance is running; close it first")
    if args.binary is None:
        release = Path("target/release/rossresscut")
        args.binary = release if release.is_file() else Path("target/debug/rossresscut")
    if not args.binary.is_file():
        sys.exit(f"error: binary not found: {args.binary}")

    if args.work_dir is None:
        workspace = Path(tempfile.mkdtemp(prefix="rossresscut-export-smoke-"))
    else:
        workspace = args.work_dir
        workspace.mkdir(parents=True, exist_ok=True)

    previous_a11y = accessibility_enabled()
    set_accessibility(True)
    time.sleep(0.5)
    try:
        media = generate_media("ffmpeg", workspace)
        completion_case(args, media, workspace)
        cancellation_case(args, media, workspace)
    except SmokeError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    finally:
        if not previous_a11y:
            set_accessibility(False)
        if args.keep:
            print(f"working directory kept at {workspace}")
        else:
            shutil.rmtree(workspace, ignore_errors=True)
    print("export UI smoke test passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
