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
        v = view(sample(), "acme", 10, SAMPLE_NOW)
        self.assertEqual(v["status"], "error")
        self.assertEqual(v["summary"], "4 tickets · 1 urgent")
        items = v["root"]["items"]
        # Most recently updated first.
        self.assertEqual([i["subtitle"].split(" · ")[0] for i in items], ["#48233", "#48213", "#48190", "#48102"])
        self.assertEqual(items[0]["subtitle"], "#48233 · new · updated 5m ago")
        self.assertEqual(items[0]["tags"], ["login", "email", "account"])
        items = items[1:]
        self.assertEqual(items[0]["title"], "Checkout fails with error 502 for all EU customers")
        self.assertEqual(items[0]["subtitle"], "#48213 · open · urgent · updated 19m ago")
        self.assertEqual(items[0]["status"], "error")
        self.assertEqual(items[0]["action"], {"type": "open_url", "url": "https://acme.zendesk.com/agent/tickets/48213"})
        self.assertEqual(items[1]["status"], "warn")
        # Normal priority is not shown; no priority at all neither.
        self.assertEqual(items[2]["subtitle"], "#48102 · hold · updated 1d ago")
        self.assertNotIn("status", items[2])

    def test_limit_keeps_the_count(self):
        v = view(sample(), "acme", 1, SAMPLE_NOW)
        self.assertEqual(len(v["root"]["items"]), 1)
        self.assertEqual(v["summary"], "4 tickets · 1 urgent")

    def test_api_error(self):
        with self.assertRaises(PluginError) as e:
            view({"error": "Couldn't authenticate you"}, "acme", 10, SAMPLE_NOW)
        self.assertIn("Couldn't authenticate you", str(e.exception))


if __name__ == "__main__":
    unittest.main()
