"""The project's numbers, counted from git and the tree, so the docs never go stale.

    python3 scripts/progress.py            rewrites the generated blocks of docs/OVERVIEW.md
    python3 scripts/progress.py --check    fails if they differ from what it would write
    python3 scripts/progress.py --json F   writes every number to F (the progress page's data)

A generated block sits between `<!-- progress:NAME -->` and `<!-- /progress -->`;
text outside the blocks is written by hand. The history is main's (up to where this
branch leaves it) plus this change, if it is not on main yet, so a PR's numbers
already count it and stay right once it merges. View scores come from
bench/reference/scores.txt: compare.py's output over every reference view, updated
by the PR that renders them all. Standard library only: .local-ci runs the check.
"""
import collections
import json
import pathlib
import re
import subprocess
import sys
import textwrap

ROOT = pathlib.Path(__file__).resolve().parent.parent
OVERVIEW = ROOT / "docs/OVERVIEW.md"
REFERENCE = ROOT / "bench/reference"
# Which capture a view belongs to, by the views file that lists it (first match).
KINDS = [
    ("views.txt", "Multiplayer world (sky, fog, fog volumes, flares, animated stages, curve LOD)"),
    ("model-views.txt", "Static and brush-entity models, rotating fans and drums"),
    ("npc-views.txt", "Single-player NPCs (spawned, scripted, turned, walked)"),
    ("script-views.txt", "Single-player NPCs (spawned, scripted, turned, walked)"),
    ("turn-views.txt", "Single-player NPCs (spawned, scripted, turned, walked)"),
    ("walk-views.txt", "Single-player NPCs (spawned, scripted, turned, walked)"),
    ("walked-views.txt", "Single-player NPCs (spawned, scripted, turned, walked)"),
]
# Lines added and removed leave out the overview itself, whose numbers would otherwise
# count their own rewriting.
NOT_COUNTED = ":!docs/OVERVIEW.md"
BINS = [(0.25, "< 0.25"), (0.5, "0.25-0.5"), (1.0, "0.5-1"), (2.0, "1-2"), (float("inf"), "> 2")]


def git(*args):
    return subprocess.run(["git", "-C", str(ROOT), *args], capture_output=True, text=True, check=True).stdout


def history():
    """Main's changes (its first-parent line: one per squashed or merged PR) up to this
    branch's merge base, oldest first, plus this change if it is not on main yet."""
    base = git("merge-base", "HEAD", "origin/main").strip()
    on_main = subprocess.run(["git", "-C", str(ROOT), "merge-base", "--is-ancestor", "HEAD",
                              "origin/main"]).returncode == 0
    changes = [change(*line.split("\t", 2), "") for line in git(
        "log", "--reverse", "--first-parent", "--format=%h%x09%ad%x09%s", "--date=short", base).splitlines()]
    # Lines changed come from a second log limited to the counted paths: limiting the
    # first would drop the changes that touch only the overview.
    stat = git("log", "--first-parent", "--diff-merges=first-parent", "--format=%x09%h", "--shortstat",
               base, "--", ".", NOT_COUNTED)
    counted = {}
    for line in stat.splitlines():
        if line.startswith("\t"):
            h = line[1:]
        elif line.strip():
            counted[h] = change("", "", "", line)
    for c in changes:
        c.update(counted.get(c["hash"], {}))
    if not on_main:
        # This change, counted as the squash merge it becomes.
        date = git("log", "-1", "--format=%ad", "--date=short", "HEAD").strip()
        subject = git("log", "-1", "--format=%s", "HEAD").strip()
        diff = git("diff", "--shortstat", base, "HEAD", "--", ".", NOT_COUNTED)
        changes.append({**change("this", date, subject, diff), "pr": "this"})
    return changes, int(git("rev-list", "--count", base)) + (0 if on_main else 1)


def change(h, date, subject, shortstat):
    """One change on main; a merge commit's subject is read for its PR and branch type."""
    merge = re.match(r"Merge pull request #(\d+) from [^/]+/(\w+)/", subject)
    squash = re.search(r"\(#(\d+)\)$", subject)
    c = {"added": int((re.search(r"(\d+) insertion", shortstat) or [0, 0])[1]),
         "removed": int((re.search(r"(\d+) deletion", shortstat) or [0, 0])[1])}
    if h:
        c.update(hash=h, date=date, subject=subject,
                 pr=(merge or squash)[1] if merge or squash else None,
                 type=merge[2] if merge else re.match(r"(\w+)", subject)[1])
    return c


def code():
    """Rust lines (sources, tests, examples) and #[test]s per crate, WGSL lines."""
    files = git("ls-files", "-z", "--", "crates").split("\0")
    crates = collections.OrderedDict()
    wgsl = 0
    for f in sorted(files):
        path = ROOT / f
        if not f or not path.is_file():
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        if f.endswith(".wgsl"):
            wgsl += text.count("\n")
        elif f.endswith(".rs"):
            c = crates.setdefault(f.split("/")[1], {"lines": 0, "tests": 0})
            c["lines"] += text.count("\n")
            c["tests"] += len(re.findall(r"#\[test\]", text))
    return crates, wgsl


