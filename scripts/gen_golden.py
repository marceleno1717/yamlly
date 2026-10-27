"""Generate golden fixtures from the Python ktui implementation.

The Rust port (ktui-core) must reproduce these outputs byte-for-byte.
Run from the yamlgenerator root with the Python venv:

    ./bin/python3 apps/ktui-rs/scripts/gen_golden.py

Writes ``crates/ktui-core/tests/fixtures/golden.json``.
"""

from __future__ import annotations

import copy
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
RS_ROOT = HERE.parent
PY_SRC = RS_ROOT.parent / "ktui" / "src"
sys.path.insert(0, str(PY_SRC))

from ktui.schema.emitter import _clean, emit_yaml_from_dict  # noqa: E402
from ktui.schema.loader import (  # noqa: E402
    get_all_resources,
    get_common_fields,
    get_leaf_field_paths,
    load_resource,
)
from ktui.ui.engine.schema_form_builder import (  # noqa: E402
    decode_path,
    encode_path,
    group_common_fields_by_section,
)
from ktui.ui.engine.state import get_nested, set_nested  # noqa: E402
from ktui.ui.widgets.resource_editor import _normalize  # noqa: E402

# ---------------------------------------------------------------------------
# Emitter cases — cover scalar quoting, nesting, folding, multi-line, numbers
# ---------------------------------------------------------------------------

