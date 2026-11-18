"""The comparison's score: each engine's answers against the probes' expectations.

    python3 -I score.py <work dir> [engine…]     (default: ours intelephense phpactor)

Reads <work dir>/probes.json and answers-<engine>.json; writes <work dir>/score.md (the
table per category, the warm latencies) and details.md (each probe, each engine).

Per probe: `ok`, `partial` or `no` (nothing, or wrong):
  completion   every expected name offered / some of them
  definition   the first place given is the declaration / another one is
  references,
  rename       exactly the lines expected / some of them
  hover        every expected text in it / some (case ignored)
  signature    the expected texts and the active argument / the texts only, or some
  diagnostics  a problem on the line whose message matches / one on the line that doesn't
  noise        no error or warning in the file
"""

import json
import re
import statistics
import sys
from pathlib import Path

CATEGORIES = ["completion", "definition", "references", "rename", "hover", "signature",
              "diagnostics", "noise"]
DOC_LINE = re.compile(r"^\s*(/\*\*|\*|\*/|#\[|$)")


def names(item):
    """What a suggestion offers, as names: `$name` → `name`, `App\\Models` → `Models`,
    `add()` → `add`."""
    if isinstance(item, str):
        item = [item]
    out = set()
    for text in item:
        if not text:
            continue
        text = text.strip().rstrip("\\")
        text = text.split("(")[0].strip()
        text = text.rsplit("\\", 1)[-1].lstrip("$").lstrip("\\")
        if text:
            out.add(text)
    return out


def declares(work, projects, project, place, expected):
    """Whether `place` (an answer's: file, line, or file, start, end, selection) is the
    expected declaration: its line, or a range starting with the declaration's docblock /
    attributes."""
    file, line = expected
    if place[0] != file:
        return False
    if len(place) == 2:
        return place[1] == line
    _, start, end, selection = place
    if line in (start, selection):
        return True
    if start <= line <= end:
        text = (Path(projects[project]) / file).read_text(encoding="utf-8").split("\n")
        return all(DOC_LINE.match(text[i]) for i in range(start, line))
    return False


def judge(work, projects, p, answer):
    result = None if answer is None else answer.get("result")
    category = p["category"]
    if category == "completion":
        offered = set()
        for item in result or []:
            offered |= names(item)
        found = [e for e in p["expect"] if e in offered]
        return grade(len(found), len(p["expect"]))
    if category == "definition":
        places = result or []
        if not places:
            return "no"
        if declares(work, projects, p["project"], places[0], p["expect"]):
            return "ok"
        if any(declares(work, projects, p["project"], x, p["expect"]) for x in places[1:]):
            return "partial"
        return "no"
    if category in ("references", "rename"):
        got = {tuple(x) for x in result or []}
        expected = {tuple(x) for x in p["expect"]}
        if got == expected:
            return "ok"
        return "partial" if got & expected else "no"
    if category == "hover":
        text = (result or "").lower()
        found = [e for e in p["expect"] if e.lower() in text]
        return grade(len(found), len(p["expect"]))
    if category == "signature":
        if not result:
            return "no"
        label = (result.get("label") or "").lower()
        found = [e for e in p["expect"] if e.lower() in label]
        if len(found) == len(p["expect"]):
            return "ok" if result.get("active") == p["active"] else "partial"
        return "partial" if found else "no"
    if category == "diagnostics":
        line = p["expect"][1]
        there = [d for d in result or [] if d["line"] == line]
        if any(re.search(p["pattern"], d["message"], re.IGNORECASE) for d in there):
            return "ok"
        return "partial" if there else "no"
    if category == "noise":
        loud = [d for d in result or [] if d["severity"] in ("error", "warning")]
        return "ok" if not loud else "no"
    raise SystemExit(f"unknown category {category}")


def grade(found, expected):
    if found == expected:
        return "ok"
    return "partial" if found else "no"


def percentile(values, q):
    if not values:
        return None
    values = sorted(values)
    k = (len(values) - 1) * q
    low, high = int(k), min(int(k) + 1, len(values) - 1)
    return values[low] + (values[high] - values[low]) * (k - low)


def main(work, engines):
    data = json.loads((work / "probes.json").read_text(encoding="utf-8"))
    probes, projects = data["probes"], data["projects"]
    answers = {}
    for engine in engines:
        path = work / f"answers-{engine}.json"
        if path.exists():
            answers[engine] = json.loads(path.read_text(encoding="utf-8"))
    engines = [e for e in engines if e in answers]
    verdicts = {e: {} for e in engines}
    for engine in engines:
        for p in probes:
            verdicts[engine][p["id"]] = judge(work, projects, p, answers[engine]["answers"].get(p["id"]))

    lines = ["| Category (probes) | " + " | ".join(f"{e} ok / partial / no" for e in engines) + " |",
             "|---|" + "---|" * len(engines)]
    totals = {e: [0, 0, 0] for e in engines}
    for category in CATEGORIES:
        ids = [p["id"] for p in probes if p["category"] == category]
        cells = []
        for e in engines:
            counts = [sum(verdicts[e][i] == v for i in ids) for v in ("ok", "partial", "no")]
            for k in range(3):
                totals[e][k] += counts[k]
            cells.append(" / ".join(map(str, counts)))
        lines.append(f"| {category} ({len(ids)}) | " + " | ".join(cells) + " |")
    n = len(probes)
    lines.append(f"| **total ({n})** | " + " | ".join(
        f"**{t[0]} / {t[1]} / {t[2]} ({100 * t[0] / n:.0f}%)**" for t in totals.values()) + " |")
    lines += ["", "Warm latency, p50 / p95 ms (the second to fourth asks of each probe):", "",
              "| Category | " + " | ".join(engines) + " |", "|---|" + "---|" * len(engines)]
    for category in CATEGORIES[:6]:
        cells = []
        for e in engines:
            warm = []
            for p in probes:
                if p["category"] == category:
                    a = answers[e]["answers"].get(p["id"]) or {}
                    warm += (a.get("ms") or [])[1:]
            p50, p95 = percentile(warm, 0.5), percentile(warm, 0.95)
            cells.append("—" if p50 is None else f"{p50:.2f} / {p95:.2f}")
        lines.append(f"| {category} | " + " | ".join(cells) + " |")
    lines += ["", "Index (first, seconds): " + "; ".join(
        f"{e}: " + ", ".join(f"{k} {v.get('index_seconds', 0):.1f}" for k, v in answers[e]["projects"].items())
        for e in engines)]
    (work / "score.md").write_text("\n".join(lines) + "\n", encoding="utf-8")

    details = ["| Probe | " + " | ".join(engines) + " |", "|---|" + "---|" * len(engines)]
    for p in probes:
        details.append(f"| {p['id']} {p['category']} | " + " | ".join(verdicts[e][p["id"]] for e in engines) + " |")
    (work / "details.md").write_text("\n".join(details) + "\n", encoding="utf-8")
    print("\n".join(lines))


if __name__ == "__main__":
    work = Path(sys.argv[1]).resolve()
    main(work, sys.argv[2:] or ["ours", "intelephense", "phpactor"])
