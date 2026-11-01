"""Parse tests: the sample reply (sample.json) through view(). Run with `python3 -m unittest`."""

import json
import os
import unittest

from dashboard_plugin import PluginError
from view import SAMPLE_NOW, view

HERE = os.path.dirname(os.path.abspath(__file__))


def sample():
    with open(os.path.join(HERE, "sample.json"), encoding="utf-8") as f:
        return json.load(f)


class ViewTest(unittest.TestCase):
    def test_sample(self):
        v = view(sample(), 20, SAMPLE_NOW)
        self.assertEqual(v["status"], "error")
        self.assertEqual(v["summary"], "4 monitors · 1 down · 1 seem down · 1 paused")
        checks = v["root"]["checks"]
        self.assertEqual([c["name"] for c in checks], ["API", "Mail server", "Website", "Old blog"])
        self.assertEqual(checks[0]["status"], "error")
        self.assertEqual(checks[0]["detail"], "down for 23m: Connection Timeout")
        self.assertEqual(checks[0]["uptime"], 98.712)
        # No response time while it is down.
        self.assertNotIn("latency_ms", checks[0])
        self.assertEqual(checks[1]["detail"], "seems down")
        self.assertEqual(checks[2]["detail"], "up for 3d")
        self.assertEqual(checks[2]["latency_ms"], 212)
        self.assertEqual(checks[2]["url"], "https://dashboard.uptimerobot.com/monitors/793154270")
        self.assertEqual(checks[3]["status"], "unknown")
        self.assertEqual(checks[3]["detail"], "paused")
        self.assertNotIn("uptime", checks[3])

    def test_all_up(self):
        reply = sample()
        reply["monitors"] = [m for m in reply["monitors"] if m["status"] in (0, 2)]
        v = view(reply, 20, SAMPLE_NOW)
        self.assertEqual(v["status"], "ok")
        self.assertEqual(v["summary"], "1 up · 1 paused")

    def test_failed_request(self):
        with self.assertRaises(PluginError) as e:
            view({"stat": "fail", "error": {"type": "invalid_parameter", "message": "api_key is invalid."}}, 20, SAMPLE_NOW)
        self.assertIn("api_key is invalid", str(e.exception))


if __name__ == "__main__":
    unittest.main()
