"""API keys and tokens for the token-API plugins in plugins/ext.

A secret comes from a command (`token_command: [op, read, "op://..."]`) if one is set, else
from an environment variable (`token_env: BUILDKITE_TOKEN`). Never from the config file.
A command runs once; its output is kept for the life of the plugin, so a password manager
is not asked again on every refresh. `forget()` drops it after the API rejects it.

This file is the same in every package that uses it; crates/cli/tests/ext_dev_b.rs checks
that the copies match, so change one and copy it to the others.
"""

import json
import os
import subprocess
from typing import Any, Dict, Optional

from dashboard_plugin import config_error, upstream_error

# How long a command may take, in seconds.
TIMEOUT = 20

_cache: Dict[str, str] = {}


def check(config: Dict[str, Any], env_key: str, command_key: Optional[str]) -> None:
    """Raise a config error unless `env_key` names a variable and `command_key`, when set,
    is a list of strings. The host does not check config_schema, so this is the check."""
    env = config.get(env_key)
    if not isinstance(env, str) or not env.strip():
        raise config_error(f"{env_key} must be the name of an environment variable")
    if command_key is None:
        return
    command = config.get(command_key)
    if command is None:
        return
    if not isinstance(command, list) or not command or not all(isinstance(a, str) and a for a in command):
        raise config_error(f'{command_key} must be a list of strings, e.g. [op, read, "op://vault/item/token"]')


def secret(config: Dict[str, Any], env_key: str, command_key: Optional[str], what: str = "API token") -> str:
    """The secret from `command_key`'s command if set, else from the `env_key` variable.
    Errors name the command or variable, never the secret or the command's output."""
    command = config.get(command_key) if command_key else None
    if command:
        cache_key = json.dumps(command)
        if cache_key in _cache:
            return _cache[cache_key]
        name = json.dumps(command[0])
        try:
            done = subprocess.run(
                command, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=TIMEOUT
            )
        except subprocess.TimeoutExpired:
            raise upstream_error(f"{command_key} {name} took longer than {TIMEOUT}s") from None
        except (OSError, subprocess.SubprocessError):
            raise upstream_error(f"{command_key} {name} could not run") from None
        if done.returncode != 0:
            raise upstream_error(f"{command_key} {name} failed (exit {done.returncode})")
        value = done.stdout.strip()
        if not value:
            raise upstream_error(f"{command_key} {name} printed nothing")
        _cache[cache_key] = value
        return value
    value = os.environ.get(config[env_key], "").strip()
    if not value:
        also = f", or set {env_key} / {command_key}" if command_key else f", or set {env_key}"
        raise upstream_error(f"no {what}: set ${config[env_key]} before starting the dashboard{also}")
    return value


def forget() -> None:
    """Drop secrets read from commands, so the next refresh runs them again."""
    _cache.clear()
