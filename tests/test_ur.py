import json
import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import time
import unittest

UR = Path(__file__).resolve().parent.parent
CONF = UR / "tmux.conf"
HOOK = UR / "scripts" / "hooks" / "attention"


def wait_until(check, message, timeout=10):
    deadline = time.monotonic() + timeout
    while not check():
        if time.monotonic() > deadline:
            raise AssertionError(message())
        time.sleep(0.03)


class Tmux:
    """A tmux server on its own socket, loading ur's tmux.conf, with a temporary home."""

    def __init__(self, env=None):
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        self.socket = self.root / "tmux.sock"
        self.env = {key: value for key, value in os.environ.items() if key != "TMUX"}
        self.env.update({"HOME": str(self.root), **(env or {})})
        self.call("-f", str(CONF), "new-session", "-d", "-s", "test", "-x", "80", "-y", "24",
                  "/bin/sh")
        self.client = None

    def run(self, *args, env=None):
        return subprocess.run(["tmux", "-S", str(self.socket), *args], env=env or self.env,
                              text=True, capture_output=True)

    def call(self, *args):
        output = self.run(*args)
        assert output.returncode == 0, f"tmux {args}: {output.stderr}"
        return output.stdout

    def attach(self):
        self.client = subprocess.Popen(
            ["tmux", "-S", str(self.socket), "-C", "attach-session", "-t", "test"],
            env=self.env, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL)
        wait_until(lambda: self.call("list-clients").strip(), lambda: "tmux client did not attach")

    def attention(self, pane="%0"):
        return self.call("display-message", "-p", "-t", pane, "#{@agent-attention}").strip()

    def fire(self, agent, state, payload="{}", pane="%0"):
        env = {**os.environ, "TMUX": f"{self.socket},0,0", "TMUX_PANE": pane}
        subprocess.run([str(HOOK), agent, state], input=payload, text=True, env=env, check=True)

    def close(self):
        if self.client:
            self.client.kill()
            self.client.wait()
            self.client.stdin.close()
        self.run("kill-server")
        self.directory.cleanup()


class TmuxTest(unittest.TestCase):
    def tmux(self, env=None):
        test = Tmux(env)
        self.addCleanup(test.close)
        return test


class PaneBorders(TmuxTest):
    def test_borders_show_titles_and_ox_tui_titles_mark_the_window(self):
        test = self.tmux()
        for option, value in [("pane-border-status", "top"), ("pane-border-lines", "single")]:
            self.assertEqual(test.call("show-options", "-gwv", option).strip(), value)
        for title, mark in [("ox-tui: needs permission", "?"), ("ox-tui: finished", "✓"),
                            ("ox-tui: turn error", "✗"), ("ox-tui: ready", None)]:
            test.call("select-pane", "-t", "%0", "-T", title)
            self.assertEqual(
                test.call("display-message", "-p", "-t", "%0", "#{E:pane-border-format}").strip(),
                f"0 {title}")
            status = test.call("display-message", "-p", "-t", "%0", "#{E:window-status-format}")
            for other in "?✓✗":
                self.assertEqual(other in status, other == mark, (title, status))


