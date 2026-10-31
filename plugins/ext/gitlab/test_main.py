"""Parse tests on sample.json, a hand-written GitLab API response. Run: python3 -m unittest"""

import io
import json
import os
import subprocess
import unittest
import urllib.parse
from unittest import mock

import main
from dashboard_plugin import PluginError

HERE = os.path.dirname(os.path.abspath(__file__))


def sample():
    with open(os.path.join(HERE, "sample.json"), encoding="utf-8") as f:
        return json.load(f)


class SampleTest(unittest.TestCase):
    def test_sections_from_the_sample(self):
        s = sample()
        replies = [s["/api/v4/merge_requests"], s["/api/v4/issues"]]
        v = main.view(main.DEFAULT_SECTIONS, replies, 10, main.SAMPLE_NOW)
        self.assertEqual(v["summary"], "3 my merge requests · 2 assigned issues")
        heading, mrs, _, issues = v["root"]["children"]
        self.assertEqual(heading["text"], "**My merge requests**")

        ready, conflict, draft = mrs["items"]
        self.assertEqual(ready["status"], "ok")
        self.assertEqual(ready["subtitle"], "acme/web!42 · by @dana · updated 1h ago")
        self.assertEqual(ready["action"]["url"], "https://gitlab.com/acme/web/-/merge_requests/42")
        self.assertEqual(conflict["status"], "error")
        self.assertIn("conflicts", conflict["tags"])
        self.assertNotIn("status", draft)
        self.assertEqual(draft["tags"], ["draft"])

        overdue, plain = issues["items"]
        self.assertEqual(overdue["status"], "error")
        self.assertEqual(overdue["tags"], ["due 2026-10-25", "bug", "priority::high"])
        self.assertNotIn("status", plain)

    def test_limit_and_bad_items(self):
        reply = sample()["/api/v4/issues"] + [{"iid": 1}, "junk"]
        section = [{"label": "Issues", "type": "issues", "params": {}}]
        v = main.view(section, [reply], 1, main.SAMPLE_NOW)
        self.assertEqual(len(v["root"]["children"][1]["items"]), 1)
        v = main.view(section, [reply], 10, main.SAMPLE_NOW)
        self.assertEqual(len(v["root"]["children"][1]["items"]), 2)

    def test_not_a_list_is_an_upstream_error(self):
        section = [{"label": "Issues", "type": "issues", "params": {}}]
        with self.assertRaises(PluginError) as e:
            main.view(section, [{"message": "401 Unauthorized"}], 10, main.SAMPLE_NOW)
        self.assertEqual(e.exception.code, -32001)

    def test_times(self):
        self.assertEqual(main.parse_time("2026-10-27T12:00:00Z"), main.SAMPLE_NOW)
        self.assertEqual(main.parse_time("2026-10-27T14:00:00.123+02:00"), main.SAMPLE_NOW)
        self.assertIsNone(main.parse_time("yesterday"))
        self.assertEqual(main.ago("2026-10-24T12:00:00Z", main.SAMPLE_NOW), "3d ago")


class TokenTest(unittest.TestCase):
    def test_token_command_runs_once_per_render(self):
        replies = sample()
        main.init({"token_command": ["get-token"]})

        def urlopen(req, timeout):
            path = urllib.parse.unquote(urllib.parse.urlparse(req.full_url).path)
            self.assertEqual(req.get_header("Private-token"), "secret")
            return io.BytesIO(json.dumps(replies[path]).encode())

        done = subprocess.CompletedProcess([], 0, stdout="secret\n", stderr="")
        with mock.patch.object(main.subprocess, "run", return_value=done) as run:
            with mock.patch.object(main.urllib.request, "urlopen", urlopen):
                main.render()
        self.assertEqual(run.call_count, 1)


if __name__ == "__main__":
    unittest.main()
