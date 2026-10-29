#!/usr/bin/env python3
"""Prepare the first WinGet submission from the actual release ZIP and dist plan."""

import argparse
import hashlib
import json
import re
import tarfile
import tempfile
import zipfile
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--package-id", default="peymanAzad.Ario")
    args = parser.parse_args()
    if not re.fullmatch(r"[A-Za-z0-9]+(?:\.[A-Za-z0-9]+)+", args.package_id):
        parser.error("package-id must contain dot-separated publisher and package names")

    plan = json.loads(args.plan.read_text())
    if plan["announcement_is_prerelease"]:
        parser.error("WinGet manifests are only generated for stable releases")
    release = next(item for item in plan["releases"] if item["app_name"] == "ario")
    version = release["app_version"]
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
        parser.error("WinGet releases must use a major.minor.patch version")
    hosting = release["hosting"]["github"]
    url = hosting["artifact_base_url"] + hosting["artifact_download_path"] + "/" + args.archive.name
    digest = hashlib.sha256(args.archive.read_bytes()).hexdigest().upper()
    with zipfile.ZipFile(args.archive) as archive:
        paths = []
        for name in ("ario.exe", "ario_daemon.exe"):
            matches = [path for path in archive.namelist() if Path(path).name == name]
            if len(matches) != 1:
                parser.error(f"expected exactly one {name} in the Windows archive")
            paths.append(matches[0])
    if Path(paths[0]).parent != Path(paths[1]).parent:
        parser.error("the TUI and daemon must be installed beside each other")

    quote = json.dumps
    header = f"PackageIdentifier: {args.package_id}\nPackageVersion: {quote(version)}\n"
    installer = header + """InstallerType: zip
NestedInstallerType: portable
UpgradeBehavior: uninstallPrevious
Commands:
  - ario
  - ario_daemon
Dependencies:
  PackageDependencies:
    - PackageIdentifier: aria2.aria2
Installers:
  - Architecture: x64
    NestedInstallerFiles:
"""
    for path, alias in zip(paths, ("ario", "ario_daemon")):
        installer += f"      - RelativeFilePath: {quote(path)}\n        PortableCommandAlias: {alias}\n"
    installer += f"    InstallerUrl: {quote(url)}\n    InstallerSha256: {digest}\n"
    installer += "ManifestType: installer\nManifestVersion: 1.9.0\n"
    locale = header + """PackageLocale: en-US
Publisher: peymanAzad
PublisherUrl: https://github.com/peymanAzad
PublisherSupportUrl: https://github.com/peymanAzad/ario/issues
PackageName: Ario
PackageUrl: https://github.com/peymanAzad/ario
License: MIT
LicenseUrl: https://github.com/peymanAzad/ario/blob/master/LICENSE
ShortDescription: A terminal download manager powered by aria2
Moniker: ario
Tags:
  - download
  - terminal
  - bittorrent
ManifestType: defaultLocale
ManifestVersion: 1.9.0
"""
    version_manifest = header + "DefaultLocale: en-US\nManifestType: version\nManifestVersion: 1.9.0\n"
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        directory = root / "manifests" / args.package_id[0].lower()
        directory = directory.joinpath(*args.package_id.split("."), version)
        directory.mkdir(parents=True)
        for suffix, content in (("installer", installer), ("locale.en-US", locale), ("", version_manifest)):
            filename = args.package_id + (f".{suffix}" if suffix else "") + ".yaml"
            (directory / filename).write_text(content, encoding="utf-8")
        with tarfile.open(args.output, "w:gz") as archive:
            archive.add(root / "manifests", arcname="manifests")
    print(args.output)


if __name__ == "__main__":
    main()
