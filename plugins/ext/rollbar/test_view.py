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
        v = view(sample(), "acme", "web", "warning", 10, SAMPLE_NOW)
        self.assertEqual(v["status"], "error")
        self.assertEqual(v["summary"], "4 active items · 1 critical")
        items = v["root"]["items"]
        # Most recent first; the info item is below the warning floor.
        self.assertEqual(items[0]["title"], "OperationalError: could not connect to server: Connection refused")
        self.assertEqual(items[0]["subtitle"], "#1192 · 212 occurrences · production · 1m ago")
        self.assertEqual(items[0]["status"], "error")
        self.assertEqual(items[0]["tags"], ["critical"])
        self.assertEqual(items[0]["action"], {"type": "open_url", "url": "https://rollbar.com/acme/web/items/1192/"})
        self.assertEqual(items[1]["subtitle"], "#1187 · 4.8k occurrences · production · 4m ago")
        self.assertEqual(items[1]["status"], "warn")
        # A numeric level: 30 is warning.
        self.assertEqual(items[3]["tags"], ["warning"])
        self.assertNotIn("status", items[3])
        self.assertEqual(items[3]["subtitle"], "#1151 · 1 occurrence · production · 19h ago")

    def test_level_and_limit(self):
        v = view(sample(), "acme", "web", "critical", 10, SAMPLE_NOW)
        self.assertEqual(len(v["root"]["items"]), 1)
        v = view(sample(), "acme", "web", "debug", 2, SAMPLE_NOW)
        self.assertEqual(len(v["root"]["items"]), 2)
        self.assertEqual(v["summary"], "5 active items · 1 critical")

    def test_api_error(self):
        with self.assertRaises(PluginError):
            view({"err": 1, "message": "invalid access token"}, "acme", "web", "warning", 10, SAMPLE_NOW)
        with self.assertRaises(PluginError):
            view({"err": 0}, "acme", "web", "warning", 10, SAMPLE_NOW)


if __name__ == "__main__":
    unittest.main()
