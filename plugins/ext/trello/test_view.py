"""Parses sample.json, a hand-written Trello /1/boards/{id}/lists?cards=open response, and
checks the view.

    python3 -m unittest
"""

import json
import os
import unittest

from view import SAMPLE_NOW, view

with open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "sample.json"), encoding="utf-8") as f:
    BOARD = json.load(f)


class ViewTest(unittest.TestCase):
    def test_every_list_soonest_due_first_undated_last(self):
        v = view(BOARD, [], SAMPLE_NOW, 10)
        self.assertEqual(v["status"], "error")
        self.assertEqual(v["summary"], "6 cards · 1 overdue · 1 due soon")
        bars, lst = v["root"]["children"]
        self.assertEqual(
            [[b["label"], b["value"]] for b in bars["bars"]],
            [["Backlog", 3], ["Doing", 2], ["Review", 1], ["Done", 0]],
        )
        self.assertEqual(
            [[i["title"], i["subtitle"], i.get("status"), i["tags"]] for i in lst["items"]],
            [
                ["Fix the flaky login test", "Doing · overdue 20h", "error", ["bug"]],
                ["Write the migration guide", "Doing · due in 21h", "warn", ["docs"]],
                ["Audit third-party scripts", "Backlog · due in 10d", None, ["red"]],
                ["Dark mode for the settings page", "Backlog", None, ["design"]],
                ["Export reports as CSV", "Backlog", None, []],
                ["New onboarding emails", "Review · done", "ok", []],
            ],
        )
        self.assertEqual(lst["items"][0]["action"], {"type": "open_url", "url": "https://trello.com/c/Mn34Op56"})

    def test_only_the_named_lists_any_case(self):
        v = view(BOARD, ["review", "Done"], SAMPLE_NOW, 10)
        self.assertEqual(v["status"], "ok")
        self.assertEqual(v["summary"], "1 card")
        # The bars still show the whole board.
        self.assertEqual(len(v["root"]["children"][0]["bars"]), 4)


if __name__ == "__main__":
    unittest.main()
