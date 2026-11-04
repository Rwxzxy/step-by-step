#!/usr/bin/env python3
"""两个真实的步步实例在同一台（无界面）机器上的端到端测试。

在 Linux CI 上用 Xvfb 运行：启动两个实例（不同的数据目录、不同的端口），然后通过
`STEPPY_E2E=1` 时才开启的 127.0.0.1 自动化接口驱动它们，验证：

1. 创建家庭组 + 6 位配对码 SPAKE2 配对（经由真实 TCP 连接）。
2. 加密会话建立后的日程同步。
3. Claude hook（经由真正的 steppy-hook 程序）→ 本机会话 → 同步到另一台，
   并且默认不同步消息正文。
4. 摆放位置 + 宠物串门（两阶段交接 + 租约）+ 召回：任何时刻只在一台机器上。
5. 踢出成员后更换组密钥，被踢的机器离开家庭组。

用法：python3 e2e/two_instances.py <steppy 可执行文件> <steppy-hook 可执行文件>
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import struct
import urllib.request
import zlib

APP, HOOK = sys.argv[1], sys.argv[2]
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FIX = os.path.join(ROOT, "crates", "steppy-core", "tests", "fixtures", "claude")


class Instance:
    def __init__(self, name, base):
        self.name = name
        self.dir = os.path.join(base, name)
        self.data = os.path.join(self.dir, "data")
        self.home = os.path.join(self.dir, "home")
        os.makedirs(self.data)
        os.makedirs(self.home)
        self.endpoint = os.path.join(self.data, "steppy", "endpoint.json")
        self.log = open(os.path.join(self.dir, "log.txt"), "a")
        self.start()

    def start(self):
        env = dict(os.environ, XDG_DATA_HOME=self.data, HOME=self.home, STEPPY_E2E="1", STEPPY_LOG="debug")
        self.proc = subprocess.Popen([APP], env=env, stdout=self.log, stderr=subprocess.STDOUT)
        wait(lambda: os.path.exists(self.endpoint) and self.ep().get("pid") == self.proc.pid, f"{self.name} 启动", 60)
        wait(lambda: self.snap()["snapshot"]["net_port"] > 0, f"{self.name} 网络就绪", 30)

    def restart(self):
        """正常退出再启动（数据目录不变）。"""
        self.proc.terminate()
        try:
            self.proc.wait(10)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            self.proc.wait(10)
        self.start()

    def crash(self):
        """模拟断电 / 拔网线：直接杀掉进程，不做任何清理。"""
        self.proc.kill()
        self.proc.wait(10)

    def log_text(self):
        with open(os.path.join(self.dir, "log.txt"), errors="replace") as f:
            return f.read()

    def ep(self):
        try:
            with open(self.endpoint) as f:
                return json.load(f)
        except Exception:
            return {}

    def req(self, method, path, body=None):
        ep = self.ep()
        data = json.dumps(body).encode() if body is not None else None
        r = urllib.request.Request(f"http://127.0.0.1:{ep['port']}{path}", data=data, method=method)
        r.add_header("Authorization", f"Bearer {ep['token']}")
        r.add_header("Content-Type", "application/json")
        try:
            with urllib.request.urlopen(r, timeout=60) as resp:
                text = resp.read().decode()
                return json.loads(text) if text else None
        except urllib.error.HTTPError as e:
            raise RuntimeError(f"{self.name} {path}: {e.code} {e.read().decode()}")

    def snap(self):
        return self.req("GET", "/v1/debug/snapshot")

    def call(self, cmd, **args):
        return self.req("POST", "/v1/debug/call", {"cmd": cmd, "args": args})

    def hook(self, fixture, **replace):
        with open(os.path.join(FIX, fixture)) as f:
            payload = json.load(f)
        payload.update(replace)
        subprocess.run([HOOK, "claude", "--endpoint", self.endpoint], input=json.dumps(payload).encode(), check=True, timeout=10)

    def stop(self):
        self.proc.terminate()
        try:
            self.proc.wait(10)
        except subprocess.TimeoutExpired:
            self.proc.kill()
        self.log.close()


def write_png(path, w, h, pixel):
    """不依赖第三方库写一张 RGBA PNG。pixel(x, y) 返回 (r, g, b, a)。"""
    raw = bytearray()
    for y in range(h):
        raw.append(0)
        for x in range(w):
            raw.extend(pixel(x, y))

    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)

    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)))
        f.write(chunk(b"IDAT", zlib.compress(bytes(raw), 6)))
        f.write(chunk(b"IEND", b""))


def make_skin(folder):
    """一张缩小一半的 Codex 图集（768 × 936，8 列 × 9 行）：每帧中间一个 32 × 76 的彩色小人，每行颜色不同。"""
    os.makedirs(folder)
    with open(os.path.join(folder, "pet.json"), "w") as f:
        json.dump({"id": "test-cat", "displayName": "测试小猫", "spriteVersionNumber": 1}, f)
    colors = [(230, 120, 60), (60, 160, 90), (60, 120, 220), (200, 80, 200), (240, 200, 40), (220, 60, 60), (90, 200, 200), (120, 90, 60), (150, 150, 150)]
    clear = (0, 0, 0, 0)

    def px(x, y):
        fx, fy, row = x % 96, y % 104, y // 104
        if 32 <= fx < 64 and 24 <= fy < 100:
            return (*colors[row], 255)
        return clear

    write_png(os.path.join(folder, "spritesheet.png"), 768, 936, px)


def wait(cond, what, timeout=30):
    t0 = time.time()
    last = None
    while time.time() - t0 < timeout:
        try:
            if cond():
                print(f"  ✓ {what}（{time.time() - t0:.1f}s）")
                return
        except Exception as e:  # noqa: BLE001
            last = e
        time.sleep(0.3)
    raise AssertionError(f"超时：{what}；最后的错误：{last}")


def main():
    base = tempfile.mkdtemp(prefix="steppy-e2e-")
    a = b = None
    ok = False
    try:
        print("启动两个实例…")
        a = Instance("A", base)
        b = Instance("B", base)
        ida = a.snap()["snapshot"]["me"]["node_id"]
        idb = b.snap()["snapshot"]["me"]["node_id"]
        a.call("patch_settings", machine_name="A-台式机", owner_name="小明")
        port_a = a.snap()["snapshot"]["net_port"]
        b.call("patch_settings", machine_name="B-笔记本", owner_name="小红", manual_peers=[f"127.0.0.1:{port_a}"])

        print("配对…")
        a.call("create_group")
        code = a.call("start_pairing")
        assert len(code) == 6 and code.isdigit(), code
        wrong = "000000" if code != "000000" else "111111"
        try:
            b.call("join_group", code=wrong)
            raise AssertionError("错误的配对码不应该成功")
        except RuntimeError as e:
            assert "配对码" in str(e), e
            print("  ✓ 错误的配对码被拒绝")
        assert b.snap()["snapshot"]["group"] is None
        msg = b.call("join_group", code=code)
        print("  ", msg)
        wait(lambda: idb in a.snap()["peers"] and ida in b.snap()["peers"], "加密会话建立", 40)
        assert a.snap()["snapshot"]["group"]["group_id"] == b.snap()["snapshot"]["group"]["group_id"]
        assert a.snap()["snapshot"]["pairing"] is None, "配对成功后配对码应作废"

        print("日程同步…")
        ev_id = a.call(
            "save_event",
            title="周会",
            notes="",
            all_day=False,
            start="2026-11-03T09:00:00",
            end="2026-11-03T10:00:00",
            repeat="weekdays",
            remind_before_min=10,
        )
        wait(lambda: any(r["id"] == ev_id and r["data"]["title"] == "周会" for r in b.snap()["calendar"]), "B 收到 A 的日程")

        print("Agent 进度同步…")
        b.hook("user_prompt_submit.json", session_id="sess-b", cwd="/home/xiaohong/game")
        b.hook("post_tool_use_todowrite.json", session_id="sess-b", cwd="/home/xiaohong/game")
        b.hook("permission_request.json", session_id="sess-b", cwd="/home/xiaohong/game")

        def remote_session():
            for m in a.snap()["snapshot"]["machines"]:
                if m["node_id"] == idb:
                    for s in m["sessions"]:
                        if s["session_id"] == "sess-b":
                            return s
            return None

        wait(lambda: remote_session() and remote_session()["status"] == "waiting", "A 看到 B 的会话在等批准")
        s = remote_session()
        assert [p["status"] for p in s["plan"]] == ["completed", "in_progress", "pending"], s["plan"]
        assert "last_prompt" not in s, "默认不应同步提示词"
        print("  ✓ 计划条目已同步，提示词没有同步")
        b.hook("stop.json", session_id="sess-b", cwd="/home/xiaohong/game")
        wait(lambda: remote_session()["status"] == "done", "A 看到 B 的会话完成")

        print("串门…")
        a.call("set_layout", order=[ida, idb])
        wait(lambda: b.snap()["snapshot"]["group"]["layout"] == [ida, idb], "B 收到摆放位置")
        wait(lambda: a.snap()["snapshot"]["pet"]["right_neighbor"] == idb, "A 的右边是 B")
        a.call("go_visit", side="right")

        def where():
            sa = a.snap()
            try:
                sb = b.snap()
                at_b = ida in sb["visible_guests"]
            except Exception:  # noqa: BLE001  B 已离线
                at_b = False
            at_home = sa["frames"].get("pet", {}).get("visible") and sa["owner_state"]["state"] == "home"
            assert not (at_home and at_b), "宠物同时出现在两台机器上！"
            return sa["owner_state"]["state"], at_b

        wait(lambda: where() == ("away", True), "A 的宠物走出右边缘，出现在 B 上", 60)
        wait(lambda: b.snap()["frames"].get(f"guest-{ida}", {}).get("visible") is True, "B 上的访客窗口可见")
        for _ in range(10):
            where()
            time.sleep(0.5)
        a.call("recall")
        wait(lambda: where() == ("home", False), "召回后回到 A", 60)
        assert "宠物回家：Returned" in a.log_text(), "召回应该走正常的 Return 流程，而不是超时收回"
        print("  ✓ 正常回家（Return），不是超时收回")

        print("串门途中对方断电…")
        a.call("go_visit", side="right")
        wait(lambda: where() == ("away", True), "再次出门到 B", 60)
        b.crash()
        t0 = time.time()
        wait(lambda: where()[0] == "home", "B 断电后宠物回到 A", 60)
        print(f"  ✓ 约 {time.time() - t0:.0f} 秒后收回，期间从未同时出现")
        assert "宠物回家：Reclaimed" in a.log_text()

        print("断线期间的变化自动补齐…")
        offline_ev = a.call(
            "save_event",
            title="B 离线时加的日程",
            notes="",
            all_day=True,
            start="2026-11-04T00:00:00",
            end="2026-11-05T00:00:00",
            repeat="none",
            remind_before_min=None,
        )
        b.start()
        wait(lambda: idb in a.snap()["peers"] and ida in b.snap()["peers"], "B 重启后自动重连", 60)
        wait(lambda: any(r["id"] == offline_ev for r in b.snap()["calendar"]), "B 补齐离线期间的日程")

        print("桌面建造…")
        assert "panel" not in a.snap()["windows"], "面板应该按需创建，平时不常驻"
        assert a.snap()["strips"] == [], "没有方块时不应创建建造带窗口"
        a.call("set_build_mode", on=True)
        wait(lambda: len(a.snap()["strips"]) >= 1, "进入建造模式后出现建造带窗口")
        wait(lambda: a.snap()["pet_pos"] is not None and a.snap()["frames"]["pet"]["anim"] in ("idle", "walk", "sleep"), "宠物站在地面上")
        y0 = a.snap()["pet_pos"][1]
        cols = a.call("world_fill", label=a.snap()["strips"][0], rows=2, block="planks")
        assert cols > 10, cols
        wait(lambda: a.snap()["pet_pos"][1] == y0 - 48, "铺两层木板后宠物站到了方块上（高 48 像素）")
        a.call("world_clear")
        wait(lambda: a.snap()["pet_pos"][1] == y0, "清空后宠物落回地面")
        a.call("set_build_mode", on=False)
        wait(lambda: a.snap()["strips"] == [], "退出建造模式且没有方块时，建造带窗口被销毁")

        print("搭建的方块重启后还在…")
        a.call("set_build_mode", on=True)
        wait(lambda: len(a.snap()["strips"]) >= 1, "进入建造模式")
        strip = a.snap()["strips"][0]
        a.call("world_fill", label=strip, rows=1, block="bricks")
        for x in range(4):  # 水不是实心的，宠物会走进水里、踩在砖上
            a.call("world_set", label=strip, x=x, y=1, block="water")
        a.call("set_build_mode", on=False)
        blocks = a.snap()["blocks"]
        assert blocks > 10, blocks
        a.restart()
        wait(lambda: a.snap()["blocks"] == blocks, "重启后方块数量不变")
        wait(lambda: len(a.snap()["strips"]) >= 1, "重启后自动显示建造带")
        wait(lambda: a.snap()["pet_pos"] is not None and a.snap()["pet_pos"][1] == y0 - 24, "宠物站在保留下来的方块上")
        a.call("patch_settings", keep_world=False)
        a.restart()
        wait(lambda: a.snap()["blocks"] == 0 and a.snap()["strips"] == [], "关闭“保留”后，重启时清空")
        a.call("patch_settings", keep_world=True)
        wait(lambda: idb in a.snap()["peers"] and ida in b.snap()["peers"], "A 重启后和 B 重新连上", 60)
        a.call("open_panel", tab="overview")
        wait(lambda: "panel" in a.snap()["windows"], "打开面板时才创建面板窗口")

        print("导入像素形象…")
        def holder_w():
            rects = a.snap()["pet_hit"] or []
            return rects[-1][2] if rects else None

        wait(lambda: holder_w() is not None and abs(holder_w() - 112) < 2, "默认形象的点击范围 112 像素宽")
        bad = os.path.join(base, "bad-pet")
        os.makedirs(bad)
        write_png(os.path.join(bad, "spritesheet.png"), 64, 64, lambda x, y: (0, 0, 0, 255))
        try:
            a.call("import_skin", path=bad)
            raise AssertionError("尺寸不对的精灵图不应该导入成功")
        except RuntimeError as e:
            assert "64 × 64" in str(e), e
            print("  ✓ 尺寸不对的精灵图被拒绝，并说明了原因")
        make_skin(os.path.join(base, "my-pet"))
        assert a.call("import_skin", path=os.path.join(base, "my-pet")) == "test-cat"
        assert a.snap()["snapshot"]["skin"] == "test-cat"
        # 小人在标准帧里是 64 × 152，按身高缩放到约 95 像素：点击范围应该约 40 像素宽。
        wait(lambda: abs(holder_w() - 64 * 112 * 0.85 / 152) < 2, "宠物窗口换上了导入的形象（点击范围贴合小人）")
        a.call("set_pet_skin", id=None)
        wait(lambda: abs(holder_w() - 112) < 2, "换回默认的步步")
        a.call("set_pet_skin", id="test-cat")
        wait(lambda: abs(holder_w() - 40) < 2, "再换上导入的形象")

        print("踢出成员…")
        epoch0 = a.snap()["snapshot"]["group"]["epoch"]
        a.call("kick", node=idb)
        wait(lambda: b.snap()["snapshot"]["group"] is None, "B 被移出家庭组")
        assert a.snap()["snapshot"]["group"]["epoch"] == epoch0 + 1, "踢人后应更换组密钥"
        wait(lambda: idb not in a.snap()["peers"], "连接已断开")
        time.sleep(6)
        assert idb not in a.snap()["peers"], "被踢的机器不应该能重新连上"
        print("  ✓ 组密钥已更换，被踢的机器无法重连")
        ok = True
        print("全部通过")
    finally:
        for inst in (a, b):
            if inst:
                inst.stop()
        if not ok:
            for inst in ("A", "B"):
                p = os.path.join(base, inst, "log.txt")
                if os.path.exists(p):
                    print(f"\n===== {inst} 日志（最后 60 行）=====")
                    with open(p, errors="replace") as f:
                        lines = [l for l in f.readlines() if "steppy" in l or "panicked" in l]
                        print("".join(lines[-60:]))
        shutil.rmtree(base, ignore_errors=True)
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
