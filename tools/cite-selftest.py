#!/usr/bin/env python3
"""Self-test of `releaseant cite-update` on real repositories' origin/main.

usage: cite-selftest.py <releaseant-binary> <repo-dir> [<repo-dir> ...]

For each repo: `git archive origin/main` into a scratch directory (the published repo is never touched),
pretend the minor bump happened (every `[package]`/`[workspace.package]` version line moves from X.Y.Z to
X.(Y+1).0), run `cite-update` dry-run, and print the summary and the diff for review. A repo with no
citation of its own X.Y is reported as such (that is a result, not a pass).
"""
import io, os, re, shutil, subprocess, sys, tarfile, tempfile


def safe_extract(data, dest):
    """Write the regular files and directories of a tar (our own `git archive`) under `dest`, each path
    checked to stay inside it; links, devices and anything else are skipped. Nothing is handed to
    `tarfile.extract*`, so a member name can never choose where a file lands."""
    root = os.path.realpath(dest)
    with tarfile.open(fileobj=io.BytesIO(data)) as t:
        for m in t.getmembers():
            target = os.path.realpath(os.path.join(root, m.name))
            if target != root and not target.startswith(root + os.sep):
                raise SystemExit("refusing a tar member outside the target: " + m.name)
            if m.isdir():
                os.makedirs(target, exist_ok=True)
            elif m.isfile():
                os.makedirs(os.path.dirname(target), exist_ok=True)
                src = t.extractfile(m)
                with open(target, "wb") as out:
                    shutil.copyfileobj(src, out)


def main():
    exe, repos = sys.argv[1], sys.argv[2:]
    for repo in repos:
        subprocess.run(["git", "-C", repo, "fetch", "-q", "origin", "main"], check=True)
        sha = subprocess.run(["git", "-C", repo, "rev-parse", "origin/main"], check=True, capture_output=True, text=True).stdout.strip()
        tar = subprocess.run(["git", "-C", repo, "archive", sha], check=True, capture_output=True).stdout
        d = tempfile.mkdtemp(prefix="cite-selftest-")
        try:
            safe_extract(tar, d)
            cur = None
            for dp, dn, fn in os.walk(d):
                if "Cargo.toml" in fn:
                    p = os.path.join(dp, "Cargo.toml")
                    t = open(p, encoding="utf-8", newline="").read()
                    m = re.search(r'(?m)^version\s*=\s*"(\d+)\.(\d+)\.(\d+)"', t)
                    if m and cur is None:
                        cur = (int(m.group(1)), int(m.group(2)), m.group(0))
            if cur is None:
                print(f"== {repo}: no own version line found"); continue
            old = f"{cur[0]}.{cur[1]}"
            new = f"{cur[0]}.{cur[1] + 1}"
            for dp, dn, fn in os.walk(d):
                if "Cargo.toml" in fn:
                    p = os.path.join(dp, "Cargo.toml")
                    t = open(p, encoding="utf-8", newline="").read()
                    # only the package/workspace.package version lines, not dependency tables
                    t2 = re.sub(r'(?m)^version\s*=\s*"%s\.\d+"' % re.escape(old), 'version = "%s.0"' % new, t)
                    open(p, "w", encoding="utf-8", newline="").write(t2)
            r = subprocess.run([exe, "cite-update", "--root", d, "--old", old, "--new", new, "--served", old + ".0"], capture_output=True, text=True, encoding="utf-8", errors="replace")
            print(f"== {repo} @ {sha[:8]}: pretend {old} -> {new}; exit {r.returncode}")
            print(r.stdout[:20000].replace(d.replace("\\", "/"), "<tree>"))
            print(r.stderr[:2000])
        finally:
            shutil.rmtree(d, ignore_errors=True)


if __name__ == "__main__":
    main()
