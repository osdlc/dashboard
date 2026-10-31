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
        v = view(sample(), 95123, 10, SAMPLE_NOW)
        self.assertEqual(v["status"], "error")
        self.assertEqual(v["summary"], "3 open errors · 1 critical")
        items = v["root"]["items"]
        self.assertEqual(items[0]["title"], "ActiveRecord::RecordNotFound: Couldn't find User with 'id'=42")
        self.assertEqual(items[0]["subtitle"], "1.2k notices · production · users#show · 7m ago")
        self.assertEqual(items[0]["status"], "warn")
        self.assertEqual(
            items[0]["action"],
            {"type": "open_url", "url": "https://airbrake.io/projects/95123/groups/3410958123771200417"},
        )
        # No action name: just the component.
        self.assertEqual(items[1]["subtitle"], "87 notices · production · PaymentsJob · 30m ago")
        self.assertEqual(items[1]["status"], "error")
        self.assertEqual(items[2]["tags"], ["muted"])
        self.assertNotIn("status", items[2])

    def test_resolved_and_limit(self):
        reply = sample()
        reply["groups"][1]["resolved"] = True
        v = view(reply, 95123, 1, SAMPLE_NOW)
        self.assertEqual(v["summary"], "2 open errors")
        self.assertEqual(v["status"], "warn")
        self.assertEqual(len(v["root"]["items"]), 1)

    def test_bad_reply(self):
        with self.assertRaises(PluginError):
            view({"message": "Unauthorized"}, 1, 10, SAMPLE_NOW)


if __name__ == "__main__":
    unittest.main()
