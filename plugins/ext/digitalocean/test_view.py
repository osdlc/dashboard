"""Parse tests: the sample reply (sample.json) through view(). Run with `python3 -m unittest`."""

import json
import os
import unittest

from dashboard_plugin import PluginError
from view import view

HERE = os.path.dirname(os.path.abspath(__file__))


def sample():
    with open(os.path.join(HERE, "sample.json"), encoding="utf-8") as f:
        return json.load(f)


class ViewTest(unittest.TestCase):
    def test_sample(self):
        v = view(sample(), 20)
        self.assertEqual(v["status"], "warn")
        self.assertEqual(v["summary"], "5 droplets · 3 active · 1 off · $102/mo")
        items = v["root"]["items"]
        self.assertEqual(items[0]["title"], "web-1")
        # The public address, not the private one.
        self.assertEqual(items[0]["subtitle"], "s-1vcpu-2gb · fra1 · 203.0.113.21")
        self.assertEqual(items[0]["status"], "ok")
        self.assertEqual(items[0]["tags"], ["web", "prod"])
        self.assertEqual(items[0]["action"], {"type": "open_url", "url": "https://cloud.digitalocean.com/droplets/512340001"})
        self.assertEqual(items[3]["subtitle"], "off · s-1vcpu-1gb · ams3 · 198.51.100.7")
        self.assertEqual(items[3]["status"], "unknown")
        self.assertEqual(items[4]["subtitle"], "new · s-2vcpu-4gb · fra1")

    def test_limit_keeps_the_totals(self):
        v = view(sample(), 2)
        self.assertEqual(len(v["root"]["items"]), 2)
        self.assertTrue(v["summary"].startswith("5 droplets"))

    def test_api_error(self):
        with self.assertRaises(PluginError) as e:
            view({"id": "unauthorized", "message": "Unable to authenticate you"}, 20)
        self.assertIn("Unable to authenticate you", str(e.exception))


if __name__ == "__main__":
    unittest.main()
