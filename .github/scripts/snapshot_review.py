#!/usr/bin/env python3
"""Show the failing visual tests of a pull request in a PR comment.

Runs in .github/workflows/visual-report.yml, after the CI workflow finished for a pull request.
That workflow runs with write permissions, also for pull requests from forks, so everything
read from the CI run's artifacts is untrusted data: this script only parses it, never runs it.

Contract for a visual test suite, in the CI workflow (see the rust-tests job):
upload an artifact named `snapshot-review-<slug>` (lowercase letters, digits and dashes) on every
pull request run where the visual tests ran, whether they passed or not. Its top level holds:

- report.json: {"version": 1, "suite": "<display name>", "outcome": "success" | "failure"}
- for each failing snapshot <name>: <name>.new.png (the image the test rendered) and, if
  available, <name>.png (the accepted snapshot) and <name>.diff.png (the difference).

Other files are ignored. The comment shows at most MAX_SHOWN failures per suite and links the
artifact for the rest. The images it shows are committed to the branch BRANCH, which this script
rewrites as a single commit each time: it holds only the latest images of each open pull request.

Commands:
  report  after a CI run of a pull request: publish its images and write or update the comment
  prune   remove the images of closed pull requests from BRANCH

Run it inside a checkout of the repository. Needs `git` and the GitHub CLI `gh`, with GH_TOKEN
set; `git push` authenticates through `gh` as well.
"""

from __future__ import annotations

import argparse
import dataclasses
import io
import json
import os
import re
import subprocess
import sys
import tempfile
import zipfile
from collections.abc import Callable

BRANCH = "ci-snapshot-review"
MARKER = "<!-- snapshot-review -->"
BOT_LOGIN = "github-actions[bot]"
MAX_SHOWN = 5
MAX_ARTIFACT_BYTES = 200 * 2**20
MAX_FILE_BYTES = 20 * 2**20
MAX_FILES = 1000
COMMENT_LIMIT = 65536
PNG_MAGIC = b"\x89PNG\r\n\x1a\n"

ARTIFACT_RE = re.compile(r"^snapshot-review-([a-z0-9]+(?:-[a-z0-9]+)*)$")
FILE_RE = re.compile(r"^[A-Za-z0-9_][A-Za-z0-9_.-]{0,150}$")
PR_DIR_RE = re.compile(r"^pr-(\d+)/")

BRANCH_README = b"""\
# Visual test review images

This branch is written by `.github/workflows/visual-report.yml` and rewritten as a single
commit on every update. It holds the images of failing visual tests shown in pull request
comments, in `pr-<number>/<run id>-<attempt>/<suite>/`, and only for open pull requests.
Don't commit to it.
"""


class InvalidArtifact(Exception):
    pass


@dataclasses.dataclass
class Failure:
    name: str
    new: bytes
    reference: bytes | None
    diff: bytes | None


@dataclasses.dataclass
class Suite:
    slug: str
    title: str
    outcome: str
    failures: list[Failure]
    artifact_name: str
    artifact_url: str

    @property
    def shown(self) -> list[Failure]:
        return self.failures[:MAX_SHOWN]


# ---- parsing the artifacts ---------------------------------------------------------------------


def read_artifact_zip(data: bytes) -> dict[str, bytes]:
    """Return the top-level files of an artifact zip, skipping anything unexpected."""
    files: dict[str, bytes] = {}
    with zipfile.ZipFile(io.BytesIO(data)) as zf:
        for info in zf.infolist()[:MAX_FILES]:
            if info.is_dir() or not FILE_RE.match(info.filename):
                continue
            if info.file_size > MAX_FILE_BYTES:
                continue
            with zf.open(info) as f:
                content = f.read(MAX_FILE_BYTES + 1)
            if len(content) <= MAX_FILE_BYTES:
                files[info.filename] = content
    return files


def clean_title(title: str, fallback: str) -> str:
    cleaned = re.sub(r"[^\w .()+/-]", "", title)[:60].strip()
    return cleaned or fallback


def parse_artifact(
    slug: str, files: dict[str, bytes], artifact_name: str, artifact_url: str
) -> Suite:
    try:
        report = json.loads(files["report.json"])
    except KeyError as e:
        raise InvalidArtifact("report.json is missing") from e
    except ValueError as e:
        raise InvalidArtifact("report.json is not valid JSON") from e
    if not isinstance(report, dict) or report.get("version") != 1:
        raise InvalidArtifact("report.json has no supported version (expected 1)")
    outcome = report.get("outcome")
    if outcome not in ("success", "failure"):
        raise InvalidArtifact('report.json: outcome must be "success" or "failure"')
    title = report.get("suite")
    if not isinstance(title, str):
        raise InvalidArtifact("report.json: suite must be a string")

    failures = []
    if outcome == "failure":
        pngs = {
            name: data
            for name, data in files.items()
            if name.endswith(".png") and data.startswith(PNG_MAGIC)
        }
        names = sorted(
            name.removesuffix(".new.png") for name in pngs if name.endswith(".new.png")
        )
        failures = [
            Failure(
                name=name,
                new=pngs[f"{name}.new.png"],
                reference=pngs.get(f"{name}.png"),
                diff=pngs.get(f"{name}.diff.png"),
            )
            for name in names
        ]
    return Suite(
        slug=slug,
        title=clean_title(title, fallback=slug),
        outcome=outcome,
        failures=failures,
        artifact_name=artifact_name,
        artifact_url=artifact_url,
    )


