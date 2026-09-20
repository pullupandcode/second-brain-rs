#!/usr/bin/env python3
"""Plan versioned releases and publish verified immutable assets (Python 3.11+)."""
from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import tomllib


class ReleaseError(RuntimeError):
    """A failed precondition; never repair remote history implicitly."""


VERSION = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\Z")
SHA = re.compile(r"[0-9a-f]{40}\Z")
IMMUTABLE_BASELINE = "v0.5.1"
TARGETS = {
    "x86_64-unknown-linux-gnu": "tar.gz",
    "aarch64-apple-darwin": "tar.gz",
    "x86_64-apple-darwin": "tar.gz",
    "x86_64-pc-windows-msvc": "zip",
}


def version_tuple(value: str) -> tuple[int, int, int]:
    if not isinstance(value, str) or not VERSION.fullmatch(value):
        raise ReleaseError(f"Expected stable X.Y.Z version, got {value!r}")
    return tuple(map(int, value.split(".")))


def command(argv: list[str], *, cwd: Path | None = None, body: bytes | None = None,
            missing_ok: bool = False) -> bytes | None:
    result = subprocess.run(argv, cwd=cwd, input=body, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, check=False)
    if result.returncode:
        error = result.stderr.decode("utf-8", errors="replace").strip()
        if missing_ok and re.search(r"\(HTTP 404\)(?:\s|$)", error):
            return None
        raise ReleaseError(f"{argv[0]} failed: {error}")
    return result.stdout


def api(repository: str, route: str, *, method: str = "GET", body: dict | None = None,
        missing_ok: bool = False):
    argv = ["gh", "api", "--method", method, f"repos/{repository}/{route}"]
    payload = None
    if body is not None:
        argv += ["--input", "-"]
        payload = json.dumps(body).encode()
    raw = command(argv, body=payload, missing_ok=missing_ok)
    if raw is None:
        return None
    try:
        return json.loads(raw)
    except (ValueError, UnicodeDecodeError) as error:
        raise ReleaseError(f"Invalid JSON from GitHub {route}") from error


def source_version(root: Path) -> str:
    try:
        manifest = tomllib.loads((root / "Cargo.toml").read_text())
        version = manifest["package"]["version"]
        if manifest["package"]["name"] != "second-brain-rs":
            raise ReleaseError("Unexpected Cargo package name")
        version_tuple(version)
        lock = tomllib.loads((root / "Cargo.lock").read_text())
        matches = [p for p in lock["package"] if p.get("name") == "second-brain-rs"]
        if len(matches) != 1 or matches[0].get("version") != version:
            raise ReleaseError("Cargo.lock must contain exactly one matching second-brain-rs version")
        return version
    except (OSError, KeyError, TypeError, tomllib.TOMLDecodeError) as error:
        raise ReleaseError(f"Invalid Cargo version metadata: {error}") from error


def release_notes(root: Path, version: str) -> str:
    text = (root / "CHANGELOG.md").read_text()
    headings = list(re.finditer(r"^## \[([^\]\n]+)\][^\n]*$", text, re.MULTILINE))
    matches = [i for i, heading in enumerate(headings) if heading[1] == version]
    if len(matches) != 1:
        raise ReleaseError(f"CHANGELOG.md needs exactly one section for {version}")
    i = matches[0]
    heading = headings[i]
    dated = re.fullmatch(r"## \[" + re.escape(version) + r"\] - (\d{4}-\d{2}-\d{2})", heading[0])
    if not dated:
        raise ReleaseError("Release changelog heading must have an exact version and YYYY-MM-DD date")
    try:
        datetime.date.fromisoformat(dated[1])
    except ValueError as error:
        raise ReleaseError("Invalid changelog release date") from error
    end = headings[i + 1].start() if i + 1 < len(headings) else len(text)
    notes = text[heading.end():end].strip()
    if not notes:
        raise ReleaseError(f"Empty changelog section for {version}")
    return notes


def plan(root: Path, base: str) -> dict[str, str]:
    if not SHA.fullmatch(base):
        raise ReleaseError("--base must be a full lowercase git commit SHA")
    current = source_version(root)
    release_notes(root, current)
    try:
        previous = tomllib.loads(command(["git", "show", f"{base}:Cargo.toml"], cwd=root).decode())["package"]["version"]
    except (KeyError, ValueError, UnicodeDecodeError) as error:
        raise ReleaseError("Cannot read base Cargo package version") from error
    before, after = version_tuple(previous), version_tuple(current)
    if after < before:
        raise ReleaseError(f"Version downgrade: {previous} -> {current}")
    result = {"version": current, "release": str(after > before).lower()}
    output = "".join(f"{name}={value}\n" for name, value in result.items())
    print(output, end="")
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as stream:
            stream.write(output)
    return result


