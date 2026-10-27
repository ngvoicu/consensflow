#!/usr/bin/env python3
"""Runs a command again and again under a bounded CPU load, and counts how often it fails.

    python3 tools/under-load.py --burners 28 --seconds 600 --runs 50 --copies 1 \
        [--stop-after 1] [--run-limit 300] [--log DIR] -- COMMAND ARG...

How a test that fails only on a loaded machine is reproduced, and shown mended (T-581: the load
of a gate that ran beside other trees' builds was 50 to 150 on 14 cores). Nothing here is
unbounded:

* The load is `--burners` perl processes (at most 64) that spin for `--seconds` and end by
  SIGALRM, so they end even if this script is killed; the script ends the ones it started when it
  ends, whatever ended it.
* Each round runs `--copies` copies of the command at once (at most 32), every one ended, with
  the processes it started, after `--run-limit` seconds. At most `--runs` rounds, and none begun
  after `--seconds` have passed. It stops after `--stop-after` rounds that failed (0: never).
* The output of the copies of a failed round is kept in `--log`, or printed.
* The load average is printed before every round, and a summary line at the end, so that every
  record of "N tries to fail" carries the load it was at.

A test binary of cargo is found with `cargo test --no-run`, and is run by its path with
`--exact <test name>`; running it by path leaves the build alone while it runs.
"""
import argparse
import os
import signal
import subprocess
import sys
import time

BURN = "alarm shift; 1 while 1"


def loadavg():
    return " ".join(f"{load:.1f}" for load in os.getloadavg())


def start_burners(count, seconds):
    return [
        subprocess.Popen(
            ["/usr/bin/perl", "-e", BURN, str(seconds)],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        for _ in range(count)
    ]


def stop_burners(burners):
    for burner in burners:
        if burner.poll() is None:
            burner.kill()
    for burner in burners:
        try:
            burner.wait(timeout=10)
        except subprocess.TimeoutExpired:
            pass


def start_copy(command):
    return subprocess.Popen(
        command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, start_new_session=True
    )


def finish_copy(process, limit):
    """How the copy ended: its code, whether its time ran out, its output."""
    try:
        output, _ = process.communicate(timeout=limit)
        timed_out = False
    except subprocess.TimeoutExpired:
        timed_out = True
        os.killpg(process.pid, signal.SIGKILL)
        output, _ = process.communicate()
    return process.returncode, timed_out, output.decode(errors="replace")


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--burners", type=int, default=28)
    parser.add_argument("--seconds", type=int, default=600)
    parser.add_argument("--runs", type=int, default=50)
    parser.add_argument("--copies", type=int, default=1)
    parser.add_argument("--stop-after", type=int, default=1)
    parser.add_argument("--run-limit", type=int, default=300)
    parser.add_argument("--log", default=None)
    parser.add_argument("command", nargs="+")
    args = parser.parse_args()
    if not 0 <= args.burners <= 64 or not 1 <= args.seconds <= 7200:
        sys.exit("burners 0..64, seconds 1..7200")
    if not 1 <= args.runs <= 1000 or not 1 <= args.copies <= 32:
        sys.exit("runs 1..1000, copies 1..32")
    if args.log:
        os.makedirs(args.log, exist_ok=True)
    began = time.monotonic()
    deadline = began + args.seconds
    burners = start_burners(args.burners, args.seconds)
    rounds, failed_rounds, copies_run, first = 0, 0, 0, None
    try:
        for number in range(1, args.runs + 1):
            if time.monotonic() >= deadline:
                break
            print(f"round {number}: load {loadavg()}", flush=True)
            started = time.monotonic()
            running = [start_copy(args.command) for _ in range(args.copies)]
            ended = [finish_copy(process, args.run_limit) for process in running]
            rounds += 1
            copies_run += args.copies
            bad = [(code, timed_out, output) for code, timed_out, output in ended if code != 0 or timed_out]
            took = time.monotonic() - started
            if not bad:
                print(f"round {number}: ok in {took:.1f}s", flush=True)
                continue
            failed_rounds += 1
            first = first or number
            print(f"round {number}: {len(bad)} of {args.copies} FAILED in {took:.1f}s", flush=True)
            for index, (code, timed_out, output) in enumerate(bad, 1):
                if args.log:
                    path = os.path.join(args.log, f"round-{number}-{index}.txt")
                    with open(path, "w") as out:
                        out.write(f"code={code} timed_out={timed_out}\n{output}")
                else:
                    print(f"--- code={code} timed_out={timed_out}\n{output[-3000:]}")
            if args.stop_after and failed_rounds >= args.stop_after:
                break
    finally:
        stop_burners(burners)
    print(
        f"SUMMARY rounds={rounds} copies_run={copies_run} failed_rounds={failed_rounds} "
        f"first_failed_round={first} burners={args.burners} load_end={loadavg()} "
        f"elapsed={time.monotonic() - began:.0f}s",
        flush=True,
    )
    sys.exit(1 if failed_rounds else 0)


if __name__ == "__main__":
    main()
