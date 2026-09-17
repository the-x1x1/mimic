"""Structured engine errors with stable codes (docs/ARCHITECTURE.md §20)."""

from __future__ import annotations

from typing import Any


class EngineError(Exception):
    code = "engine_error"

    def __init__(self, message: str, *, code: str | None = None, details: dict[str, Any] | None = None):
        super().__init__(message)
        if code:
            self.code = code
        self.details = details or {}

    def to_wire(self) -> dict[str, Any]:
        return {"code": self.code, "message": str(self), "details": self.details}


class InvalidParamsError(EngineError):
    code = "invalid_params"


class UnknownMethodError(EngineError):
    code = "unknown_method"


class NotConfiguredError(EngineError):
    code = "not_configured"


class NotFoundError(EngineError):
    code = "not_found"


class UnsupportedError(EngineError):
    code = "unsupported"


class CanceledError(EngineError):
    code = "canceled"
