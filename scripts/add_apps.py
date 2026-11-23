"""Add the new Forge apps to catalog.json and copy their icons.

Reads each app's local folder for the facts that are only known after it is built:
installer AppId, EXE/installer sizes, screenshots and icon. Descriptions and
features are written here; adjust after the build reports if a feature changed.
Run: python scripts/add_apps.py  (from the forgedock folder)
"""

import json
import os
import re
import shutil

DESK = r"C:\Users\dotnet\Desktop"
HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

NEW = [
    {
        "id": "privacyforge",
        "name": "PrivacyForge",
        "accent": ["#0B7A78", "#F2B53A"],
        "tagline": "See what Windows shares, switch it off, and undo any of it.",
        "category": "System",
        "description": [
            "A privacy and debloat tool for Windows 10 and 11.",
            "PrivacyForge shows which Windows settings share data, show ads or run AI features, explains each one in plain words, and switches them off with one click. Every change is recorded, so it can be undone one by one or all at once.",
            "It can also remove built-in apps you never use, and tells you when a Windows update has switched a setting back on.",
        ],
        "features": [
            "Privacy score and a one-click Apply recommended that shows every change first",
            "Telemetry, diagnostic data, feedback prompts and the advertising ID",
            "Ads and suggestions in Start, the lock screen, Explorer, Settings and notifications",
            "Copilot, Recall, web search in Start, Widgets and activity history",
            "App permissions: location, camera, microphone, contacts, calendar and more",
            "135 settings in 12 categories, each explained, with a Recommended, Limited or Not recommended level",
            "Remove built-in Store apps, with critical parts of Windows protected",
            "Undo any single change or everything; restore point before big changes",
            "Notices when a Windows update turned a setting back on",
            "Profiles to export and import; a report of what changed",
        ],
        "needs_admin": "Only for PC-wide settings and removing apps for all users",
    },
    {
        "id": "metaforge",
        "name": "MetaForge",
        "accent": ["#2549D6", "#2BD9A0"],
        "tagline": "See and remove the hidden information in your files before you share them.",
        "category": "Security",
        "description": [
            "A metadata viewer, editor and cleaner for Windows.",
            "Photos can carry the place they were taken, documents the names of everyone who edited them, and downloads the address they came from. MetaForge shows this in plain language and removes it, file by file or a whole folder at once.",
            "Photos are cleaned without re-encoding, so their quality does not change.",
        ],
        "features": [
            "A privacy badge on every file: location, names, device serials, comments or clean",
            "Photos: EXIF, GPS, XMP, IPTC and PNG text, removed losslessly",
            "Office documents and PDFs: authors, last saved by, comments, template paths",
            "Audio and video tags: MP3, FLAC, M4A and MP4, including GPS from phones",
            "The hidden 'downloaded from' note Windows adds to downloads",
            "Clean for sharing presets, to the same file, a copy or another folder",
            "Edit titles, authors, dates and GPS; compare two files",
            "Export metadata to CSV or JSON",
            "Optional Explorer right-click menu and a command line",
            "Kurdish and Arabic file names shown correctly",
        ],
        "needs_admin": "No",
    },
    {
        "id": "displayforge",
        "name": "DisplayForge",
        "accent": ["#1F4FD6", "#FFB82E"],
        "tagline": "Brightness and colour for every monitor, from the tray.",
        "category": "System",
        "description": [
            "Monitor control for Windows: brightness and contrast for external monitors and laptop screens from one tray slider.",
            "DisplayForge talks to external monitors over DDC/CI, so you no longer need the buttons on the back of the screen. It adds a warm night light, schedules that follow sunrise and sunset, and profiles that switch when you open a certain app.",
        ],
        "features": [
            "Tray slider per monitor, a link-all slider and quick presets",
            "External monitors over DDC/CI: brightness, contrast, volume and input source",
            "Laptop screen brightness",
            "Dimming below the monitor's minimum",
            "Night light with a colour temperature slider",
            "Schedules by time or sunrise and sunset, with smooth transitions",
            "Per-app profiles, hotkeys and idle dimming",
            "Resolution and refresh rate with a confirm-or-revert countdown",
            "Monitor details from EDID; identify monitors",
            "Test patterns: dead pixels, uniformity, banding and colour bars",
        ],
        "needs_admin": "No",
    },
    {
        "id": "powerforge",
        "name": "PowerForge",
        "accent": ["#0A5C6E", "#F9C440"],
        "tagline": "Battery health, power plans, keep-awake, and why your PC won't sleep.",
        "category": "System",
        "description": [
            "A power and battery tool for Windows laptops and desktops.",
            "PowerForge shows how worn your battery is and how it changes over time, switches power plans automatically, keeps the PC awake when you need it, and explains in plain words what stops the PC from sleeping or what woke it up.",
        ],
        "features": [
            "Charge, live charge and discharge rate, time left",
            "Battery health and cycle count, with a history of capacity over time",
            "Charge-limit and low-battery reminders",
            "Power plans with plain-language settings, switched automatically by power source or app",
            "Keep awake: for a while, until a time, while an app runs or while a download is active",
            "What is blocking sleep, what woke the PC, wake timers and devices allowed to wake it",
            "Sleep and wake history",
            "Energy report summarised in plain language",
            "Estimated power use per app",
            "Tray icon, reports to HTML or CSV, and a command line",
        ],
        "needs_admin": "Only for some sleep diagnostics and the energy report",
    },
    {
        "id": "eventforge",
        "name": "EventForge",
        "accent": ["#3AA0F5", "#E8466F"],
        "tagline": "What went wrong on your PC, in plain language.",
        "category": "System",
        "description": [
            "Windows records every crash, blue screen and failed update in its event logs, but they are hard to read. EventForge reads them for you.",
            "It shows how stable your PC has been, lists each problem with how serious it is and what probably caused it, and gives numbered steps to fix it. Technical details are one click away for those who want them.",
        ],
        "features": [
            "A stability score per day and a clear status on the home page",
            "Blue screens, with the stop code explained and the minidump read",
            "Unexpected shutdowns, app crashes and hangs, failed Windows updates",
            "Disk, hardware and driver errors",
            "Every problem explained: what happened, how serious, likely cause, how to fix",
            "Repeats grouped, and apps ranked by how often they crash",
            "A day-by-day timeline with search and filters",
            "Startup and shutdown times",
            "A report for support, with private details hidden",
        ],
        "needs_admin": "No (some logs need it, asked only when you choose)",
    },
]