class AgentHooks(TmuxTest):
    def test_permission_stays_until_resolved_and_results_clear_on_focus(self):
        test = self.tmux()
        test.attach()
        test.call("split-window", "-h", "-t", "%0", "/bin/sh")
        test.fire("claude", "permission")
        self.assertEqual(test.attention(), "permission")
        test.call("select-pane", "-t", "%0")
        self.assertEqual(test.attention(), "permission")
        test.fire("claude", "clear", '{"agent_id":"background"}')
        self.assertEqual(test.attention(), "permission")
        test.fire("claude", "clear")
        self.assertEqual(test.attention(), "")
        test.fire("claude", "finished")
        self.assertEqual(test.attention(), "")
        test.call("select-pane", "-t", "%1")
        for result, mark in [("finished", "✓"), ("error", "✗")]:
            test.fire("claude", result)
            self.assertEqual(test.attention(), result)
            self.assertIn(mark, test.call("display-message", "-p", "-t", "%0",
                                          "#{E:window-status-format}"))
            test.call("select-pane", "-t", "%0")
            self.assertEqual(test.attention(), "")
            test.call("select-pane", "-t", "%1")
        # Codex requests are not matched against a Claude transcript.
        test.fire("codex", "permission",
                  '{"transcript_path":"/missing","tool_name":"Bash","tool_input":{}}')
        self.assertEqual(test.attention(), "permission")
        test.fire("codex", "clear")
        self.assertEqual(test.attention(), "")

    def test_claude_permission_results_clear_only_the_request_they_resolve(self):
        test = self.tmux()
        transcript = test.root / "transcript.jsonl"
        transcript.write_text("")
        event = json.dumps({"transcript_path": str(transcript), "tool_name": "Bash",
                            "tool_input": {"command": "test"}})

        def append(text):
            with transcript.open("a") as output:
                output.write(text)

        def tool_use(id):
            return json.dumps({"message": {"content": [
                {"type": "tool_use", "id": id, "name": "Bash", "input": {"command": "test"}}]}})

        def tool_result(id, error):
            return json.dumps({"message": {"content": [
                {"type": "tool_result", "tool_use_id": id, "is_error": error}]}})

        def fire(state):
            test.fire("claude", state, event)

        for id in ["reject", "escape", "approve"]:
            if id == "escape":
                fire("permission")
                time.sleep(0.25)
            append(tool_use(id) + "\n")
            if id != "escape":
                fire("permission")
            self.assertEqual(test.attention(), "permission")
            # An unrelated failure must not acknowledge the pending permission.
            append(tool_result("other", True) + "\n")
            time.sleep(0.25)
            self.assertEqual(test.attention(), "permission")
            result = tool_result(id, id != "approve")
            half = len(result) // 2
            append(result[:half])
            time.sleep(0.25)
            self.assertEqual(test.attention(), "permission")
            append(result[half:] + "\n")
            wait_until(lambda: test.attention() == "", lambda: "permission mark was not cleared",
                       timeout=5)
        append(tool_use("old") + "\n")
        fire("permission")
        append(tool_use("newer") + "\n")
        fire("permission")
        append(tool_result("old", True) + "\n")
        time.sleep(0.3)
        self.assertEqual(test.attention(), "permission")
        fire("error")
        append(tool_result("newer", True) + "\n")
        time.sleep(0.3)
        self.assertEqual(test.attention(), "error")
        fire("clear")