def decisions():
    heads = [l for l in (ROOT / "docs/DECISIONS.md").read_text().splitlines() if l.startswith("## ")]
    return {
        "entries": len(heads),
        "owner": sum("chosen by the owner" in h for h in heads),
        "claude": sum("decided by Claude" in h for h in heads),
    }


def views():
    """Every reference view's score, with its kind; [] if scores.txt is missing."""
    kind_of = {}
    for name, kind in KINDS:
        for line in (REFERENCE / name).read_text().splitlines():
            parts = line.split()
            if len(parts) > 1 and not line.startswith("#"):
                kind_of.setdefault(parts[1], kind)
    scores = REFERENCE / "scores.txt"
    if not scores.exists():
        return None, []
    lines = scores.read_text().splitlines()
    measured = next((l[2:].strip() for l in lines if l.startswith("# ")), "")
    out = []
    for line in lines:
        m = re.match(r"(\S+)\s+([\d.]+)\s+([\d.]+)%\s+([\d.]+)%$", line)
        if m:
            out.append({"view": m[1], "mean": float(m[2]), "off": float(m[3]),
                        "kind": kind_of.get(m[1], "Other")})
    return measured, out


def stats():
    commits, ncommits = history()
    prs = [c for c in commits if c["pr"]]
    types = collections.Counter(c["type"] for c in prs)
    days = collections.Counter(c["date"] for c in prs)
    crates, wgsl = code()
    measured, scored = views()
    return {
        "as_of": commits[-1]["date"],
        "last_pr": next(c["pr"] for c in reversed(prs) if c["pr"] != "this"),
        "commits": ncommits,
        "prs": len(prs),
        "first_day": commits[0]["date"],
        "pr_types": dict(types.most_common()),
        "prs_per_day": dict(sorted(days.items())),
        "added": sum(c["added"] for c in commits),
        "removed": sum(c["removed"] for c in commits),
        "tracked_files": len([f for f in git("ls-files", "-z").split("\0") if f]),
        "crates": crates,
        "rust_lines": sum(c["lines"] for c in crates.values()),
        "tests": sum(c["tests"] for c in crates.values()),
        "wgsl_lines": wgsl,
        "decisions": decisions(),
        "views_measured": measured,
        "views": scored,
        "history": [{k: c[k] for k in ("hash", "date", "subject", "pr", "type", "added", "removed")} for c in prs],
    }


def growth(changes):
    """Lines of Rust and #[test]s in the tree after each change on main (the page's chart)."""
    cache = {}
    cat = subprocess.Popen(["git", "-C", str(ROOT), "cat-file", "--batch"], stdin=subprocess.PIPE,
                           stdout=subprocess.PIPE)

    def blob(oid):
        if oid not in cache:
            cat.stdin.write(oid.encode() + b"\n")
            cat.stdin.flush()
            size = int(cat.stdout.readline().split()[2])
            text = cat.stdout.read(size + 1)[:-1]
            cache[oid] = (text.count(b"\n"), text.count(b"#[test]"))
        return cache[oid]

    out = []
    for c in changes:
        if c["hash"] == "this":
            continue
        lines = tests = 0
        for row in git("ls-tree", "-r", c["hash"], "--", "crates").splitlines():
            meta, path = row.split("\t", 1)
            if path.endswith(".rs"):
                n, t = blob(meta.split()[2])
                lines, tests = lines + n, tests + t
        out.append({"date": c["date"], "pr": c["pr"], "rust_lines": lines, "tests": tests})
    cat.stdin.close()
    return out


def day(d):
    months = "Jan Feb Mar Apr May Jun Jul Aug Sep Oct Nov Dec".split()
    y, m, dd = d.split("-")
    return f"{months[int(m) - 1]} {int(dd)}"


