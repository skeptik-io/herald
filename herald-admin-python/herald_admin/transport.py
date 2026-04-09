from __future__ import annotations

import json
import urllib.request
import urllib.error
import urllib.parse
from typing import Any

from .errors import HeraldError


class HttpTransport:
    def __init__(self, base_url: str, token: str, timeout: int = 30) -> None:
        self._base_url = base_url.rstrip("/")
        self._token = token
        self._timeout = timeout

    def request(self, method: str, path: str, body: Any = None) -> Any:
        url = f"{self._base_url}{path}"
        data = json.dumps(body).encode() if body is not None else None

        req = urllib.request.Request(url, data=data, method=method)
        req.add_header("Authorization", f"Bearer {self._token}")
        if data is not None:
            req.add_header("Content-Type", "application/json")

        try:
            with urllib.request.urlopen(req, timeout=self._timeout) as resp:
                if resp.status == 204:
                    return None
                raw = resp.read()
                if not raw:
                    return None
                return json.loads(raw)
        except urllib.error.HTTPError as e:
            _HTTP_CODES = {
                400: "BAD_REQUEST", 401: "UNAUTHORIZED", 403: "FORBIDDEN",
                404: "NOT_FOUND", 409: "CONFLICT", 429: "RATE_LIMITED",
                500: "INTERNAL", 503: "UNAVAILABLE",
            }
            code = _HTTP_CODES.get(e.code, "INTERNAL")
            message = f"HTTP {e.code}"
            try:
                body_data = json.loads(e.read())
                if "error" in body_data:
                    message = body_data["error"]
            except Exception:
                pass
            raise HeraldError(code, message, e.code) from None