WIRE_EXTRA = [
    "Firewall: block an app from the internet in one click, for an hour, until restart or always",
    "Optional alert when an app connects to the internet for the first time, with Allow or Block",
    "Lockdown mode: only the apps you allow can go online",
]


def mb(path):
    return os.path.getsize(path) / 1_000_000 if os.path.exists(path) else None


def entry(app):
    folder = os.path.join(DESK, app["id"])
    name = app["name"]
    iss = os.path.join(folder, "installer", f"{name}.iss")
    app_id = None
    if os.path.exists(iss):
        m = re.search(r"AppId=\{\{([0-9A-Fa-f-]+)\}", open(iss, encoding="utf-8-sig").read())
        if m:
            app_id = "{" + m.group(1).upper() + "}"
    exe = mb(os.path.join(folder, "dist", f"{name}.exe"))
    setup = mb(os.path.join(folder, "dist", f"{name}-Setup.exe"))
    size = f"About {round(exe)} MB (EXE), {round(setup)} MB installer" if exe and setup else "About 5 MB"
    shots_dir = os.path.join(folder, "docs", "screenshots")
    shots = sorted(f for f in os.listdir(shots_dir) if f.endswith(".png")) if os.path.isdir(shots_dir) else []
    raw = f"https://raw.githubusercontent.com/zheerkhalid/{app['id']}/main"
    accent = app.get("accent", ["#4F6BFF", "#9B5CFF"])
    icon_src = os.path.join(folder, "docs", "assets", f"{app['id']}-icon.png")
    if os.path.exists(icon_src):
        shutil.copy(icon_src, os.path.join(HERE, "assets", "icons", f"{app['id']}-icon.png"))
    return {
        "id": app["id"],
        "name": name,
        "tagline": app["tagline"],
        "category": app["category"],
        "status": "available",
        "description": app["description"],
        "features": app["features"],
        "specs": {
            "platform": "Windows 10 and 11 (64-bit)",
            "language": "Rust",
            "licence": "MIT",
            "needs_admin": app["needs_admin"],
            "size": size,
        },
        "repo": f"zheerkhalid/{app['id']}",
        "accent": accent,
        "icon": f"{raw}/docs/assets/{app['id']}-icon.png",
        "screenshots": [f"{raw}/docs/screenshots/{s}" for s in shots],
        "assets": {
            "setup": f"{name}-Setup.exe",
            "portable": f"{name}-*-portable-x64.zip",
            "exe": f"{name}.exe",
        },
        "detect": {"inno_app_id": app_id, "display_name": name},
    }


def main():
    path = os.path.join(HERE, "catalog.json")
    cat = json.load(open(path, encoding="utf-8"))
    ids = {a["id"] for a in cat["apps"]}
    for app in NEW:
        if app["id"] in ids or not os.path.isdir(os.path.join(DESK, app["id"])):
            continue
        cat["apps"].append(entry(app))
        print("added", app["id"])
    wire = next(a for a in cat["apps"] if a["id"] == "wireforge")
    for f in WIRE_EXTRA:
        if f not in wire["features"]:
            wire["features"].append(f)
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        json.dump(cat, fh, indent=2, ensure_ascii=False)
        fh.write("\n")
    print(len(cat["apps"]), "apps")


if __name__ == "__main__":
    main()
