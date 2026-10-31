"""Parses sample.json, a hand-written Buildkite builds response, and checks the view.

    python3 -m unittest
"""

import json
import os
import unittest

from view import SAMPLE_NOW, state, view

with open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "sample.json"), encoding="utf-8") as f:
    SAMPLE = json.load(f)


class ViewTest(unittest.TestCase):
    def test_maps_buildkite_states(self):
        self.assertEqual([state(b) for b in SAMPLE], ["running", "failed", "passed", "other", "passed"])

    def test_counts_sorts_and_links_builds(self):
        v = view(SAMPLE, SAMPLE_NOW)
        self.assertEqual(v["status"], "error")
        self.assertEqual(v["summary"], "failing: web")
        stats, lst = v["root"]["children"]
        self.assertEqual([s["value"] for s in stats["children"]], ["2", "1", "1"])
        # Newest first: the blocked deploy (10:52) comes before the 10:48 api build.
        self.assertEqual(
            [i["subtitle"] for i in lst["items"]],
            [
                "api · #1482 · 2m ago",
                "web · #905 · 25m ago · took 4m 12s",
                "deploy · #312 · 1h ago",
                "api · #1481 · 1h ago · took 3m 2s",
                "web · #904 · 9h ago · took 11m 40s",
            ],
        )
        self.assertEqual(lst["items"][1]["title"], "Move settings page to the new form components")
        self.assertEqual(lst["items"][2]["tags"], ["main", "blocked"])
        self.assertEqual(lst["items"][1]["action"], {"type": "open_url", "url": "https://buildkite.com/acme/web/builds/905"})

    def test_no_builds_is_unknown_not_ok(self):
        v = view([], SAMPLE_NOW)
        self.assertEqual(v["status"], "unknown")
        self.assertEqual(v["summary"], "0 passed · 0 running")


if __name__ == "__main__":
    unittest.main()
