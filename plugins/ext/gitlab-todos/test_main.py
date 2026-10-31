"""Parse tests on sample.json, a hand-written GitLab API response. Run: python3 -m unittest"""

import json
import os
import unittest

import main
from dashboard_plugin import PluginError

HERE = os.path.dirname(os.path.abspath(__file__))


def sample():
    with open(os.path.join(HERE, "sample.json"), encoding="utf-8") as f:
        return json.load(f)["/api/v4/todos"]


class SampleTest(unittest.TestCase):
    def test_todos_from_the_sample(self):
        v = main.view(sample(), 20, main.SAMPLE_NOW)
        self.assertEqual(v["summary"], "4 to-dos · 2 need you")
        review, failed, mention, assigned = v["root"]["items"]
        self.assertEqual(review["status"], "warn")
        self.assertEqual(review["subtitle"], "acme/web!47 · @sam asked for your review · 25m ago")
        self.assertEqual(review["tags"], ["MR"])
        self.assertEqual(failed["status"], "error")
        # A failed pipeline is not "by" anyone, and opens the pipeline.
        self.assertEqual(failed["subtitle"], "acme/api!118 · pipeline failed · 2h ago")
        self.assertEqual(failed["action"]["url"], "https://gitlab.com/acme/api/-/pipelines/55120")
        self.assertNotIn("status", mention)
        # No references on the target: built from the project and iid.
        self.assertTrue(assigned["subtitle"].startswith("acme/infra#12 · @sam assigned you"))

    def test_unknown_actions_and_bad_items(self):
        todos = [
            {"action_name": "some_new_action", "target_url": "https://gitlab.com/x", "target": {"title": "T"}},
            {"action_name": "assigned", "target": {"title": "no link"}},
            {"target_url": "https://gitlab.com/y", "target": {}},
        ]
        v = main.view(todos, 20, main.SAMPLE_NOW)
        self.assertEqual(len(v["root"]["items"]), 1)
        self.assertEqual(v["root"]["items"][0]["subtitle"], "some new action")
        self.assertEqual(v["summary"], "1 to-do")

    def test_empty_and_errors(self):
        v = main.view([], 20, main.SAMPLE_NOW)
        self.assertEqual(v["summary"], "0 to-dos")
        self.assertEqual(v["root"]["empty_text"], "Nothing to do")
        with self.assertRaises(PluginError) as e:
            main.view({"message": "401 Unauthorized"}, 20, main.SAMPLE_NOW)
        self.assertEqual(e.exception.code, -32001)


if __name__ == "__main__":
    unittest.main()