LONG = "word " * 40
EMIT_CASES: list[dict] = [
    {"kind": "Pod", "apiVersion": "v1", "metadata": {"name": "x"}},
    {
        "apiVersion": "apps/v1",
        "kind": "Deployment",
        "metadata": {"name": "web", "namespace": "default",
                     "labels": {"app": "web", "app.kubernetes.io/name": "web"}},
        "spec": {
            "replicas": 3,
            "selector": {"matchLabels": {"app": "web"}},
            "template": {
                "metadata": {"labels": {"app": "web"}},
                "spec": {
                    "containers": [
                        {"name": "nginx", "image": "nginx:1.25",
                         "ports": [{"containerPort": 80}, {"containerPort": 443}],
                         "env": [{"name": "A", "value": "1"}, {"name": "B", "value": "true"}],
                         "args": ["--port", "8080", "-v"]},
                        {"name": "side", "image": "busybox"},
                    ]
                },
            },
        },
        "empty": {}, "none": None, "blank": "", "lst": [], "nested": {"a": {"b": {}}},
    },
    {"strings": {
        "num": "80", "float": "1.5", "neg": "-3", "exp": "1e3", "hex": "0x1F", "oct": "0o17",
        "under": "1_000", "t": "true", "T": "True", "f": "false", "y": "yes", "n": "no",
        "on": "on", "off": "off", "null": "null", "tilde": "~", "Null": "Null",
        "colon": "a: b", "colon_end": "a:", "url": "http://x.y/z", "hash": "a #b", "hash2": "a#b",
        "lead_hash": "#x", "dash": "-x", "dash_sp": "- x", "q": "?x", "star": "*x", "amp": "&x",
        "bang": "!x", "pipe": "|x", "gt": ">x", "pct": "%x", "at": "@x", "tick": "`x",
        "sq": "'x", "dq": "\"x", "mid_sq": "it's", "mid_dq": 'say "hi"', "brace": "{x}",
        "bracket": "[x]", "comma": "a,b", "lead_comma": ",x", "space_lead": " x",
        "space_trail": "x ", "tab": "a\tb", "nl": "line1\nline2", "nl_trail": "line1\n",
        "unicode": "héllo ✓", "dots": "...", "dashes": "---", "colon_only": ":", "eq": "=",
        "date": "2024-01-01", "time": "12:30", "ver": "1.2.3", "inf": ".inf", "nan": ".nan",
        "plus": "+1", "zero": "0", "zz": "007", "img": "repo/img@sha256:abc", "path": "/var/log",
        "cpu": "500m", "mem": "128Mi", "star_mid": "a*b", "long": LONG.strip(),
        "long_nospace": "x" * 150, "backslash": "a\\b", "ctrl": "a\x07b",
    }},
    {"numbers": {"i": 0, "neg": -5, "big": 123456789012, "f": 1.5, "f0": 2.0, "fneg": -0.25,
                 "b1": True, "b0": False}},
    {"lists": {"scalars": ["a", "1", 2, True], "nested": [["a", "b"], ["c"]],
               "dicts": [{"a": 1, "b": {"c": [1, 2]}}], "mixed": [{"x": []}, {}, "", None, "keep"]}},
    {"keys": {"80": "num-key", "true": "bool-key", "a b": "space", "a:b": "colon", "-k": "dash",
              "with.dot": 1, "": "empty-key"}},
    {"top": [1, 2, 3]},
    # Line-wrapping in every container context (ruamel best_width=120)
    {"root": LONG.strip()},
    {"l": [LONG.strip(), [LONG.strip()], {"k": LONG.strip()}]},
    {"q": "a: " + LONG.strip()}, {"dq": "it's " + LONG.strip()},
    {"tab": "a\t" + LONG.strip()}, {"nl": "x\n" + LONG.strip()},
    {"lead": " " + LONG.strip()},
    {"k": {"deep": {"deeper": [{"x": LONG.strip()}]}}},
    {"w": " ".join(["ab"] * 70), "w3": " ".join(["abc"] * 50), "w7": " ".join(["abcdefg"] * 30),
     "dbl": "x  " * 50, "sp_end": ("y " * 70)},
    {"m": "line1\nline2 it's", "sq": "it's \"q\"", "nl2": "a\n\nb", "nl_sp": "a \nb", "br_sp": "a\n b"},
    {"dqwrap": "\t" + "abcdefghij " * 15, "dqwrap2": "it's" + " w" * 80,
     "dqq": "it's \"" + "zz " * 50, "dq_dbl": "it's  " + "word  " * 30},
    {"sqwrap": "'" + " word" * 40, "sqwrap2": "#" + " ab" * 70},
    # Keys: long (>=128 -> complex '?' key) and multi-line
    {"x" * 130: 1}, {"k\nl": "v"},
    # Floats (Python repr + ruamel '.0e' tweak)
    {"floats": [1e20, 1e-7, 0.1, 100.0, 1e16, 1e15, 123456.789, 1.5e300, 0.0001, 0.00001]},
    # Root-level sequence and empty root
    ["a", {"b": 1}, ["c"]],
    {},
    # Unicode width accounting + supplementary plane
    {"u": "é " * 70, "emoji": "😀 x", "nbsp": "a\u00a0b", "c1": "a\x85b", "bom": "\ufeffx"},
    # Resolver first-char gating
    {"r": ["_", "__1", "0b101", "-0x1F", ".5", "1.", "+.inf", "-.Inf", "NULL", "FALSE", "<<",
           "2024-1-1 1:00:00", "2024-01-01T10:00:00Z", "!", "&", "*"]},
]

# ---------------------------------------------------------------------------
# State cases — sequences of set_nested ops followed by get_nested probes
# ---------------------------------------------------------------------------