# ---- images on the branch ----------------------------------------------------------------------


def image_path(run_dir: str, suite: Suite, failure: Failure, kind: str) -> str:
    suffix = {"reference": ".png", "new": ".new.png", "diff": ".diff.png"}[kind]
    return f"{run_dir}/{suite.slug}/{failure.name}{suffix}"


def published_files(run_dir: str, suites: list[Suite]) -> dict[str, bytes]:
    """The images shown in the comment, by their path on the branch."""
    files = {}
    for suite in suites:
        for failure in suite.shown:
            for kind in ("reference", "new", "diff"):
                data = getattr(failure, kind)
                if data is not None:
                    files[image_path(run_dir, suite, failure, kind)] = data
    return files


def base_tree(
    tree: dict[str, str | bytes], open_pulls: set[int], drop_pull: int | None = None
) -> dict[str, str | bytes]:
    """Keep the images of open pull requests (except `drop_pull`) and nothing else."""
    kept: dict[str, str | bytes] = {"README.md": BRANCH_README}
    for path, blob in tree.items():
        m = PR_DIR_RE.match(path)
        if m and int(m[1]) in open_pulls and int(m[1]) != drop_pull:
            kept[path] = blob
    return kept


# ---- the comment -------------------------------------------------------------------------------


def image_url(repo: str, path: str) -> str:
    return f"https://raw.githubusercontent.com/{repo}/{BRANCH}/{path}"


def plural(n: int, word: str) -> str:
    return f"{n} {word}" if n == 1 else f"{n} {word}s"


def render_suite(repo: str, run_dir: str, suite: Suite) -> list[str]:
    if suite.outcome == "success":
        return [f"#### ✅ {suite.title}: all visual tests pass", ""]
    if not suite.failures:
        return [
            f"#### ❌ {suite.title}: visual tests failed, but no snapshot differs",
            "",
            "See the test log of the CI run.",
            "",
        ]

    def img(failure: Failure, kind: str) -> str:
        if getattr(failure, kind) is None:
            return "(none)"
        url = image_url(repo, image_path(run_dir, suite, failure, kind))
        label = {"reference": "accepted", "new": "new", "diff": "difference"}[kind]
        return f'<img src="{url}" alt="{failure.name}: {label}">'

    total, shown = len(suite.failures), suite.shown
    summary = (
        f"Show {len(shown)} of {total}" if total > len(shown) else "Show the images"
    )
    lines = [
        f"#### ❌ {suite.title}: {plural(total, 'failing snapshot')}",
        "",
        "<details>",
        f"<summary>{summary}</summary>",
        "",
    ]
    for failure in shown:
        lines += [f"##### `{failure.name}`", ""]
        if failure.diff is None:
            lines += ["Difference: none in the artifact", ""]
        else:
            lines += ["Difference:", "", img(failure, "diff"), ""]
        lines += [
            "| Accepted | New |",
            "| --- | --- |",
            f"| {img(failure, 'reference')} | {img(failure, 'new')} |",
            "",
        ]
    lines += ["</details>", ""]
    artifact = f"[{suite.artifact_name}]({suite.artifact_url})"
    if total > len(shown):
        more = plural(total - len(shown), "more failure")
        lines.append(f"{more} and all images are in the artifact {artifact}.")
    else:
        lines.append(f"All images are in the artifact {artifact}.")
    lines.append("")
    return lines


def render_comment(
    repo: str, head_sha: str, run_url: str, run_dir: str, suites: list[Suite]
) -> str:
    if any(s.outcome == "failure" for s in suites):
        lines = [MARKER, f"### ❌ Visual tests failed in {head_sha}", ""]
    else:
        lines = [MARKER, f"### ✅ Visual tests pass as of {head_sha}", ""]
    for suite in sorted(suites, key=lambda s: s.title):
        lines += render_suite(repo, run_dir, suite)
    lines.append(f"[CI run]({run_url})")
    body = "\n".join(lines) + "\n"
    if len(body) > COMMENT_LIMIT:
        note = f"\n\nThis comment was cut off at GitHub's size limit. See the [CI run]({run_url}).\n"
        body = body[: COMMENT_LIMIT - len(note)] + note
    return body


