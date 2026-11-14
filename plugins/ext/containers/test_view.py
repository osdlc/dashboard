"""Parse tests: the recorded CLI output in testdata/ok through view. Run with `python3 -m unittest`."""

import os
import unittest

import view

OK = os.path.join(os.path.dirname(os.path.abspath(__file__)), "testdata", "ok")


def read(name):
    with open(os.path.join(OK, name), encoding="utf-8") as f:
        return f.read()


def engines(show_all=False):
    out = []
    for ctx in view.json_lines(read("contexts.jsonl")):
        if view.socket_path(ctx) is not None:
            out.append(view.engine_down(ctx))
        else:
            suffix = "-all" if show_all else ""
            out.append(view.engine_up(ctx, read(f"ps-{ctx['Name']}{suffix}.jsonl")))
    return out


def all_vms():
    return (view.colima_rows(read("colima.jsonl")) + view.lima_rows(read("limactl.jsonl"))
            + view.podman_rows(read("podman.json")))


def tables(v):
    return [c for c in v["root"]["children"] if c["type"] == "table"]


class ParseTest(unittest.TestCase):
    def test_json_lines_reads_objects_arrays_and_nothing(self):
        self.assertEqual(view.json_lines(""), [])
        self.assertEqual(view.json_lines('{"a":1}\n\n{"a":2}\n'), [{"a": 1}, {"a": 2}])
        self.assertEqual(view.json_lines('[{"a":1}]'), [{"a": 1}])
        with self.assertRaises(ValueError):
            view.json_lines("not json")

    def test_size(self):
        self.assertEqual(view.size(8 * 1024**3), "8 GiB")
        self.assertEqual(view.size(512 * 1024**2), "512 MiB")
        self.assertEqual(view.size(None), "?")
        self.assertEqual(view.size(0), "?")

    def test_vm_rows(self):
        colima = view.colima_rows(read("colima.jsonl"))
        self.assertEqual(colima[0]["res"], "4 · 8 GiB · 100 GiB")
        self.assertEqual(colima[0]["extra"], "docker")
        self.assertEqual(view.lima_rows(read("limactl.jsonl"))[0]["extra"], "vz")
        podman = view.podman_rows(read("podman.json"))
        self.assertEqual((podman[0]["status"], podman[0]["name"]), ("Stopped", "podman-machine-default"))

    def test_socket_path_is_only_for_unix_endpoints(self):
        self.assertEqual(view.socket_path({"DockerEndpoint": "unix:///x/d.sock"}), "/x/d.sock")
        self.assertIsNone(view.socket_path({"DockerEndpoint": "tcp://192.0.2.10:2375"}))
        self.assertIsNone(view.socket_path({}))


class BuildTest(unittest.TestCase):
    def test_everything_present(self):
        v = view.build(all_vms(), engines(), False, True, 30)
        self.assertEqual(v["status"], "ok")
        self.assertEqual(v["summary"], "2/4 VMs up · 2/3 engines · 3 containers")
        vms, engine_table, containers = tables(v)
        # Running VMs first, then by kind and name.
        self.assertEqual([r["cells"]["name"] for r in vms["rows"]],
                         ["default", "dev", "staging", "podman-machine-default"])
        by_engine = {r["cells"]["engine"]: r for r in engine_table["rows"]}
        self.assertEqual(by_engine["desktop"]["cells"]["state"], "not running")
        self.assertEqual(by_engine["desktop"]["status"], "unknown")
        self.assertEqual(by_engine["colima"]["cells"]["current"], "●")
        self.assertEqual(by_engine["colima"]["cells"]["count"], "2 running")
        self.assertEqual([r["cells"]["name"] for r in containers["rows"]], ["cache", "postgres", "web"])
        self.assertEqual(containers["empty_text"], "No running containers")

    def test_show_exited_lists_exited_containers_after_running_ones(self):
        v = view.build(all_vms(), engines(show_all=True), True, True, 30)
        rows = tables(v)[2]["rows"]
        self.assertEqual([r["cells"]["name"] for r in rows], ["cache", "postgres", "web", "old-build"])
        self.assertEqual(rows[-1]["status"], "unknown")
        self.assertEqual(tables(v)[2]["empty_text"], "No containers")
        self.assertIn("3 containers", v["summary"])

    def test_limit(self):
        v = view.build(all_vms(), engines(), False, True, 2)
        self.assertEqual(len(tables(v)[2]["rows"]), 2)
        self.assertIn("3 containers", v["summary"])

    def test_hide_stopped_vms(self):
        v = view.build(all_vms(), engines(), False, False, 30)
        self.assertEqual([r["cells"]["name"] for r in tables(v)[0]["rows"]], ["default", "dev"])
        self.assertEqual(v["summary"].split(" · ")[0], "2/4 VMs up")

    def test_failing_cli_is_an_error_row_and_warns(self):
        vms = view.colima_rows(read("colima.jsonl")) + [view.vm_error("lima", "instance store is corrupt")]
        v = view.build(vms, [], False, True, 30)
        self.assertEqual(v["status"], "warn")
        row = tables(v)[0]["rows"][-1]
        self.assertEqual(row["status"], "error")
        self.assertEqual(row["cells"]["status"], "instance store is corrupt")
        # Error rows stay when stopped VMs are hidden.
        v = view.build(vms, [], False, False, 30)
        self.assertEqual(len(tables(v)[0]["rows"]), 2)

    def test_engine_error_warns(self):
        ctx = {"Name": "remote", "DockerEndpoint": "tcp://192.0.2.99:2375"}
        v = view.build([], [view.engine_error(ctx, "cannot connect")], False, True, 30)
        self.assertEqual(v["status"], "warn")
        self.assertEqual(tables(v)[0]["rows"][0]["status"], "error")

    def test_nothing_found(self):
        v = view.build([], [], False, True, 30)
        self.assertEqual(v["status"], "ok")
        self.assertEqual(v["summary"], "0/0 VMs up · 0/0 engines · 0 containers")
        self.assertEqual(v["root"]["children"][1]["type"], "text")
        self.assertEqual(tables(v), [])



class SizeTest(unittest.TestCase):
    def test_podman_sizes_are_spaced_like_the_others(self):
        self.assertEqual(view._res(2, "2GiB", "100GiB"), "2 · 2 GiB · 100 GiB")
        self.assertEqual(view._res(4, "8 GiB", "1.5TB"), "4 · 8 GiB · 1.5 TB")
        self.assertEqual(view._res("?", "?", "?"), "? · ? · ?")

if __name__ == "__main__":
    unittest.main()