STATE_CASES = [
    {"start": {}, "ops": [[["a", "b", "c"], 42]], "gets": [["a", "b", "c"]]},
    {"start": {}, "ops": [[["a", "b[0]", "c"], 42]], "gets": [["a", "b[0]", "c"], ["a", "b", "c"]]},
    {"start": {"a": {"b": [{"c": 1}]}}, "ops": [[["a", "b[1]", "c"], 2]], "gets": [["a", "b[1]", "c"]]},
    {"start": {}, "ops": [[["spec", "containers", "0", "name"], "nginx"]],
     "gets": [["spec", "containers", "0", "name"], ["spec", "containers[0]", "name"]]},
    {"start": {}, "ops": [[["spec", "containers[2]", "name"], "c"]], "gets": [["spec", "containers"]]},
    {"start": {"a": {"b": "x", "c": "y"}}, "ops": [[["a", "b"], ""], [["a", "c"], None]], "gets": [["a"]]},
    {"start": {"a": "scalar"}, "ops": [[["a", "b"], 1]], "gets": [["a"]]},
    {"start": {"a": {"k": 1}}, "ops": [[["a", "0"], "z"]], "gets": [["a"]]},
    {"start": {}, "ops": [[["m", "[0]"], "v"]], "gets": [["m"]]},
    {"start": {}, "ops": [[["l[0]", "x[1]", "y"], 5]], "gets": [["l"]]},
    {"start": {"a": [1, 2]}, "ops": [[["a[1]"], ""]], "gets": [["a"]]},
    {"start": {}, "ops": [[["labels"], {"app": "x"}], [["labels"], {}]], "gets": [["labels"]]},
    {"start": {}, "ops": [[[], 1]], "gets": [[]]},
    {"start": {"a": 1}, "ops": [], "gets": [["missing"], ["a", "b"], ["a[0]"]]},
    {"start": {"s": "xyz", "d": {"0": "zero"}, "l": [1, 2]}, "ops": [],
     "gets": [["s", "0"], ["s", "2"], ["s", "5"], ["s", "a"], ["d", "0"], ["d[0]"], ["l", "5"], ["l", "x"]]},
    {"start": {}, "ops": [[["a", "b"], 0], [["a", "c"], False], [["a", "d"], []]], "gets": [["a"]]},
]

PATH_CASES = [
    "spec.template.spec.containers[0].resources.requests",
    "metadata.name",
    "spec.template.spec.containers[].ports[].containerPort",
    "spec.containers[12].env[3].value",
    "a",
    "a.b_c.d",
]
DECODE_EXTRA = ["invalid_id", "sf_", "sf_a__b-ARR-ARR3", "sf_x-ARRy"]

NORMALIZE_CASES = [
    {"spec": {"replicas": 2, "paused": False, "x": None, "y": "", "z": [], "w": {}}},
    {"a": [1, "", None, {"b": None}, [], 2.5, True]},
    {"metadata": {"name": "n", "labels": {"": "v"}}},
    {"f": [1e20, 0.1, 100.0, 1e-7, -0.0]},
    "scalar", 7, None,
]


def main() -> None:
    out: dict = {}

    out["emit"] = [{"input": c, "clean": _clean(copy.deepcopy(c)),
                    "yaml": emit_yaml_from_dict(copy.deepcopy(c))} for c in EMIT_CASES]

    state = []
    for case in STATE_CASES:
        data = copy.deepcopy(case["start"])
        for path, value in case["ops"]:
            set_nested(data, path, copy.deepcopy(value))
        probes = [{"path": p, "value": get_nested(data, p)} for p in case["gets"]]
        state.append({**case, "result": data, "probes": probes})
    out["state"] = state

    out["paths"] = [{"dotted": p, "encoded": encode_path(p), "decoded": decode_path(encode_path(p))}
                    for p in PATH_CASES]
    out["decode_extra"] = [{"id": i, "decoded": decode_path(i)} for i in DECODE_EXTRA]
    out["normalize"] = [{"input": c, "output": _normalize(c)} for c in NORMALIZE_CASES]

    resources = get_all_resources()
    out["resources"] = [{"kind": r["kind"], "apiVersion": r["apiVersion"]} for r in resources]
    out["common_fields"] = {r["kind"]: get_common_fields(r["kind"]) for r in resources}
    out["sections"] = {r["kind"]: group_common_fields_by_section(get_common_fields(r["kind"]))
                       for r in resources}
    leaf = {}
    for r in resources:
        shard = load_resource(r["kind"])
        leaf[r["kind"]] = [list(t) for t in get_leaf_field_paths(shard["fields"])] if shard else None
    out["leaf_paths"] = leaf

    dest = RS_ROOT / "crates" / "yamlly-core" / "tests" / "fixtures" / "golden.json"
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_text(json.dumps(out, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"wrote {dest} ({dest.stat().st_size // 1024} KB)")


if __name__ == "__main__":
    main()
