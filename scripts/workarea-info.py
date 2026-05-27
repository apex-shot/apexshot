#!/usr/bin/env python3
"""
Query the X11 root window's _NET_WORKAREA property to determine the usable
desktop area (excluding panels / docks / struts).  Emits logical coordinates
by dividing physical values by the Qt / GDK device-pixel-ratio detected from
the current XRandR configuration.

Usage:
  python3 workarea-info.py               # print workarea rect as JSON
  python3 workarea-info.py --json        # same as default
  python3 workarea-info.py --center      # print center point (x y)
  python3 workarea-info.py --half        # print half-width x half-height

On a 200 % HiDPI monitor the _NET_WORKAREA values are native device pixels.
Without DPR correction they are *twice* the logical coordinates used by Qt /
GTK, which causes screenshot capture regions to be offset or scaled wrong.
This script automatically scales by the detected DPR so the output matches
the coordinate space the ApexShot overlay uses.
"""

import argparse
import json
import os
import re
import subprocess
import sys
from typing import Dict, Optional, Tuple


def run_xprop(*prop_names: str) -> Dict[str, str]:
    """Run xprop -root and pluck the named properties from stdout."""
    try:
        proc = subprocess.run(
            ["xprop", "-root"],
            capture_output=True,
            text=True,
            timeout=5,
        )
    except FileNotFoundError:
        sys.exit("error: xprop not found (install x11-utils)")
    except subprocess.TimeoutExpired:
        sys.exit("error: xprop timed out")

    if proc.returncode != 0:
        sys.exit(f"error: xprop returned {proc.returncode}")

    result: Dict[str, str] = {}
    for name in prop_names:
        # _NET_WORKAREA(CARDINAL) = 0, 40, 3840, 2120, ...
        pattern = rf"^{re.escape(name)}\(.*?\)\s*=\s*(.*)$"
        m = re.search(pattern, proc.stdout, re.MULTILINE)
        if m:
            result[name] = m.group(1).strip()
    return result


def parse_cardinal_list(raw: str) -> Tuple[int, ...]:
    """Parse an xprop CARDINAL list like '0, 40, 3840, 2120' into ints."""
    parts = raw.replace(",", " ").split()
    return tuple(int(p) for p in parts)


def detect_dpr_via_xrandr() -> float:
    """
    Heuristic: read the XRandR primary output's current mode.
    Compare physical mm dimensions to pixel dimensions to derive DPR.
    Falls back to 1.0 on any error.
    """
    try:
        proc = subprocess.run(
            ["xrandr", "--query"],
            capture_output=True,
            text=True,
            timeout=5,
        )
    except (FileNotFoundError, subprocess.TimeoutExpired):
        return 1.0

    if proc.returncode != 0:
        return 1.0

    # Look for the primary connected output and its current mode
    # e.g. "DP-1 connected primary 3840x2160+0+0 (normal ...) 697mm x 392mm"
    primary_re = re.compile(
        r"^(\S+)\s+connected\s+primary\s+(\d+)x(\d+)\+(\d+)\+(\d+).*?(\d+)mm\s+x\s+(\d+)mm",
        re.MULTILINE,
    )
    m = primary_re.search(proc.stdout)
    if not m:
        # Fallback: first connected output with mm info
        alt_re = re.compile(
            r"^(\S+)\s+connected\s+(\d+)x(\d+)\+(\d+)\+(\d+).*?(\d+)mm\s+x\s+(\d+)mm",
            re.MULTILINE,
        )
        m = alt_re.search(proc.stdout)

    if not m:
        return 1.0

    px_w, px_h = int(m.group(2)), int(m.group(3))
    mm_w, mm_h = int(m.group(6)), int(m.group(7))

    if mm_w <= 0 or mm_h <= 0:
        return 1.0

    # Standard DPI = 96.  Physical DPI = pixels / inches.
    dpi_x = px_w / (mm_w / 25.4)
    dpi_y = px_h / (mm_h / 25.4)
    avg_dpi = (dpi_x + dpi_y) / 2.0

    # Qt rounds DPR to nearest integer for integer scale factors, or uses
    # the fractional value for fractional scaling.  We return the raw
    # ratio so the caller can decide.
    dpr = avg_dpi / 96.0

    # Clamp to reasonable range — panel-reported EDID dimensions can be
    # wildly wrong on some monitors.
    return max(0.5, min(dpr, 4.0))


