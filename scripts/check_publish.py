#!/usr/bin/env python3
"""Audit Git's publication candidates without printing private content."""
import argparse
from pathlib import Path
import re
import subprocess
import sys


ROOT = Path(__file__).resolve().parent.parent
PRIVATE_DIRS = {"pictures", ".build", "dist", ".nimbalyst", ".local-archive", "local-data", "artifacts"}
MEDIA_SUFFIXES = {".jpg", ".jpeg", ".heic", ".heif", ".png", ".gif", ".webp", ".mov", ".mp4", ".m4v", ".avi", ".mkv", ".zip", ".dmg", ".pem", ".key"}
PATTERNS = [
    ("personal absolute path", re.compile(r"/(?:Users|home)/[^/\s]+/")),
    ("sample filename", re.compile(r"(?:IMG_|MVIMG_|PXL_|CRall_)\d{4,}|20\d{6}_\d{6,}")),
    ("sample/device model", re.compile(r"\bSM-[A-Z0-9]{3,}\b")),
    ("fixed content fingerprint", re.compile(r"(?<![0-9a-fA-F])[0-9a-fA-F]{64}(?![0-9a-fA-F])")),
    ("access credential", re.compile(r"gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|AKIA[0-9A-Z]{16}")),
    ("private key", re.compile(r"-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----")),
]


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT)


def audit(staged=False):
    listing = git("ls-files", "--cached", "-z") if staged else git("ls-files", "--cached", "--others", "--exclude-standard", "-z")
    paths = sorted({name.decode("utf-8") for name in listing.split(b"\0") if name})
    failures = []
    checked = 0
    for name in paths:
        path = ROOT / name
        if not staged and not path.exists() and not path.is_symlink():
            continue
        parts = Path(name).parts
        if any(p in PRIVATE_DIRS or p.startswith(("converted-", "auxiliary-exports")) for p in parts):
            failures.append((name, 0, "private data directory"))
            continue
        if path.suffix.lower() in MEDIA_SUFFIXES or Path(name).name.startswith(".env"):
            failures.append((name, 0, "media, package, or credential file"))
            continue
        mode = git("ls-files", "--stage", "--", name).split(b" ", 1)[0]
        if (staged and mode == b"120000") or (not staged and path.is_symlink()):
            failures.append((name, 0, "symbolic link requires manual review"))
            continue
        data = git("show", ":" + name) if staged else path.read_bytes()
        try:
            content = data.decode("utf-8")
        except UnicodeDecodeError:
            failures.append((name, 0, "binary file requires manual review"))
            continue
        checked += 1
        for number, line in enumerate(content.splitlines(), 1):
            for reason, pattern in PATTERNS:
                if pattern.search(line):
                    failures.append((name, number, reason))
    for name, number, reason in failures:
        print(f"FAIL {name}:{number}: {reason}")
    print(f"Publication audit: {checked} text files, {len(failures)} findings")
    return 1 if failures else 0


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--staged", action="store_true", help="Audit the complete index snapshot")
    sys.exit(audit(parser.parse_args().staged))
