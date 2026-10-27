#!/usr/bin/env python3
"""Regenerate the bundled synthetic deck and PDF using only the standard library."""
from pathlib import Path
from xml.sax.saxutils import escape
import zipfile

from common import HERE

SLIDES = [
    ("SIMULATED Lantern orientation", "Invented system, people and company. No production data."),
    ("Lantern dispatch system", "The durable spool routes paper-kite orders. Mira Vale owns recovery."),
    ("Access and leave", "Lantern-Readers is read-only. PTO allowance is 20 days; arrange coverage five working days ahead."),
]


def build(directory):
    directory = Path(directory)
    directory.mkdir(parents=True, exist_ok=True)
    files = {
        "[Content_Types].xml": '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>' + ''.join(f'<Override PartName="/ppt/slides/slide{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>' for i in range(1, 4)) + '</Types>',
        "_rels/.rels": '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>',
        "ppt/presentation.xml": '<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst>' + ''.join(f'<p:sldId id="{255+i}" r:id="rId{i}"/>' for i in range(1, 4)) + '</p:sldIdLst><p:sldSz cx="12192000" cy="6858000"/></p:presentation>',
        "ppt/_rels/presentation.xml.rels": '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">' + ''.join(f'<Relationship Id="rId{i}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide{i}.xml"/>' for i in range(1, 4)) + '</Relationships>',
    }
    for i, (title, body) in enumerate(SLIDES, 1):
        shapes = ''.join(f'<p:sp><p:nvSpPr><p:cNvPr id="{j}" name="Text {j}"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>{escape(text)}</a:t></a:r></a:p></p:txBody></p:sp>' for j, text in enumerate([title, body], 2))
        files[f"ppt/slides/slide{i}.xml"] = '<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>' + shapes + '</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>'
    with zipfile.ZipFile(directory / "orientation.pptx", "w", zipfile.ZIP_DEFLATED) as archive:
        for name, content in sorted(files.items()):
            entry = zipfile.ZipInfo(name, date_time=(2026, 9, 1, 0, 0, 0))
            entry.compress_type = zipfile.ZIP_DEFLATED
            entry.external_attr = 0o600 << 16
            archive.writestr(entry, content)
    lines = ["SIMULATED - Acme Paper Kites PTO policy", "Effective 2026-09-01. This is an invented policy.",
             "Annual allowance: 20 days. Mira Vale is the coverage lead.",
             "Arrange coverage five working days before planned leave.",
             "Submit the request yourself; a draft does not mean approval.",
             "Official demo policy: https://policy.acme.example/pto"]
    stream = 'BT /F1 14 Tf 50 740 Td ' + ' 0 -26 Td '.join(f'({line}) Tj' for line in lines) + ' ET'
    objects = [b'<< /Type /Catalog /Pages 2 0 R >>', b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
               b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>',
               b'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',
               f'<< /Length {len(stream.encode())} >>\nstream\n{stream}\nendstream'.encode()]
    pdf = bytearray(b'%PDF-1.4\n')
    offsets = [0]
    for i, obj in enumerate(objects, 1):
        offsets.append(len(pdf))
        pdf.extend(f'{i} 0 obj\n'.encode() + obj + b'\nendobj\n')
    start = len(pdf)
    pdf.extend(f'xref\n0 {len(offsets)}\n0000000000 65535 f \n'.encode())
    for offset in offsets[1:]:
        pdf.extend(f'{offset:010d} 00000 n \n'.encode())
    pdf.extend(f'trailer\n<< /Size {len(offsets)} /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n'.encode())
    (directory / 'pto-policy.pdf').write_bytes(pdf)


if __name__ == '__main__':
    build(HERE / 'onboarding')