def detect_dpr_via_xdpyinfo() -> Optional[float]:
    """
    Alternative: read screen dimensions via xdpyinfo and compare to
    reported millimeter dimensions.  Returns None on failure.
    """
    try:
        proc = subprocess.run(
            ["xdpyinfo"],
            capture_output=True,
            text=True,
            timeout=5,
        )
    except (FileNotFoundError, subprocess.TimeoutExpired):
        return None

    if proc.returncode != 0:
        return None

    # "dimensions:    3840x2160 pixels (697x392 millimeters)"
    m = re.search(r"dimensions:\s+(\d+)x(\d+)\s+pixels\s+\((\d+)x(\d+)\s+millimeters\)",
                  proc.stdout)
    if not m:
        return None

    px_w, px_h = int(m.group(1)), int(m.group(2))
    mm_w, mm_h = int(m.group(3)), int(m.group(4))
    if mm_w <= 0 or mm_h <= 0:
        return None

    dpi_x = px_w / (mm_w / 25.4)
    dpi_y = px_h / (mm_h / 25.4)
    avg_dpi = (dpi_x + dpi_y) / 2.0
    return max(0.5, min(avg_dpi / 96.0, 4.0))


def get_dpr() -> float:
    """
    Detect the device-pixel-ratio used by Qt/GTK toolkits.
    Tries xrandr first (per-output primary), then xdpyinfo.
    """
    dpr = detect_dpr_via_xrandr()
    if dpr != 1.0:
        return dpr

    alt = detect_dpr_via_xdpyinfo()
    if alt is not None:
        return alt

    return 1.0


def get_workarea(dpr: float) -> Tuple[int, int, int, int]:
    """
    Retrieve _NET_WORKAREA, scale from device pixels to logical pixels
    using *dpr*, and return (x, y, width, height) in logical coords.
    """
    props = run_xprop("_NET_WORKAREA", "_NET_CURRENT_DESKTOP")
    raw = props.get("_NET_WORKAREA", "")
    if not raw:
        sys.exit("error: _NET_WORKAREA not available (are you on X11?)")

    values = parse_cardinal_list(raw)
    if len(values) < 4:
        sys.exit("error: _NET_WORKAREA has fewer than 4 values")

    # _NET_WORKAREA stores entries per virtual desktop, 4 ints each.
    # Use the current desktop index if available, otherwise desktop 0.
    desktop_idx = 0
    if "_NET_CURRENT_DESKTOP" in props:
        cd = parse_cardinal_list(props["_NET_CURRENT_DESKTOP"])
        if cd:
            desktop_idx = cd[0]

    offset = desktop_idx * 4
    if offset + 3 >= len(values):
        offset = 0  # fallback to first desktop

    x = values[offset]
    y = values[offset + 1]
    w = values[offset + 2]
    h = values[offset + 3]

    if dpr != 1.0:
        x = round(x / dpr)
        y = round(y / dpr)
        w = round(w / dpr)
        h = round(h / dpr)

    return x, y, w, h


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Query X11 _NET_WORKAREA with DPR-aware scaling"
    )
    parser.add_argument(
        "--json", action="store_true", default=True,
        help="output workarea as JSON (default)",
    )
    parser.add_argument(
        "--center", action="store_true",
        help="output center point (x y) in logical pixels",
    )
    parser.add_argument(
        "--half", action="store_true",
        help="output half-width x half-height in logical pixels",
    )
    parser.add_argument(
        "--dpr", action="store_true",
        help="output detected device-pixel-ratio",
    )
    parser.add_argument(
        "--raw", action="store_true",
        help="output raw _NET_WORKAREA (device pixels, no scaling)",
    )
    args = parser.parse_args()

    dpr = get_dpr()

    if args.dpr:
        print(f"{dpr:.2f}")
        return

    if args.raw:
        props = run_xprop("_NET_WORKAREA")
        raw = props.get("_NET_WORKAREA", "")
        if not raw:
            sys.exit("error: _NET_WORKAREA not available")
        values = parse_cardinal_list(raw)
        x, y, w, h = values[0], values[1], values[2], values[3]
        print(f"{x}x{y}+{w}+{h}")
        return

    x, y, w, h = get_workarea(dpr)

    if args.center:
        cx = x + w // 2
        cy = y + h // 2
        print(f"{cx} {cy}")
    elif args.half:
        print(f"{w // 2}x{h // 2}")
    else:
        # Default JSON output
        print(json.dumps({
            "x": x,
            "y": y,
            "width": w,
            "height": h,
            "dpr": round(dpr, 2),
        }))


if __name__ == "__main__":
    main()
