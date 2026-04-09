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
            code = "INTERNAL"
            message = f"HTTP {e.code}"
            try:
                body_data = json.loads(e.read())
                if "error" in body_data:
                    message = body_data["error"]
            except Exception:
                pass
            if e.code == 401:
                code = "UNAUTHORIZED"
            elif e.code == 404:
                code = "NOT_FOUND"
            raise HeraldError(code, message, e.code) from None
