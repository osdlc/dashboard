"""Parses sample.json, a hand-written Asana GET /tasks response, and checks the view.

    python3 -m unittest
"""

import json
import os
import unittest

from view import PAGE, SAMPLE_TODAY, due, view

with open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "sample.json"), encoding="utf-8") as f:
    DATA = json.load(f)["data"]


class ViewTest(unittest.TestCase):
    def test_due_dates_in_words(self):
        self.assertEqual(due("2026-10-24", SAMPLE_TODAY), "overdue 3d")
        self.assertEqual(due("2026-10-27", SAMPLE_TODAY), "due today")
        self.assertEqual(due("2026-10-28", SAMPLE_TODAY), "due tomorrow")
        self.assertEqual(due("2026-11-03", SAMPLE_TODAY), "due in 7d")
        self.assertEqual(due(None, SAMPLE_TODAY), "no due date")

    def test_overdue_first_then_by_date_undated_last(self):
        v = view(DATA, SAMPLE_TODAY, 10)
        self.assertEqual(v["status"], "error")
        self.assertEqual(v["summary"], "1 overdue · 2 today · 6 open")
        lst = v["root"]["children"][1]
        self.assertEqual(
            [[i["title"], i["subtitle"], i.get("status")] for i in lst["items"]],
            [
                ["Write the Q4 roadmap draft", "Planning · overdue 3d", "error"],
                ["Approve design for the pricing page", "Launch · Design · due today", "warn"],
                ["Review onboarding checklist", "People · due today", "warn"],
                ["Send launch notes to support", "Launch · due tomorrow", None],
                ["Renew the domain certificates", "Ops · due in 7d", None],
                ["Book venue for the team offsite", "no due date", None],
            ],
        )
        self.assertEqual(
            lst["items"][0]["action"],
            {"type": "open_url", "url": "https://app.asana.com/0/1208000000000001/1208000000000101"},
        )

    def test_limit_cuts_the_list_not_the_counts(self):
        v = view(DATA, SAMPLE_TODAY, 2)
        self.assertEqual(len(v["root"]["children"][1]["items"]), 2)
        self.assertEqual(v["summary"], "1 overdue · 2 today · 6 open")

    def test_a_full_page_says_there_may_be_more(self):
        v = view([{"name": f"t{n}", "due_on": None, "permalink_url": "https://app.asana.com/0/1/2", "projects": []} for n in range(PAGE)], SAMPLE_TODAY, 10)
        self.assertEqual(v["summary"], "0 overdue · 0 today · 100+ open")
        self.assertEqual(v["root"]["children"][0]["children"][2]["value"], "100+")
        self.assertEqual(v["root"]["children"][1]["empty_text"], "No open tasks")


if __name__ == "__main__":
    unittest.main()
