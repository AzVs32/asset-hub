#!/usr/bin/env python3
"""ah-id 检查流程，需要 Python 3.12+；--docker 可检查 Linux 容器。

执行和日志由仓库 .github/checks 提供，本文件只定义 ah-id 的检查内容。
"""
import json
import os
from pathlib import Path
import sys
import tarfile
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
# 本机从仓库查找共享工具；容器通过只读挂载和环境变量指定同一份工具。
# 沿父目录查找，避免把当前库在仓库中的层级写死。
SHARED = os.environ.get("AH_CHECK_SUPPORT_DIR") or next(
    (parent / ".github/checks" for parent in ROOT.parents
     if (parent / ".github/checks/docker_checks.py").is_file()), None,
)
if SHARED is None or not (Path(SHARED) / "docker_checks.py").is_file():
    raise SystemExit("未找到 .github/checks，请在完整仓库内运行；独立库副本可直接使用 Cargo。")
# 共享目录在容器内只读，不生成 __pycache__。sys.path 用来指定导入模块的查找目录。
sys.dont_write_bytecode = True
sys.path.insert(0, str(SHARED))
from docker_checks import run_checks


def check_packages(ctx):
    """验证实际归档中的文件可用，整个过程不会发布包。"""
    # 联合验证实际生成的归档，无须先发布宏包，也不依赖 Cargo 的临时注册表验证实现。
    ctx.cargo("生成打包产物", "package", "--workspace", "--all-features", "--allow-dirty", "--no-verify")
    archives = Path(ctx.env["CARGO_TARGET_DIR"]) / "package"
    manifests = {"ah-id-macros": "macros/Cargo.toml", "ah-id": "Cargo.toml"}
    # with 会在离开此代码块时自动删除临时解压目录，包括检查失败的情况。
    with tempfile.TemporaryDirectory(prefix="ah-id-package-") as temporary:
        ctx.start("解包产物", "Python tarfile：解压宏包和运行库")
        extracted = Path(temporary)
        packages = {}
        for name, manifest in manifests.items():
            version = tomllib.loads((ctx.root / manifest).read_text(encoding="utf-8"))["package"]["version"]
            directory = f"{name}-{version}"
            with tarfile.open(archives / f"{directory}.crate") as archive:
                archive.extractall(extracted, filter="data")
            # Cargo 归档的根目录就是“包名-版本”，无需再用通配符猜测路径。
            packages[name] = extracted / directory
        runtime = packages["ah-id"] / "Cargo.toml"
        # json.dumps 保留路径中的引号和反斜杠，Windows 路径也能用于此 TOML 配置。
        patch = "patch.crates-io.ah-id-macros.path=" + json.dumps(str(packages["ah-id-macros"]))
        # 本地 patch 会更新解包副本的 lockfile，因此此处不使用 --locked。
        ctx.cargo("编译打包产物", "check", "--manifest-path", str(runtime), "--all-features", "--config", patch, locked=False)


def check_package(ctx):
    """本机和容器共用的检查清单；ctx 提供命令执行、环境和日志能力。"""
    ctx.cargo("格式检查", "fmt", "--all", "--", "--check", locked=False)
    ctx.cargo("默认功能测试", "test", "--workspace")
    ctx.cargo("全部功能测试", "test", "--workspace", "--all-features")
    ctx.cargo("无默认功能测试", "test", "--no-default-features")
    # 示例声明所需功能；分别验证独立使用及启用派生宏的用法。
    for example in tomllib.loads((ctx.root / "Cargo.toml").read_text(encoding="utf-8"))["example"]:
        required = example.get("required-features", [])
        feature_sets = [required] if "derive" in required else [required, [*required, "derive"]]
        for features in feature_sets:
            ctx.cargo(f"示例：{example['name']}（{','.join(features)}）", "run",
                      "--example", example["name"], "--no-default-features", "--features", ",".join(features))
    ctx.cargo("依赖改名示例", "run", "--manifest-path", "tests/fixtures/renamed/Cargo.toml")
    ctx.cargo("Clippy 检查", "clippy", "--workspace", "--all-targets", "--all-features", "--", "-D", "warnings")
    previous = ctx.env.get("RUSTDOCFLAGS", "")
    ctx.env["RUSTDOCFLAGS"] = (previous + " -D warnings").strip()
    ctx.cargo("文档检查", "doc", "--workspace", "--all-features", "--no-deps")
    check_packages(ctx)


# 直接执行此文件时启动检查；导入此文件只定义函数，不会自动运行 Cargo。
if __name__ == "__main__":
    # 传入函数本身，由共享入口选择执行模式；SystemExit 将返回值变成进程退出码。
    raise SystemExit(run_checks(ROOT, check_package, script=__file__, description=__doc__))
