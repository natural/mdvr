#!/usr/bin/env python3
import re
import statistics
import subprocess
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BINARY = ROOT / "target/release/mdvr"
RUNS = 5

if not BINARY.is_file():
    raise SystemExit(f"release binary missing: {BINARY}")

with tempfile.TemporaryDirectory(prefix="mdvr-reload-") as directory:
    document = Path(directory) / "reload.md"
    log_path = Path(directory) / "mdvr.log"
    source = "# Reload performance\n\n" + "".join(
        f"Paragraph {i}: " + "readable markdown text " * 20 + "\n\n"
        for i in range(2300)
    )
    source = source[: 1024 * 1024]
    document.write_text(source)
    with log_path.open("w") as log:
        process = subprocess.Popen([BINARY, document], stdout=log, stderr=log)
    try:
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if "rendered generation" in log_path.read_text():
                break
            time.sleep(0.02)
        else:
            raise SystemExit("initial render timed out")

        seen = 1
        totals = []
        for run in range(RUNS):
            started = time.monotonic()
            document.write_text(source + f"\n<!-- reload {run} -->\n")
            deadline = started + 10
            while time.monotonic() < deadline:
                count = log_path.read_text().count("rendered generation")
                if count > seen:
                    seen = count
                    totals.append((time.monotonic() - started) * 1000)
                    break
                time.sleep(0.01)
            else:
                raise SystemExit(f"reload {run + 1} timed out")
            time.sleep(0.25)
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()

    try:
        native = [
            float(value)
            for value in re.findall(
                r"rendered generation \d+ in ([\d.]+) ms", log_path.read_text()
            )
        ][-RUNS:]
    except ValueError as error:
        raise SystemExit(f"invalid render timing: {error}") from error
    if len(native) != RUNS:
        raise SystemExit(f"expected {RUNS} render timings, found {len(native)}")
    print(f"fixture_bytes={document.stat().st_size} runs={RUNS}")
    print("commit_to_ready_ms=" + ",".join(f"{value:.1f}" for value in native))
    print(f"commit_to_ready_median_ms={statistics.median(native):.1f}")
    print("save_to_ready_ms=" + ",".join(f"{value:.1f}" for value in totals))
    print(f"save_to_ready_median_ms={statistics.median(totals):.1f}")
