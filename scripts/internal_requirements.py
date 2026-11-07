"""Stable internal requirements supported by the release tools.

Keep this policy aligned with internal_requirement_matches in release-sync.
Unfamiliar syntax needs explicit review instead of an automatic rewrite.
"""
import re


def matches(requirement, version):
    match = re.fullmatch(r'([=^~]?)(\d+)\.(\d+)\.(\d+)', requirement)
    candidate = re.fullmatch(r'(\d+)\.(\d+)\.(\d+)', version)
    if not match or not candidate:
        raise ValueError('Unsupported internal dependency requirement: ' + requirement)
    operator = match[1]
    base = tuple(map(int, match.groups()[1:]))
    target = tuple(map(int, candidate.groups()))
    if operator == '=':
        return target == base
    boundary = 1 if operator == '~' else next((i for i, n in enumerate(base) if n), 2)
    return target >= base and target[:boundary + 1] == base[:boundary + 1]
