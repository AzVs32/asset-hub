#!/usr/bin/env python3
"""ah-config 统一检查，需要 Python 3.12+；--docker 可检查 Linux 容器。

复用仓库 .github/checks 的执行与日志能力，不发布包。
"""
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tarfile
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
SHARED = os.environ.get("AH_CHECK_SUPPORT_DIR") or next(
    (parent / ".github/checks" for parent in ROOT.parents
     if (parent / ".github/checks/docker_checks.py").is_file()), None,
)
if SHARED is None or not (Path(SHARED) / "docker_checks.py").is_file():
    raise SystemExit("未找到 .github/checks，请在完整仓库内运行；独立库副本可直接使用 Cargo。")
sys.dont_write_bytecode = True
sys.path.insert(0, str(SHARED))
from docker_checks import run_checks
from runner import CheckFailure

CONSUMER = "tests/fixtures/consumer/Cargo.toml"


def check_compile_failure(ctx, binary, expected, code=None):
    """必须由指定使用方产生预期诊断，不能把其他 Cargo 失败算作通过。"""
    command = ["cargo", "check", "--manifest-path", CONSUMER, "--bin", binary,
               "--locked", "--message-format=json"]
    ctx.start(f"预期编译失败：{binary}", shlex.join(command))
    result = subprocess.run(command, cwd=ctx.root, env=ctx.env,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                            text=True, encoding="utf-8", errors="replace")
    matched = False
    for line in result.stdout.splitlines(keepends=True):
        try:
            entry = json.loads(line)
        except ValueError:
            ctx.write(line)
            continue
        if entry.get("reason") != "compiler-message":
            continue
        message = entry["message"]
        ctx.write(message.get("rendered") or message["message"] + "\n")
        # 同时核对目标和源码位置，排除依赖 crate 或其他使用方的编译错误。
        diagnostic_code = (message.get("code") or {}).get("code")
        source = f"src/bin/{binary}.rs"
        if (entry["target"]["name"] == binary and message["level"] == "error"
                and expected in message["message"]
                and (code is None or diagnostic_code == code)
                and any(span["file_name"].replace("\\", "/").endswith(source)
                        for span in message["spans"])):
            matched = True
    if result.returncode != 101 or not matched:
        ctx.write(f"未得到预期诊断：{expected}（Cargo 退出码 {result.returncode}）\n")
        raise CheckFailure(ctx.phase, ctx.command, result.returncode or 1)
    ctx.write(f"[{ctx.label}][{ctx.phase}] 通过（已确认预期编译错误）\n")


def check_packages(ctx):
    """从真实发布归档编译，而不是从源码目录编译。"""
    ctx.cargo("生成打包产物", "package", "--workspace", "--all-features",
              "--allow-dirty", "--no-verify")
    archives = Path(ctx.env["CARGO_TARGET_DIR"]) / "package"
    manifests = {"ah-config-macros": "macros/Cargo.toml", "ah-config": "Cargo.toml"}
    with tempfile.TemporaryDirectory(prefix="ah-config-package-") as temporary:
        ctx.start("解包产物", "Python tarfile：解压宏包和运行库")
        packages = {}
        for name, manifest in manifests.items():
            version = tomllib.loads((ctx.root / manifest).read_text(encoding="utf-8"))["package"]["version"]
            directory = f"{name}-{version}"
            with tarfile.open(archives / f"{directory}.crate") as archive:
                archive.extractall(temporary, filter="data")
            packages[name] = Path(temporary) / directory
        # 仅将解包后的宏包接到解包后的运行库，无须预先发布宏包。
        patch = "patch.crates-io.ah-config-macros.path=" + json.dumps(str(packages["ah-config-macros"]))
        runtime = str(packages["ah-config"] / "Cargo.toml")
        # patch 会更新临时副本的锁文件，不能使用源码检查的 --locked。
        ctx.cargo("编译打包产物", "check", "--manifest-path", runtime,
                  "--all-targets", "--all-features", "--config", patch, locked=False)
        ctx.cargo("打包产物无默认功能检查", "check", "--manifest-path", runtime,
                  "--no-default-features", "--config", patch, locked=False)


def check_package(ctx):
    """统一覆盖常规测试、独立使用方、宏错误契约与发布归档。"""
    ctx.cargo("格式检查", "fmt", "--all", "--", "--check", locked=False)
    # 默认功能即全部功能；这些测试也执行五个案例中的行为测试和文档测试。
    ctx.cargo("全部功能测试", "test", "--workspace", "--all-features")
    ctx.cargo("无默认功能测试", "test", "-p", "ah-config", "--no-default-features")
    for release in (False, True):
        arguments = ["run", "--manifest-path", CONSUMER, "--bin", "ah-config-consumer"]
        if release:
            arguments.append("--release")
        ctx.cargo("独立使用方：Release/LTO" if release else "独立使用方：Debug", *arguments)
    check_compile_failure(ctx, "conflicting_defaults", "duplicate default option")
    check_compile_failure(ctx, "wrong_defaults", "mismatched types", "E0308")
    check_compile_failure(ctx, "wrong_validator", "no method named `map_err`", "E0599")
    check_compile_failure(ctx, "generic", "configuration declarations require a concrete non-generic type")
    for binary in ("config_default_conflict", "config_derived_default_conflict",
                   "value_default_conflict", "value_derived_default_conflict"):
        check_compile_failure(ctx, binary, "conflicting implementations", "E0119")
    for binary in ("missing_default_field", "missing_variant_default_field"):
        check_compile_failure(ctx, binary, "fields of a default value require")
    check_compile_failure(ctx, "wrong_value_default", "mismatched types", "E0308")
    for binary in ("implicit_default", "implicit_value_default"):
        check_compile_failure(ctx, binary, "Default", "E0277")
    ctx.cargo("Clippy 检查", "clippy", "--workspace", "--all-targets",
              "--all-features", "--", "-D", "warnings")
    ctx.env["RUSTDOCFLAGS"] = (ctx.env.get("RUSTDOCFLAGS", "") + " -D warnings").strip()
    ctx.cargo("文档检查", "doc", "--workspace", "--all-features", "--no-deps")
    ctx.cargo("无默认功能文档检查", "doc", "-p", "ah-config", "--no-default-features", "--no-deps")
    check_packages(ctx)


if __name__ == "__main__":
    raise SystemExit(run_checks(ROOT, check_package, script=__file__, description=__doc__))
