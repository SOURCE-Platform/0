#!/usr/bin/env python3
"""Synthetic Dashlane export fixtures (spec §10.7; Phase I).

Writes src-tauri/vault-helper/tests/fixtures/dashlane/. Fixed seed, so a
re-run produces the same bytes. Every value is invented: passwords are
`fixture-…` tokens, hosts end in `.test`, card numbers are the published
test numbers. Never point this at, or mix it with, a real export.

The 10^5-row stress file is not committed; the test writes it itself.

Usage: python3 scripts/gen-dashlane-fixtures.py
"""

import io
import random
import zipfile
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "src-tauri/vault-helper/tests/fixtures/dashlane"
CRED_HEADER = "username,username2,username3,title,password,note,url,category,otpSecret"
PAY_HEADER = "name,card_number,expiration,cardholder,cvv,billing_city"
NOTE_HEADER = "title,note,category"
TEST_CARDS = ["4111111111111111", "5555555555554444", "378282246310005"]


def cred(rng, i, user=None, host=None, otp=""):
    user = user or f"user{i}@example.test"
    host = host or f"site{i}.example.test"
    pw = f"fixture-{rng.randrange(16**8):08x}"
    return f"{user},,,Site {i},{pw},,https://{host}/login,,{otp}"


class Fixed:
    """Members with a fixed timestamp, so the ZIP bytes are reproducible."""

    def __init__(self, zf):
        self.zf = zf

    def writestr(self, name, text):
        info = zipfile.ZipInfo(name, date_time=(2026, 10, 16, 0, 0, 0))
        info.compress_type = zipfile.ZIP_DEFLATED
        self.zf.writestr(info, text)


def write(name, data):
    (OUT / name).write_bytes(data.encode() if isinstance(data, str) else data)


def main():
    rng = random.Random(20261016)
    OUT.mkdir(parents=True, exist_ok=True)

    write("valid-minimal.csv", "\n".join([CRED_HEADER, cred(rng, 1), cred(rng, 2)]) + "\n")

    # Quoting: commas, newlines and doubled quotes inside quoted fields.
    write("embedded.csv", CRED_HEADER + "\n"
          + 'quote@example.test,,,"Comma, Inc.","fixture-""q"",1","line one\nline two",https://comma.example.test,,\n'
          + cred(rng, 3) + "\n")

    # Row 3 has an invalid UTF-8 byte; rows 2 and 4 import.
    write("invalid-utf8.csv", (CRED_HEADER + "\n" + cred(rng, 4) + "\n").encode()
          + b"bad\xff@example.test,,,Bad,fixture-bad,,https://bad.example.test,,\n"
          + (cred(rng, 5) + "\n").encode())

    # Row 2 has a 70 KiB note (over the 64 KiB field cap).
    write("oversized.csv", CRED_HEADER + "\n"
          + f"big@example.test,,,Big,fixture-big,{'n' * 70 * 1024},https://big.example.test,,\n"
          + cred(rng, 6) + "\n")

    # The same identity 50 times (different passwords): one import.
    write("duplicate-storm.csv", CRED_HEADER + "\n"
          + "\n".join(cred(rng, 7, "same@example.test", "same.example.test") for _ in range(50)) + "\n")

    # A row with an OTP secret, and one with the wrong column count.
    write("otp-and-short.csv", CRED_HEADER + "\n"
          + cred(rng, 8, otp="JBSWY3DPEHPK3PXP") + "\nshort,row\n")

    write("empty.csv", "")
    write("header-only.csv", CRED_HEADER + "\n")
    write("unknown-columns.csv", "foo,bar\n1,2\n")

    # The ZIP with every member type, plus one we do not read.
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as zf:
        z = Fixed(zf)
        z.writestr("credentials.csv", "\n".join([CRED_HEADER, cred(rng, 9), cred(rng, 10)]) + "\n")
        z.writestr("payments.csv", PAY_HEADER + "\n"
                   + "\n".join(f"Card {i},{n},{(i % 12) + 1:02d}/2031,Fixture Holder,{100 + i},Testville"
                               for i, n in enumerate(TEST_CARDS)) + "\n")
        z.writestr("securenotes.csv", NOTE_HEADER + "\n"
                   + '"Fixture note","fixture body, with a comma\nand a second line",\n'
                   + "Second note,fixture-body-2,\n")
        z.writestr("ids.csv", "type,number\npassport,fixture-id\n")
    write("mixed.zip", buf.getvalue())

    # A row over 256 KiB whose quoted field holds newlines: only it is
    # skipped; the rows after it import (final review VER-B2).
    write("oversized-quoted.csv", CRED_HEADER + "\n"
          + f'big@example.test,,,Big,fixture-big,"{"q" * (256 * 1024)}\nstill inside\nx",https://big.example.test,,\n'
          + cred(rng, 11) + "\n" + cred(rng, 12) + "\n")

    # A ZIP whose credentials member is damaged (one stored byte flipped,
    # so its CRC-32 no longer matches); the payments member is intact.
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_STORED) as zf:
        for name, text in [("credentials.csv", "\n".join([CRED_HEADER, cred(rng, 13), cred(rng, 14)]) + "\n"),
                           ("payments.csv", PAY_HEADER + "\nCard,4111111111111111,05/2031,Fixture Holder,123,Testville\n")]:
            info = zipfile.ZipInfo(name, date_time=(2026, 10, 16, 0, 0, 0))
            zf.writestr(info, text)
    data = bytearray(buf.getvalue())
    at = data.index(b"user13@") + 2
    data[at] ^= 0x20
    write("damaged.zip", bytes(data))


if __name__ == "__main__":
    main()
