import csv
import hashlib
import io
import json
import os
import platform
import re
import struct
import subprocess
import time
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path


def require_runner(kind, environment=None, system=None):
    environment = os.environ if environment is None else environment
    system = platform.system() if system is None else system
    if environment.get("GITHUB_ACTIONS") != "true" or environment.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise RuntimeError("Native lifecycle requires a disposable GitHub-hosted runner")
    if {"linux": "Linux", "macos": "Darwin"}.get(kind) != system:
        raise RuntimeError("Native lifecycle platform mismatch")


def contained_path(base, target):
    base = Path(base).resolve(strict=True)
    target = Path(target).resolve()
    if target == base or base not in target.parents:
        raise RuntimeError("Lifecycle path escaped its temporary directory")
    return target


def file_sha256(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as handle:
        while block := handle.read(1024 * 1024):
            digest.update(block)
    return digest.hexdigest()


def run(arguments, *, input_data=None, check=True, timeout=60):
    result = subprocess.run([str(value) for value in arguments], input=input_data, capture_output=True, text=True, timeout=timeout)
    if check and result.returncode:
        raise RuntimeError(f"{Path(str(arguments[0])).name} failed ({result.returncode}): {result.stderr.strip() or result.stdout.strip()}")
    return result


@dataclass(frozen=True)
class Word:
    text: str
    left: int
    top: int
    width: int
    height: int
    line: tuple

    @property
    def center(self):
        return (self.left + self.width / 2, self.top + self.height / 2)


def normalized(value):
    return re.sub(r"[^a-z0-9]", "", value.lower())


def read_words(tsv, scale=1):
    if not isinstance(scale, int) or scale < 1:
        raise RuntimeError("Invalid screenshot OCR scale")
    words = []
    for row in csv.DictReader(io.StringIO(tsv), delimiter="\t"):
        if row["level"] != "5" or not row["text"].strip() or float(row["conf"]) < 25:
            continue
        dimensions = [int(row[key]) for key in ["left", "top", "width", "height"]]
        if any(value < 0 for value in dimensions) or not dimensions[2] or not dimensions[3]:
            raise RuntimeError("Invalid screenshot text coordinates")
        line = tuple(int(row[key]) for key in ["page_num", "block_num", "par_num", "line_num"])
        dimensions = [round(dimensions[0] / scale), round(dimensions[1] / scale),
                      max(1, round(dimensions[2] / scale)), max(1, round(dimensions[3] / scale))]
        words.append(Word(row["text"].strip(), *dimensions, line))
    return words


def phrase_boxes(words, phrase):
    wanted = [normalized(token) for token in phrase.split()]
    grouped = defaultdict(list)
    for word in words:
        grouped[word.line].append(word)
    matches = []
    for line in grouped.values():
        line.sort(key=lambda word: word.left)
        for offset in range(len(line) - len(wanted) + 1):
            tokens = line[offset:offset + len(wanted)]
            if [normalized(token.text) for token in tokens] != wanted:
                continue
            left = min(token.left for token in tokens)
            top = min(token.top for token in tokens)
            right = max(token.left + token.width for token in tokens)
            bottom = max(token.top + token.height for token in tokens)
            matches.append(Word(phrase, left, top, right - left, bottom - top, tokens[0].line))
    return matches


class MissingControl(RuntimeError):
    pass


class AmbiguousControl(RuntimeError):
    pass


def select_control(words, label, width, height, region=(0, 0, 1, 1), anchor=None):
    left, top, right, bottom = region
    matches = [box for box in phrase_boxes(words, label)
               if left * width <= box.center[0] < right * width and top * height <= box.center[1] < bottom * height
               and box.left >= 0 and box.top >= 0 and box.left + box.width <= width and box.top + box.height <= height]
    if anchor:
        anchors = phrase_boxes(words, anchor)
        matches = [box for box in matches if any(
            0.20 * width <= origin.center[0] <= 0.65 * width and origin.center[0] < box.left
            and abs(origin.center[1] - box.center[1]) <= max(24 * height / 760, origin.height * 2)
            for origin in anchors)]
    if not matches:
        raise MissingControl(f"Visible control not found: {label}" + (f" on {anchor} row" if anchor else ""))
    if len(matches) != 1:
        raise AmbiguousControl(f"Ambiguous visible control: {label} ({len(matches)} candidates)")
    return matches[0]


def png_dimensions(path):
    with Path(path).open("rb") as handle:
        header = handle.read(24)
    if header[:8] != b"\x89PNG\r\n\x1a\n" or header[12:16] != b"IHDR":
        raise RuntimeError("Expected a real PNG screenshot")
    width, height = struct.unpack(">II", header[16:24])
    if not (600 <= width <= 6000 and 400 <= height <= 4000):
        raise RuntimeError("Unexpected native window screenshot dimensions")
    return width, height


def descendants(process_id, text):
    parents = {}
    for line in text.splitlines():
        fields = line.split()
        if len(fields) == 2:
            parents[int(fields[0])] = int(fields[1])
    owned = {process_id}
    while True:
        found = {child for child, parent in parents.items() if parent in owned}
        updated = owned | found
        if updated == owned:
            return owned
        owned = updated


class NativeGui:
    def __init__(self, kind, temporary, output, application):
        self.kind = kind
        self.temporary = Path(temporary)
        self.output = Path(output)
        self.application = application
        self.sequence = 0
        self.clicks = []
        self.permissions = None

    def window(self):
        if self.application.poll() is not None:
            raise RuntimeError(f"Packaged application exited ({self.application.returncode})")
        if self.kind == "macos":
            result = run([self.temporary / "macos-gui", "window", self.application.pid], check=False)
            if result.returncode:
                raise MissingControl("No visible packaged macOS application window")
            info = json.loads(result.stdout)
            self.permissions = {name: info[name] for name in ["accessibilityTrusted", "screenCaptureAllowed"]}
            return info
        result = run(["xdotool", "search", "--onlyvisible", "--name", "^XY Switch$"], check=False)
        owned = descendants(self.application.pid, run(["ps", "-eo", "pid=,ppid="]).stdout)
        candidates = []
        for identifier in result.stdout.split():
            owner = run(["xdotool", "getwindowpid", identifier], check=False)
            if not owner.returncode and int(owner.stdout.strip()) in owned:
                geometry = run(["xdotool", "getwindowgeometry", "--shell", identifier]).stdout
                values = dict(line.split("=", 1) for line in geometry.splitlines() if "=" in line)
                if int(values["WIDTH"]) >= 600 and int(values["HEIGHT"]) >= 400:
                    candidates.append({"id": identifier, "width": int(values["WIDTH"]), "height": int(values["HEIGHT"])})
        if not candidates:
            raise MissingControl("No visible window owned by the packaged Linux application")
        if len(candidates) != 1:
            raise AmbiguousControl("Multiple visible application windows")
        return candidates[0]

    def capture(self, label):
        info = self.window()
        self.sequence += 1
        name = re.sub(r"[^a-zA-Z0-9-]", "-", label)
        image = self.output / f"{self.application.pid}-{self.sequence:03}-{name}.png"
        if self.kind == "linux":
            run(["import", "-window", info["id"], "PNG32:" + str(image)])
        else:
            run(["screencapture", "-x", "-o", "-l", info["id"], image])
        width, height = png_dimensions(image)
        ocr_image = self.temporary / f"ocr-{self.application.pid}-{self.sequence}.png"
        if self.kind == "linux":
            run(["convert", image, "-resize", "200%", ocr_image])
        else:
            run(["sips", "-z", height * 2, width * 2, image, "--out", ocr_image])
        text = run(["tesseract", ocr_image, "stdout", "-l", "eng", "--psm", "11", "tsv"], timeout=30).stdout
        image.with_suffix(".tsv").write_text(text, encoding="utf-8")
        words = read_words(text, scale=2)
        if len(words) < 10:
            raise MissingControl("Packaged window has not rendered readable content")
        return info, image, width, height, words

    def ready(self, label, timeout=90):
        deadline = time.monotonic() + timeout
        failure = None
        while time.monotonic() < deadline:
            try:
                self.capture(label)
                return
            except MissingControl as error:
                failure = error
                time.sleep(1)
        raise RuntimeError(f"Native GUI readiness timed out: {failure}")

    def point(self, info, width, height, horizontal, vertical, operation="click", amount=0):
        if self.kind == "linux":
            run(["xdotool", "windowfocus", "--sync", info["id"]])
            run(["xdotool", "mousemove", "--window", info["id"], round(horizontal), round(vertical)])
            if operation == "click":
                run(["xdotool", "click", "1"])
            else:
                run(["xdotool", "click", "--repeat", "4", "--delay", "80", "5" if amount < 0 else "4"])
        else:
            horizontal = info["x"] + horizontal * info["width"] / width
            vertical = info["y"] + vertical * info["height"] / height
            arguments = [self.temporary / "macos-gui", operation, self.application.pid, horizontal, vertical]
            if operation == "scroll":
                arguments.append(amount)
            run(arguments)

    def click(self, label, *, region=(0, 0, 1, 1), anchor=None, allow_scroll=False):
        failure = None
        for attempt in range(12):
            try:
                info, image, width, height, words = self.capture("before-" + label)
                try:
                    control = select_control(words, label, width, height, region, anchor)
                except MissingControl:
                    if allow_scroll and attempt >= 2:
                        self.point(info, width, height, width * 0.86, height * 0.72, "scroll", -300)
                    raise
                self.point(info, width, height, *control.center)
                self.clicks.append({"label": label, "anchor": anchor, "screenshot": image.name,
                                   "windowId": info["id"], "imageDimensions": [width, height],
                                   "rectangle": [control.left, control.top, control.width, control.height]})
                time.sleep(0.6)
                return
            except MissingControl as error:
                failure = error
                time.sleep(0.6)
        raise RuntimeError(f"Native pointer click failed: {failure}")

    def routing(self):
        self.click("Settings", region=(0, 0, 0.20, 1))
        self.click("Local routing", region=(0.20, 0.05, 1, 0.38))

    def routing_toggle(self, running):
        for _attempt in range(3):
            info = self.window()
            self.point(info, info["width"], info["height"], info["width"] * 0.86, info["height"] * 0.55, "scroll", 600)
        time.sleep(0.3)
        label = "Stop routing" if running else "Start routing"
        self.click(label, region=(0.45, 0.08, 1, 0.65))

    def takeover_toggle(self, active):
        self.click("Restore original configuration" if active else "Connect to routing",
                   region=(0.45, 0.20, 1, 0.96), anchor="Codex", allow_scroll=True)