class Install(TmuxTest):
    def test_launchers_add_hooks_in_panes_and_preserve_user_configuration(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        root = Path(directory.name)
        config = root / "config with spaces"
        binaries = root / "bin"
        binaries.mkdir()
        personal = '{"theme":"dark","hooks":{"Stop":[]}}'
        agents = ["claude", "codex", "opencode"]
        for agent in agents:
            home = root / f".{agent}"
            home.mkdir()
            (home / "settings.json").write_text(personal)
            binary = binaries / agent
            binary.write_text(
                "#!/usr/bin/env python3\nimport json, os, sys\n"
                "print(json.dumps({'args': sys.argv[1:], 'home': os.environ['HOME'], "
                "'claude': os.getenv('CLAUDE_CONFIG_DIR'), 'codex': os.getenv('CODEX_HOME'), "
                "'opencode': os.getenv('OPENCODE_CONFIG_DIR'), "
                "'content': os.getenv('OPENCODE_CONFIG_CONTENT')}))\n")
            binary.chmod(0o755)
        for _ in range(2):
            subprocess.run([str(UR / "scripts" / "install")], check=True, capture_output=True,
                           env={**os.environ, "HOME": str(root), "XDG_CONFIG_HOME": str(config)})
        self.assertEqual(
            (root / ".local/bin/ur").read_text(),
            f"#!/bin/sh\nexec {shlex.quote(str(UR / 'scripts' / 'ur'))} \"$@\"\n")
        path = f"{binaries}:{os.environ['PATH']}"
        environment = {key: value for key, value in os.environ.items()
                       if key not in ("CLAUDE_CONFIG_DIR", "CODEX_HOME", "OPENCODE_CONFIG_DIR",
                                      "OPENCODE_CONFIG_CONTENT")}
        test = self.tmux({**environment, "HOME": str(root), "XDG_CONFIG_HOME": str(config),
                          "PATH": path, "SHELL": "/bin/sh"})
        test.call("set-option", "-g", "default-shell", "/bin/sh")
        test.call("new-window", "-n", "launchers")
        plugin = (UR / "scripts" / "hooks" / "opencode.js").as_uri()
        for agent in agents:
            settings = config / f"ur/hooks/{agent}.json"
            hooks = json.loads(settings.read_text())
            if agent == "opencode":
                self.assertEqual(hooks["plugin"], [plugin])
            else:
                self.assertEqual(hooks["hooks"]["Stop"][0]["hooks"][0]["command"],
                                 f"{HOOK} {agent} finished")
                self.assertIsInstance(hooks["hooks"]["PermissionRequest"], list)
            record = root / f"{agent}-launch.json"
            test.call("send-keys", "-t", "test:launchers", "-l",
                      f"{agent} 'a prompt with spaces' > {shlex.quote(str(record))}")
            test.call("send-keys", "-t", "test:launchers", "Enter")
            launch = {}

            def launched():
                try:
                    launch.update(json.loads(record.read_text()))
                    return True
                except (OSError, ValueError):
                    return False

            wait_until(launched,
                       lambda: test.call("capture-pane", "-p", "-t", "test:launchers"))
            self.assertEqual(launch["home"], str(root))
            self.assertIsNone(launch["claude"])
            self.assertIsNone(launch["codex"])
            self.assertIsNone(launch["opencode"])
            if agent == "claude":
                self.assertEqual(launch["args"],
                                 ["--settings", str(settings), "a prompt with spaces"])
            elif agent == "opencode":
                self.assertEqual(launch["args"], ["a prompt with spaces"])
                self.assertEqual(json.loads(launch["content"]), {"plugin": hooks["plugin"]})
            else:
                self.assertEqual(launch["args"][0], "-c")
                self.assertIn("hooks={SessionStart", launch["args"][1])
                self.assertIn("codex finished", launch["args"][1])
                self.assertEqual(launch["args"][2], "a prompt with spaces")
            outside = subprocess.run([agent, "a prompt with spaces"], check=True,
                                     capture_output=True, text=True,
                                     env={**environment, "PATH": path, "HOME": str(root)})
            outside = json.loads(outside.stdout)
            self.assertEqual(outside["args"], ["a prompt with spaces"])
            self.assertIsNone(outside["content"])
            custom = subprocess.run(
                [str(config / f"ur/bin/{agent}")], check=True, capture_output=True, text=True,
                env={**environment, "PATH": path, "CLAUDE_CONFIG_DIR": "/custom claude",
                     "CODEX_HOME": "/custom codex", "OPENCODE_CONFIG_DIR": "/custom opencode",
                     "OPENCODE_CONFIG_CONTENT": '{"theme":"dark","plugin":["personal"]}'})
            custom = json.loads(custom.stdout)
            self.assertEqual(custom["claude"], "/custom claude")
            self.assertEqual(custom["codex"], "/custom codex")
            self.assertEqual(custom["opencode"], "/custom opencode")
            if agent == "opencode":
                content = json.loads(custom["content"])
                self.assertEqual(content["theme"], "dark")
                self.assertEqual(content["plugin"], ["personal", plugin])
            self.assertEqual((root / f".{agent}/settings.json").read_text(), personal)


class Dev(unittest.TestCase):
    def test_dev_replaces_only_its_server_opens_workspaces_and_reloads_config_edits(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        home = Path(directory.name)
        (home / ".config/ur").mkdir(parents=True)
        (home / ".config/ur/tmux.conf").write_text("new-session -d -s ur -n workspace\n")
        config = home / "tmux.conf"
        source = CONF.read_text()
        config.write_text(f"{source}\nset -g @dev-test initial\n")
        env = {key: value for key, value in os.environ.items() if key != "TMUX"}
        env.update({"TMUX_TMPDIR": str(home), "HOME": str(home)})

        def tmux(socket, *args):
            return subprocess.run(["tmux", "-L", socket, *args], env=env, text=True,
                                  capture_output=True)

        self.addCleanup(tmux, "ur-dev", "kill-server")
        self.addCleanup(tmux, "ur", "kill-server")
        for socket in ["ur", "ur-dev"]:
            self.assertEqual(
                tmux(socket, "-f", "/dev/null", "new-session", "-d", "-s", socket,
                     "sleep 30").returncode, 0)
        self.assertEqual(tmux("ur-dev", "set", "-g", "@dev-test", "old").returncode, 0)
        dev = subprocess.Popen(
            ["script", "-q", "/dev/null", str(UR / "scripts" / "dev"), str(config)],
            env={**env, "SHELL": "/bin/sh", "TERM": "xterm-256color"}, stdin=subprocess.PIPE,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

        def wait_mark(expected):
            wait_until(lambda: tmux("ur-dev", "show", "-gv", "@dev-test").stdout.strip()
                       == expected, lambda: f"dev mark did not become {expected}")

        wait_mark("initial")
        self.assertEqual(tmux("ur-dev", "list-windows", "-a", "-F", "#S:#W").stdout,
                         "ur:workspace\n")
        self.assertEqual(tmux("ur", "list-sessions", "-F", "#S").stdout, "ur\n")
        config.write_text(f"{source}\nset -g @dev-test updated\n")
        wait_mark("updated")
        wait_until(lambda: tmux("ur-dev", "list-clients").stdout,
                   lambda: "dev client did not attach", timeout=5)
        self.assertEqual(tmux("ur-dev", "detach-client", "-s", "ur").returncode, 0)
        self.assertEqual(dev.wait(timeout=10), 0)
        dev.stdin.close()


class Restart(unittest.TestCase):
    def test_restart_replaces_the_server_only_from_outside_ur(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        home = Path(directory.name)
        env = {key: value for key, value in os.environ.items() if key != "TMUX"}
        env.update({"TMUX_TMPDIR": str(home), "HOME": str(home), "SHELL": "/bin/sh"})
        ur = str(UR / "scripts" / "ur")

        def tmux(*args):
            return subprocess.run(["tmux", "-L", "ur", *args], env=env, text=True,
                                  capture_output=True)

        self.addCleanup(tmux, "kill-server")
        self.assertEqual(tmux("-f", "/dev/null", "new-session", "-d", "-s", "ur",
                              "sleep 30").returncode, 0)
        self.assertEqual(tmux("set", "-g", "@restart-test", "old").returncode, 0)

        inside = {**env, "TMUX": f"{home}/tmux-{os.getuid()}/ur,1,0"}
        refused = subprocess.run([ur, "restart"], env=inside, text=True, capture_output=True)
        self.assertEqual(refused.returncode, 1, refused.stderr)
        self.assertEqual(tmux("show", "-gv", "@restart-test").stdout, "old\n")

        restart = subprocess.Popen(
            ["script", "-q", "/dev/null", ur, "restart"],
            env={**env, "TERM": "xterm-256color"}, stdin=subprocess.PIPE,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        wait_until(lambda: tmux("list-clients").stdout
                   and tmux("show", "-gv", "history-limit").stdout == "50000\n",
                   lambda: "restart did not attach to a server loading tmux.conf", timeout=5)
        self.assertEqual(tmux("show", "-gv", "@restart-test").stdout, "")
        self.assertEqual(tmux("list-sessions", "-F", "#S").stdout, "ur\n")
        self.assertEqual(tmux("detach-client", "-s", "ur").returncode, 0)
        self.assertEqual(restart.wait(timeout=10), 0)
        restart.stdin.close()


if __name__ == "__main__":
    unittest.main()