# ---- git and GitHub ----------------------------------------------------------------------------


def run(
    *args: str,
    input: bytes | None = None,
    env: dict[str, str] | None = None,
    check: bool = True,
) -> bytes:
    result = subprocess.run(
        args, input=input, stdout=subprocess.PIPE, env=env, check=False
    )
    if check and result.returncode != 0:
        raise SystemExit(
            f"Command failed with exit code {result.returncode}: {' '.join(args[:3])} ..."
        )
    return result.stdout


def gh_api(path: str, *args: str, input: bytes | None = None) -> bytes:
    return run("gh", "api", *args, path, input=input)


def gh_lines(path: str, jq: str) -> list[list[str]]:
    out = gh_api(path, "--paginate", "--jq", jq).decode()
    return [line.split("\t") for line in out.splitlines() if line]


def open_pulls(repo: str) -> list[tuple[int, str, str]]:
    """(number, head sha, head repository) of each open pull request."""
    rows = gh_lines(
        f"repos/{repo}/pulls?state=open&per_page=100",
        '.[] | [.number, .head.sha, (.head.repo.full_name // "")] | @tsv',
    )
    return [(int(number), sha, head_repo) for number, sha, head_repo in rows]


def load_suites(repo: str, run_id: str) -> list[Suite]:
    rows = gh_lines(
        f"repos/{repo}/actions/runs/{run_id}/artifacts?per_page=100",
        ".artifacts[] | [.id, .name, .size_in_bytes, .expired] | @tsv",
    )
    latest: dict[str, tuple[int, int]] = {}
    for artifact_id, name, size, expired in rows:
        if expired != "false" or not ARTIFACT_RE.match(name):
            continue
        # a re-run can leave artifacts of the same name from several attempts
        if name not in latest or int(artifact_id) > latest[name][0]:
            latest[name] = (int(artifact_id), int(size))

    suites = []
    for name, (artifact_id, size) in sorted(latest.items()):
        if size > MAX_ARTIFACT_BYTES:
            print(
                f"::warning::Ignoring artifact {name}: larger than {MAX_ARTIFACT_BYTES} bytes"
            )
            continue
        data = gh_api(f"repos/{repo}/actions/artifacts/{artifact_id}/zip")
        url = f"https://github.com/{repo}/actions/runs/{run_id}/artifacts/{artifact_id}"
        slug = ARTIFACT_RE.match(name)[1]
        try:
            suites.append(parse_artifact(slug, read_artifact_zip(data), name, url))
        except zipfile.BadZipFile:
            print(f"::warning::Ignoring artifact {name}: not a valid zip file")
        except InvalidArtifact as e:
            print(f"::warning::Ignoring artifact {name}: {e}")
    return suites


def find_comment(repo: str, pull: int) -> str | None:
    rows = gh_lines(
        f"repos/{repo}/issues/{pull}/comments?per_page=100",
        f'.[] | select(.user.login == "{BOT_LOGIN}" and (.body | startswith("{MARKER}"))) | [.id] | @tsv',
    )
    return rows[0][0] if rows else None


def write_comment(repo: str, pull: int, comment_id: str | None, body: str) -> None:
    payload = json.dumps({"body": body}).encode()
    if comment_id is None:
        gh_api(
            f"repos/{repo}/issues/{pull}/comments",
            "--method",
            "POST",
            "--input",
            "-",
            input=payload,
        )
        print(f"Commented on pull request #{pull}")
    else:
        gh_api(
            f"repos/{repo}/issues/comments/{comment_id}",
            "--method",
            "PATCH",
            "--input",
            "-",
            input=payload,
        )
        print(f"Updated the comment on pull request #{pull}")


def git(
    *args: str, input: bytes | None = None, env: dict[str, str] | None = None
) -> str:
    return run("git", *args, input=input, env=env).decode().strip()


def fetch_branch() -> tuple[str | None, dict[str, str | bytes]]:
    """The commit BRANCH points to on origin, and its files (path -> blob id)."""
    if not git("ls-remote", "origin", f"refs/heads/{BRANCH}"):
        return None, {}
    git(
        "fetch",
        "--quiet",
        "--no-tags",
        "--depth=1",
        "origin",
        f"+refs/heads/{BRANCH}:refs/snapshot-review/old",
    )
    commit = git("rev-parse", "refs/snapshot-review/old")
    tree: dict[str, str | bytes] = {}
    for entry in git("ls-tree", "-r", "-z", commit).split("\0"):
        if entry:
            meta, path = entry.split("\t", 1)
            tree[path] = meta.split()[2]
    return commit, tree