def local_assets(directory: Path, version: str) -> tuple[dict[str, tuple[str, int]], bytes]:
    names = {f"second-brain-rs-v{version}-{target}.{extension}" for target, extension in TARGETS.items()}
    present = {p.name for p in directory.iterdir()}
    if present - (names | {"SHA256SUMS"}) or not names <= present:
        raise ReleaseError("Asset directory must contain exactly the four target archives and optional SHA256SUMS")
    expected = {}
    for name in sorted(names):
        path = directory / name
        if path.is_symlink() or not path.is_file():
            raise ReleaseError(f"Asset must be a regular file: {name}")
        with path.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        expected[name] = (digest, path.stat().st_size)
    sums = "".join(f"{digest}  {name}\n" for name, (digest, _) in expected.items()).encode()
    manifest = directory / "SHA256SUMS"
    if manifest.exists() and (manifest.is_symlink() or not manifest.is_file() or manifest.read_bytes() != sums):
        raise ReleaseError("Existing SHA256SUMS differs from archive bytes")
    expected["SHA256SUMS"] = (hashlib.sha256(sums).hexdigest(), len(sums))
    return expected, sums


def tag_target(repository: str, tag: str) -> str | None:
    ref = api(repository, f"git/ref/tags/{tag}", missing_ok=True)
    if ref is None:
        return None
    obj = ref.get("object", {})
    for _ in range(10):
        sha = obj.get("sha", "")
        if not isinstance(sha, str) or not SHA.fullmatch(sha):
            raise ReleaseError("Malformed remote tag object")
        if obj.get("type") == "commit":
            return sha
        if obj.get("type") != "tag":
            break
        obj = api(repository, f"git/tags/{sha}").get("object", {})
    raise ReleaseError("Remote tag does not resolve to a commit")


def verify_assets(repository: str, remote: dict, expected: dict[str, tuple[str, int]], *, complete: bool) -> set[str]:
    found = set()
    for asset in remote.get("assets", []):
        name = asset.get("name")
        if name not in expected or name in found:
            raise ReleaseError(f"Unexpected or duplicate remote asset: {name!r}")
        digest, size = expected[name]
        if asset.get("state") != "uploaded" or asset.get("size") != size:
            raise ReleaseError(f"Remote asset is incomplete or has wrong size: {name}")
        actual = asset.get("digest")
        if actual is None or actual == "":
            asset_id = asset.get("id")
            if not isinstance(asset_id, int) or asset_id <= 0:
                raise ReleaseError("Missing remote asset ID")
            raw = command(["gh", "api", "--method", "GET", f"repos/{repository}/releases/assets/{asset_id}",
                           "-H", "Accept: application/octet-stream"])
            actual = "sha256:" + hashlib.sha256(raw).hexdigest()
            if len(raw) != size:
                raise ReleaseError(f"Downloaded asset has wrong size: {name}")
        if actual != f"sha256:{digest}":
            raise ReleaseError(f"Remote asset checksum mismatch: {name}; refusing to overwrite")
        found.add(name)
    if complete and found != set(expected):
        raise ReleaseError("Published release does not contain the complete expected asset set")
    return found


def listed_releases(repository: str):
    """Authenticated listing includes drafts; tag lookup only finds published releases."""
    page = 1
    while True:
        releases = api(repository, f"releases?per_page=100&page={page}")
        if not isinstance(releases, list) or any(not isinstance(item, dict) for item in releases):
            raise ReleaseError("Invalid release listing")
        yield from releases
        if len(releases) < 100:
            return
        page += 1


def find_release(repository: str, tag: str) -> dict | None:
    published = api(repository, f"releases/tags/{tag}", missing_ok=True)
    if published is not None:
        return published
    matches = [item for item in listed_releases(repository) if item.get("tag_name") == tag]
    if len(matches) > 1:
        raise ReleaseError(f"Multiple releases claim {tag}; refusing ambiguous draft retry")
    if not matches:
        return None
    draft = matches[0]
    release_id = draft.get("id")
    if draft.get("draft") is not True or type(release_id) is not int or release_id <= 0:
        raise ReleaseError("Inconsistent draft discovery response")
    current = api(repository, f"releases/{release_id}")
    if current.get("id") != release_id or current.get("tag_name") != tag or current.get("draft") is not True:
        raise ReleaseError("Draft changed during discovery")
    return current


def latest_allowed(repository: str, version: str) -> bool:
    for item in listed_releases(repository):
        tag = item.get("tag_name", "")
        if item.get("draft") or item.get("prerelease") or not tag.startswith("v"):
            continue
        if VERSION.fullmatch(tag[1:]) and version_tuple(tag[1:]) > version_tuple(version):
            return False
    return True


def require_current_main(repository: str, sha: str) -> None:
    if api(repository, "commits/main").get("sha") != sha:
        raise ReleaseError("Release commit is no longer current main; refusing stale publication")


