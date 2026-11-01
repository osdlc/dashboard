"""Parse tests on sample.json, hand-written Azure DevOps API responses. Run: python3 -m unittest"""

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
BASE = "https://dev.azure.com/acme/Web"


def sample():
    with open(os.path.join(HERE, "sample.json"), encoding="utf-8") as f:
        return json.load(f)


def calls(replies):
    """A `call` that answers from `replies` and records what was asked."""
    asked = []

    def call(path, params, body):
        asked.append((path, params, body))
        return replies[path]

    return call, asked


class SampleTest(unittest.TestCase):
    def setUp(self):
        main.config.update(org="acme", project="Web", work_items=main.DEFAULT_WIQL, pull_requests=True, limit=10)

    def test_view_from_the_sample(self):
        call, asked = calls(sample())
        v = main.view(main.fetch(call), BASE, 10, main.SAMPLE_NOW)
        self.assertEqual(v["summary"], "3 work items · 3 pull requests")
        # The WIQL query goes in the body; the fields call asks for the ids it found.
        self.assertEqual(asked[0][2], {"query": main.DEFAULT_WIQL})
        self.assertEqual(asked[1][1]["ids"], "4127,4090,3988")

        _, items, _, prs = v["root"]["children"]
        bug, task, story = items["items"]
        # In the query's order, not the order the fields call sent.
        self.assertEqual(bug["subtitle"], "Bug 4127 · Active · updated 1h ago")
        self.assertEqual(bug["status"], "error")
        self.assertEqual(bug["tags"], ["checkout", "customer-reported"])
        self.assertEqual(bug["action"]["url"], f"{BASE}/_workitems/edit/4127")
        self.assertEqual(task["status"], "warn")
        self.assertNotIn("status", story)

        approved, conflict, draft = prs["items"]
        self.assertEqual(approved["status"], "ok")
        self.assertEqual(approved["subtitle"], "storefront !812 · fix/gift-card-total · by Dana Ruiz · 3h ago")
        self.assertEqual(approved["action"]["url"], f"{BASE}/_git/storefront/pullrequest/812")
        self.assertEqual(conflict["status"], "error")
        self.assertEqual(conflict["tags"], ["conflicts", "waiting for author"])
        self.assertEqual(draft["tags"], ["draft"])
        self.assertNotIn("status", draft)

    def test_no_work_items_found_skips_the_fields_call(self):
        call, asked = calls({"_apis/wit/wiql": {"workItems": []}, "_apis/git/pullrequests": {"value": []}})
        v = main.view(main.fetch(call), BASE, 10, main.SAMPLE_NOW)
        self.assertEqual([a[0] for a in asked], ["_apis/wit/wiql", "_apis/git/pullrequests"])
        self.assertEqual(v["summary"], "0 work items · 0 pull requests")

    def test_pull_requests_only(self):
        main.config.update(work_items=None, reviewer_id="abc")
        call, asked = calls(sample())
        v = main.view(main.fetch(call), BASE, 10, main.SAMPLE_NOW)
        self.assertEqual(len(asked), 1)
        self.assertEqual(asked[0][1]["searchCriteria.reviewerId"], "abc")
        self.assertEqual(v["summary"], "3 pull requests")
        main.config.update(reviewer_id=None)

    def test_a_sign_in_page_is_an_upstream_error(self):
        with self.assertRaises(PluginError) as e:
            main.view({"pull_requests": {"message": "TF400813"}}, BASE, 10, main.SAMPLE_NOW)
        self.assertEqual(e.exception.code, -32001)


class TokenTest(unittest.TestCase):
    def test_token_command_runs_once_per_render(self):
        replies = sample()
        main.init({"org": "acme", "project": "Web", "reviewer_id": None, "token_command": ["get-token"]})

        def urlopen(req, timeout):
            path = urllib.parse.unquote(urllib.parse.urlparse(req.full_url).path).removeprefix("/acme/Web/")
            self.assertEqual(req.get_header("Authorization"), "Basic OnNlY3JldA==")
            return io.BytesIO(json.dumps(replies[path]).encode())

        done = subprocess.CompletedProcess([], 0, stdout="secret\n", stderr="")
        with mock.patch.object(main.subprocess, "run", return_value=done) as run:
            with mock.patch.object(main.urllib.request, "urlopen", urlopen):
                main.render()
        self.assertEqual(run.call_count, 1)


if __name__ == "__main__":
    unittest.main()
