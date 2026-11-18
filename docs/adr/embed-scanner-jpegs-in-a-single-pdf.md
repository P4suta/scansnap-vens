---
date: 2026-11-18
status: proposed
---

# Preserve scanner images in one document

## Context

One Scan button press saves the whole duplex stack as one document.
The scanner returns a Joint Photographic Experts Group (JPEG) image for each side at 300 dots per inch.
Paperless-ngx watches its consumption directory for documents to import.
[Paperless-ngx usage](https://docs.paperless-ngx.com/usage/#the-consumption-directory)

## Decision

Use pdf-writer to embed each corrected JPEG as a DCTDecode image stream.
[pdf-writer documentation](https://docs.rs/pdf-writer/0.15.0/pdf_writer/)

Size each page from its pixel dimensions and scan resolution.
Stage the document in the output folder and publish it atomically after the scan completes.

## Alternatives

A separate JPEG per side preserves the scanner output but splits the document.
An external PDF converter adds an installation and process dependency.
Decoding and re-encoding JPEGs costs time and can degrade the image.

## Consequences

The daemon buffers the compressed PDF and limits its size.
Paperless-ngx handles optical character recognition after import.
Reconsider buffering if the size limit prevents scanning an ordinary stack.

## Confirmation

The emulator replay produced one document with six images from three sheets.
On 2026-11-18 one Scan button press saved a two-page PDF at 300 dots per inch.
Poppler rendered both pages.
