"""Offline checks: python3 -I -B -m unittest discover -s tools/probes/chief-harness."""

import ast
from contextlib import ExitStack, redirect_stdout
import hashlib
import io
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tempfile
import types
import unittest
from unittest import mock


HERE = Path(__file__).resolve().parent


def load_probe(name):
    """Load the script without its final main() call; no harness or login is read."""
    path = HERE / (name + ".py")
    tree = ast.parse(path.read_bytes(), filename=str(path))
    last = tree.body.pop()
    if not (isinstance(last, ast.Expr) and isinstance(last.value, ast.Call)
            and isinstance(last.value.func, ast.Name) and last.value.func.id == "main"):
        raise AssertionError("expected the probe's final main() call")
    module = types.ModuleType(name)
    module.__file__ = str(path)
    with mock.patch.object(sys, "argv", [str(path)]), mock.patch.object(
            os.path, "expanduser", return_value="/unused-probe-home"):
        exec(compile(tree, str(path), "exec"), module.__dict__)
    return module


class ProbeHelpers(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.probes = [load_probe(name) for name in ("models", "resume_matrix")]

    def test_digest_missing_and_synthetic_content(self):
        with tempfile.TemporaryDirectory() as folder:
            source = Path(folder) / "fixture"
            for probe in self.probes:
                with self.subTest(probe=probe.__name__):
                    self.assertIsNone(probe.digest(str(source)))
                    source.write_bytes(b"synthetic test bytes")
                    self.assertEqual(probe.digest(str(source)),
                                     hashlib.sha256(b"synthetic test bytes").hexdigest()[:12])
                    source.unlink()

    def test_digest_does_not_treat_unreadable_as_missing(self):
        for probe in self.probes:
            with self.subTest(probe=probe.__name__), mock.patch.object(
                    probe, "open", side_effect=PermissionError("fixture denied"), create=True):
                with self.assertRaisesRegex(PermissionError, "fixture denied"):
                    probe.digest("/unreadable-fixture")

    def test_digest_reports_a_read_failure_after_open(self):
        for probe in self.probes:
            handle = mock.MagicMock()
            handle.__enter__.return_value = handle
            handle.read.side_effect = FileNotFoundError("file lost during read")
            with self.subTest(probe=probe.__name__), mock.patch.object(
                    probe, "open", return_value=handle, create=True):
                with self.assertRaisesRegex(FileNotFoundError, "file lost during read"):
                    probe.digest("/opened-fixture")

    def test_cleanup_reports_persistent_failure_and_retries(self):
        def denied(path, ignore_errors=False):
            if not ignore_errors:
                raise PermissionError("fixture cleanup denied")

        with tempfile.TemporaryDirectory() as folder:
            for probe in self.probes:
                with self.subTest(probe=probe.__name__), mock.patch.object(
                        probe.shutil, "rmtree", side_effect=denied) as remove, mock.patch.object(
                            probe.time, "sleep") as sleep:
                    with self.assertRaisesRegex(PermissionError, "fixture cleanup denied"):
                        probe.remove(folder)
                    self.assertEqual(remove.call_count, 10)
                    self.assertEqual(sleep.call_count, 9)

    def test_cleanup_retries_transient_failure_and_accepts_missing(self):
        original = shutil.rmtree
        for probe in self.probes:
            with self.subTest(probe=probe.__name__), tempfile.TemporaryDirectory() as parent:
                target = Path(parent) / "scratch"
                target.mkdir()
                tries = []

                def temporary_failure(path, *args, **kwargs):
                    tries.append(path)
                    if len(tries) < 3:
                        raise PermissionError("temporarily busy")
                    return original(path, *args, **kwargs)

                with mock.patch.object(probe.shutil, "rmtree", side_effect=temporary_failure), \
                        mock.patch.object(probe.time, "sleep"):
                    probe.remove(str(target))
                self.assertEqual(len(tries), 3)
                self.assertFalse(target.exists())
                probe.remove(str(target))

    def test_cleanup_does_not_confuse_missing_child_with_missing_root(self):
        original = shutil.rmtree
        for probe in self.probes:
            with self.subTest(probe=probe.__name__), tempfile.TemporaryDirectory() as parent:
                target = Path(parent) / "scratch"
                target.mkdir()
                tries = []

                def child_disappeared(path, *args, **kwargs):
                    tries.append(path)
                    if len(tries) == 1:
                        raise FileNotFoundError("a child vanished")
                    return original(path, *args, **kwargs)

                with mock.patch.object(probe.shutil, "rmtree", side_effect=child_disappeared), \
                        mock.patch.object(probe.time, "sleep"):
                    probe.remove(str(target))
                self.assertEqual(len(tries), 2)
                self.assertFalse(target.exists())

    def test_copy_synthetic_login_and_report_errors(self):
        for probe in self.probes:
            with self.subTest(probe=probe.__name__), tempfile.TemporaryDirectory() as parent:
                source = Path(parent) / "fixture"
                target = Path(parent) / "private" / "fixture"
                source.write_bytes(b"synthetic test bytes")
                probe.copy_secret(str(source), str(target))
                self.assertEqual(target.read_bytes(), b"synthetic test bytes")
                if os.name != "nt":
                    self.assertEqual(stat.S_IMODE(target.stat().st_mode), 0o600)
                with mock.patch.object(probe.shutil, "copyfile", side_effect=PermissionError("copy denied")):
                    with self.assertRaisesRegex(PermissionError, "copy denied"):
                        probe.copy_secret(str(source), str(target))

    def test_environment_filter_is_case_insensitive(self):
        source = {"CoNsEnSfLoW_TOKEN": "fixture", "OpenAI_API_KEY": "fixture",
                  "Keep_Me": "yes"}
        for probe in self.probes:
            with self.subTest(probe=probe.__name__), mock.patch.dict(os.environ, source, clear=True):
                env = probe.clean_env()
                self.assertEqual({key.upper() for key in env}, {"KEEP_ME"})

    def test_pi_optional_files_do_not_hide_inaccessible_source(self):
        original_stat = os.stat
        for probe in self.probes:
            with self.subTest(probe=probe.__name__), tempfile.TemporaryDirectory() as folder:
                owner = Path(folder) / "fixture-home"
                source = owner / ".pi" / "agent"

                def denied(path, *args, **kwargs):
                    if isinstance(path, (str, bytes, os.PathLike)) and os.fsdecode(path).startswith(str(source)):
                        raise PermissionError("fixture source denied")
                    return original_stat(path, *args, **kwargs)

                with mock.patch.object(probe, "HOME", str(owner)), mock.patch.object(
                        os, "stat", side_effect=denied), mock.patch.object(
                            probe, "exe", return_value="stand-in", create=True), mock.patch.object(
                                probe, "run", return_value={"code": 0, "out": "", "err": "", "seconds": 0},
                                create=True), mock.patch.object(probe, "Lines", create=True) as lines, \
                        mock.patch.object(probe, "stop", create=True):
                    lines.return_value.answer.return_value = None
                    with self.assertRaisesRegex(PermissionError, "fixture source denied"):
                        if probe.__name__ == "models":
                            probe.pi(str(Path(folder) / "scratch"))
                        else:
                            next(probe.pi(str(Path(folder) / "scratch"), "a", "b"))

    def test_pi_copies_present_synthetic_files_and_skips_missing(self):
        for probe in self.probes:
            for signed_out in ([False, True] if probe.__name__ == "models" else [False]):
                with self.subTest(probe=probe.__name__, signed_out=signed_out), \
                        tempfile.TemporaryDirectory() as folder:
                    owner = Path(folder) / "fixture-home"
                    source = owner / ".pi" / "agent"
                    source.mkdir(parents=True)
                    (source / "auth.json").write_bytes(b"synthetic login")
                    (source / "settings.json").write_bytes(b"synthetic settings")
                    scratch = Path(folder) / "scratch"
                    with mock.patch.object(probe, "HOME", str(owner)), mock.patch.object(
                            probe, "SIGNED_OUT", signed_out, create=True), mock.patch.object(
                                probe, "exe", return_value="stand-in", create=True), mock.patch.object(
                                    probe, "run", return_value={"code": 0, "out": "", "err": "", "seconds": 0},
                                    create=True), mock.patch.object(probe, "Lines", create=True) as lines, \
                            mock.patch.object(probe, "stop", create=True):
                        lines.return_value.answer.return_value = {"success": True, "data": {"models": []}}
                        if probe.__name__ == "models":
                            probe.pi(str(scratch))
                        else:
                            next(probe.pi(str(scratch), "a", "b"))
                    copied = scratch / "pi-agent"
                    self.assertEqual((copied / "settings.json").read_bytes(), b"synthetic settings")
                    self.assertFalse((copied / "models.json").exists())
                    if signed_out:
                        self.assertFalse((copied / "auth.json").exists())
                    else:
                        self.assertEqual((copied / "auth.json").read_bytes(), b"synthetic login")

    def test_isolated_direct_and_stdin_invocations_need_no_local_imports(self):
        for name in ("models", "resume_matrix"):
            for streamed in (False, True):
                with self.subTest(probe=name, streamed=streamed), tempfile.TemporaryDirectory() as folder:
                    path = HERE / (name + ".py")
                    env = {key: value for key, value in os.environ.items()
                           if not key.upper().startswith(("CONSENSFLOW_", "CLAUDE_CODE_", "ANTHROPIC_",
                                                          "OPENAI_", "CODEX_", "PI_"))}
                    env.update(HOME=folder, USERPROFILE=folder)
                    ran = subprocess.run([sys.executable, "-I", "-" if streamed else str(path)],
                                         input=path.read_bytes() if streamed else None,
                                         cwd=folder, env=env, capture_output=True)
                    self.assertNotEqual(ran.returncode, 0)
                    self.assertIn(b"python3 -I tools/probes/chief-harness/", ran.stderr)

    def test_stand_in_codex_runs_copy_check_and_cleanup_end_to_end(self):
        for probe in self.probes:
            with self.subTest(probe=probe.__name__), tempfile.TemporaryDirectory() as folder:
                fixture_home = Path(folder) / "fixture-home"
                login = fixture_home / ".codex" / "auth.json"
                login.parent.mkdir(parents=True)
                login.write_bytes(b"synthetic login")
                scratch = Path(folder) / "scratch"
                scratch.mkdir()
                calls = []

                def verify_copy(env):
                    self.assertEqual(Path(env["CODEX_HOME"], "auth.json").read_bytes(), b"synthetic login")

                class StandInLines:
                    def __init__(self, argv, env, cwd):
                        verify_copy(env)
                        self.proc = object()
                        self.replies = iter([{"id": 1}, {"id": 2, "result": {"data": []}}])

                    def send(self, message):
                        calls.append(message)

                    def answer(self, wanted):
                        answer = next(self.replies)
                        if not wanted(answer):
                            raise AssertionError("the stand-in sent the wrong response")
                        return answer

                def stand_in_run(argv, env, cwd, stdin=None):
                    verify_copy(env)
                    calls.append(argv)
                    return {"code": 0, "seconds": 0, "out": '{"type":"thread.started","thread_id":"fixture"}\n',
                            "err": ""}

                output = io.StringIO()
                with ExitStack() as stack:
                    stack.enter_context(mock.patch.object(probe, "HOME", str(fixture_home)))
                    stack.enter_context(mock.patch.object(probe, "HARNESS", "codex"))
                    stack.enter_context(mock.patch.object(probe, "SIGNED_OUT", False, create=True))
                    stack.enter_context(mock.patch.object(probe.tempfile, "mkdtemp", return_value=str(scratch)))
                    stack.enter_context(mock.patch.object(probe.shutil, "which", return_value="stand-in"))
                    stack.enter_context(mock.patch.object(probe, "Lines", StandInLines, create=True))
                    stack.enter_context(mock.patch.object(probe, "stop", create=True))
                    stack.enter_context(mock.patch.object(probe, "exe", return_value="stand-in", create=True))
                    stack.enter_context(mock.patch.object(probe, "run", side_effect=stand_in_run, create=True))
                    stack.enter_context(mock.patch.object(sys, "argv", [probe.__file__, "codex"]))
                    stack.enter_context(redirect_stdout(output))
                    probe.main()
                self.assertFalse(scratch.exists())
                self.assertIn("throwaway removed: True", output.getvalue())
                self.assertIn("owner's logins unchanged: True", output.getvalue())
                self.assertEqual(len(calls), 3 if probe.__name__ == "models" else 5)
                self.assertEqual(login.read_bytes(), b"synthetic login")


if __name__ == "__main__":
    unittest.main()
