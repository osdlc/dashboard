"""Parses sample.json, a hand-written Jenkins /api/json response, and checks the view.

    python3 -m unittest
"""

import json
import os
import unittest

from view import SAMPLE_NOW, state, view

with open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "sample.json"), encoding="utf-8") as f:
    JOBS = json.load(f)["jobs"]


class ViewTest(unittest.TestCase):
    def test_maps_job_colors(self):
        self.assertEqual([state(j) for j in JOBS], ["passing", "passing", "other", "other", "failing", "unstable"])

    def test_skips_folders_and_lists_failing_jobs_first(self):
        v = view(JOBS, SAMPLE_NOW)
        self.assertEqual(v["status"], "error")
        self.assertEqual(v["summary"], "failing: web-e2e")
        stats, table = v["root"]["children"]
        self.assertEqual([s["value"] for s in stats["children"]], ["2", "1", "1"])
        self.assertEqual(
            [[r["cells"][k] for k in ("job", "build", "result", "when")] + [r["status"]] for r in table["rows"]],
            [
                ["web-e2e", "#1544", "failure", "30m ago · 12m 12s", "error"],
                ["lint", "#931", "unstable", "3h ago · 41s", "warn"],
                ["api-tests", "#2210", "success", "1h ago · 4m 14s", "ok"],
                ["deploy-staging", "#388", "running", "2m ago", "warn"],
                ["legacy-reports", "", "disabled", "", "unknown"],
            ],
        )
        self.assertEqual(table["rows"][0]["action"], {"type": "open_url", "url": "https://ci.example.com/job/web-e2e/1544/"})
        self.assertEqual(table["rows"][4]["action"], {"type": "open_url", "url": "https://ci.example.com/job/legacy-reports/"})

    def test_all_green(self):
        v = view(JOBS[:2], SAMPLE_NOW)
        self.assertEqual(v["status"], "ok")
        self.assertEqual(v["summary"], "2 passing · 1 running")


if __name__ == "__main__":
    unittest.main()
