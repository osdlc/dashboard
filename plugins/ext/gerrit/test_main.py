"""Parse tests on sample.json, a hand-written Gerrit API response. Run: python3 -m unittest"""

import json
import os
import unittest

import main
from dashboard_plugin import PluginError

HERE = os.path.dirname(os.path.abspath(__file__))
URL = "https://review.example.com"


def sample_text():
    """The sample as Gerrit sends it, after the magic line."""
    with open(os.path.join(HERE, "sample.json"), encoding="utf-8") as f:
        return ")]}'\n" + json.dumps(json.load(f)["/a/changes/"])


class SampleTest(unittest.TestCase):
    def test_changes_from_the_sample(self):
        replies = main.parse(sample_text(), 2)
        v = main.view(main.DEFAULT_SECTIONS, replies, URL, 10, main.SAMPLE_NOW)
        self.assertEqual(v["summary"], "3 mine · 2 to review")
        _, mine, _, review = v["root"]["children"]
        approved, disliked, failed = mine["items"]
        self.assertEqual(approved["status"], "ok")
        self.assertEqual(approved["tags"], ["CR+2", "V+1"])
        self.assertEqual(approved["action"]["url"], "https://review.example.com/c/platform/build/+/48213")
        self.assertEqual(approved["subtitle"], "platform/build 48213 · main · by Dana Ruiz · updated 1h ago")
        self.assertEqual(disliked["status"], "warn")
        self.assertIn("2 unresolved", disliked["subtitle"])
        self.assertEqual(failed["status"], "error")
        self.assertEqual(failed["tags"], ["V-1"])
        recommended, plain = review["items"]
        self.assertNotIn("status", recommended)
        self.assertEqual(recommended["tags"], ["CR+1", "V+1"])
        self.assertEqual(plain["subtitle"].split(" · ")[-1], "updated 1d ago")

    def test_one_query_is_a_flat_list(self):
        replies = main.parse(")]}'\n" + json.dumps(json.loads(sample_text()[5:])[0]), 1)
        self.assertEqual(len(replies), 1)
        self.assertEqual(len(replies[0]), 3)

    def test_other_labels_are_tags_only(self):
        c = {
            "_number": 7,
            "project": "openstack/nova",
            "subject": "WIP change",
            "labels": {"Workflow": {"rejected": {}}, "Verified": {"approved": {}}},
            "work_in_progress": True,
        }
        item = main.change(c, URL, main.SAMPLE_NOW)
        self.assertEqual(item["tags"], ["wip", "V+1", "Workflow-1"])
        self.assertNotIn("status", item)
        # Submittable without a +2 (some servers) is green too.
        item = main.change({**c, "labels": {}, "submittable": True}, URL, main.SAMPLE_NOW)
        self.assertEqual(item["status"], "ok")

    def test_bad_replies(self):
        for text in ["<html>login</html>", ")]}'\n{}", ")]}'\n[[], []]"]:
            with self.assertRaises(PluginError) as e:
                main.parse(text, 3)
            self.assertEqual(e.exception.code, -32001)
        self.assertIsNone(main.change({"subject": "no number"}, URL, main.SAMPLE_NOW))

    def test_gerrit_times(self):
        self.assertEqual(main.parse_time("2026-10-27 12:00:00.000000000"), main.SAMPLE_NOW)
        self.assertEqual(main.parse_time("2026-10-27T12:00:00Z"), main.SAMPLE_NOW)


if __name__ == "__main__":
    unittest.main()
