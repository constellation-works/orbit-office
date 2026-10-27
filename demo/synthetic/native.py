"""Prepare the demo-local native helper; never install a host program."""
from pathlib import Path
import platform
import shlex

from common import HERE, run


def prepare(root, env):
    if platform.system() != 'Darwin':
        raise RuntimeError('this synthetic pack requires macOS AppKit, PDFKit and say')
    tools = Path(root) / 'native-tools'
    tools.mkdir(mode=0o700, exist_ok=True)
    binary = tools / 'synthetic-native'
    if not binary.exists():
        run(['swiftc', '-module-cache-path', tools / 'module-cache', HERE / 'native.swift', '-o', binary],
            env=env, timeout=180)
    # Office's optional converter contract is argv-compatible with pdftotext.
    # This explicit demo adapter extracts the REAL generated PDF through PDFKit.
    # It is not Poppler, does not replace a host tool, and is never used in CI
    # as evidence about the Poppler implementation.
    converter = tools / 'pdftotext'
    converter.write_text('#!/bin/sh\nexec ' + shlex.quote(str(binary)) + ' pdf "$@"\n')
    converter.chmod(0o700)
    return tools
