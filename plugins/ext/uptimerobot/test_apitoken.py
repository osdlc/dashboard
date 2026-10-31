"""Tests for apitoken.py. Run with `python3 -m unittest`. The same in every package that
carries apitoken.py."""

import os
import subprocess
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))

# Reads one line from stdin into the secret, then prints the caller's next stdin line.
# The plugin's stdin is the host's JSON-RPC channel, so the command must not see it.
CHILD = """
import sys
import apitoken
print(apitoken.secret({"token_command": ["sh", "-c", "read x; echo tok$x"]}, "token_env", "token_command"))
print(sys.stdin.readline().strip())
"""


class SecretTest(unittest.TestCase):
    def test_command_does_not_read_the_plugin_stdin(self):
        done = subprocess.run(
            [sys.executable, "-B", "-c", CHILD],
            input="request\n",
            capture_output=True,
            text=True,
            cwd=HERE,
            timeout=30,
        )
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(done.stdout.split(), ["tok", "request"])


if __name__ == "__main__":
    unittest.main()
