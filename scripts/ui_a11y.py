#!/usr/bin/env python3
"""Read or drive the rossresscut UI through AT-SPI.

This is a development tool, not part of the application runtime. It requires:

- a rossresscut build with eframe's `accesskit` feature (the default), and
- the session's AT-SPI status enabled. On niri it is off by default:

    ./scripts/ui_a11y.py --enable

Typical use:

    ./scripts/ui_a11y.py --dump
    ./scripts/ui_a11y.py --dump --role button
    ./scripts/ui_a11y.py --dump --rects
    ./scripts/ui_a11y.py --click "⚙"
    ./scripts/ui_a11y.py --click "文件" --role button
    ./scripts/ui_a11y.py --disable

Custom-painted areas such as the timeline and the video preview expose no
useful roles or actions. Use screenshots for colours, layout and video frames.
"""

from __future__ import annotations

import argparse
import subprocess
import sys
import time

try:
    import gi

    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi, GLib
except Exception as error:  # noqa: BLE001 - the message is the useful part
    sys.exit(f"error: PyGObject Atspi bindings are unavailable: {error}")

# AccessKit's AT-SPI bridge logs harmless compatibility warnings with some
# pyatspi versions. Filter those messages so the tree stays readable.
def _log_writer(*args: object) -> GLib.LogWriterOutput:
    fields = args[1] if len(args) > 1 else ""
    if "AT-SPI" in str(fields):
        return GLib.LogWriterOutput.HANDLED
    return GLib.LogWriterOutput.UNHANDLED


GLib.log_set_writer_func(_log_writer, None)

APP_NAME = "rossresscut"
STATE_NAMES = ("enabled", "focused", "checked", "selected", "editable")


def run_busctl(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["busctl", "--user", *args],
        capture_output=True,
        text=True,
    )


def accessibility_enabled() -> bool:
    result = run_busctl(
        "get-property",
        "org.a11y.Bus",
        "/org/a11y/bus",
        "org.a11y.Status",
        "IsEnabled",
    )
    return result.returncode == 0 and "true" in result.stdout


def set_accessibility(enabled: bool) -> None:
    result = run_busctl(
        "set-property",
        "org.a11y.Bus",
        "/org/a11y/bus",
        "org.a11y.Status",
        "IsEnabled",
        "b",
        "true" if enabled else "false",
    )
    if result.returncode != 0:
        detail = (result.stderr or result.stdout).strip()
        sys.exit(f"error: could not set AT-SPI status: {detail}")


def find_app(timeout: float = 2.0) -> Atspi.Accessible | None:
    deadline = time.monotonic() + timeout
    while True:
        desktop = Atspi.get_desktop(0)
        for index in range(desktop.get_child_count()):
            app = desktop.get_child_at_index(index)
            if (app.get_name() or "").lower() == APP_NAME:
                return app
        if time.monotonic() >= deadline:
            return None
        time.sleep(0.2)


def state_names(node: Atspi.Accessible) -> list[str]:
    try:
        states = node.get_state_set()
    except Exception:  # noqa: BLE001 - AT-SPI nodes can disappear mid-walk
        return []
    enabled = []
    for name in STATE_NAMES:
        state = getattr(Atspi.StateType, name.upper(), None)
        if state is not None and states.contains(state):
            enabled.append(name)
    return enabled


def rect_text(node: Atspi.Accessible) -> str:
    try:
        rect = node.get_extents(Atspi.CoordType.SCREEN)
    except Exception:  # noqa: BLE001
        return ""
    return f" [{rect.x},{rect.y} {rect.width}x{rect.height}]"


