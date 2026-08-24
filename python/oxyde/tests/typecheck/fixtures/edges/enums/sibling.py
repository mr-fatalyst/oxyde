"""Enum in a dedicated module — imported by the model module."""

from __future__ import annotations

from enum import Enum


class Size(str, Enum):
    S = "s"
    M = "m"
    L = "l"