def publish(root: Path, assets: Path, sha: str, repository: str) -> None:
    if not SHA.fullmatch(sha) or not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ReleaseError("Supply a full lowercase SHA and owner/repository")
    version = source_version(root)
    notes = release_notes(root, version)
    if command(["git", "rev-parse", "HEAD"], cwd=root).decode().strip() != sha:
        raise ReleaseError("Release SHA does not match checked-out HEAD")
    assets = assets.resolve(strict=True)
    if "#" in str(assets):
        raise ReleaseError(f"Asset directory path must not contain '#': {assets}")
    expected, sums = local_assets(assets, version)
    # The current repository setting requires Administration:read, which the
    # publisher intentionally lacks. Administrators must keep it enabled. This
    # historical baseline is a sanity check, NOT proof of the current setting.
    baseline = api(repository, f"releases/tags/{IMMUTABLE_BASELINE}")
    if baseline.get("tag_name") != IMMUTABLE_BASELINE or baseline.get("draft") is not False or baseline.get("immutable") is not True:
        raise ReleaseError(f"Expected immutable published baseline {IMMUTABLE_BASELINE}")
    tag = f"v{version}"
    target = tag_target(repository, tag)
    if target is not None and target != sha:
        raise ReleaseError("Existing tag targets another commit; refusing to overwrite")
    remote = find_release(repository, tag)
    if remote is not None:
        if remote.get("tag_name") != tag or target is None:
            raise ReleaseError("Existing release has inconsistent tag identity")
        if remote.get("draft") is False:
            if remote.get("immutable") is not True:
                raise ReleaseError("Existing published release is mutable; refusing to repair history")
            verify_assets(repository, remote, expected, complete=True)
            print(f"Verified existing immutable release {tag}")
            return
        if remote.get("draft") is not True:
            raise ReleaseError("Malformed remote release state")
        present = verify_assets(repository, remote, expected, complete=False)
    else:
        present = set()
    require_current_main(repository, sha)
    make_latest = "true" if latest_allowed(repository, version) else "false"
    if target is None:
        annotated = api(repository, "git/tags", method="POST", body={
            "tag": tag, "message": f"Release {tag}", "object": sha, "type": "commit"})
        object_sha = annotated.get("sha", "")
        if not isinstance(object_sha, str) or not SHA.fullmatch(object_sha):
            raise ReleaseError("GitHub did not return an annotated tag SHA")
        api(repository, "git/refs", method="POST", body={"ref": f"refs/tags/{tag}", "sha": object_sha})
    if remote is None:
        remote = api(repository, "releases", method="POST", body={
            "tag_name": tag, "target_commitish": sha, "name": tag, "body": notes,
            "draft": True, "prerelease": False})
    release_id = remote.get("id")
    if not isinstance(release_id, int) or release_id <= 0:
        raise ReleaseError("Missing draft release ID")
    with tempfile.TemporaryDirectory(prefix="release-checksums-") as temporary:
        checksum_path = Path(temporary) / "SHA256SUMS"
        checksum_path.write_bytes(sums)
        uploads = {name: checksum_path if name == "SHA256SUMS" else assets / name
                   for name in sorted(set(expected) - present)}
        # gh treats "path#label" as a display-label separator; never pass a '#' through.
        if any("#" in str(path) for path in uploads.values()):
            raise ReleaseError("Asset paths must not contain '#'")
        for path in uploads.values():
            command(["gh", "release", "upload", tag, str(path), "--repo", repository])
    remote = api(repository, f"releases/{release_id}")
    if remote.get("id") != release_id or remote.get("tag_name") != tag or remote.get("draft") is not True:
        raise ReleaseError("Draft changed during asset upload")
    verify_assets(repository, remote, expected, complete=True)
    if tag_target(repository, tag) != sha:
        raise ReleaseError("Tag changed during publication")
    require_current_main(repository, sha)
    api(repository, f"releases/{release_id}", method="PATCH", body={
        "draft": False, "prerelease": False, "name": tag, "body": notes, "make_latest": make_latest})
    published = api(repository, f"releases/tags/{tag}")
    if published.get("id") != release_id or published.get("draft") is not False or published.get("immutable") is not True:
        raise ReleaseError("Publication did not produce a verified immutable release")
    if tag_target(repository, tag) != sha:
        raise ReleaseError("Published tag targets another commit")
    verify_assets(repository, published, expected, complete=True)
    print(f"Published and verified immutable release {tag}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="action", required=True)
    planner = commands.add_parser("plan")
    planner.add_argument("--base", required=True)
    publisher = commands.add_parser("publish")
    publisher.add_argument("--assets", type=Path, required=True)
    publisher.add_argument("--sha", required=True)
    publisher.add_argument("--repository", required=True)
    args = parser.parse_args()
    try:
        if args.action == "plan":
            plan(Path.cwd(), args.base)
        else:
            publish(Path.cwd(), args.assets, args.sha, args.repository)
    except (ReleaseError, OSError) as error:
        print(f"release: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
