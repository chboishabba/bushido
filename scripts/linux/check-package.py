#!/usr/bin/env python3
"""Inspect a Bushido Debian candidate without installing or executing it."""
import argparse
import configparser
import io
import pathlib
import re
import subprocess
import tarfile
import tempfile


def run(*args):
    return subprocess.check_output(args)


def require(condition, reason):
    if not condition:
        raise ValueError(reason)


def check_package(package):
    def field(name):
        return run('dpkg-deb', '-f', str(package), name).decode().strip()

    require(field('Package') in {'bushido', 'bushido-browser'}, 'unexpected package name')
    arch = field('Architecture')
    require(arch in {'amd64', 'arm64'}, 'unsupported candidate architecture')
    depends = field('Depends')
    for name in ('libwebkit2gtk-4.1-0', 'libgtk-3-0', 'libsecret-tools', 'xdg-utils', 'ca-certificates'):
        require(re.search(r'(?<![a-z0-9+.-])' + re.escape(name) + r'(?![a-z0-9+.-])', depends), 'missing dependency: ' + name)
    require(re.search(r'libwebkit2gtk-4\.1-0\s*\(>=\s*2\.40\)', depends), 'missing WebKitGTK minimum')
    # Stream archive members without extraction; never run package executables.
    proc = subprocess.Popen(['dpkg-deb', '--fsys-tarfile', str(package)], stdout=subprocess.PIPE)
    desktop = None
    binary = False
    icons = set()
    license_found = False
    try:
        with tarfile.open(fileobj=proc.stdout, mode='r|*') as archive:
            for entry in archive:
                path = pathlib.PurePosixPath(entry.name)
                require(not path.is_absolute() and '..' not in path.parts, 'unsafe archive path')
                name = str(path)
                if name == 'usr/bin/bushido-browser':
                    require(entry.isfile() and entry.mode & 0o111, 'binary must be executable regular file')
                    header = archive.extractfile(entry).read(20)
                    machine = 62 if arch == 'amd64' else 183
                    require(len(header) == 20 and header[:6] == b'\x7fELF\x02\x01' and int.from_bytes(header[18:20], 'little') == machine, 'ELF architecture mismatch')
                    binary = True
                elif name.startswith('usr/share/applications/') and name.endswith('.desktop'):
                    require(desktop is None and entry.isfile() and entry.size < 65536, 'invalid desktop entry')
                    desktop = archive.extractfile(entry).read().decode('utf-8')
                elif name.startswith('usr/share/icons/hicolor/') and '/apps/' in name and entry.isfile():
                    icons.add(path.stem)
                elif name == 'usr/share/doc/bushido/LICENSE' and entry.isfile() and entry.size > 0:
                    license_found = True
        require(proc.wait() == 0, 'package archive read failed')
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait()
        proc.stdout.close()
    require(binary and desktop and license_found, 'missing executable, desktop entry or license')
    config = configparser.ConfigParser(interpolation=None)
    config.read_file(io.StringIO(desktop))
    d = config['Desktop Entry']
    require(d.get('Type') == 'Application', 'desktop Type must be Application')
    require(d.get('Exec') in {'bushido-browser', '/usr/bin/bushido-browser'}, 'unexpected or URL-accepting desktop Exec')
    require(not d.get('MimeType'), 'MIME handlers not accepted before URL activation is implemented')
    require(d.get('Terminal') == 'false', 'desktop launcher must not require terminal')
    require(d.get('Icon') in icons, 'desktop icon missing from packaged icons')
    require({'Network', 'WebBrowser'} <= set(d.get('Categories', '').split(';')), 'browser desktop categories missing')
    return arch


def self_test():
    # Synthetic package only: copies a local ELF for the archive structure.
    import platform
    import shutil
    arch = {'x86_64': 'amd64', 'aarch64': 'arm64'}.get(platform.machine())
    require(arch is not None, 'fixture requires x86-64 or aarch64')
    deps = 'libwebkit2gtk-4.1-0 (>= 2.40), libgtk-3-0, libsecret-tools, xdg-utils, ca-certificates'
    desktop = '[Desktop Entry]\nName=Bushido\nType=Application\nExec=bushido-browser\nIcon=bushido\nTerminal=false\nCategories=Network;WebBrowser;\n'
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp) / 'root'
        out = pathlib.Path(tmp) / 'fixture.deb'
        for path in ('DEBIAN', 'usr/bin', 'usr/share/applications', 'usr/share/icons/hicolor/32x32/apps', 'usr/share/doc/bushido'):
            (root / path).mkdir(parents=True)
        # dpkg-deb requires the package control directory to be traversable.
        (root / 'DEBIAN').chmod(0o755)
        shutil.copy2('/bin/true', root / 'usr/bin/bushido-browser')
        (root / 'usr/share/icons/hicolor/32x32/apps/bushido.png').write_bytes(b'fixture')
        (root / 'usr/share/doc/bushido/LICENSE').write_text('fixture license')
        def build(dependencies=deps, entry=desktop, architecture=arch):
            (root / 'DEBIAN/control').write_text(f'Package: bushido\nVersion: 0.14.0\nArchitecture: {architecture}\nMaintainer: Fixture <fixture@example.test>\nDepends: {dependencies}\nDescription: synthetic packaging fixture\n')
            (root / 'usr/share/applications/bushido.desktop').write_text(entry)
            run('dpkg-deb', '--build', '--root-owner-group', str(root), str(out))
        build()
        require(check_package(out) == arch, 'valid package fixture failed')
        mutations = [
            {'dependencies': deps.replace(', libsecret-tools', '')},
            {'dependencies': deps.replace(' (>= 2.40)', '')},
            {'entry': desktop.replace('Exec=bushido-browser', 'Exec=bushido-browser %U')},
            {'entry': desktop + 'MimeType=x-scheme-handler/https;\n'},
            {'entry': desktop.replace('Icon=bushido', 'Icon=other')},
            {'entry': desktop.replace('Network;WebBrowser;', 'Utility;')},
            {'entry': desktop.replace('Terminal=false', 'Terminal=true')},
            {'architecture': 'arm64' if arch == 'amd64' else 'amd64'},
        ]
        for mutation in mutations:
            build(**mutation)
            try:
                check_package(out)
            except ValueError:
                continue
            raise AssertionError(f'unsafe fixture accepted: {mutation}')
        print(f'{1 + len(mutations)}/{1 + len(mutations)} synthetic package checks PASS')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('package', nargs='?', type=pathlib.Path)
    parser.add_argument('--self-test', action='store_true')
    args = parser.parse_args()
    if args.self_test and args.package is None:
        self_test()
    elif args.package is not None and not args.self_test:
        print('Static package checks PASS:', check_package(args.package))
    else:
        parser.error('provide a package or --self-test')
