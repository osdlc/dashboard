"""Parse tests on sample.json, hand-written CircleCI API v2 responses. Run: python3 -m unittest"""

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
APP = "https://app.circleci.com"


def sample():
    with open(os.path.join(HERE, "sample.json"), encoding="utf-8") as f:
        return json.load(f)


class SampleTest(unittest.TestCase):
    def data(self, limit=3, projects=("gh/acme/api", "gh/acme/web")):
        replies = sample()
        return main.fetch(lambda path, _params: replies[path], list(projects), None, limit)

    def test_pipelines_from_the_sample(self):
        v = main.view(self.data(), APP, 3, main.SAMPLE_NOW)
        self.assertEqual(v["status"], "error")
        self.assertEqual(v["summary"], "6 pipelines · api failing")
        failed, passed, _, running, held, _ = v["root"]["items"]
        self.assertEqual(failed["title"], "Retry the billing webhook on 502")
        self.assertEqual(failed["subtitle"], "api #1432 · main · build-and-test failed · 1h ago")
        self.assertEqual(failed["status"], "error")
        # The link goes to the worst workflow.
        self.assertEqual(
            failed["action"]["url"],
            f"{APP}/pipelines/github/acme/api/1432/workflows/fda08377-fe7e-46b1-8992-3a7aaecac9c3",
        )
        self.assertEqual(passed["status"], "ok")
        self.assertIn("passed", passed["subtitle"])
        self.assertNotIn("status", running)
        self.assertEqual(held["status"], "warn")
        self.assertEqual(held["tags"], ["@kim"])

    def test_only_the_newest_pipeline_sets_the_tile(self):
        v = main.view(self.data(projects=("gh/acme/web",)), APP, 3, main.SAMPLE_NOW)
        self.assertEqual(v["status"], "ok")
        self.assertEqual(v["summary"], "3 pipelines")
        v = main.view(self.data(limit=1), APP, 1, main.SAMPLE_NOW)
        self.assertEqual(len(v["root"]["items"]), 2)

    def test_errored_config_and_no_workflows(self):
        p = {"id": "x", "number": 9, "state": "errored", "vcs": {"branch": "dev"}}
        item = main.pipeline("bb/acme/web", p, [], APP, main.SAMPLE_NOW)
        self.assertEqual(item["title"], "Pipeline #9")
        self.assertEqual(item["subtitle"], "web #9 · dev · config error")
        self.assertEqual(item["status"], "error")
        self.assertEqual(item["action"]["url"], f"{APP}/pipelines/bitbucket/acme/web/9")

    def test_bad_reply(self):
        with self.assertRaises(PluginError) as e:
            main.items({"message": "Project not found"})
        self.assertEqual(e.exception.code, -32001)


class TokenTest(unittest.TestCase):
    def test_token_command_runs_once_per_render(self):
        replies = sample()
        main.init({"projects": ["gh/acme/api", "gh/acme/web"], "token_command": ["get-token"]})

        def urlopen(req, timeout):
            path = urllib.parse.unquote(urllib.parse.urlparse(req.full_url).path)
            self.assertEqual(req.get_header("Circle-token"), "secret")
            return io.BytesIO(json.dumps(replies[path]).encode())

        done = subprocess.CompletedProcess([], 0, stdout="secret\n", stderr="")
        with mock.patch.object(main.subprocess, "run", return_value=done) as run:
            with mock.patch.object(main.urllib.request, "urlopen", urlopen):
                main.render()
        self.assertEqual(run.call_count, 1)


if __name__ == "__main__":
    unittest.main()
