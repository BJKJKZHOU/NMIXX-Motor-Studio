#!/usr/bin/env python3
"""Real-browser, disconnected frontend smoke test. Never sends motor commands.

Requires the project's npm dependencies and the optional Playwright test tooling.
This tests actual Svelte mount/plot initialization, not the native Tauri shell.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from urllib.error import URLError
from urllib.request import urlopen

DESKTOP = Path(__file__).resolve().parents[1]
LOCALES = ("C", "POSIX", "en-US", "zh-CN")
PAGES = ("Motor", "Encoder", "Limits / Safety", "Control Architecture", "Motion", "Control Tuning", "Analysis", "Parameters")


def stop_server(process: subprocess.Popen) -> None:
    if process.poll() is not None:
        return
    if os.name == "posix":
        os.killpg(process.pid, signal.SIGTERM)
    else:
        process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        if os.name == "posix":
            os.killpg(process.pid, signal.SIGKILL)
        else:
            process.kill()
        process.wait(timeout=5)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--browser", choices=("chromium", "webkit"), default="chromium")
    parser.add_argument("--port", type=int, default=1431)
    parser.add_argument("--executable", help="Optional installed Chromium executable (Chromium only)")
    args = parser.parse_args()
    if not 1 <= args.port <= 65535:
        parser.error("--port must be between 1 and 65535")
    if args.executable and args.browser != "chromium":
        parser.error("--executable is supported only for Chromium")
    try:
        from playwright.sync_api import expect, sync_playwright
    except ImportError:
        print("Install: python3 -m pip install -r scripts/requirements-startup.txt", file=sys.stderr)
        return 1
    npm = shutil.which("npm")
    if not npm or not (DESKTOP / "node_modules/vite").exists():
        print("Install project dependencies first: cd apps/desktop && npm ci", file=sys.stderr)
        return 1
    url = f"http://127.0.0.1:{args.port}/tests/startup.html"
    with tempfile.TemporaryFile(mode="w+", encoding="utf-8") as log:
        server = subprocess.Popen(
            [npm, "run", "dev", "--", "--port", str(args.port), "--strictPort"],
            cwd=DESKTOP, stdout=log, stderr=subprocess.STDOUT,
            start_new_session=os.name == "posix",
        )
        try:
            deadline = time.monotonic() + 30
            while True:
                if server.poll() is not None:
                    raise RuntimeError("Vite exited before the smoke test (check port/dependencies)")
                try:
                    with urlopen(url, timeout=1) as response:
                        if response.status == 200:
                            break
                except (URLError, TimeoutError):
                    pass
                if time.monotonic() > deadline:
                    raise RuntimeError("Vite did not become ready within 30 seconds")
                time.sleep(0.1)
            with sync_playwright() as playwright:
                launch = {"headless": True}
                if args.executable:
                    launch["executable_path"] = args.executable
                browser = getattr(playwright, args.browser).launch(**launch)
                try:
                    for locale in LOCALES:
                        context = browser.new_context(viewport={"width": 1400, "height": 900})
                        context.add_init_script("localStorage.setItem('nmixx.ui.language', 'en')")
                        # Test-scoped POSIX-locale injection before application imports.
                        # Production code does not modify Navigator or Intl.
                        context.add_init_script("""
                            Object.defineProperty(navigator, 'language', { get: () => %s });
                            Object.defineProperty(navigator, 'languages', { get: () => [%s] });
                        """ % (json.dumps(locale), json.dumps(locale)))
                        errors: list[str] = []
                        page = context.new_page()
                        page.on("pageerror", lambda error: errors.append(str(error)))
                        try:
                            page.goto(url, wait_until="networkidle")
                            expect(page.locator(".workbench")).to_be_visible()
                            expect(page.locator(".activity-bar")).to_be_visible()
                            expect(page.locator(".connection-page")).to_be_visible()
                            expect(page.locator(".statusbar")).to_be_visible()
                            assert page.evaluate("navigator.language") == locale
                            for title in PAGES:
                                page.locator(f'nav.activity-bar button[title="{title}"]').click()
                                expect(page.locator(".workbench")).to_be_visible()
                                # Let lifecycle effects, ResizeObserver and rejected
                                # promise handlers run after each page transition.
                                page.wait_for_timeout(100)
                                if title == "Control Architecture":
                                    for loop in ("Current Loop", "Speed Loop", "Position Loop"):
                                        page.get_by_role("button", name=loop, exact=True).click()
                                        page.wait_for_timeout(100)
                                        assert not errors, f"{locale} / {loop}: {errors}"
                                if title == "Analysis":
                                    expect(page.locator(".scope-echarts-view canvas")).to_be_visible()
                                assert not errors, f"{locale} / {title}: {errors}"
                            calls = page.evaluate("window.__nmixxStartupRequests")
                            allowed = {"device_list", "device_disconnect", "action_list", "motion_get", "automation_snapshot"}
                            assert set(calls) <= allowed, f"Unexpected IPC: {calls}"
                            page.goto(url + "?preview=1", wait_until="networkidle")
                            expect(page.locator(".motion-trajectory-host canvas")).to_be_visible()
                            assert not errors, f"{locale}: {errors}"
                            print(f"PASS {args.browser} / {locale}: workbench, eight pages, Scope and Motion preview")
                        finally:
                            context.close()
                finally:
                    browser.close()
            # Real Svelte/VSCode Elements event test. Synthetic Tauri IPC replies,
            # but the user clicks actual Web Components in Chromium.
            context = browser.new_context(viewport={"width": 1400, "height": 900})
            context.add_init_script("localStorage.setItem('nmixx.ui.language', 'en')")
            page = context.new_page()
            errors = []
            page.on("pageerror", lambda error: errors.append(str(error)))
            try:
                page.goto(url + "?tuning-tabs=1", wait_until="networkidle")
                tabs = page.locator("vscode-tab-header")
                expect(tabs).to_have_count(3)
                expect(page.locator(".scope-echarts-view canvas")).to_be_visible()
                tabs.nth(1).click()
                expect(page.locator("vscode-tabs")).to_have_attribute("selected-index", "1")
                expect(page.locator(".tuning-channel-list")).to_be_visible()
                page.locator(".tuning-channel-row input[type=checkbox]").check()
                expect(page.locator(".tuning-channel-row input[type=checkbox]")).to_be_checked()
                tabs.nth(2).click()
                expect(page.locator("vscode-tabs")).to_have_attribute("selected-index", "2")
                expect(page.locator(".tuning-scale-list")).to_be_visible()
                tabs.nth(0).click()
                expect(page.locator(".scope-echarts-view canvas")).to_be_visible()
                # Keyboard-driven selection belongs to VSCode Elements, not
                # application-authored extra keydown handlers.
                tabs.nth(0).focus()
                page.keyboard.press("ArrowRight")
                page.keyboard.press("Enter")
                expect(page.locator("vscode-tabs")).to_have_attribute("selected-index", "1")
                assert not errors, f"Tuning tab browser errors: {errors}"
                assert page.evaluate("window.__nmixxUIErrors") == []
                print("PASS chromium: Tuning Waveform/Channels/Scale clicks, keyboard and chart return")
            finally:
                context.close()

            # Script text stays editable. Run dispatches only the current editor
            # snapshot, and Cancel uses the shared Automation task API.
            context = browser.new_context(viewport={"width": 1400, "height": 900})
            context.add_init_script("localStorage.setItem('nmixx.ui.language', 'en')")
            page = context.new_page()
            errors = []
            page.on("pageerror", lambda error: errors.append(str(error)))
            try:
                page.goto(url + "?automation-script=1", wait_until="networkidle")
                page.get_by_role("button", name="New", exact=True).click()
                expect(page.locator(".script-tab")).to_have_count(1)
                expect(page.locator(".monaco-editor")).to_be_visible(timeout=15000)
                page.locator(".monaco-editor").click()
                page.keyboard.insert_text("\\nprint('browser-edited')\\n")
                expect(page.locator(".script-tab .dirty-mark")).to_be_visible()
                page.locator(".run-button").click()
                expect(page.locator(".output-state")).to_contain_text("RUNNING")
                commands = page.evaluate("window.__nmixxStartupArgs")
                starts = [entry for entry in commands if entry["command"] == "automation_start"]
                assert len(starts) == 1, starts
                assert "browser-edited" in starts[0]["args"]["spec"]["source"], starts
                page.get_by_role("button", name="Cancel", exact=True).first.click()
                expect(page.locator(".output-state")).to_contain_text("CANCELLED")
                assert not errors, f"Automation browser errors: {errors}"
                print("PASS chromium: Monaco editing, current-source Run IPC, Cancel feedback")
            finally:
                context.close()
            return 0
        except Exception as error:
            print(f"FAIL: {error}", file=sys.stderr)
            log.seek(0)
            print(log.read()[-12000:], file=sys.stderr)
            return 1
        finally:
            stop_server(server)


if __name__ == "__main__":
    raise SystemExit(main())