def median(xs):
    xs = sorted(xs)
    n = len(xs)
    return (xs[n // 2] + xs[(n - 1) // 2]) / 2 if n else 0.0


def blocks(s):
    """The text of each generated block, by name."""
    b = {}
    b["asof"] = (f"Numbers in the generated parts below are as of {s['as_of']} "
                 f"(through PR #{s['last_pr']}), counted by `scripts/progress.py`.")
    b["code"] = (f"{len(s['crates'])} crates in one Cargo workspace, {s['rust_lines']:,} lines of Rust "
                 f"(sources, tests and examples) plus {s['wgsl_lines']:,} lines of WGSL shaders, "
                 f"{s['tests']} tests. Each crate maps to one part of the original engine.")
    b["process"] = (f"So far: {s['decisions']['entries']} decision entries, {s['decisions']['owner']} "
                    f"chosen by the owner, {s['decisions']['claude']} made by Claude for review.")
    types = ", ".join(f"{n} {t}" for t, n in s["pr_types"].items())
    span = len(s["prs_per_day"])
    labels = ", ".join(f'"{day(d)}"' for d in s["prs_per_day"])
    counts = ", ".join(str(n) for n in s["prs_per_day"].values())
    top = max(s["prs_per_day"].values())
    b["history"] = "\n".join([
        f"{s['commits']} commits, {s['prs']} merged PRs from {s['first_day']} to {s['as_of']}, "
        f"on {span} working days: {types}. About {s['added']:,} lines added and "
        f"{s['removed']:,} removed; {s['tracked_files']} tracked files.",
        "",
        "```mermaid",
        "xychart-beta",
        '    title "PRs merged per working day"',
        f"    x-axis [{labels}]",
        f'    y-axis "PRs" 0 --> {top + 2}',
        f"    bar [{counts}]",
        "```",
    ])
    v = s["views"]
    if v:
        kinds = collections.OrderedDict((k, []) for _, k in KINDS)
        for x in v:
            kinds.setdefault(x["kind"], []).append(x["mean"])
        rows = [f"| {k} | {len(m)} | {min(m):.2f}-{max(m):.2f} |" for k, m in kinds.items() if m]
        means = [x["mean"] for x in v]
        hist = [0] * len(BINS)
        for m in means:
            hist[next(i for i, (hi, _) in enumerate(BINS) if m < hi)] += 1
        under = lambda t: sum(m < t for m in means)
        worst = sorted(v, key=lambda x: -x["mean"])[:3]
        b["views"] = "\n".join([
            f"{len(v)} reference views, 1280x720, rendered by both engines and compared (mean "
            f"absolute difference per channel, 0-255; {s['views_measured']}):",
            "",
            "| Kind | Views | Mean difference |",
            "|------|------:|------|",
            *rows,
            f"| **All** | **{len(v)}** | **median {median(means):.2f}; {under(0.5)} under 0.5, "
            f"{under(1.0)} under 1.0** |",
            "",
            "```mermaid",
            "xychart-beta",
            f'    title "Views by mean difference ({len(v)} views)"',
            "    x-axis [" + ", ".join(f'"{l}"' for _, l in BINS) + "]",
            f'    y-axis "views" 0 --> {max(hist) + 3}',
            f"    bar [{', '.join(map(str, hist))}]",
            "```",
            "",
            "The largest: " + ", ".join(f"{x['view']} ({x['mean']:.2f})" for x in worst) + ".",
        ])
    rows = []
    for name, c in s["crates"].items():
        rows.append((name, c))
    b["crates"] = rows
    return b


def wrap(text, width):
    """Wraps prose lines as the hand-written text is; tables and charts stay as they are."""
    fenced = False
    out = []
    for line in text.splitlines():
        fenced ^= line.startswith("```")
        prose = not (fenced or line.startswith(("|", "```")))
        out.append(textwrap.fill(line, width) if prose and line else line)
    return "\n".join(out)


def apply(text, b):
    def table(old, rows):
        # Rewrites the line and test counts of the crate table; the prose columns stay.
        out = []
        for line in old.splitlines():
            m = re.match(r"\| `([\w-]+)` \|[^|]*\|[^|]*\|(.*)$", line)
            if m and m[1] in dict(rows):
                c = dict(rows)[m[1]]
                line = f"| `{m[1]}` | {c['lines']:,} | {c['tests']} |{m[2]}"
            out.append(line)
        names = {re.match(r"\| `([\w-]+)`", l)[1] for l in out if re.match(r"\| `([\w-]+)`", l)}
        for name, c in rows:
            if name not in names:
                out.append(f"| `{name}` | {c['lines']:,} | {c['tests']} | (describe) | |")
        return "\n".join(out)

    def sub(m):
        # A block inside a list item keeps the item's indentation.
        indent, name, body = m[1], m[2], m[3]
        if name == "crates":
            new = table(body.strip("\n"), b["crates"])
        else:
            new = b.get(name, body.strip("\n"))
        new = "\n".join(indent + l if l else l for l in wrap(new, 84 - len(indent)).splitlines())
        return f"{indent}<!-- progress:{name} -->\n{new}\n{indent}<!-- /progress -->"

    return re.sub(r"^([ \t]*)<!-- progress:(\w+) -->\n(.*?)^[ \t]*<!-- /progress -->", sub, text,
                  flags=re.S | re.M)


def main():
    s = stats()
    if "--json" in sys.argv:
        s["growth"] = growth(history()[0])
        pathlib.Path(sys.argv[sys.argv.index("--json") + 1]).write_text(json.dumps(s, indent=1))
        return
    old = OVERVIEW.read_text()
    new = apply(old, blocks(s))
    if "--check" in sys.argv:
        if new != old:
            sys.exit("docs/OVERVIEW.md's numbers are stale: run python3 scripts/progress.py")
        return
    OVERVIEW.write_text(new)


main()