def store_blobs(tree: dict[str, str | bytes]) -> dict[str, str]:
    """Store the contents in `tree` (path -> blob id or content) as blobs; return their ids."""
    return {
        path: git("hash-object", "-w", "--stdin", input=blob)
        if isinstance(blob, bytes)
        else blob
        for path, blob in tree.items()
    }


def write_commit(tree: dict[str, str], message: str) -> str:
    """Write `tree` (path -> blob id) as a commit without parents."""
    index_info = [f"100644 {blob}\t{path}\n" for path, blob in sorted(tree.items())]
    with tempfile.TemporaryDirectory() as tmp:
        env = {**os.environ, "GIT_INDEX_FILE": os.path.join(tmp, "index")}
        git("update-index", "--index-info", input="".join(index_info).encode(), env=env)
        tree_id = git("write-tree", env=env)
    env = {
        **os.environ,
        "GIT_AUTHOR_NAME": BOT_LOGIN,
        "GIT_AUTHOR_EMAIL": "41898282+github-actions[bot]@users.noreply.github.com",
    }
    env["GIT_COMMITTER_NAME"], env["GIT_COMMITTER_EMAIL"] = (
        env["GIT_AUTHOR_NAME"],
        env["GIT_AUTHOR_EMAIL"],
    )
    return git("commit-tree", tree_id, "-m", message, env=env)


def update_branch(
    change: Callable[[dict[str, str | bytes]], dict[str, str | bytes]], message: str
) -> None:
    """Replace BRANCH with a single commit holding `change(current files)`.

    Another run may push in between; then the lease fails and the change is applied again.
    """
    for attempt in range(1, 4):
        old, tree = fetch_branch()
        new = store_blobs(change(dict(tree)))
        if old is not None and new == tree:
            print(f"Branch {BRANCH} is up to date")
            return
        commit = write_commit(new, message)
        lease = f"--force-with-lease=refs/heads/{BRANCH}:{old or ''}"
        pushed = subprocess.run(
            [
                "git",
                # authenticate with GH_TOKEN, so the checkout doesn't need to store credentials
                "-c",
                "credential.helper=",
                "-c",
                "credential.helper=!gh auth git-credential",
                "push",
                "--quiet",
                lease,
                "origin",
                f"{commit}:refs/heads/{BRANCH}",
            ],
            check=False,
        )
        if pushed.returncode == 0:
            print(f"Updated branch {BRANCH}: {message}")
            return
        print(
            f"Push to {BRANCH} failed (attempt {attempt} of 3), probably a concurrent update; retrying"
        )
    raise SystemExit(f"Could not update branch {BRANCH}")


# ---- commands ----------------------------------------------------------------------------------


def env(name: str) -> str:
    value = os.environ.get(name)
    if not value:
        raise SystemExit(f"Environment variable {name} is not set")
    return value


def cmd_report() -> None:
    repo, run_id, attempt = env("GITHUB_REPOSITORY"), env("RUN_ID"), env("RUN_ATTEMPT")
    head_sha, head_repo, run_url = env("HEAD_SHA"), env("HEAD_REPO"), env("RUN_URL")

    pulls = open_pulls(repo)
    matches = [
        number
        for number, sha, pr_repo in pulls
        if sha == head_sha and pr_repo == head_repo
    ]
    if not matches:
        print(
            f"No open pull request has the head {head_sha} (any more); nothing to report"
        )
        return
    suites = load_suites(repo, run_id)
    if not suites:
        print(f"Run {run_id} has no snapshot-review-* artifacts; nothing to report")
        return

    open_numbers = {number for number, _, _ in pulls}
    for pull in matches:
        run_dir = f"pr-{pull}/{run_id}-{attempt}"
        files = published_files(run_dir, suites)
        update_branch(
            lambda tree, pull=pull, files=files: {
                **base_tree(tree, open_numbers, drop_pull=pull),
                **files,
            },
            f"Images of failing visual tests for pull request #{pull}, run {run_id}",
        )
        comment_id = find_comment(repo, pull)
        if comment_id is None and all(s.outcome == "success" for s in suites):
            print(
                f"Visual tests pass and pull request #{pull} has no comment yet; not commenting"
            )
            continue
        write_comment(
            repo,
            pull,
            comment_id,
            render_comment(repo, head_sha, run_url, run_dir, suites),
        )


def cmd_prune() -> None:
    repo = env("GITHUB_REPOSITORY")
    open_numbers = {number for number, _, _ in open_pulls(repo)}
    update_branch(
        lambda tree: base_tree(tree, open_numbers),
        "Remove images of closed pull requests",
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("command", choices=["report", "prune"])
    args = parser.parse_args()
    {"report": cmd_report, "prune": cmd_prune}[args.command]()


if __name__ == "__main__":
    sys.exit(main())
