"""Install a built candidate into a temporary application. No registry uploads."""

import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]
ARTIFACTS = ROOT / "artifacts"


def run(command, cwd, env=None):
    subprocess.run([str(value) for value in command], cwd=cwd, env=env, check=True)


def unpack(artifact, destination):
    if artifact.suffix == ".zip":
        with zipfile.ZipFile(artifact) as archive:
            for name in archive.namelist():
                if (
                    not (destination / name)
                    .resolve()
                    .is_relative_to(destination.resolve())
                ):
                    raise ValueError("Unsafe archive path")
            archive.extractall(destination)
    else:
        with tarfile.open(artifact) as archive:
            for item in archive.getmembers():
                if (
                    item.issym()
                    or item.islnk()
                    or (
                        not (destination / item.name)
                        .resolve()
                        .is_relative_to(destination.resolve())
                    )
                ):
                    raise ValueError("Unsafe archive entry")
            archive.extractall(destination, filter="data")


with tempfile.TemporaryDirectory(prefix="originchaindb-install-") as temporary:
    app = Path(temporary)
    unpack(
        next(ARTIFACTS.glob("*.crate" if "rust" == "rust" else "*.tar.gz")),
        app / "package",
    )
    module = next((app / "package").rglob("Cargo.toml")).parent
    (app / "Cargo.toml").write_text(
        '[package]\nname="installcheck"\nversion="0.0.0"\nedition="2021"\n[dependencies]\noriginchaindb={path='
        + json.dumps(module.as_posix())
        + "}\n"
    )
    (app / "src").mkdir()
    (app / "src/main.rs").write_text(
        'use originchaindb::{Client,BatchOptions,PageOptions};\nfn main(){let db=Client::new("https://example.invalid","test").unwrap();let rows=db.rows();let _b=rows.write_batches("app.items",vec![],BatchOptions::default());let _p=rows.pages("app.items","id",PageOptions::default());}\n'
    )
    run(
        ["cargo", "check", "--offline"],
        app,
        dict(
            os.environ,
            CARGO_TARGET_DIR=str(
                Path(os.environ.get("CARGO_TARGET_DIR", str(ROOT / "target"))).resolve()
            ),
        ),
    )
print("Fresh package application passed: " + "rust")
