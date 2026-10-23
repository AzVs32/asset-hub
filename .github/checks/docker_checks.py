"""共享入口：选择本机或 Docker 模式，准备环境并汇总结果。

调用方向为：包的 check.py → 本模块 → runner.py，没有反向导入。
"""
import argparse
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import traceback
import uuid

from runner import CheckContext, CheckFailure, EVENT_PREFIX, summary

DOCKER_IMAGES = {
    "debian": "debian:latest",
    "fedora": "fedora:latest",
    "arch": "archlinux:latest",
    "alpine": "alpine:latest",
}
INSTALL_COMMANDS = {
    "debian": [
        "apt-get update",
        "DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "
        "build-essential python3 curl ca-certificates",
    ],
    "fedora": ["dnf install -y gcc gcc-c++ make python3 curl-minimal ca-certificates"],
    "arch": ["pacman -Syu --noconfirm --needed base-devel python curl ca-certificates"],
    "alpine": ["apk add --no-cache build-base python3 curl ca-certificates"],
}


def docker_script(distribution, entry):
    """生成容器内的 Shell 脚本；entry 是包入口相对挂载目录的路径。"""
    stages = [("安装系统依赖", command) for command in INSTALL_COMMANDS[distribution]]
    stages.extend([
        ("下载 Rust 安装器", 'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/rustup-init.sh'),
        ("安装 Rust 工具链", "sh /tmp/rustup-init.sh -y --profile minimal --default-toolchain stable --component rustfmt --component clippy"),
        ("启用 Rust 工具链", '. "$HOME/.cargo/env"'),
        ("确认 Rust 工具链", "rustc --version --verbose"),
        # 不带 --docker，容器中的 Python 直接运行检查，避免再次创建容器。
        ("运行包检查", shlex.join(["python3", "-B", "-u", entry.as_posix()])),
    ])
    script = []
    for phase, command in stages:
        # 用标准 JSON 和 shell 引用保留包含引号、换行的命令，避免手工转义。
        # 只收起环境安装的详细输出；Rust 检查仍实时显示，完整输出统一交给宿主机保存。
        quiet = phase in {"安装系统依赖", "下载 Rust 安装器", "安装 Rust 工具链"}
        event = EVENT_PREFIX + json.dumps({"phase": phase, "command": command, "quiet": quiet})
        script.extend([
            "printf '%s\\n' " + shlex.quote(event),
            'printf "\\n[%s][%s] 开始\\n" "$AH_CHECK_LABEL" ' + shlex.quote(phase),
            "printf '%s\\n' " + shlex.quote("命令：" + command),
            command,
        ])
    return "\n".join(script)


def cleanup_container(ctx, container):
    """只清理本次命名的容器，返回清理错误；None 表示无需再清理。"""
    command = ["docker", "rm", "--force", container]
    # 捕获输出以区分“容器已被 --rm 删除”和真正失败，不依赖英文错误消息。
    # **options 将公共参数传给 subprocess.run；超时可避免清理阶段无限等待。
    options = dict(stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                   text=True, encoding="utf-8", errors="replace", timeout=15)
    try:
        removed = subprocess.run(command, **options)
        if removed.returncode == 0:
            return None
        # --rm 正常删除后，再次 rm 会返回非零。查询成功且没有匹配容器才算正常。
        # 名称包含本次随机标识并使用完整匹配，不检查或删除其他任务的容器。
        probe = subprocess.run([
            "docker", "container", "ls", "--all", "--quiet",
            "--filter", f"name=^/{container}$",
        ], **options)
        if probe.returncode == 0 and not probe.stdout.strip():
            return None
        ctx.write(f"容器清理未完成：{container}\n{removed.stdout}")
        if probe.returncode:
            ctx.write(f"容器状态查询也失败：\n{probe.stdout}")
        return CheckFailure("容器清理", shlex.join(command), removed.returncode)
    except (OSError, subprocess.TimeoutExpired) as error:
        ctx.write(f"容器清理未完成：{container}：{error}\n")
        if isinstance(error, subprocess.TimeoutExpired):
            code = 124
        elif isinstance(error, FileNotFoundError):
            code = 127
        else:
            code = 1
        return CheckFailure("容器清理", shlex.join(command), code)


