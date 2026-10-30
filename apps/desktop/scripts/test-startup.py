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
                            allowed = {"device_list", "default_schema_path", "device_disconnect", "action_list", "motion_get", "automation_snapshot"}
                            assert set(calls) <= allowed, f"Unexpected IPC: {calls}"
                            page.goto(url + "?preview=1", wait_until="networkidle")
                            expect(page.locator(".motion-trajectory-host canvas")).to_be_visible()
                            assert not errors, f"{locale}: {errors}"
                            print(f"PASS {args.browser} / {locale}: workbench, eight pages, Scope and Motion preview")
                        finally:
                            context.close()
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
                        expect(tabs).to_have_count(2)
                        chart = page.locator(".scope-echarts-view canvas")
                        expect(chart).to_be_visible()
                        channel = page.get_by_role("combobox", name="Channel", exact=True)
                        gain = page.get_by_label("Scale ×", exact=True)
                        y_min = page.get_by_label("Y min", exact=True)
                        y_max = page.get_by_label("Y max", exact=True)
                        expect(gain).to_be_visible()
                        before = page.evaluate("window.__tuningChart().getOption()")
                        assert before["xAxis"][0]["min"] == 0
                        assert before["xAxis"][0]["max"] == 10
                        assert before["dataZoom"][0]["startValue"] == 0
                        assert before["dataZoom"][0]["endValue"] == 10
                        assert [point[0] for point in before["series"][3]["data"]] == [0, .5, 1]
                        assert len(before["yAxis"]) == 1
                        assert all(series["yAxisIndex"] == 0 for series in before["series"])
                        assert before["yAxis"][0]["min"] <= 0
                        assert 500 <= before["yAxis"][0]["max"] < 600
                        assert [point[1] for point in before["series"][3]["data"]] == [0, 10, 30]
                        assert [point[1] for point in before["series"][5]["data"]] == [.5, 11, 30]
                        assert before["series"][3]["name"] == "Δ Position"
                        channel.select_option("1281")
                        gain.fill("2")
                        gain.press("Enter")
                        page.wait_for_function("window.__tuningChart().getOption().series[3].data[1][1] === 20")
                        position = page.evaluate("window.__tuningChart().getOption().series[3]")
                        assert position["data"][1][2] == 10
                        assert position["data"][0][1] == 0
                        channel.select_option("17")
                        gain.fill("2")
                        gain.press("Enter")
                        page.wait_for_function("window.__tuningChart().getOption().series[0].data[1][1] === 1")
                        after = page.evaluate("window.__tuningChart().getOption()")
                        assert after["yAxis"][0]["min"] == before["yAxis"][0]["min"]
                        assert after["yAxis"][0]["max"] == before["yAxis"][0]["max"]
                        assert after["series"][0]["data"][1][2] == .5
                        assert after["series"][0]["encode"]["tooltip"] == [2]
                        assert after["series"][1]["data"] == before["series"][1]["data"]
                        y_min.fill("-2")
                        y_min.press("Enter")
                        y_max.fill("2")
                        y_max.press("Enter")
                        page.wait_for_function("window.__tuningChart().getOption().yAxis[0].max === 2")
                        expect(gain).to_have_value("2")
                        y_max.fill("-3")
                        y_max.press("Enter")
                        expect(y_max).to_have_value("2")
                        gain.fill("0")
                        gain.press("Enter")
                        expect(gain).to_have_value("2")
                        channel.select_option("1282")
                        expect(gain).to_have_value("1")
                        expect(y_max).to_have_value("2")
                        gain.fill("0.002")
                        gain.press("Enter")
                        page.wait_for_function("window.__tuningChart().getOption().series[2].data[1][1] === 1")
                        scaled = page.evaluate("window.__tuningChart().getOption()")
                        assert len(scaled["yAxis"]) == 1
                        assert all(series["yAxisIndex"] == 0 for series in scaled["series"])
                        assert scaled["series"][2]["data"][1][2] == 500
                        assert scaled["series"][2]["name"] == "Speed ×0.002"
                        # Equal displayed values must land on the exact same pixel across units.
                        pixels = page.evaluate("""() => {
                            const c = window.__tuningChart();
                            return [0, 2].map(seriesIndex => c.convertToPixel({seriesIndex}, [0.5, 1]));
                        }""")
                        assert pixels[0] == pixels[1]
                        # Show the middle sample and check the actual tooltip uses raw speed.
                        page.evaluate("window.__tuningChart().dispatchAction({type:'dataZoom', start:0, end:100})")
                        page.evaluate("window.__tuningChart().dispatchAction({type:'showTip', seriesIndex:2, dataIndex:1})")
                        expect(page.locator(".scope-echarts-view")).to_contain_text("500 rad/s")
                        channel.select_option("17")
                        expect(gain).to_have_value("2")
                        expect(y_min).to_have_value("-2")
                        expect(y_max).to_have_value("2")
                        tabs.nth(1).click()
                        expect(page.locator("vscode-tabs")).to_have_attribute("selected-index", "1")
                        expect(page.locator(".tuning-channel-list")).to_be_visible()
                        page.locator(".tuning-channel-row input[type=checkbox]").check()
                        tabs.nth(0).click()
                        expect(chart).to_be_visible()
                        expect(y_min).to_have_value("-2")
                        expect(y_max).to_have_value("2")
                        page.evaluate("window.__tuningChart().dispatchAction({type:'dataZoom', start:20, end:100})")
                        page.wait_for_function("window.__nmixxStartupRequests.filter(x => x === 'tuning_experiment_snapshot').length > 1")
                        expect(y_max).to_have_value("2")
                        expect(gain).to_have_value("2")
                        channel.select_option("1282")
                        expect(gain).to_have_value("0.002")
                        assert len(page.evaluate("window.__tuningChart().getOption().yAxis")) == 1
                        assert not page.evaluate("window.__nmixxStartupRequests.some(x => ['parameter_write','motion_run','motor_enable','tuning_experiment_start'].includes(x))")
                        # 验证完整预计时长先展开，运行中的样本增长不会推动或缩短横轴。
                        page.evaluate("window.__tuningState = 'RUNNING'; window.__tuningRecordedSeconds = 7")
                        page.wait_for_function("window.__nmixxStartupRequests.filter(x => x === 'tuning_experiment_snapshot').length > 4")
                        chart.dblclick(position={"x": 250, "y": 150})
                        page.wait_for_function("window.__tuningChart().getOption().dataZoom[0].endValue === 10")
                        live = page.evaluate("window.__tuningChart().getOption()")
                        assert live["xAxis"][0]["min"] == 0 and live["xAxis"][0]["max"] == 10
                        assert [point[0] for point in live["series"][3]["data"]] == [0, .5, 1]
                        page.evaluate("window.__tuningChart().dispatchAction({type:'dataZoom', startValue:2, endValue:4})")
                        page.evaluate("window.__tuningRecordedSeconds = 8")
                        page.wait_for_timeout(250)
                        zoomed = page.evaluate("window.__tuningChart().getOption().dataZoom[0]")
                        assert zoomed["startValue"] == 2 and zoomed["endValue"] == 4
                        page.evaluate("window.__tuningRecordedSeconds = 10.1")
                        page.wait_for_function("window.__tuningChart().getOption().xAxis[0].max === 11")
                        # 新记录必须重新按本次计划展开，不能沿用上一程的局部放大范围。
                        page.evaluate("window.__tuningCaptureId = 2; window.__tuningRecordedSeconds = 1; window.__tuningPlannedSeconds = 4")
                        page.wait_for_function("window.__tuningChart().getOption().dataZoom[0].endValue === 4 && window.__tuningChart().getOption().dataZoom[0].startValue === 0")
                        assert page.evaluate("window.__tuningChart().getOption().xAxis[0].max") == 4
                        page.evaluate("window.__tuningState = 'COMPLETED'")
                        # Keyboard-driven selection belongs to VSCode Elements, not
                        # application-authored extra keydown handlers.
                        tabs.nth(0).focus()
                        page.keyboard.press("ArrowRight")
                        page.keyboard.press("Enter")
                        expect(page.locator("vscode-tabs")).to_have_attribute("selected-index", "1")
                        assert not errors, f"Tuning tab browser errors: {errors}"
                        assert page.evaluate("window.__nmixxUIErrors") == []
                        print("PASS chromium: Tuning shared Y axis, relative position, planned full X axis, live growth, independent gains and tabs")
                    finally:
                        context.close()

                    # 通过真正的 App 导航销毁/重建页面，验证显示偏好不依赖组件存活。
                    context = browser.new_context(viewport={"width": 1400, "height": 900})
                    context.add_init_script("localStorage.setItem('nmixx.ui.language', 'en')")
                    page = context.new_page()
                    errors = []
                    page.on("pageerror", lambda error: errors.append(str(error)))
                    try:
                        page.goto(url + "?tuning-tabs=1&tuning-navigation=1", wait_until="networkidle")
                        page.locator(".connection-actions vscode-button").click()
                        expect(page.locator(".connection-status")).to_contain_text("AxDr_L")
                        tuning_nav = page.locator('nav.activity-bar button[title="Control Tuning"]')
                        motor_nav = page.locator('nav.activity-bar button[title="Motor"]')
                        tuning_nav.click()
                        expect(page.locator(".tuning-root .scope-echarts-view canvas")).to_be_visible()
                        channel = page.get_by_role("combobox", name="Channel", exact=True)
                        gain = page.get_by_label("Scale ×", exact=True)
                        y_min = page.get_by_label("Y min", exact=True)
                        y_max = page.get_by_label("Y max", exact=True)
                        gain.fill("5")
                        gain.press("Enter")
                        channel.select_option("1282")
                        gain.fill("0.01")
                        gain.press("Enter")
                        y_min.fill("-10")
                        y_min.press("Enter")
                        y_max.fill("10")
                        y_max.press("Enter")
                        page.evaluate("window.__tuningChart().dispatchAction({type:'dataZoom', startValue:.2, endValue:.8})")
                        page.evaluate("window.__tuningChart().dispatchAction({type:'legendToggleSelect', name:'Vbus'})")
                        page.wait_for_function("window.__tuningChart().getOption().legend[0].selected.Vbus === false")
                        for _ in range(2):
                            motor_nav.click()
                            expect(page.locator(".tuning-root .scope-echarts-view")).to_have_count(0)
                            tuning_nav.click()
                            expect(gain).to_have_value("0.01")
                            expect(channel).to_have_value("1282")
                            expect(y_min).to_have_value("-10")
                            expect(y_max).to_have_value("10")
                            page.wait_for_function("window.__tuningChart().getOption().dataZoom[0].startValue === .2")
                            restored = page.evaluate("window.__tuningChart().getOption()")
                            assert restored["dataZoom"][0]["endValue"] == .8
                            assert restored["legend"][0]["selected"]["Vbus"] is False
                            assert restored["series"][0]["name"] == "Iq ×5"
                        # 新实验重置横轴，继续保留用于对比的倍率、Y 范围和图例显隐。
                        motor_nav.click()
                        page.evaluate("window.__tuningCaptureId = 2; window.__tuningPlannedSeconds = 4")
                        tuning_nav.click()
                        expect(gain).to_have_value("0.01")
                        expect(y_max).to_have_value("10")
                        page.wait_for_function("window.__tuningChart().getOption().dataZoom[0].endValue === 4")
                        assert page.evaluate("window.__tuningChart().getOption().dataZoom[0].startValue") == 0
                        assert page.evaluate("window.__tuningChart().getOption().legend[0].selected.Vbus") is False
                        # 新连接不能继承旧会话采集编号对应的局部视图。
                        page.locator('nav.activity-bar button[title="Connection"]').click()
                        page.locator(".connection-actions vscode-button").click()
                        expect(page.locator(".connection-status")).to_contain_text("Disconnected")
                        page.locator(".connection-actions vscode-button").click()
                        expect(page.locator(".connection-status")).to_contain_text("AxDr_L")
                        tuning_nav.click()
                        expect(channel).to_have_value("17")
                        expect(gain).to_have_value("1")
                        assert page.evaluate("window.__tuningChart().getOption().legend[0].selected.Vbus") is True
                        assert not errors, errors
                        assert not page.evaluate("window.__nmixxStartupRequests.some(x => ['parameter_write','motion_run','motor_enable','tuning_experiment_start'].includes(x))")
                        print("PASS chromium: Tuning view survives App navigation; new capture resets only X; reconnect clears session view")
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
                        expect(page.locator(".automation-workspace")).to_be_visible()
                        assert not errors, f"Automation mount errors: {errors}"
                        page.locator(".toolbar-actions button:has(.codicon-new-file)").click()
                        expect(page.locator(".script-tab")).to_have_count(1)
                        expect(page.locator(".monaco-editor")).to_be_visible(timeout=15000)
                        page.locator(".monaco-editor").click()
                        page.keyboard.insert_text("\\nprint('browser-edited')\\n")
                        expect(page.locator(".script-tab .dirty-mark")).to_be_visible()
                        page.locator(".toolbar-actions button:has(.codicon-new-file)").click()
                        expect(page.locator(".script-tab")).to_have_count(2)
                        # The edited first model must survive a switch to the
                        # second tab before Run captures its source.
                        page.locator(".script-tab").first.locator(".tab-name").click()
                        expect(page.locator(".script-tab.active")).to_contain_text("Untitled-1.py")
                        page.locator(".run-button").click()
                        expect(page.locator(".output-state")).to_contain_text("RUNNING")
                        commands = page.evaluate("window.__nmixxStartupArgs")
                        starts = [entry for entry in commands if entry["command"] == "automation_start"]
                        assert len(starts) == 1, starts
                        assert "browser-edited" in starts[0]["args"]["spec"]["source"], starts
                        # Locate the domain Cancel control by its stable icon:
                        # language labels and accessible names may vary with locale.
                        page.locator(".toolbar-actions button:has(.codicon-debug-stop)").click()
                        expect(page.locator(".output-state")).to_contain_text("CANCELLED")
                        assert not errors, f"Automation browser errors: {errors}"
                        print("PASS chromium: Monaco editing, current-source Run IPC, Cancel feedback")
                    finally:
                        context.close()
                    # Full App shell with a connected, simulated device:
                    # the displayed Enable/Disable state must come from the
                    # same Parameter readbacks observed by the business pages.
                    context = browser.new_context(viewport={"width": 1400, "height": 900})
                    context.add_init_script("localStorage.setItem('nmixx.ui.language', 'en')")
                    page = context.new_page()
                    errors = []
                    page.on("pageerror", lambda error: errors.append(str(error)))
                    try:
                        page.goto(url + "?motor-controls=1", wait_until="networkidle")
                        expect(page.locator("#connection-port")).to_have_value("COM7")
                        power = page.locator(".global-actions > button").first
                        expect(power).to_contain_text("UNKNOWN")
                        page.locator(".connection-actions vscode-button").click()
                        expect(page.locator(".connection-status")).to_contain_text("AxDr_L")
                        expect(power).to_contain_text("DISABLED")
                        expect(page.locator(".global-actions > button.motor-disabled")).to_have_count(1)
                        # Starting identification has its own semantic API. Blocked
                        # preflight reports in-page text without an Enable modal.
                        dialogs = []
                        page.on("dialog", lambda dialog: (dialogs.append(dialog.message), dialog.dismiss()))
                        page.locator('nav.activity-bar button[title="Motor"]').click()
                        page.locator('vscode-button[title="Start Rs/Ls"]').click()
                        expect(page.locator(".action-status").filter(has_text="Prerequisites not met")).to_have_count(1)
                        ids = [entry for entry in page.evaluate("window.__nmixxStartupArgs")
                          if entry["command"] == "identification_start"]
                        assert len(ids) == 1 and ids[0]["args"]["allowEnable"] is True, ids
                        assert not dialogs, dialogs
                        assert page.evaluate("window.__nmixxStartupRequests").count("motor_enable") == 0
                        page.locator('nav.activity-bar button[title="Connection"]').click()
                        power.click()
                        expect(power).to_contain_text("ENABLED")
                        expect(page.locator(".global-actions > button.motor-enabled")).to_have_count(1)
                        page.locator(".global-actions .stop-action").click()
                        expect(power).to_contain_text("ENABLED")
                        power.click()
                        expect(power).to_contain_text("DISABLED")
                        commands = page.evaluate("window.__nmixxStartupRequests")
                        for command in ("motor_enable", "motor_stop", "motor_disable"):
                            assert commands.count(command) == 1, commands
                        page.locator(".connection-actions vscode-button").click()
                        expect(page.locator(".connection-status")).to_contain_text("Disconnected")
                        expect(power).to_contain_text("UNKNOWN")
                        assert not errors, f"Global control browser errors: {errors}"
                        print("PASS chromium: Connect, modal-free blocked identification, Motor Enable/Stop/Disable and Disconnect")
                    finally:
                        context.close()
                    # Refresh must expose the board even while the editable path
                    # contains the debugger's name. A datalist filters that out.
                    context = browser.new_context(viewport={"width": 1400, "height": 900})
                    context.add_init_script("localStorage.setItem('nmixx.ui.language', 'en')")
                    page = context.new_page()
                    errors = []
                    page.on("pageerror", lambda error: errors.append(str(error)))
                    try:
                        debugger = "/dev/serial/by-id/usb-MICROLINK_MICROLINK_CMSIS-DAP_012345ABCDEF-if01"
                        board = "/dev/serial/by-id/usb-STMicroelectronics_STM32_USB_Device_000000000001-if00"
                        page.goto(url + "?ports=1", wait_until="networkidle")
                        port = page.locator("#connection-port")
                        picker = page.get_by_role("combobox", name="Available ports", exact=True)
                        expect(port).to_have_value(debugger)
                        page.evaluate("ports => window.__nmixxPorts = ports", [debugger, board])
                        page.locator('vscode-button[title="Refresh ports"]').click()
                        expect(port).to_have_value(debugger)
                        expect(picker.locator("option")).to_have_count(3)
                        assert picker.locator("option").all_text_contents()[1:] == [debugger, board]
                        picker.select_option(board)
                        expect(port).to_have_value(board)
                        expect(picker).to_have_value("")
                        page.locator('vscode-button[title="Refresh ports"]').click()
                        expect(port).to_have_value(board)
                        port.fill("/dev/custom-port")
                        expect(picker.locator("option")).to_have_count(3)
                        picker.select_option(debugger)
                        expect(port).to_have_value(debugger)
                        page.evaluate("window.__nmixxPorts = []")
                        page.locator('vscode-button[title="Refresh ports"]').click()
                        expect(picker).to_be_disabled()
                        expect(port).to_have_value("")
                        calls = page.evaluate("window.__nmixxStartupRequests")
                        assert set(calls) <= {"device_list", "default_schema_path"}, calls
                        assert not errors, errors
                        print("PASS chromium: debugger plus board port discovery, full list selection, refresh and no automatic connection")
                    finally:
                        context.close()

                    # Firmware-selected controllers must be shown from readback,
                    # and rejected/state-forbidden switches must not change them.
                    context = browser.new_context(viewport={"width": 1400, "height": 900})
                    context.add_init_script("localStorage.setItem('nmixx.ui.language', 'en')")
                    context.add_init_script("if (!localStorage.getItem('nmixx.connection.hostSchemaPath')) localStorage.setItem('nmixx.connection.hostSchemaPath', '../../../AxDr_L_Motor/build/host/axdr-host-schema.toml')")
                    page = context.new_page()
                    errors = []
                    page.on("pageerror", lambda error: errors.append(str(error)))
                    try:
                        page.goto(url + "?controllers=1", wait_until="networkidle")
                        expect(page.locator("#connection-schema")).to_have_value("/firmware/build/host/axdr-host-schema.toml")
                        page.get_by_role("button", name="Choose HostSchema file").click()
                        expect(page.locator("#connection-schema")).to_have_value("/selected/axdr-host-schema.toml")
                        page.reload(wait_until="networkidle")
                        expect(page.locator("#connection-schema")).to_have_value("/selected/axdr-host-schema.toml")
                        assert "default_schema_path" not in page.evaluate("window.__nmixxStartupRequests")
                        page.locator(".connection-actions vscode-button").click()
                        expect(page.locator(".connection-status")).to_contain_text("AxDr_L")
                        page.locator('nav.activity-bar button[title="Control Architecture"]').click()
                        current = page.get_by_role("combobox", name="Current Controller", exact=True)
                        expect(current).to_have_value("CURRENT_CTRL_DPCC")
                        expect(page.locator(".current-diagram .block-title")).to_contain_text("DPCC")
                        expect(page.locator(".current-diagram .gain-grid")).to_have_count(0)
                        current.select_option("CURRENT_CTRL_LADRC")
                        expect(page.get_by_role("textbox", name="LADRC Current Gain", exact=True)).to_be_visible()
                        expect(page.get_by_role("textbox", name="LADRC LESO Gain", exact=True)).to_be_visible()
                        writes = [entry["args"] for entry in page.evaluate("window.__nmixxStartupArgs") if entry["command"] == "parameter_write"]
                        assert writes == [{"id": 0x0a08, "value": {"type": "u8", "value": 1}}], writes
                        page.evaluate("window.__rejectParameterWrite = true")
                        current.select_option("CURRENT_CTRL_PI")
                        expect(page.locator(".app-error")).to_contain_text("ERR_STATE")
                        expect(current).to_have_value("CURRENT_CTRL_LADRC")
                        expect(page.locator(".current-diagram .block-title")).to_contain_text("LADRC")
                        page.evaluate("window.__rejectParameterWrite = false")
                        current.select_option("CURRENT_CTRL_PI")
                        expect(page.locator(".current-diagram .gain-grid")).to_be_visible()
                        current.select_option("CURRENT_CTRL_LADRC")
                        gain = page.get_by_role("textbox", name="LADRC Current Gain", exact=True)
                        gain.fill("60")
                        expect(current).to_be_disabled()
                        gain.press("Escape")
                        expect(current).to_be_enabled()
                        page.get_by_role("button", name="Speed Loop", exact=True).click()
                        speed = page.get_by_role("combobox", name="Speed Controller", exact=True)
                        expect(speed).to_have_value("SPEED_CTRL_LADRC")
                        expect(page.get_by_role("textbox", name="LADRC Speed Gain", exact=True)).to_be_visible()
                        expect(page.locator(".speed-diagram .gain-grid")).to_have_count(0)
                        speed.select_option("SPEED_CTRL_PI")
                        expect(page.locator(".speed-diagram .gain-grid")).to_be_visible()
                        speed.select_option("SPEED_CTRL_LADRC")
                        page.locator(".global-actions > button").first.click()
                        expect(page.locator(".global-actions > button").first).to_contain_text("ENABLED")
                        expect(speed).to_be_disabled()
                        expect(page.get_by_role("textbox", name="LADRC Speed Gain", exact=True)).to_be_disabled()
                        page.get_by_role("button", name="Current Loop", exact=True).click()
                        expect(current).to_be_disabled()
                        expect(page.get_by_role("textbox", name="LADRC Current Gain", exact=True)).to_be_enabled()
                        assert not errors, f"Controller/Schema browser errors: {errors}"
                        print("PASS chromium: Schema discovery/persistence, PI/LADRC/DPCC readback, writes, rejection and motor-state locks")
                    finally:
                        context.close()
                    # Encoder task Actions: real page, mocked firmware completion.
                    context = browser.new_context(viewport={"width": 1280, "height": 820})
                    context.add_init_script("localStorage.setItem('nmixx.ui.language', 'en')")
                    page = context.new_page()
                    errors = []
                    page.on("pageerror", lambda error: errors.append(str(error)))
                    try:
                        page.goto(url + "?encoder=1", wait_until="networkidle")
                        page.locator(".connection-actions vscode-button").click()
                        expect(page.locator(".connection-status")).to_contain_text("AxDr_L")
                        page.locator('nav.activity-bar button[title="Encoder"]').click()
                        root = page.locator(".encoder-root")
                        phase = root.locator("vscode-button", has_text="Start").filter(has_not_text="calibration").first
                        calibration = root.locator("vscode-button", has_text="Start calibration")
                        expect(root).to_contain_text("Phase-aligned; encoder not calibrated")
                        assert calibration.bounding_box()["y"] > phase.bounding_box()["y"]
                        calibration.click()
                        expect(root.locator(".state-running")).to_have_count(1)
                        expect(phase).to_have_attribute("disabled", "")
                        expect(root.locator("select").first).to_be_disabled()
                        # The page stays subscribed when navigating away.
                        page.locator('nav.activity-bar button[title="Motor"]').click()
                        page.evaluate("window.__completeEncoderTask()")
                        page.locator('nav.activity-bar button[title="Encoder"]').click()
                        expect(root).to_contain_text("Phase alignment and encoder calibration valid")
                        expect(root.locator(".state-success")).to_have_count(1)
                        phase.click()
                        expect(calibration).to_have_attribute("disabled", "")
                        page.evaluate("window.__completeEncoderTask()")
                        expect(root).to_contain_text("Phase-aligned; encoder not calibrated")
                        calibration.click()
                        page.locator(".stop-action").click()
                        expect(root).to_contain_text("Stopped")
                        expect(root).to_contain_text("Not phase-aligned")
                        page.evaluate("window.__rejectEncoderStart = true")
                        calibration.click()
                        expect(root.locator(".action-reason")).to_contain_text("prerequisites not met")
                        calls = page.evaluate("window.__nmixxStartupRequests")
                        assert "motor_enable" not in calls and "motor_run" not in calls
                        assert not errors, errors
                        print("PASS encoder: calibration below phase search, busy locks, navigation, completion, Stop and rejection")
                    finally:
                        context.close()
                    # Legacy helper calls must not leave Run stuck at its initial disabled state.
                    context = browser.new_context(viewport={"width": 1280, "height": 820})
                    context.add_init_script("localStorage.setItem('nmixx.ui.language', 'en')")
                    page = context.new_page()
                    errors = []
                    page.on("pageerror", lambda error: errors.append(str(error)))
                    try:
                        page.goto(url + "?motion=1", wait_until="networkidle")
                        page.locator(".connection-actions vscode-button").click()
                        expect(page.locator(".connection-status")).to_contain_text("AxDr_L")
                        page.locator('nav.activity-bar button[title="Motion"]').click()
                        root = page.locator(".motion-simple-page")
                        run = root.locator(".motion-run")
                        stop = root.locator(".motion-runbar button").nth(1)
                        power = page.locator(".global-actions > button").first
                        expect(run).to_be_disabled()
                        max_speed = root.get_by_role("textbox", name="Max speed rad/s", exact=True)
                        max_speed.fill("1000")
                        expect(root.locator(".motion-speed-conversion")).to_have_text("≈ 9549.3 RPM")
                        max_speed.press("Escape")
                        expect(root.locator(".motion-speed-conversion")).to_have_text("≈ 95.5 RPM")
                        max_speed.fill("")
                        expect(root.locator(".motion-speed-conversion")).to_have_text("— RPM")
                        max_speed.press("Escape")
                        root.get_by_role("combobox", name="Profile", exact=True).select_option("1")
                        power.click()
                        expect(power).to_contain_text("ENABLED")
                        expect(run).to_be_enabled()
                        delta = root.get_by_role("textbox", name="Delta position turn", exact=True)
                        delta.fill("3")
                        expect(run).to_be_disabled()
                        delta.press("Escape")
                        expect(run).to_be_enabled()
                        delta.fill("2")
                        delta.press("Enter")
                        expect(run).to_be_enabled()
                        acceleration = root.get_by_role("textbox", name="Acceleration rad/s²", exact=True)
                        acceleration.fill("20")
                        expect(run).to_be_disabled()
                        acceleration.press("Enter")
                        expect(acceleration).to_be_enabled()
                        expect(run).to_be_enabled()
                        run.click()
                        expect(root).to_contain_text("Device motor state: RUN")
                        expect(run).to_be_disabled()
                        expect(root.get_by_role("combobox", name="Profile", exact=True)).to_be_disabled()
                        stop.click()
                        expect(stop).to_be_disabled()
                        expect(stop).to_be_enabled()
                        expect(run).to_be_enabled()
                        power.click()
                        expect(run).to_be_disabled()
                        root.get_by_role("combobox", name="Mode", exact=True).select_option("speed")
                        target_speed = root.get_by_role("textbox", name="Target speed rad/s", exact=True)
                        expect(target_speed).to_be_visible()
                        target_speed.fill("-1000")
                        expect(root.locator(".motion-speed-conversion")).to_have_text("≈ -9549.3 RPM")
                        target_speed.press("Escape")
                        power.click()
                        expect(run).to_be_enabled()
                        assert page.evaluate("window.__nmixxStartupRequests.filter(x => x === 'motion_run').length") == 1
                        assert not errors, errors
                        print("PASS Motion: disabled/configured/enabled Run, dirty drafts, Enter/Escape, busy writes, Run/Stop and mode switch")
                    finally:
                        context.close()
                finally:
                    browser.close()
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
