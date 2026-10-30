"""Quản trị glossary: đọc/ghi file JSON có phiên bản mà crate `glossary` (Rust) đọc."""

from .model import Glossary, GlossaryError, Term, apply_changes, dump, load, save, validate

__all__ = [
    "Glossary",
    "GlossaryError",
    "Term",
    "apply_changes",
    "dump",
    "load",
    "save",
    "validate",
]
