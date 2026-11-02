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
import urllib.request

APP, HOOK = sys.argv[1], sys.argv[2]
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FIX = os.path.join(ROOT, "crates", "steppy-core", "tests", "fixtures", "claude")


class Instance:
    def __init__(self, name, base):
        self.name = name
        self.dir = os.path.join(base, name)
        self.data = os.path.join(self.dir, "data")
        home = os.path.join(self.dir, "home")
        os.makedirs(self.data)
        os.makedirs(home)
        env = dict(os.environ, XDG_DATA_HOME=self.data, HOME=home, STEPPY_E2E="1")
        self.log = open(os.path.join(self.dir, "log.txt"), "w")
        self.proc = subprocess.Popen([APP], env=env, stdout=self.log, stderr=subprocess.STDOUT)
        self.endpoint = os.path.join(self.data, "steppy", "endpoint.json")
        wait(lambda: os.path.exists(self.endpoint) and self.ep().get("pid") == self.proc.pid, f"{name} 启动", 60)
        wait(lambda: self.snap()["snapshot"]["net_port"] > 0, f"{name} 网络就绪", 30)

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
            sa, sb = a.snap(), b.snap()
            at_home = sa["frames"].get("pet", {}).get("visible") and sa["owner_state"]["state"] == "home"
            at_b = ida in sb["visible_guests"]
            assert not (at_home and at_b), "宠物同时出现在两台机器上！"
            return sa["owner_state"]["state"], at_b

        wait(lambda: where() == ("away", True), "A 的宠物走出右边缘，出现在 B 上", 60)
        wait(lambda: b.snap()["frames"].get(f"guest-{ida}", {}).get("visible") is True, "B 上的访客窗口可见")
        for _ in range(10):
            where()
            time.sleep(0.5)
        a.call("recall")
        wait(lambda: where() == ("home", False), "召回后回到 A", 60)

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
