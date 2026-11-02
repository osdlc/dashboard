"""Parses sample.json, a hand-written Todoist /api/v1/tasks/filter response, and checks the view.

    python3 -m unittest
"""

import datetime
import json
import os
import unittest

import view as view_module
from view import SAMPLE_TODAY, days, due, label, view

with open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "sample.json"), encoding="utf-8") as f:
    RESULTS = json.load(f)["results"]


class ViewTest(unittest.TestCase):
    def test_priorities_read_like_the_app(self):
        self.assertEqual([label(p) for p in (4, 3, 2, 1)], ["p1", "p2", "p3", "p4"])

    def test_due_dates_with_and_without_a_time(self):
        self.assertEqual([days(t, SAMPLE_TODAY) for t in RESULTS], [-2, 0, 0, 0])
        self.assertIsNone(days({**RESULTS[0], "due": None}, SAMPLE_TODAY))

    def test_a_fixed_time_zone_due_time_is_shown_in_local_time(self):
        # Due today at 21:00 in Los Angeles: the API sends the UTC time, tomorrow at 04:00.
        task = {"due": {"date": "2026-10-28T04:00:00.000000Z", "timezone": "America/Los_Angeles"}}
        view_module.TZ = datetime.timezone(datetime.timedelta(hours=-7))
        try:
            self.assertEqual(days(task, "2026-10-27"), 0)
            self.assertEqual(due(task, "2026-10-27"), "today 21:00")
        finally:
            view_module.TZ = None

    def test_overdue_first_then_the_most_urgent(self):
        v = view(RESULTS, SAMPLE_TODAY, 10)
        self.assertEqual(v["status"], "error")
        self.assertEqual(v["summary"], "1 overdue · 3 today")
        stats, lst = v["root"]["children"]
        self.assertEqual([s["value"] for s in stats["children"]], ["1", "3", "2"])
        self.assertEqual(
            [[i["title"], i["subtitle"], i.get("status"), i["tags"]] for i in lst["items"]],
            [
                ["Pay the electricity bill", "overdue 2d", "error", ["p1", "home"]],
                ["Ship the release notes", "today", "warn", ["p1", "work", "release"]],
                ["Reply to the review comments on the parser PR", "today 15:00", None, ["p2", "work"]],
                ["Water the plants", "today · every 3 days", None, []],
            ],
        )
        self.assertEqual(
            lst["items"][0]["action"], {"type": "open_url", "url": "https://app.todoist.com/app/task/6Jf8VQXxpwv56VQ7"}
        )


if __name__ == "__main__":
    unittest.main()
