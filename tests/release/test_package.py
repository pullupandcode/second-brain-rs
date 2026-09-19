"""Archive contents and permissions are part of the download contract."""
import importlib.util
import pathlib
import tarfile
import tempfile
import unittest
import zipfile

SCRIPT = pathlib.Path(__file__).resolve().parents[2] / 'scripts' / 'package_release.py'
spec = importlib.util.spec_from_file_location('package_release', SCRIPT)
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)


class PackageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = pathlib.Path(self.temp.name)
        (self.root / 'Cargo.toml').write_text('[package]\nname="second-brain-rs"\nversion="0.5.2"\n')
        for name in package.DOCUMENTS:
            (self.root / name).write_text(name)
        self.binary = self.root / 'program'
        self.binary.write_bytes(b'executable fixture')
        self.out = self.root / 'dist'

    def test_tar_contains_executable_and_only_public_documents(self):
        (self.root / 'config.local.toml').write_text('secret')
        archive = package.build_archive(self.root, self.binary, self.out, 'x86_64-unknown-linux-gnu')
        self.assertEqual(archive.name, 'second-brain-rs-v0.5.2-x86_64-unknown-linux-gnu.tar.gz')
        with tarfile.open(archive) as handle:
            entries = handle.getmembers()
            self.assertEqual({pathlib.PurePosixPath(m.name).name for m in entries}, {'second-brain-rs', *package.DOCUMENTS})
            executable = next(m for m in entries if m.name.endswith('/second-brain-rs'))
            self.assertEqual(executable.mode, 0o755)
            self.assertEqual(handle.extractfile(executable).read(), b'executable fixture')
        self.assertEqual(package.unpack_archive(archive, self.root / 'unpacked').read_bytes(), b'executable fixture')

    def test_windows_zip_uses_exe_and_same_documents(self):
        archive = package.build_archive(self.root, self.binary, self.out, 'x86_64-pc-windows-msvc')
        with zipfile.ZipFile(archive) as handle:
            self.assertEqual({pathlib.PurePosixPath(n).name for n in handle.namelist()}, {'second-brain-rs.exe', *package.DOCUMENTS})
        self.assertEqual(package.unpack_archive(archive, self.root / 'unpacked').name, 'second-brain-rs.exe')

    def test_missing_binary_fails_without_creating_archive(self):
        with self.assertRaises(FileNotFoundError):
            package.build_archive(self.root, self.root / 'missing', self.out, 'aarch64-apple-darwin')
        self.assertFalse(self.out.exists())

    def test_unsupported_target_fails_before_packaging(self):
        with self.assertRaises(ValueError):
            package.build_archive(self.root, self.binary, self.out, '../../escape')

    def test_unpack_rejects_traversal_and_links(self):
        archive = self.root / 'evil.tar.gz'
        with tarfile.open(archive, 'w:gz') as handle:
            member = tarfile.TarInfo('../escaped')
            member.type = tarfile.SYMTYPE
            member.linkname = '/etc/passwd'
            handle.addfile(member)
        with self.assertRaises(ValueError):
            package.unpack_archive(archive, self.root / 'unpacked')
        self.assertFalse((self.root / 'escaped').exists())

    def test_unpack_rejects_extra_or_missing_files(self):
        archive = self.root / 'incomplete.zip'
        with zipfile.ZipFile(archive, 'w') as handle:
            handle.writestr('pkg/second-brain-rs.exe', b'fixture')
        with self.assertRaises(ValueError):
            package.unpack_archive(archive, self.root / 'unpacked')


if __name__ == '__main__':
    unittest.main()