def walk(
    node: Atspi.Accessible,
    args: argparse.Namespace,
    depth: int = 0,
    printed: list[int] | None = None,
) -> None:
    if printed is None:
        printed = [0]
    if printed[0] >= args.max:
        return

    role = node.get_role_name() or ""
    name = node.get_name() or ""
    role_matches = not args.role or args.role.lower() in role.lower()
    if role_matches:
        printed[0] += 1
        states = state_names(node)
        suffix = f" ({', '.join(states)})" if states else ""
        rect = rect_text(node) if args.rects else ""
        print(f"{'  ' * depth}{role} | {name!r}{suffix}{rect}")

    for index in range(node.get_child_count()):
        child = node.get_child_at_index(index)
        if child is not None:
            walk(child, args, depth + 1, printed)


def collect(
    node: Atspi.Accessible,
    name: str,
    role: str | None,
    contains: bool,
    found: list[Atspi.Accessible],
    limit: int = 200,
) -> None:
    if len(found) >= limit:
        return
    node_name = node.get_name() or ""
    node_role = node.get_role_name() or ""
    name_matches = name in node_name if contains else node_name == name
    role_matches = not role or role.lower() in node_role.lower()
    if name_matches and role_matches:
        found.append(node)
    for index in range(node.get_child_count()):
        child = node.get_child_at_index(index)
        if child is not None:
            collect(child, name, role, contains, found, limit)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Read or drive the rossresscut UI through AT-SPI.",
    )
    parser.add_argument("--list", action="store_true", help="list AT-SPI applications")
    parser.add_argument("--dump", action="store_true", help="print the widget tree")
    parser.add_argument("--role", help="only print or click this AT-SPI role")
    parser.add_argument("--rects", action="store_true", help="include screen rectangles")
    parser.add_argument("--max", type=int, default=400, help="maximum printed nodes")
    parser.add_argument("--click", metavar="NAME", help="click a widget by name")
    parser.add_argument(
        "--contains",
        action="store_true",
        help="match --click names by substring instead of exact equality",
    )
    parser.add_argument("--index", type=int, default=0, help="which click match to use")
    parser.add_argument("--enable", action="store_true", help="enable AT-SPI status")
    parser.add_argument("--disable", action="store_true", help="disable AT-SPI status")
    parser.add_argument("--status", action="store_true", help="print AT-SPI status")
    args = parser.parse_args()

    if args.enable:
        set_accessibility(True)
        time.sleep(0.5)
        if not (args.dump or args.click or args.list):
            return 0
    if args.disable:
        set_accessibility(False)
        return 0
    if args.status:
        print("enabled" if accessibility_enabled() else "disabled")
        return 0

    if args.list:
        desktop = Atspi.get_desktop(0)
        for index in range(desktop.get_child_count()):
            app = desktop.get_child_at_index(index)
            print(f"{index}: {app.get_name()!r} ({app.get_role_name()})")
        return 0

    if not accessibility_enabled():
        print(
            "error: AT-SPI is disabled; run ./scripts/ui_a11y.py --enable first",
            file=sys.stderr,
        )
        return 2

    app = find_app()
    if app is None:
        print(
            f"error: no {APP_NAME} application on the AT-SPI bus; "
            "is the app running?",
            file=sys.stderr,
        )
        return 2

    if args.click is not None:
        matches: list[Atspi.Accessible] = []
        collect(app, args.click, args.role, args.contains, matches)
        if not matches:
            print(f"error: no widget named {args.click!r}", file=sys.stderr)
            return 1
        if args.index >= len(matches):
            print(
                f"error: {len(matches)} matches, index {args.index} is out of range",
                file=sys.stderr,
            )
            return 1
        target = matches[args.index]
        if target.get_n_actions() == 0:
            print(
                f"error: {target.get_role_name()} {target.get_name()!r} has no actions",
                file=sys.stderr,
            )
            return 1
        if not target.do_action(0):
            print("error: the AT-SPI action was rejected", file=sys.stderr)
            return 1
        print(
            f"clicked {target.get_role_name()} {target.get_name()!r} "
            f"({len(matches)} match(es))"
        )
        return 0

    walk(app, args)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
