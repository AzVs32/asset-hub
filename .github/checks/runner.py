"""通用执行能力：检查上下文、命令执行、日志及失败信息。

本模块不导入 Docker 调度模块，各包的执行模式由上层入口决定。
"""
from collections import deque
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import time

# 容器只报告阶段及失败元数据，完整命令输出由外层同时保存。
EVENT_PREFIX = "__AH_CHECK_EVENT__"


class CheckFailure(Exception):
    """保存失败阶段、命令和原始退出码，供上层统一汇总。"""

    def __init__(self, phase, command, code):
        super().__init__(phase)
        self.phase, self.command, self.code = phase, command, code


class CheckContext:
    """一次检查的工作目录、子进程环境、执行能力和日志。

    不同实例各自持有环境字典；with 代码块结束时会自动关闭日志文件。
    """

    def __init__(self, root, label):
        self.root = Path(root).resolve()
        self.label = label
        self.env = os.environ.copy()
        self.env["PYTHONIOENCODING"] = "utf-8"
        self.env.setdefault("CARGO_TARGET_DIR", str(self.root / "target" / "ci"))
        # 解包验证也使用相同的绝对构建路径，各包之间不共享可变环境状态。
        self.env["CARGO_TARGET_DIR"] = str(Path(self.env["CARGO_TARGET_DIR"]).resolve())
        self.events = self.env.get("AH_CHECK_EVENTS") == "1"
        self.github = self.env.get("GITHUB_ACTIONS") == "true"
        self.path = self.root / "target" / "ci-logs" / f"{label}.log"
        self.phase, self.command, self.reported_code = "初始化", "python ci/check.py", None
        # 安装等准备阶段只在终端显示进度，原始输出仍全部写入日志。
        # 只保留最后 40 行供失败时展开，避免把整份安装输出存入内存。
        self.quiet = False
        self.output_tail = deque(maxlen=40)
        # 容器源码只读，由宿主机保存日志，不在容器挂载目录内创建文件。
        self.file = None
        if not self.events:
            self.path.parent.mkdir(parents=True, exist_ok=True)
            self.file = self.path.open("w", encoding="utf-8")

    def __enter__(self):
        return self

    def __exit__(self, *_):
        if self.file:
            self.file.close()

    def write(self, text, *, console=True):
        """输出立即写入日志；console=False 时仅隐藏终端输出，不丢弃日志。"""
        if console:
            sys.stdout.write(text)
            sys.stdout.flush()
        if self.file:
            self.file.write(text)
            self.file.flush()

    def start(self, phase, command, *, quiet=False):
        """开始新阶段；容器内通过结构化输出将阶段传给宿主机。"""
        self.phase, self.command, self.reported_code = phase, command, None
        self.quiet = quiet
        self.output_tail.clear()
        if self.events:
            print(EVENT_PREFIX + json.dumps({"phase": phase, "command": command, "quiet": quiet}), flush=True)
        note = "（详细输出写入日志）" if quiet else ""
        self.write(f"\n[{self.label}][{phase}] 开始{note}\n")
        self.write(f"命令：{command}\n", console=not quiet or self.events)

    def read_line(self, line):
        # 普通输出直接保存；带约定前缀的 JSON 记录容器内部的命令和失败码。
        # 后续消息不能清除已报告的失败，避免外层进程退出码掩盖内部失败。
        if line.startswith(EVENT_PREFIX):
            try:
                event = json.loads(line[len(EVENT_PREFIX):])
                if isinstance(event.get("phase"), str) and isinstance(event.get("command"), str):
                    if "code" not in event:
                        # 容器 Shell 遇错即停；收到下一阶段意味着上一准备阶段成功。
                        if self.quiet:
                            self.write(f"[{self.label}][{self.phase}] 通过\n")
                        self.quiet = event.get("quiet") is True
                        self.output_tail.clear()
                        if self.quiet:
                            self.write(f"\n[{self.label}][{event['phase']}] 开始（详细输出写入日志）\n")
                    self.phase, self.command = event["phase"], event["command"]
                    code = event.get("code")
                    if type(code) is int and code != 0:
                        self.reported_code = code
                    if self.file:
                        self.file.write(line)
                        self.file.flush()
                    return
            except (ValueError, AttributeError):
                pass
        if self.quiet:
            self.output_tail.append(line)
        # 容器内仍将所有输出传给宿主机，由宿主机决定显示内容并保存完整日志。
        self.write(line, console=not self.quiet or self.events)

    def run(self, phase, command, *, display=None, quiet=False):
        """执行参数列表形式的命令，边运行边记录输出；失败时抛出 CheckFailure。"""
        self.start(phase, display or shlex.join(command), quiet=quiet)
        started = time.monotonic()
        if self.github:
            print(f"::group::{self.label} / {phase}", flush=True)
        try:
            with subprocess.Popen(
                command, cwd=self.root, env=self.env, stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT, text=True, encoding="utf-8", errors="replace",
            ) as process:
                try:
                    for line in process.stdout:
                        self.read_line(line)
                    code = process.wait()
                except BaseException:
                    # 中断或日志读取失败时先终止直接子进程，最多等 5 秒后强制结束。
                    process.terminate()
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()
                    raise
            # 优先保留内层原始失败码，例如 Cargo 的 101，而非 Docker 外层的 1。
            code = self.reported_code or code
            if code:
                if self.quiet and not self.events and self.output_tail:
                    self.write("错误输出（最后 40 行以内，完整输出见日志）：\n" + "".join(self.output_tail))
                raise CheckFailure(self.phase, self.command, code)
            if self.quiet and self.phase != phase:
                self.write(f"[{self.label}][{self.phase}] 通过\n")
            self.write(f"[{self.label}][{phase}] 通过（{time.monotonic() - started:.1f} 秒）\n")
        except OSError as error:
            self.write(f"{error}\n")
            raise CheckFailure(self.phase, self.command, 127 if isinstance(error, FileNotFoundError) else 1) from error
        finally:
            if self.github:
                print("::endgroup::", flush=True)

    def failure(self, error):
        if self.events:
            print(EVENT_PREFIX + json.dumps({
                "phase": error.phase, "command": error.command, "code": error.code,
            }), flush=True)
        self.write(f"[{self.label}][{error.phase}] 失败（退出码 {error.code}）\n")

    def cargo(self, phase, *arguments, locked=True):
        """收集任意数量的 Cargo 参数，默认遵循已有锁文件。"""
        command = ["cargo", *arguments]
        if locked:
            # -- 后面的内容属于子工具，--locked 必须插在这个分隔符之前。
            separator = command.index("--") if "--" in command else len(command)
            command.insert(separator, "--locked")
        self.run(phase, command)


def summary(label, log_path, error=None):
    """生成统一格式的最终结果，失败时附带可定位问题的信息。"""
    if error:
        return (f"\n{label}：失败\n失败阶段：{error.phase}\n失败命令：{error.command}\n"
                f"退出码：{error.code}\n完整日志：{log_path}\n")
    return f"\n{label}：通过\n完整日志：{log_path}\n"
