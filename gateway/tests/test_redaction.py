"""The redactor: slot values and their encodings, credential shapes, never ordinary words."""

from __future__ import annotations

import base64
import unittest
import urllib.parse

from tests.support import GATEWAY  # noqa: F401  (puts src/ on sys.path)

from orbit_gateway.redact import MASK, Redactor

SLOT = "p@ss/w0rd+x=y&z"


class Values(unittest.TestCase):
    def test_slot_value_and_its_encoded_forms(self) -> None:
        redactor = Redactor([SLOT])
        forms = {
            "raw": SLOT,
            "url-encoded": urllib.parse.quote(SLOT, safe=""),
            "form-encoded": urllib.parse.quote_plus(SLOT),
            "base64": base64.b64encode(SLOT.encode()).decode(),
            "base64 unpadded": base64.b64encode(SLOT.encode()).decode().rstrip("="),
            "urlsafe base64": base64.urlsafe_b64encode(SLOT.encode()).decode(),
        }
        for name, form in forms.items():
            with self.subTest(form=name):
                self.assertEqual(redactor.text(f"before {form} after"), f"before {MASK} after")

    def test_basic_user_password_pair(self) -> None:
        pair = base64.b64encode(f"ada:{SLOT}".encode()).decode()
        redactor = Redactor([SLOT], [("ada", SLOT)])
        self.assertEqual(redactor.text(f"sent {pair}"), f"sent {MASK}")

    def test_every_slot_is_masked(self) -> None:
        values = [f"value-{index:02d}-q8w7e6r5t4" for index in range(16)]
        text = " ".join(values)
        self.assertEqual(Redactor(values).text(text), " ".join([MASK] * 16))

    def test_values_inside_json_keys_and_nested_values(self) -> None:
        redactor = Redactor([SLOT])
        out = redactor.value({SLOT: [{"note": f"x{SLOT}x"}], "n": 3, "ok": True, "none": None})
        self.assertEqual(out, {MASK: [{"note": f"x{MASK}x"}], "n": 3, "ok": True, "none": None})


class Shapes(unittest.TestCase):
    redactor = Redactor()

    def test_high_confidence_credential_shapes(self) -> None:
        pem = "-----BEGIN RSA PRIVATE KEY-----\nMIIEow\nabc\n-----END RSA PRIVATE KEY-----"
        basic = base64.b64encode(b"svc:hunter22").decode()
        cases = {
            "Authorization: Bearer abcDEF123456ghiJKL789": f"Authorization: Bearer {MASK}",
            "bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.sig": f"bearer {MASK}",
            f"Authorization: Basic {basic}": f"Authorization: Basic {MASK}",
            "key AKIAIOSFODNN7EXAMPLE here": f"key {MASK} here",
            "key ASIAIOSFODNN7EXAMPLE here": f"key {MASK} here",
            f"x {pem} y": f"x {MASK} y",
            "-----BEGIN PRIVATE KEY-----\nabc\n-----END PRIVATE KEY-----": MASK,
            "token ghp_aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789": f"token {MASK}",
            "token github_pat_11ABCDEFG0123456789_abcdefghijklmnop": f"token {MASK}",
            "slack xoxb-1234567890-abcdefghij": f"slack {MASK}",
            "slack xoxp-1234567890-abcdefghij": f"slack {MASK}",
            '{"key": "sk-proj-abc123def456ghi789jkl"}': f'{{"key": "{MASK}"}}',
        }
        for text, expected in cases.items():
            with self.subTest(text=text):
                self.assertEqual(self.redactor.text(text), expected)

    def test_ordinary_words_are_never_masked(self) -> None:
        for text in (
            "Bearer tokens are described in RFC 6750",
            "Use Basic authentication for the legacy endpoint",
            "Basic auth is weak",
            "password secret token api_key credentials",
            '{"password": "", "token_type": "bearer", "secret": null}',
            "the desk-lamp-assessment-checklist-for-office is long",
            "task-0123456789abcdefghij0123 is an id",
            "risk-0123456789abcdefghijklmnop",
            "AKIA is a prefix; AKIAshort is not a key",
            "ghp_short and xoxb- alone",
            "MYAKIAIOSFODNN7EXAMPLEX is part of an identifier",
            "a sha256 of 9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
        ):
            with self.subTest(text=text):
                self.assertEqual(self.redactor.text(text), text)

    def test_field_names_are_kept_when_values_are_masked(self) -> None:
        out = self.redactor.value({"password": "Bearer abcDEF123456ghiJKL789", "secret": "visible"})
        self.assertEqual(out, {"password": f"Bearer {MASK}", "secret": "visible"})


if __name__ == "__main__":
    unittest.main()
