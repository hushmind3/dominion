#!/usr/bin/env python3
"""Independent, bounded parallel development cycles across DOMINION branches.

This runs locally, not inside ChatGPT. It never resets watched checkouts,
force-pushes, merges branches, launches daemons, or grants a judge write access.
"""
from __future__ import annotations

import argparse
import concurrent.futures
import datetime as dt
import json
import os
import contextlib
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import time

BRANCHES = ("main", "dominion-sub2", "dominion-sub3", "dominion-sub4")
ROLES = {"main": "메인", "dominion-sub2": "서브2", "dominion-sub3": "서브3", "dominion-sub4": "서브4"}


@contextlib.contextmanager
def singleton_lock(path: Path):
    """OS-released advisory lock: never run two coordinators on one state root."""
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a+") as f:
        if os.name == "nt":
            import msvcrt
            f.seek(0)
            try:
                msvcrt.locking(f.fileno(), msvcrt.LK_NBLCK, 1)
            except OSError as exc:
                raise RuntimeError("another DOMINION orchestrator is active") from exc
        else:
            import fcntl
            try:
                fcntl.flock(f.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError as exc:
                raise RuntimeError("another DOMINION orchestrator is active") from exc
        try:
            yield
        finally:
            if os.name == "nt":
                f.seek(0)
                msvcrt.locking(f.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                fcntl.flock(f.fileno(), fcntl.LOCK_UN)


def forbidden_file(path: str) -> bool:
    """Do not automatically publish common local credentials/state artifacts."""
    name = Path(path).name.lower()
    return (name in {".env", "auth.json", "id_rsa", "id_ed25519", ".npmrc", ".pypirc"}
            or name.startswith(".env.") and not name.endswith(".example")
            or name.endswith((".pem", ".p12", ".key", ".sqlite", ".sqlite3")))


def git(repo: Path, *args: str, check: bool = True) -> str:
    p = subprocess.run(["git", "-C", str(repo), *args], text=True, capture_output=True)
    if check and p.returncode:
        raise RuntimeError(f"git {' '.join(args)}: {p.stderr.strip() or p.stdout.strip()}")
    return p.stdout.strip()


def invoke(command: list[str], cwd: Path, log: Path, timeout: int, env: dict[str, str],
           *, separate_stderr: bool = False) -> tuple[int, bool]:
    log.parent.mkdir(parents=True, exist_ok=True)
    with log.open("w", encoding="utf-8") as stream:
        try:
            errfile = log.with_suffix(".stderr.log").open("w", encoding="utf-8") if separate_stderr else None
            p = subprocess.Popen(command, cwd=cwd, env=env, stdout=stream,
                                 stderr=errfile if errfile else subprocess.STDOUT,
                                 start_new_session=(os.name != "nt"))
            try:
                code = p.wait(timeout=timeout)
                if errfile: errfile.close()
                return code, False
            except subprocess.TimeoutExpired:
                if os.name != "nt":
                    import signal
                    os.killpg(p.pid, signal.SIGTERM)
                else:
                    p.terminate()
                try:
                    p.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    if os.name != "nt":
                        os.killpg(p.pid, signal.SIGKILL)
                    else:
                        p.kill()
                    p.wait()
                stream.write("\n[orchestrator] hard timeout: agent stopped\n")
                if errfile: errfile.close()
                return 124, True
        except OSError as e:
            stream.write(f"\n[orchestrator] process start failed: {e}\n")
            if "errfile" in locals() and errfile: errfile.close()
            return 127, False


def command_from_template(template: str | None, prompt: str, *, judge: bool = False) -> list[str]:
    if template:
        parts = shlex.split(template)
        if not parts or not any("{prompt}" in part for part in parts):
            raise ValueError("Command template must include {prompt} as an argument")
        return [part.replace("{prompt}", prompt) for part in parts]
    return ["codex", "exec", "--sandbox", "read-only" if judge else "workspace-write",
            "--ephemeral", prompt]


def prompt_for(branch: str, previous_feedback: str, base: str) -> str:
    return f"""You are the independent DOMINION {ROLES[branch]} developer, assigned ONLY branch {branch}.
This is an autonomous development cycle in a detached, isolated git worktree at {base[:12]}.
Your task is to independently identify the highest-value real user problem or missing capability,
trace its actual caller through Core/Harness/Strategy/Host/UI, implement a usable improvement,
remove newly obsolete paths, run targeted regressions, and commit only your own verified changes.
Do not wait for further user messages. Do not touch another branch or any sibling worktree.
Do not force-push, merge, reset user data, read private credentials, launch background daemons,
run paid model evaluations or modify external accounts. CI completion is not the goal.
Preserve the main UI design language and top-level navigation. Prefer actual user task completion
and evidence over cosmetic refactoring. No invented results or success claims.
Before editing, inspect current files and git status. Keep the repository clean and do not
stage unrelated, secret or generated files. End by reporting implementation, tests actually run,
remaining gaps, and commit SHA. If blocked, record the precise blocker.
Review the user's latest role rules if they exist in the repository.
Previous independent judge feedback (may be empty or unverified):
{previous_feedback[:12000] or '(first round; independently select the most valuable improvement)'}"""


def prepare(repo: Path, state: Path, branch: str, round_id: str) -> tuple[Path, str]:
    # Never checkout, reset, or mutate the user's watched application tree.
    remote_ref = f"refs/remotes/origin/{branch}"
    sha = git(repo, "rev-parse", "--verify", remote_ref)
    target = state / "worktrees" / round_id / branch
    if target.exists():
        raise RuntimeError(f"Worktree already exists; refusing to overwrite: {target}")
    target.parent.mkdir(parents=True, exist_ok=True)
    git(repo, "worktree", "add", "--detach", str(target), sha)
    return target, sha


def run_branch(repo: Path, state: Path, branch: str, round_id: str, feedback: str,
               timeout: int, template: str | None, publish: bool) -> dict:
    out = state / "runs" / round_id / branch
    out.mkdir(parents=True, exist_ok=True)
    result: dict = {"branch": branch, "round": round_id, "status": "not_started"}
    started = time.monotonic()
    try:
        target, base = prepare(repo, state, branch, round_id)
        result.update({"base": base, "worktree": str(target)})
        prompt = prompt_for(branch, feedback, base)
        (out / "prompt.txt").write_text(prompt, encoding="utf-8")
        command = command_from_template(template, prompt)
        env = {**os.environ, "DOMINION_BRANCH": branch, "DOMINION_ROLE": "developer"}
        code, timed_out = invoke(command, target, out / "agent.log", timeout, env)
        head = git(target, "rev-parse", "HEAD")
        porcelain = git(target, "status", "--porcelain")
        changes = git(target, "diff", "--name-only", f"{base}..HEAD").splitlines()
        risky = [path for path in changes if forbidden_file(path)]
        result.update({"exit_code": code, "timeout": timed_out, "head": head,
                       "risky_paths": risky,
                       "changed_commits": git(target, "rev-list", "--count", f"{base}..HEAD"),
                       "diff_stat": git(target, "diff", "--stat", f"{base}..HEAD"),
                       "dirty": bool(porcelain), "dirty_paths": porcelain[:2000]})
        # A successful agent process does not imply task success or clean code.
        if timed_out or code:
            result["status"] = "agent_failed"
        elif porcelain:
            result["status"] = "needs_review_uncommitted"
        elif risky:
            result["status"] = "needs_review_sensitive_paths"
        elif head == base:
            result["status"] = "no_commit"
        else:
            result["status"] = "committed_local"
            if publish:
                # Normal fast-forward-only push. A concurrently updated remote wins.
                push = subprocess.run(["git", "-C", str(target), "push", "origin",
                                       f"HEAD:refs/heads/{branch}"], text=True, capture_output=True)
                result["push_detail"] = (push.stderr or push.stdout)[-2000:]
                result["status"] = "published" if push.returncode == 0 else "push_conflict_or_error"
        result["elapsed_seconds"] = round(time.monotonic() - started, 1)
    except Exception as e:
        result.update({"status": "error", "error": str(e), "elapsed_seconds": round(time.monotonic() - started, 1)})
    (out / "result.json").write_text(json.dumps(result, indent=2, ensure_ascii=False), encoding="utf-8")
    return result


def judge_round(repo: Path, state: Path, round_id: str, results: list[dict],
                timeout: int, template: str | None) -> dict:
    folder = state / "runs" / round_id / "judge"
    folder.mkdir(parents=True, exist_ok=True)
    evidence = json.dumps(results, indent=2, ensure_ascii=False)
    prompt = f"""You are the independent DOMINION judge. Read-only. Never modify any branch or git file.
Judge 4 competing products against the same rubric (completion 20, run/recovery 15,
context/Room/Team 15, model/tool/SDK 10, architecture 10, UI/UX 15, measured cost/latency 10,
frontier implementation 5). CI status and code volume confer ZERO bonus.
Source evidence for round {round_id}: {evidence}
You may inspect each listed detached worktree's code and the actual agent logs.
Do not invent runtime successes, numerical grades, costs or UI improvements: those require
common scenarios and real results. Distinguish code review, tests, and live-model E2E.
Return exactly one JSON object with keys:
"summary" (string), "feedback" (object mapping all four branch names to concrete actionable
Korean feedback), "verified" (array), "missing_evidence" (array), "recommendations" (array).
Prioritize branch-specific next improvements and note any severe safety risks. No markdown fences.
"""
    (folder / "prompt.txt").write_text(prompt, encoding="utf-8")
    # Judge sees the same frozen source snapshot, never a watched checkout.
    judge_tree = state / "worktrees" / round_id / "judge"
    base = next((r.get("base") for r in results if r.get("branch") == "main" and r.get("base")), None)
    if not base:
        report = {"round": round_id, "status": "judge_skipped_missing_main_snapshot", "feedback": {}}
        (folder / "report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
        return report
    git(repo, "worktree", "add", "--detach", str(judge_tree), base)
    cmd = command_from_template(template, prompt, judge=True)
    env = {**os.environ, "DOMINION_ROLE": "judge"}
    code, timed_out = invoke(cmd, judge_tree, folder / "judge.log", timeout, env, separate_stderr=True)
    raw = (folder / "judge.log").read_text(encoding="utf-8", errors="replace")
    # CLI final output is normally stdout; strip only well-formed fences.
    cleaned = raw.strip()
    if cleaned.startswith("```"):
        cleaned = cleaned.split("\n", 1)[-1].rsplit("```", 1)[0].strip()
    report: dict = {"round": round_id, "exit_code": code, "timeout": timed_out,
                    "status": "unverified_judge_output", "feedback": {}}
    if code == 0 and not timed_out:
        try:
            parsed = json.loads(cleaned)
            if isinstance(parsed, dict) and isinstance(parsed.get("feedback"), dict):
                report.update({"status": "judge_feedback_recorded", "analysis": parsed,
                               "feedback": {b: str(parsed["feedback"].get(b, "")) for b in BRANCHES}})
        except (TypeError, ValueError):
            pass
    (folder / "report.json").write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding="utf-8")
    return report


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description="4-branch DOMINION development + independent judge, one command")
    p.add_argument("--repo", required=True, type=Path, help="Git clone containing all origin branch refs")
    p.add_argument("--state", type=Path, help="Outside-repo durable logs/worktrees; defaults to adjacent directory")
    p.add_argument("--rounds", type=int, default=1, help="1=one full cycle; 0=run cycles until interrupted")
    p.add_argument("--parallel", type=int, default=4, choices=(1, 2, 3, 4))
    p.add_argument("--timeout", type=int, default=2700, help="Per agent and judge timeout in seconds")
    p.add_argument("--cooldown", type=int, default=30, help="Seconds between rounds; 0 disables delay")
    p.add_argument("--agent-command", help='Optional argv template, e.g. "codex exec --sandbox workspace-write {prompt}"')
    p.add_argument("--judge-command", help='Optional argv template, e.g. "codex exec --sandbox read-only {prompt}"')
    p.add_argument("--publish", action="store_true", help="Fast-forward push each committed branch, never force/merge")
    p.add_argument("--skip-judge", action="store_true")
    args = p.parse_args(argv)
    if args.rounds < 0 or args.timeout < 5 or args.cooldown < 0:
        p.error("--rounds must be >= 0, --timeout >= 5, --cooldown >= 0")
    repo = args.repo.resolve()
    if not repo.is_dir() or git(repo, "rev-parse", "--show-toplevel") != str(repo):
        p.error("--repo must be the top-level directory of a Git checkout")
    state = (args.state or (repo.parent / "dominion-autopilot-state")).resolve()
    if state == repo or repo in state.parents:
        p.error("--state must be outside the target repository")
    state.mkdir(parents=True, exist_ok=True)
    for t in (args.agent_command, args.judge_command):
        if t:
            command_from_template(t, "probe")
    if not args.agent_command and shutil.which("codex") is None:
        p.error("codex not found; install/sign in to Codex CLI or set --agent-command")
    if not args.skip_judge and not args.judge_command and shutil.which("codex") is None:
        p.error("codex not found for read-only judge; set --judge-command or --skip-judge")
    iteration = 0
    try:
        with singleton_lock(state / "autopilot.lock"):
            while args.rounds == 0 or iteration < args.rounds:
                iteration += 1
                round_id = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
                print(f"[DOMINION] round {iteration}: {round_id}", flush=True)
                # Only fetch changes; never update a checked-out branch.
                git(repo, "fetch", "--no-tags", "origin", *(f"+refs/heads/{b}:refs/remotes/origin/{b}" for b in BRANCHES))
                previous = state / "latest_feedback.json"
                feedback = json.loads(previous.read_text(encoding="utf-8")) if previous.is_file() else {}
                with concurrent.futures.ThreadPoolExecutor(max_workers=args.parallel) as pool:
                    futures = [pool.submit(run_branch, repo, state, b, round_id,
                                           str(feedback.get(b, "")), args.timeout,
                                           args.agent_command, args.publish) for b in BRANCHES]
                    results = [future.result() for future in futures]
                print("[DOMINION] " + " | ".join(f"{r['branch']}:{r['status']}" for r in results), flush=True)
                if not args.skip_judge:
                    report = judge_round(repo, state, round_id, results, args.timeout, args.judge_command)
                    if report["status"] == "judge_feedback_recorded":
                        previous.write_text(json.dumps(report["feedback"], ensure_ascii=False, indent=2), encoding="utf-8")
                    print(f"[DOMINION] judge: {report['status']}", flush=True)
                (state / "latest_round.json").write_text(json.dumps({"id": round_id, "results": results},
                                                                      ensure_ascii=False, indent=2), encoding="utf-8")
                if (args.rounds == 0 or iteration < args.rounds) and args.cooldown:
                    time.sleep(args.cooldown)
    except KeyboardInterrupt:
        print("[DOMINION] interrupted; existing worktrees/logs retained for review", file=sys.stderr)
        return 130
    except Exception as e:
        print(f"[DOMINION] stopped: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())