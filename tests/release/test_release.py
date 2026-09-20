"""Release publication acceptance tests; GitHub and git are fully mocked."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("release_helper", Path(__file__).parents[2] / "scripts" / "release.py")
release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release)
SHA = "a" * 40
BASE = "b" * 40
VERSION = "0.5.2"
REPO = "example/second-brain-rs"
NAMES = [f"second-brain-rs-v{VERSION}-{target}.{extension}" for target, extension in [
    ("x86_64-unknown-linux-gnu", "tar.gz"), ("aarch64-apple-darwin", "tar.gz"),
    ("x86_64-apple-darwin", "tar.gz"), ("x86_64-pc-windows-msvc", "zip")]]


class GitHub:
    def __init__(self, assets):
        self.assets = assets
        self.calls = []
        self.enabled = True
        self.baseline_immutable = True
        self.main_sha = SHA
        self.advance_on_upload = False
        self.tag = None
        self.annotated = False
        self.release = None
        self.older = []
        self.base_version = "0.5.1"
        self.head = SHA
        self.fail_lookup = False
        self.fail_upload = None
        self.corrupt_upload = False
        self.lock_published = True
        self.latest = None
        self.downloads = {}

    @property
    def mutations(self):
        return [call for call in self.calls if call[:3] == ["gh", "release", "upload"] or "POST" in call or "PATCH" in call]

    def record(self, name, digest=True):
        data = self.assets[name]
        result = {"id": len(self.downloads) + 100, "name": name, "size": len(data), "state": "uploaded"}
        if digest:
            result["digest"] = "sha256:" + hashlib.sha256(data).hexdigest()
        self.downloads[result["id"]] = data
        return result

    def existing(self, names=(), published=False, immutable=True):
        self.tag = SHA
        self.annotated = True
        self.release = {"id": 7, "tag_name": f"v{VERSION}", "draft": not published, "immutable": immutable,
                        "assets": [self.record(name) for name in names]}

    def __call__(self, argv, **kwargs):
        self.calls.append(list(argv))
        status, output, error = 0, b"", b""
        if argv[:2] == ["git", "show"]:
            output = f'[package]\nname="second-brain-rs"\nversion="{self.base_version}"\n'.encode()
        elif argv[:3] == ["git", "rev-parse", "HEAD"]:
            output = (self.head + "\n").encode()
        elif argv[:3] == ["gh", "release", "upload"]:
            name = Path(argv[4]).name
            if name == self.fail_upload:
                status, error = 1, b"simulated interrupted upload"
            else:
                self.assets[name] = Path(argv[4]).read_bytes()
                item = self.record(name)
                if self.corrupt_upload:
                    item["digest"] = "sha256:" + "0" * 64
                self.release["assets"].append(item)
                if self.advance_on_upload:
                    self.main_sha = "f" * 40
        elif argv[:2] == ["gh", "api"]:
            method = argv[argv.index("--method") + 1]
            endpoint = argv[argv.index("--method") + 2]
            body = json.loads(kwargs.get("input") or b"{}")
            prefix = f"repos/{REPO}/"
            self.assert_prefix(endpoint, prefix)
            route = endpoint[len(prefix):]
            data = None
            if route == "immutable-releases":
                data = {"enabled": self.enabled}
            elif route.startswith("git/ref/tags/"):
                if self.tag:
                    data = {"object": {"type": "tag" if self.annotated else "commit", "sha": "c" * 40 if self.annotated else self.tag}}
                else:
                    status, error = 1, b"gh: Not Found (HTTP 404)"
            elif route == "git/tags/" + "c" * 40:
                data = {"object": {"type": "commit", "sha": self.tag}}
            elif route == "git/tags" and method == "POST":
                assert body["object"] == SHA and body["type"] == "commit"
                data = {"sha": "c" * 40}
            elif route == "git/refs" and method == "POST":
                assert body["sha"] == "c" * 40
                self.tag = SHA
                self.annotated = True
                data = {"ref": body["ref"]}
            elif route == "commits/main":
                data = {"sha": self.main_sha}
            elif route == "releases/tags/v0.5.1":
                data = {"tag_name": "v0.5.1", "draft": False, "immutable": self.baseline_immutable}
            elif route.startswith("releases/tags/"):
                if self.fail_lookup:
                    status, error = 1, b"gh: server failure (HTTP 500)"
                elif self.release and not self.release["draft"]:
                    data = self.release
                else:
                    status, error = 1, b"gh: Not Found (HTTP 404)"
            elif route.startswith("releases?"):
                page = int(route.rsplit("page=", 1)[1])
                listed = self.older + ([self.release] if self.release else [])
                data = listed[(page - 1) * 100:page * 100]
            elif route == "releases" and method == "POST":
                assert body["draft"] is True
                self.release = {"id": 7, "tag_name": body["tag_name"], "draft": True, "immutable": False, "assets": []}
                data = self.release
            elif route == "releases/7" and method == "GET":
                data = self.release
            elif route == "releases/7" and method == "PATCH":
                assert body["draft"] is False
                self.latest = body["make_latest"]
                self.release.update(draft=False, immutable=self.lock_published)
                data = self.release
            elif route.startswith("releases/assets/"):
                output = self.downloads[int(route.rsplit("/", 1)[1])]
            else:
                raise AssertionError((argv, body))
            if data is not None:
                output = json.dumps(data).encode()
        else:
            raise AssertionError(argv)
        return subprocess.CompletedProcess(argv, status, output, error)

    @staticmethod
    def assert_prefix(endpoint, prefix):
        assert endpoint.startswith(prefix), endpoint


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.dist = self.root / "dist"
        self.dist.mkdir()
        (self.root / "Cargo.toml").write_text(f'[package]\nname="second-brain-rs"\nversion="{VERSION}"\n')
        (self.root / "Cargo.lock").write_text(f'[[package]]\nname="second-brain-rs"\nversion="{VERSION}"\n')
        (self.root / "CHANGELOG.md").write_text(f'# Changelog\n\n## [Unreleased]\n\n## [{VERSION}] - 2026-09-18\n\n- Automate verified releases.\n\n## [0.5.1] - 2026-09-18\n\n- Older release.\n')
        self.assets = {name: ("binary: " + name).encode() for name in NAMES}
        for name, data in self.assets.items():
            (self.dist / name).write_bytes(data)
        self.assets["SHA256SUMS"] = "".join(f"{hashlib.sha256(self.assets[name]).hexdigest()}  {name}\n" for name in sorted(NAMES)).encode()
        self.gh = GitHub(self.assets.copy())
        self.addCleanup(patch.stopall)
        patch.object(subprocess, "run", self.gh).start()

    def publish(self):
        return release.publish(self.root, self.dist, SHA, REPO)

    def reject(self):
        with self.assertRaises(release.ReleaseError):
            self.publish()

    def test_plan_bump_and_outputs(self):
        output = self.root / "output"
        with patch.dict(os.environ, {"GITHUB_OUTPUT": str(output)}):
            self.assertEqual(release.plan(self.root, BASE), {"version": VERSION, "release": "true"})
        self.assertEqual(output.read_text(), f"version={VERSION}\nrelease=true\n")

    def test_plan_same_version_skips_and_downgrade_rejects(self):
        self.gh.base_version = VERSION
        self.assertEqual(release.plan(self.root, BASE)["release"], "false")
        self.gh.base_version = "1.0.0"
        with self.assertRaises(release.ReleaseError):
            release.plan(self.root, BASE)

    def test_plan_rejects_lock_mismatch_and_empty_changelog(self):
        (self.root / "Cargo.lock").write_text('[[package]]\nname="second-brain-rs"\nversion="0.5.1"\n')
        with self.assertRaises(release.ReleaseError):
            release.plan(self.root, BASE)
        (self.root / "Cargo.lock").write_text(f'[[package]]\nname="second-brain-rs"\nversion="{VERSION}"\n')
        (self.root / "CHANGELOG.md").write_text(f'## [{VERSION}] - 2026-09-18\n\n## [0.5.1]\n- other\n')
        with self.assertRaises(release.ReleaseError):
            release.plan(self.root, BASE)

    def test_plan_rejects_nonstable_version(self):
        (self.root / "Cargo.toml").write_text('[package]\nname="second-brain-rs"\nversion="0.5.2-rc1"\n')
        with self.assertRaises(release.ReleaseError):
            release.plan(self.root, BASE)

    def test_publish_success_drafts_uploads_then_locks(self):
        self.publish()
        self.assertFalse(self.gh.release["draft"])
        self.assertTrue(self.gh.release["immutable"])
        self.assertEqual({a["name"] for a in self.gh.release["assets"]}, set(self.assets))
        self.assertEqual(self.gh.latest, "true")
        self.assertNotIn("--clobber", str(self.gh.calls))
        self.assertNotIn("DELETE", str(self.gh.calls))
        self.assertNotIn("immutable-releases", str(self.gh.calls))

    def test_immutable_baseline_required_without_admin_permission(self):
        self.gh.baseline_immutable = False
        self.reject()
        self.assertEqual(self.gh.mutations, [])

    def test_stale_main_fails_before_mutation(self):
        self.gh.main_sha = "f" * 40
        self.reject()
        self.assertEqual(self.gh.mutations, [])

    def test_main_advance_during_upload_keeps_release_draft(self):
        self.gh.advance_on_upload = True
        self.reject()
        self.assertTrue(self.gh.release["draft"])
        self.assertIsNone(self.gh.latest)

    def test_wrong_tag_never_overwrites(self):
        self.gh.tag = "d" * 40
        self.reject()
        self.assertEqual(self.gh.mutations, [])

    def test_missing_archive_wrong_sha_or_bad_manifest_never_mutates(self):
        missing = self.dist / NAMES[0]
        data = missing.read_bytes(); missing.unlink()
        self.reject(); self.assertEqual(self.gh.mutations, [])
        missing.write_bytes(data)
        self.gh.head = "e" * 40
        self.reject(); self.assertEqual(self.gh.mutations, [])
        self.gh.head = SHA
        (self.dist / "SHA256SUMS").write_text("forged\n")
        self.reject(); self.assertEqual(self.gh.mutations, [])

    def test_partial_same_tag_draft_uploads_only_missing_assets(self):
        self.gh.existing(NAMES[:2])
        self.publish()
        uploaded = [Path(c[4]).name for c in self.gh.calls if c[:3] == ["gh", "release", "upload"]]
        self.assertEqual(set(uploaded), set(self.assets) - set(NAMES[:2]))
        self.assertFalse(any("git/refs" in str(c) for c in self.gh.mutations))

    def test_draft_lookup_paginates_and_refreshes_by_release_id(self):
        self.gh.existing(NAMES[:1])
        self.gh.older = [{"tag_name": "v9.0.0", "draft": True, "prerelease": False}] * 100
        self.publish()
        self.assertTrue(any(any(arg.endswith("/releases/7") for arg in c) and "GET" in c for c in self.gh.calls))
        self.assertTrue(any("per_page=100&page=2" in str(c) for c in self.gh.calls))

    def test_duplicate_same_tag_drafts_fail_without_mutation(self):
        self.gh.existing(NAMES[:1])
        self.gh.older = [dict(self.gh.release, id=8)]
        self.reject()
        self.assertEqual(self.gh.mutations, [])

    def test_interrupted_upload_stays_draft_then_retry_succeeds(self):
        self.gh.fail_upload = sorted(NAMES)[1]
        self.reject()
        self.assertTrue(self.gh.release["draft"])
        self.gh.fail_upload = None
        self.publish()
        self.assertFalse(self.gh.release["draft"])

    def test_draft_asset_hash_mismatch_or_extra_asset_fails_without_mutation(self):
        self.gh.existing(NAMES[:1])
        self.gh.release["assets"][0]["digest"] = "sha256:" + "0" * 64
        self.reject(); self.assertEqual(self.gh.mutations, [])
        self.gh.release["assets"] = [{"name": "unexpected.zip"}]
        self.reject(); self.assertEqual(self.gh.mutations, [])

    def test_published_rerun_is_read_only_and_checks_hashes(self):
        self.gh.existing(self.assets, published=True)
        self.publish()
        self.assertEqual(self.gh.mutations, [])
        self.gh.release["assets"][0]["digest"] = "sha256:" + "0" * 64
        self.reject(); self.assertEqual(self.gh.mutations, [])

    def test_published_mutable_or_incomplete_release_is_rejected(self):
        self.gh.existing(self.assets, published=True, immutable=False)
        self.reject(); self.assertEqual(self.gh.mutations, [])
        self.gh.existing(NAMES[:1], published=True)
        self.reject(); self.assertEqual(self.gh.mutations, [])

    def test_missing_digest_uses_authenticated_download(self):
        self.gh.existing(self.assets, published=True)
        for asset in self.gh.release["assets"]:
            asset.pop("digest")
        self.publish()
        self.assertEqual(self.gh.mutations, [])
        self.assertTrue(any("application/octet-stream" in str(c) for c in self.gh.calls))

    def test_api_failure_is_not_release_absence(self):
        self.gh.fail_lookup = True
        self.reject(); self.assertEqual(self.gh.mutations, [])

    def test_older_version_does_not_move_latest_backward(self):
        self.gh.older = [{"tag_name": "v0.9.0", "draft": False, "prerelease": False}]
        self.publish()
        self.assertEqual(self.gh.latest, "false")

    def test_latest_comparison_paginates_and_ignores_drafts_and_prereleases(self):
        self.gh.older = [{"tag_name": "v9.0.0", "draft": True, "prerelease": False}] * 100
        self.gh.older += [{"tag_name": "v9.0.0-rc1", "draft": False, "prerelease": True},
                          {"tag_name": "v0.8.0", "draft": False, "prerelease": False}]
        self.publish()
        self.assertEqual(self.gh.latest, "false")
        self.assertTrue(any("per_page=100&page=2" in str(c) for c in self.gh.calls))

    def test_duplicate_changelog_section_is_rejected(self):
        path = self.root / "CHANGELOG.md"
        path.write_text(path.read_text() + f"\n## [{VERSION}] - 2026-09-18\n- duplicate\n")
        with self.assertRaises(release.ReleaseError):
            release.plan(self.root, BASE)

    def test_published_retry_after_main_advance_remains_read_only(self):
        self.gh.existing(self.assets, published=True)
        self.gh.main_sha = "f" * 40
        self.publish()
        self.assertEqual(self.gh.mutations, [])

    def test_uploaded_hash_failure_prevents_publishing(self):
        self.gh.corrupt_upload = True
        self.reject()
        self.assertTrue(self.gh.release["draft"])
        self.assertIsNone(self.gh.latest)

    def test_asset_path_containing_hash_is_never_passed_to_gh_upload(self):
        hashed = self.root / "dist#1"
        hashed.mkdir()
        for name in NAMES:
            (hashed / name).write_bytes(self.assets[name])
        with self.assertRaises(release.ReleaseError):
            release.publish(self.root, hashed, SHA, REPO)
        self.assertEqual(self.gh.mutations, [])

    def test_post_publish_immutability_must_be_verified(self):
        self.gh.lock_published = False
        self.reject()
        self.assertFalse(self.gh.release["draft"])


if __name__ == "__main__":
    unittest.main()
