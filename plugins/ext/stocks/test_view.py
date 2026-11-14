"""Parse tests: the saved replies in samples/ through parse() and view(). Run with `python3 -m unittest`."""

import json
import os
import unittest

from view import SAMPLE_NOW, QuoteError, link, money, parse, view

HERE = os.path.dirname(os.path.abspath(__file__))


def reply(symbol):
    with open(os.path.join(HERE, "samples", symbol + ".json"), encoding="utf-8") as f:
        return json.load(f)


def results(*symbols):
    out = []
    for s in symbols:
        try:
            out.append((s, parse(reply(s))))
        except QuoteError as e:
            out.append((s, e))
    return out


class ViewTest(unittest.TestCase):
    def test_sample(self):
        v = view(results("SPY", "QQQ", "AAPL", "MSFT", "NVDA"), 2.0, True, SAMPLE_NOW)
        self.assertEqual(v["status"], "warn")
        self.assertEqual(v["summary"], "SPY 754.37 -0.4% · QQQ 679.89 -1.2% · AAPL 356.27 +0.4% · closed")
        table, chart = v["root"]["children"]
        self.assertEqual([c["key"] for c in table["columns"]], ["symbol", "name", "price", "change", "percent"])
        rows = table["rows"]
        self.assertEqual([r["cells"]["symbol"] for r in rows], ["SPY", "QQQ", "AAPL", "MSFT", "NVDA"])
        self.assertEqual(rows[2]["cells"]["price"], "$356.27")
        self.assertEqual(rows[2]["cells"]["change"], "+1.39")
        self.assertEqual(rows[2]["cells"]["percent"], "+0.39%")
        self.assertEqual(rows[2]["action"]["url"], "https://finance.yahoo.com/quote/AAPL")
        # Only NVDA fell 2% or more.
        self.assertEqual([r.get("status") for r in rows], [None, None, None, None, "warn"])
        self.assertEqual(chart["type"], "series")
        self.assertEqual(chart["style"], "line")
        self.assertEqual(chart["unit"], "%")
        self.assertEqual([s["name"] for s in chart["series"]], ["SPY", "QQQ", "AAPL", "MSFT", "NVDA"])
        # AAPL's sample has a null close, which is skipped, not drawn as zero.
        self.assertEqual(len(chart["series"][2]["points"]), 9)
        self.assertEqual(chart["series"][2]["points"][-1], (1794603600000, 0.392))

    def test_chart_off(self):
        v = view(results("SPY"), 2.0, False, SAMPLE_NOW)
        self.assertEqual(v["root"]["type"], "table")

    def test_open_market(self):
        v = view(results("SPY"), 2.0, False, 1794590000)
        self.assertEqual(v["summary"], "SPY 754.37 -0.4%")

    def test_warn_drop(self):
        self.assertEqual(view(results("NVDA"), 2.5, False, SAMPLE_NOW)["status"], "ok")
        self.assertEqual(view(results("NVDA"), 2.0, False, SAMPLE_NOW)["status"], "warn")

    def test_unknown_symbol(self):
        v = view(results("AAPL", "NOSUCHSYMBOLXX"), 2.0, False, SAMPLE_NOW)
        row = v["root"]["rows"][1]
        self.assertEqual(row["status"], "error")
        self.assertEqual(row["cells"]["price"], "not found")
        self.assertNotIn("action", row)
        self.assertEqual(v["status"], "warn")

    def test_every_symbol_failed(self):
        v = view(results("NOSUCHSYMBOLXX"), 2.0, True, SAMPLE_NOW)
        self.assertEqual(v["status"], "error")
        self.assertEqual(v["summary"], "no quotes: NOSUCHSYMBOLXX not found")
        self.assertEqual(v["root"]["type"], "table")

    def test_at_most_six_lines(self):
        many = [(f"S{i}", parse(reply("SPY"))) for i in range(8)]
        v = view(many, 2.0, True, SAMPLE_NOW)
        self.assertEqual(len(v["root"]["children"][1]["series"]), 6)
        self.assertEqual(len(v["root"]["children"][0]["rows"]), 8)

    def test_previous_close_fallback(self):
        r = reply("SPY")
        meta = r["chart"]["result"][0]["meta"]
        meta["previousClose"] = meta.pop("chartPreviousClose")
        self.assertEqual(parse(r)["prev"], 757.12)

    def test_helpers(self):
        self.assertEqual(money(1234.5, "USD"), "$1,234.50")
        self.assertEqual(money(0.9213, "EUR"), "€0.9213")
        self.assertEqual(money(12.3, "CHF"), "12.30 CHF")
        self.assertEqual(link("^GSPC")["url"], "https://finance.yahoo.com/quote/%5EGSPC")
        self.assertEqual(link("EURUSD=X")["url"], "https://finance.yahoo.com/quote/EURUSD%3DX")


if __name__ == "__main__":
    unittest.main()