def check_docker(root, script, distribution):
    """逐个执行环境；单个环境失败后继续，最终任一失败都会返回非零。"""
    entry = Path(script).resolve().relative_to(root)
    shared = Path(__file__).resolve().parent
    # 连接失败时只报告一次，不重复启动四个注定失败的检查。
    with CheckContext(root, "docker") as ctx:
        try:
            ctx.run("连接 Docker", ["docker", "info", "--format", "{{.ServerVersion}}"])
        except CheckFailure as error:
            ctx.failure(error)
            ctx.write(summary("Docker", ctx.path, error))
            return 1
    results = []
    for name in (DOCKER_IMAGES if distribution == "all" else [distribution]):
        image = DOCKER_IMAGES[name]
        error = None
        with CheckContext(root, name) as ctx:
            container = f"ah-check-{name}-{uuid.uuid4().hex[:8]}"
            started = False
            try:
                ctx.run("拉取镜像", ["docker", "pull", image], quiet=True)
                # 源码只读挂载。显式拉取之后使用本地镜像，避免重复拉取掩盖失败阶段。
                command = [
                    "docker", "run", "--rm", "--pull=never", "--name", container,
                    "--volume", f"{root}:/workspace:ro",
                    "--volume", f"{shared}:/ci-support:ro", "--workdir", "/workspace",
                    "--env", "CARGO_TARGET_DIR=/tmp/cargo-target",
                    "--env", "AH_CHECK_SUPPORT_DIR=/ci-support", "--env", "AH_CHECK_EVENTS=1", "--env", f"AH_CHECK_LABEL={name}",
                    image, "sh", "-ec", docker_script(name, entry),
                ]
                started = True
                ctx.run("启动容器", command, display=shlex.join(command[:-1]) + " <内置容器脚本>")
            except CheckFailure as failure:
                error = failure
                ctx.failure(error)
            finally:
                # Ctrl+C 时也只清理本次命名的容器，不影响用户的其他容器。
                if started:
                    cleanup_error = cleanup_container(ctx, container)
                    if cleanup_error:
                        ctx.failure(cleanup_error)
                        # 原检查失败优先保留；若仅清理失败，也不能把整个流程报告为通过。
                        if error is None:
                            error = cleanup_error
            ctx.write(summary(name, ctx.path, error))
            results.append((name, ctx.path, error))
    print("\n========== 最终检查结果 ==========", flush=True)
    for name, path, error in results:
        print(summary(name, path, error), end="", flush=True)
    return int(any(error is not None for _, _, error in results))


def run_checks(root, check, *, script, description=None):
    """读取参数并选择模式；check 是各包传入的检查函数，接收 CheckContext。"""
    # Windows 重定向输出时也使用 UTF-8，避免中文日志受系统代码页影响。
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", errors="replace")
    root = Path(root).resolve()
    parser = argparse.ArgumentParser(description=description)
    # --docker 不带值时使用 all；不传 --docker 则直接使用本机工具链。
    parser.add_argument(
        "--docker", nargs="?", const="all", choices=["all", *DOCKER_IMAGES],
        metavar="发行版", help="检查全部容器，或指定 debian、fedora、arch、alpine",
    )
    args = parser.parse_args()
    try:
        if args.docker:
            return check_docker(root, script, args.docker)
        with CheckContext(root, os.environ.get("AH_CHECK_LABEL", "native")) as ctx:
            error = None
            try:
                # 此处才执行传入的函数；检查内容由各包定义，共享入口不规定检查清单。
                check(ctx)
            except CheckFailure as failure:
                error = failure
            except Exception:
                # 文件操作等非子进程异常也必须记录并判定为失败。
                ctx.write(traceback.format_exc())
                error = CheckFailure(ctx.phase, ctx.command, 1)
            if error:
                ctx.failure(error)
            if not ctx.events:
                ctx.write(summary(ctx.label, ctx.path, error))
                if error and ctx.github:
                    print(f"::error::{root.name} 在 {error.phase} 阶段失败，退出码 {error.code}", flush=True)
            # bool 转 int：没有错误为 0，存在错误为 1，供终端和 CI 判断结果。
            return int(error is not None)
    except KeyboardInterrupt:
        print("\n检查已中断。", file=sys.stderr)
        return 130
